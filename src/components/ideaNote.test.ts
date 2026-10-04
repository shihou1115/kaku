import { describe, expect, it } from "vitest";
import { ideaNote, type IdeaNoteInput } from "./ideaNote";

const base: IdeaNoteInput = {
  name: "展開案",
  now: new Date(2026, 9, 4, 14, 35),
  model: "test-model",
  path: "manuscript/01.md",
  context: {
    body: "本文",
    body_truncated: false,
    entries: [],
    dropped_entries: 0,
    total_chars: 2,
  },
  history: [],
  historySent: 0,
  question: "続きは?",
  answer: "案は三つ。",
};

const turn = (q: string) => ({ question: q, answer: `${q}への答え` });

describe("相談の記録(テスト計画 E1・E4)", () => {
  it("送らなかった古い往復には、送っていないと書く", () => {
    const md = ideaNote({
      ...base,
      history: [turn("一つ目"), turn("二つ目"), turn("三つ目")],
      historySent: 1,
    });
    expect(md).toContain("直近 1往復(古い 2往復は長すぎるため送っていません)");
    const marks = md.split("この往復は、長すぎるため AI へ送っていません").length - 1;
    expect(marks).toBe(2);
    // 印は送らなかった往復(依頼1・依頼2)にだけ付く
    expect(md.indexOf("## 依頼 3")).toBeGreaterThan(md.lastIndexOf("送っていません"));
    expect(md).toContain("## 依頼 4\n\n続きは?");
  });

  it("全部送ったなら、何も足さない", () => {
    const md = ideaNote({ ...base, history: [turn("一つ目")], historySent: 1 });
    expect(md).not.toContain("送っていません");
    expect(md).not.toContain("送った往復");
  });

  it("実際に渡した資料と、上限で落ちた数を書く", () => {
    const md = ideaNote({
      ...base,
      context: {
        ...base.context,
        entries: [
          { path: "codex/characters/架純.md", title: "架純", source: "manual", text: "幼馴染", truncated: false },
          { path: "codex/terms/魔法.md", title: "魔法", source: "mention", text: "長い", truncated: true },
        ],
        dropped_entries: 2,
      },
    });
    expect(md).toContain("- 架純(手動) — codex/characters/架純.md");
    // 1件の上限で切った資料には、途中までだと書く(テスト計画 E2)
    expect(md).toContain("- 魔法(自動・長いため途中まで) — codex/terms/魔法.md");
    expect(md).toContain("2件は渡していません");
  });

  it("渡した本文の字数は文字で数える(𠮷や絵文字を2字と数えない)", () => {
    const md = ideaNote({ ...base, context: { ...base.context, body: "𠮷田👍" } });
    expect(md).toContain("- 渡した本文: 3字");
  });
});
