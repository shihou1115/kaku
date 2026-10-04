/**
 * 「この相談を残す」で `ideas/` へ書く中身(M-03 / 03 §4.3)。
 *
 * 組み立ては画面から切り出して、純関数としてテストする(テスト計画 E4)。
 * **記録は実際と食い違ってはいけない**: 実際に渡った本文・資料・往復だけを「渡した」と書く。
 */

import type { ChatTurn, ContextPreview } from "../api";
import { frontmatter } from "./saveNote";

export type IdeaNoteInput = {
  name: string;
  now: Date;
  model?: string;
  /** 相談したときに開いていた文書 */
  path: string | null;
  /** 実際に送った素材(送信のたびに組み立て直したもの) */
  context: ContextPreview;
  /** 画面が持っていた往復(会話モード)。単発なら空 */
  history: ChatTurn[];
  /**
   * 実際に送った往復の数(直近から)。history より少なければ、古い往復は長すぎるため
   * AI へ送っていない(テスト計画 E1)
   */
  historySent: number;
  question: string;
  answer: string;
};

export function ideaNote(i: IdeaNoteInput): string {
  const entries = i.context.entries;
  const materials = [
    ...(entries.length > 0
      ? entries.map(
          (e) =>
            `- ${e.title}(${e.source === "manual" ? "手動" : "自動"}${
              e.truncated ? "・長いため途中まで" : ""
            }) — ${e.path}`,
        )
      : ["- (なし)"]),
    // 上限で落ちた分も黙って隠さない — 隠すと「全部渡したうえでの応答」として読める
    ...(i.context.dropped_entries > 0
      ? [`- ※ 上限を超えたため ${i.context.dropped_entries}件は渡していません`]
      : []),
  ];

  // 送らなかった往復(古い方から)。送っていないものを、渡したように読ませない
  const unsent = Math.max(0, i.history.length - i.historySent);
  const n = i.history.length;

  return [
    frontmatter({
      title: i.name,
      created: i.now.toISOString(),
      model: i.model,
      source: i.path ?? undefined,
    }),
    "## 対象",
    "",
    `- 文書: ${i.path ?? "(ファイルを開いていない)"}`,
    // 字数は文字で数える(𠮷や絵文字を2字と数えない)
    `- 渡した本文: ${Array.from(i.context.body).length}字${
      i.context.body_truncated ? "(長いため末尾を切り捨て)" : ""
    }`,
    ...(unsent > 0
      ? [`- 送った往復: 直近 ${i.historySent}往復(古い ${unsent}往復は長すぎるため送っていません)`]
      : []),
    "",
    "### 渡した設定資料",
    "",
    ...materials,
    "",
    // 会話モードでは**やりとり全体**を残す。最後の1往復だけでは何の話か読めない
    ...i.history.flatMap((t, k) => [
      `## 依頼 ${k + 1}`,
      "",
      ...(k < unsent ? ["> ※ この往復は、長すぎるため AI へ送っていません", ""] : []),
      t.question,
      "",
      `## 応答 ${k + 1}`,
      "",
      t.answer,
      "",
    ]),
    n > 0 ? `## 依頼 ${n + 1}` : "## 依頼",
    "",
    i.question,
    "",
    n > 0 ? `## 応答 ${n + 1}` : "## 応答",
    "",
    i.answer,
    "",
  ].join("\n");
}
