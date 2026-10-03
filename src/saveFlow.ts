/**
 * 保存の流れ(自動保存・切替・終了・外部編集との競合)の判断。
 *
 * App.tsx の中にあった頃はテストが1件も無く、次をどれも実機で踏むまで見つけられなかった:
 *  - 保存に失敗したまま切替・削除・終了へ進む(05-roadmap §9 指摘1)
 *  - 競合の二択で「あとで決める」を選ぶと、二度と二択へ戻れない
 *  - 保存の完了を待っていた複数の呼び出しが、そろって書き直して自分の書き込みと競合する
 *
 * **判断だけをここへ出し、React の状態は App が持ったままにする**
 * (依存を増やさずに vitest で確かめるため)。
 */

import type { SaveOutcome } from "./api";

/**
 * 保存を試みる理由。**競合の二択を出し直すかどうかが変わる。**
 *
 * - `auto`: 入力の区切り・フォーカス喪失。打つたびに聞き直すとダイアログが出続けるので、
 *   同じ外部変更については一度しか聞かない
 * - `save`: 明示の保存(Ctrl+S・ヘッダーのボタン)
 * - `proceed`: 切替・削除・改名・分割・終了など、本文を手放す(またはディスク側を
 *   書き換える)操作の前
 */
export type FlushReason = "auto" | "save" | "proceed";

export type FlushResult =
  /** 保存するものが無かった */
  | { kind: "clean" }
  | { kind: "saved"; modifiedMs: number }
  /** **書いていない。** 読み込んだ後にアプリの外で書き換えられていた(T-08) */
  | { kind: "conflict"; path: string; actualMs: number }
  | { kind: "error"; message: string };

/** このあと本文を手放して(書き換えられて)よいか */
export function canProceed(r: FlushResult): boolean {
  return r.kind === "clean" || r.kind === "saved";
}

/**
 * 競合の二択を出すか。
 *
 * 自動保存では、同じ外部変更(同じ更新時刻)について二度は聞かない。
 * **本人が何かをしようとしたとき(`save` / `proceed`)は毎回出す。**
 * ここで黙ると、「あとで決める」を選んだあとに二択へ戻る道が無くなり、
 * 切替も終了もできないまま「保存できません」とだけ言われ続ける。
 */
export function shouldPrompt(
  reason: FlushReason,
  actualMs: number,
  lastPromptedMs: number,
): boolean {
  return reason !== "auto" || actualMs !== lastPromptedMs;
}

/**
 * 操作を止めたときの説明。止めなくてよければ null。
 *
 * 競合と保存の失敗は**原因も、本人がすべきことも違う**ので言い分ける。
 * 以前はどちらも「保存に失敗したため」と出しており、競合のときに
 * 的外れな対処(読み取り専用・空き容量)へ誘導していた。
 */
export function blockedMessage(r: FlushResult, what: string): string | null {
  switch (r.kind) {
    case "clean":
    case "saved":
      return null;
    case "conflict":
      return `アプリの外で変更されているため、${what}を止めました。どちらを残すか選んでください`;
    case "error":
      return `保存に失敗したため、${what}を中止しました。本文はこのまま残っています(${r.message})`;
  }
}

/**
 * 開いているファイルがディスク上で変わっていたときの扱い(03 §5-1 / §5-2)。
 *
 * 未保存の変更が無ければ黙って読み直し、あれば二択へ回す。
 * **勝手に混ぜも捨てもしない。**
 */
export function diskChange(
  diskMs: number,
  knownMs: number,
  dirty: boolean,
): "same" | "reload" | "conflict" {
  if (diskMs === knownMs) return "same";
  return dirty ? "conflict" : "reload";
}

/**
 * 競合を「別名で保存」するときの名前。拡張子は元のまま、無ければ `.md`。
 *
 * 以前は `/.md$/`(点のエスケープ漏れ)で拡張子を落としていた。
 * 拡張子の無い `…cmd` のような名前だと、名前の末尾まで削っていた。
 */
export function conflictCopyPath(path: string, stamp: string): string {
  const slash = path.lastIndexOf("/");
  const dot = path.lastIndexOf(".");
  // 先頭の点(`.foo`)は拡張子ではない。フォルダー名の点も拡張子ではない
  const hasExt = dot > slash + 1;
  const stem = hasExt ? path.slice(0, dot) : path;
  const ext = hasExt ? path.slice(dot) : ".md";
  return `${stem}-競合${stamp}${ext}`;
}

/** 保存の判断に要る、その時点の状態 */
export type Snapshot = {
  path: string | null;
  text: string;
  savedText: string;
  /** 読み込んだ(または最後に書いた)ときの更新時刻。0 は「分からない」 */
  modifiedMs: number;
};

/**
 * 保存を1本ずつ流す。
 *
 * 実行中の保存があれば、終わるのを待ってから**状態を見直して**判断する。
 * 同じ保存を複数の呼び出しが待っていた場合(自動保存の最中にファイルを切り替え、
 * 同時にウィンドウのフォーカスも外れた、など)、先に起きた方が次の保存を始めている。
 * 待ち直さずにそろって書くと、後の方が**自分の書き込みを外部の変更と取り違えて**
 * 競合になる。
 */
export function createSaver(deps: {
  /** 呼んだ時点の最新の状態(App の live の箱) */
  current: () => Snapshot;
  save: (
    path: string,
    text: string,
    expectedMs: number | null,
  ) => Promise<SaveOutcome>;
  /**
   * 書けたとき。**同期で状態の箱まで更新すること。** 再描画を待つと、
   * 直後の自動保存が古い時刻で照合して自分の変更を競合と誤判定する。
   * 保存中に別のファイルへ移っていた場合は呼ばない(その画面の状態は触らない)
   */
  saved: (path: string, text: string, modifiedMs: number) => void;
}) {
  let inflight: Promise<FlushResult> | null = null;

  async function flush(): Promise<FlushResult> {
    while (inflight) await inflight;
    const { path, text, savedText, modifiedMs } = deps.current();
    if (!path || text === savedText) return { kind: "clean" };

    const task = (async (): Promise<FlushResult> => {
      try {
        const r = await deps.save(path, text, modifiedMs || null);
        if (r.kind === "Conflict") {
          return { kind: "conflict", path, actualMs: r.actual_ms };
        }
        if (deps.current().path === path) deps.saved(path, text, r.modified_ms);
        return { kind: "saved", modifiedMs: r.modified_ms };
      } catch (e) {
        return { kind: "error", message: String(e) };
      }
    })();
    inflight = task;
    try {
      return await task;
    } finally {
      if (inflight === task) inflight = null;
    }
  }

  return {
    flush,
    /** 保存の途中か(閉じる操作で待つべきものがあるか) */
    busy: () => inflight !== null,
  };
}
