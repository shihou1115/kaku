import { describe, expect, it } from "vitest";
import type { SaveOutcome } from "./api";
import {
  blockedMessage,
  canProceed,
  clockStamp,
  conflictCopyPath,
  createSaver,
  diskChange,
  shouldPrompt,
  type Snapshot,
} from "./saveFlow";

/**
 * 手で進める保存。呼ばれた順に控え、終わらせるまで待たせる。
 * `onDisk` は競合のあとに読み直したときのディスクの中身(既定は外部で書き換えられた本文)
 */
function fakeDisk(
  onDisk: { text: string; modifiedMs: number } | Error = {
    text: "外部で書き換えた本文",
    modifiedMs: 5000,
  },
) {
  const calls: {
    path: string;
    text: string;
    expectedMs: number | null;
    finish: (o: SaveOutcome) => void;
    fail: (e: unknown) => void;
  }[] = [];
  const save = (path: string, text: string, expectedMs: number | null) =>
    new Promise<SaveOutcome>((finish, fail) => {
      calls.push({ path, text, expectedMs, finish, fail });
    });
  const read = async () => {
    if (onDisk instanceof Error) throw onDisk;
    return onDisk;
  };
  return { calls, save, read };
}

/** App の live の箱の代わり。saved で同期に書き換わる(App と同じ約束) */
function fakeApp(init: Partial<Snapshot> = {}) {
  const state: Snapshot = {
    path: "manuscript/01.md",
    text: "本文",
    savedText: "本文",
    modifiedMs: 1000,
    ...init,
  };
  return {
    state,
    current: () => ({ ...state }),
    saved: (_path: string, text: string, ms: number) => {
      state.savedText = text;
      state.modifiedMs = ms;
    },
  };
}

/** 保留中の Promise の続きを流しきる */
const settle = () => new Promise((r) => setTimeout(r, 0));

describe("保存の直列化(saveFlow.createSaver)", () => {
  it("保存するものが無ければ書かない", async () => {
    const disk = fakeDisk();
    const app = fakeApp();
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });
    expect(await saver.flush()).toEqual({ kind: "clean" });
    expect(disk.calls).toHaveLength(0);
  });

  it("書けたら、次の保存は新しい更新時刻で照合する", async () => {
    // **再描画を待たずに時刻を更新しないと、自分の書き込みを競合と誤判定する**(28ca70f)
    const disk = fakeDisk();
    const app = fakeApp({ text: "本文+1" });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });

    const first = saver.flush();
    await settle();
    expect(disk.calls[0].expectedMs).toBe(1000);
    disk.calls[0].finish({ kind: "Saved", modified_ms: 2000 });
    expect(await first).toEqual({ kind: "saved", modifiedMs: 2000 });
    expect(app.state.savedText).toBe("本文+1");

    app.state.text = "本文+2";
    const second = saver.flush();
    await settle();
    expect(disk.calls[1].expectedMs).toBe(2000);
    disk.calls[1].finish({ kind: "Saved", modified_ms: 3000 });
    await second;
  });

  it("競合なら書かずに返し、画面の状態にも触らない", async () => {
    const disk = fakeDisk();
    const app = fakeApp({ text: "書きかけ" });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });

    const r = saver.flush();
    await settle();
    disk.calls[0].finish({ kind: "Conflict", actual_ms: 5000 });
    expect(await r).toEqual({
      kind: "conflict",
      path: "manuscript/01.md",
      actualMs: 5000,
    });
    // 「保存済み」にしてしまうと、未保存の編集が捨ててよいものに見える
    expect(app.state.savedText).toBe("本文");
    expect(app.state.modifiedMs).toBe(1000);
  });

  it("失敗は成功と区別できる形で返す", async () => {
    const disk = fakeDisk();
    const app = fakeApp({ text: "書きかけ" });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });

    const r = saver.flush();
    await settle();
    disk.calls[0].fail("アクセスが拒否されました");
    const got = await r;
    expect(got).toEqual({ kind: "error", message: "アクセスが拒否されました" });
    expect(canProceed(got)).toBe(false);
    expect(app.state.savedText).toBe("本文");
  });

  it("実行中の保存が終わるまで次の保存を始めない", async () => {
    const disk = fakeDisk();
    const app = fakeApp({ text: "本文+1" });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });

    const a = saver.flush();
    await settle();
    expect(saver.busy()).toBe(true);
    const b = saver.flush();
    await settle();
    // 待っている間は書かない
    expect(disk.calls).toHaveLength(1);

    disk.calls[0].finish({ kind: "Saved", modified_ms: 2000 });
    await a;
    // 書いた後に変更が無ければ、待っていた側は書き直さない
    expect(await b).toEqual({ kind: "clean" });
    expect(disk.calls).toHaveLength(1);
    expect(saver.busy()).toBe(false);
  });

  it("同じ保存を待っていた複数の呼び出しが、そろって書き直さない", async () => {
    // そろって書くと、後の方が先の書き込みを外部の変更と取り違えて競合になる
    const disk = fakeDisk();
    const app = fakeApp({ text: "本文+1" });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });

    const a = saver.flush();
    await settle();
    // 保存中にさらに打ち込み、切替とフォーカス喪失が同時に保存を求めた
    app.state.text = "本文+2";
    const b = saver.flush();
    const c = saver.flush();
    await settle();

    disk.calls[0].finish({ kind: "Saved", modified_ms: 2000 });
    await settle();
    expect(disk.calls).toHaveLength(2);
    expect(disk.calls[1].text).toBe("本文+2");
    expect(disk.calls[1].expectedMs).toBe(2000);

    disk.calls[1].finish({ kind: "Saved", modified_ms: 3000 });
    await Promise.all([a, b, c]);
    expect(disk.calls).toHaveLength(2);
    expect(app.state.savedText).toBe("本文+2");
  });

  it("保存中に別のファイルへ移っていたら、その画面の状態は触らない", async () => {
    const disk = fakeDisk();
    const app = fakeApp({ text: "本文+1" });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });

    const a = saver.flush();
    await settle();
    // 競合を「別名で保存」して別のファイルへ移った
    Object.assign(app.state, {
      path: "manuscript/01-競合143005.md",
      text: "本文+1",
      savedText: "本文+1",
      modifiedMs: 7000,
    });
    disk.calls[0].finish({ kind: "Saved", modified_ms: 2000 });
    expect(await a).toEqual({ kind: "saved", modifiedMs: 2000 });
    expect(app.state.modifiedMs).toBe(7000);
  });

  it("時刻が分からないファイルは照合せずに書く", async () => {
    // 0 は「読めなかった」の印。照合に使うと保存が一切通らなくなる(project::is_stale)
    const disk = fakeDisk();
    const app = fakeApp({ text: "本文+1", modifiedMs: 0 });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });
    const a = saver.flush();
    await settle();
    expect(disk.calls[0].expectedMs).toBeNull();
    disk.calls[0].finish({ kind: "Saved", modified_ms: 2000 });
    await a;
  });
});

describe("時刻だけ変わったファイル(同期ソフト等が触っただけ)", () => {
  it("中身が読み込んだときのままなら、二択を出さずに書く", async () => {
    // 以前は時刻の食い違いだけで二択を出していた
    const disk = fakeDisk({ text: "本文", modifiedMs: 5000 });
    const app = fakeApp({ text: "本文+1" });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });

    const r = saver.flush();
    await settle();
    disk.calls[0].finish({ kind: "Conflict", actual_ms: 5000 });
    await settle();
    // 読み直した時刻で照合し直して書く
    expect(disk.calls).toHaveLength(2);
    expect(disk.calls[1].expectedMs).toBe(5000);
    expect(disk.calls[1].text).toBe("本文+1");
    disk.calls[1].finish({ kind: "Saved", modified_ms: 6000 });
    expect(await r).toEqual({ kind: "saved", modifiedMs: 6000 });
    expect(app.state.savedText).toBe("本文+1");
    expect(app.state.modifiedMs).toBe(6000);
  });

  it("中身が変わっていれば、書かずに二択へ回す", async () => {
    const disk = fakeDisk({ text: "外部で書き換えた本文", modifiedMs: 5000 });
    const app = fakeApp({ text: "本文+1" });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });

    const r = saver.flush();
    await settle();
    disk.calls[0].finish({ kind: "Conflict", actual_ms: 5000 });
    expect(await r).toEqual({
      kind: "conflict",
      path: "manuscript/01.md",
      actualMs: 5000,
    });
    expect(disk.calls).toHaveLength(1);
  });

  it("読み直せなければ、二択へ回す(書かない側に倒す)", async () => {
    const disk = fakeDisk(new Error("読めません"));
    const app = fakeApp({ text: "本文+1" });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });

    const r = saver.flush();
    await settle();
    disk.calls[0].finish({ kind: "Conflict", actual_ms: 5000 });
    expect((await r).kind).toBe("conflict");
    expect(disk.calls).toHaveLength(1);
  });

  it("読み直したあとで中身が変わっていれば、照合し直しで二択になる", async () => {
    const disk = fakeDisk({ text: "本文", modifiedMs: 5000 });
    const app = fakeApp({ text: "本文+1" });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });

    const r = saver.flush();
    await settle();
    disk.calls[0].finish({ kind: "Conflict", actual_ms: 5000 });
    await settle();
    disk.calls[1].finish({ kind: "Conflict", actual_ms: 7000 });
    expect(await r).toEqual({
      kind: "conflict",
      path: "manuscript/01.md",
      actualMs: 7000,
    });
    expect(app.state.savedText).toBe("本文");
  });
});

describe("競合の二択を出し直すか(T-08)", () => {
  it("自動保存では、同じ外部変更について二度は聞かない", () => {
    expect(shouldPrompt("auto", 5000, 0)).toBe(true);
    expect(shouldPrompt("auto", 5000, 5000)).toBe(false);
  });

  it("外部がまた変わったら、自動保存でも聞き直す", () => {
    expect(shouldPrompt("auto", 6000, 5000)).toBe(true);
  });

  it("「あとで決める」の後でも、本人の操作では毎回二択へ戻す", () => {
    // ここで黙ると、切替も終了もできないまま二択へ戻る道が無くなる
    expect(shouldPrompt("save", 5000, 5000)).toBe(true);
    expect(shouldPrompt("proceed", 5000, 5000)).toBe(true);
  });
});

describe("保存できなかったときに操作を止める", () => {
  it("保存できた・保存するものが無いときは止めない", () => {
    expect(canProceed({ kind: "clean" })).toBe(true);
    expect(canProceed({ kind: "saved", modifiedMs: 1 })).toBe(true);
    expect(blockedMessage({ kind: "clean" }, "切り替え")).toBeNull();
  });

  it("競合と保存の失敗を言い分ける", () => {
    const conflict = blockedMessage(
      { kind: "conflict", path: "a.md", actualMs: 1 },
      "ファイルの切り替え",
    );
    expect(conflict).toContain("アプリの外で変更されている");
    expect(conflict).toContain("ファイルの切り替えを止めました");
    expect(conflict).not.toContain("保存に失敗");

    const error = blockedMessage(
      { kind: "error", message: "ディスクがいっぱいです" },
      "削除",
    );
    expect(error).toContain("保存に失敗したため、削除を中止しました");
    expect(error).toContain("ディスクがいっぱいです");
  });
});

describe("ディスク側の変化の扱い(03 §5-1 / §5-2)", () => {
  const known = { ms: 1000, savedText: "保存した本文" };

  it("変わっていなければ何もしない", () => {
    expect(diskChange({ ms: 1000, text: "保存した本文" }, known, true)).toBe("same");
  });

  it("中身が同じで時刻だけ違えば、読み直さずに時刻を合わせる", () => {
    // 同期ソフトが触っただけ。合わせないと次の保存が競合と誤判定する。
    // 未保存の変更があっても二択は出さない(混ぜる相手がいない)
    expect(diskChange({ ms: 2000, text: "保存した本文" }, known, true)).toBe("adopt");
    expect(diskChange({ ms: 2000, text: "保存した本文" }, known, false)).toBe(
      "adopt",
    );
  });

  it("中身が違い、未保存の変更が無ければ読み直す", () => {
    expect(diskChange({ ms: 2000, text: "書き換え" }, known, false)).toBe("reload");
  });

  it("中身が違い、未保存の変更があれば、勝手に読み直さず二択へ回す", () => {
    expect(diskChange({ ms: 2000, text: "書き換え" }, known, true)).toBe("conflict");
  });

  it("時刻が同じでも中身が違えば読み直す(同じミリ秒の書き込み)", () => {
    // 改名前の保存とリンクの書き換えが同じミリ秒に入ると、時刻では区別できない
    expect(diskChange({ ms: 1000, text: "リンクを書き換えた本文" }, known, false)).toBe(
      "reload",
    );
  });
});

describe("保存の完了待ち(ディスクと見比べる前)", () => {
  it("実行中の保存が終わるまで待ち、自分では書かない", async () => {
    const disk = fakeDisk();
    const app = fakeApp({ text: "本文+1" });
    const saver = createSaver({ ...app, save: disk.save, read: disk.read });

    const a = saver.flush();
    await settle();
    let done = false;
    const s = saver.settled().then(() => {
      done = true;
    });
    await settle();
    expect(done).toBe(false);

    disk.calls[0].finish({ kind: "Saved", modified_ms: 2000 });
    await Promise.all([a, s]);
    expect(done).toBe(true);
    expect(disk.calls).toHaveLength(1);
    // 待ち終えた時点で、見比べる相手(保存した本文と時刻)は新しくなっている
    expect(app.state.savedText).toBe("本文+1");
    expect(app.state.modifiedMs).toBe(2000);
  });
});

describe("競合を別名で保存するときの名前", () => {
  it("拡張子の前に印を入れる", () => {
    expect(conflictCopyPath("manuscript/01-出会い.md", "143005")).toBe(
      "manuscript/01-出会い-競合143005.md",
    );
  });

  it("拡張子は元のまま残す", () => {
    expect(conflictCopyPath("plot/メモ.txt", "143005")).toBe(
      "plot/メモ-競合143005.txt",
    );
  });

  it("拡張子の無い名前を削らない", () => {
    // `/.md$/` だと「cmd」の末尾3文字を拡張子として落としていた
    expect(conflictCopyPath("plot/cmd", "143005")).toBe("plot/cmd-競合143005.md");
  });

  it("フォルダー名の点を拡張子と取り違えない", () => {
    expect(conflictCopyPath("plot/v1.2/メモ", "143005")).toBe(
      "plot/v1.2/メモ-競合143005.md",
    );
  });

  it("時刻はゼロ埋めする", () => {
    // 埋めないと4時台が `45926` になり、名前の長さも並びも揃わない
    expect(clockStamp(new Date(2026, 9, 4, 4, 59, 26))).toBe("045926");
    expect(clockStamp(new Date(2026, 9, 4, 14, 5, 9))).toBe("140509");
    expect(clockStamp(new Date(2026, 9, 4, 0, 0, 0))).toBe("000000");
  });

  it("先頭の点は拡張子ではない", () => {
    expect(conflictCopyPath("plot/.memo", "143005")).toBe(
      "plot/.memo-競合143005.md",
    );
  });
});
