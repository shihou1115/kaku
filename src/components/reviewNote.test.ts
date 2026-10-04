import { describe, expect, it } from "vitest";
import { reviewNote, type ReviewNoteInput } from "./reviewNote";

const base: ReviewNoteInput = {
  name: "第一話の講評",
  now: new Date(2026, 9, 4, 14, 35),
  model: "test-model",
  path: "manuscript/01.md",
  aspects: ["style", "reader"],
  materials: [],
  notPassed: 0,
  warning: null,
  unchecked: 0,
  overall: "テンポは良い。",
  comments: [],
};

describe("講評の記録(テスト計画 E4)", () => {
  it("渡した資料は、実際に渡したものだけを書く。渡らなかった数も黙らない", () => {
    const md = reviewNote({
      ...base,
      materials: [
        { path: "codex/characters/架純.md", title: "架純", kind: "手動" },
        { path: "codex/locations/高校.md", title: "青葉高校", kind: "自動" },
      ],
      notPassed: 3,
    });
    expect(md).toContain("- 架純(手動) — codex/characters/架純.md");
    expect(md).toContain("- 青葉高校(自動) — codex/locations/高校.md");
    expect(md).toContain("3件は渡していません");
  });

  it("見ていない末尾があれば書く(全文を見た講評として読ませない)", () => {
    expect(reviewNote({ ...base, unchecked: 1200 })).toContain("末尾 1200字は見ていません");
    expect(reviewNote(base)).not.toContain("見ていません");
  });

  it("警告(打ち切り・拒否など)を書く", () => {
    const md = reviewNote({ ...base, warning: "2箇所で応答が途中で打ち切られました" });
    expect(md).toContain("> ⚠ 2箇所で応答が途中で打ち切られました");
  });

  it("採否の印と、本文に見つからない引用の印を書く", () => {
    const md = reviewNote({
      ...base,
      comments: [
        {
          aspect: "style",
          comment: "冗長",
          quote: "雨だった",
          found: true,
          suggestion: "削る",
          verdict: "done",
        },
        { aspect: "reader", comment: "引きが弱い", quote: "幻の一文", found: false, suggestion: "" },
        { aspect: "reader", comment: "説明が多い", quote: "", found: false, suggestion: "", verdict: "dropped" },
      ],
    });
    expect(md).toContain("- 冗長 【対応済み】");
    expect(md).toContain("  - 引用: 「雨だった」\n");
    expect(md).toContain("  - 方向: 削る");
    expect(md).toContain("「幻の一文」 ※本文に見つかりません");
    expect(md).toContain("- 説明が多い 【棄却】");
    expect(md).toContain("## 指摘(3件)");
  });

  it("資料も指摘も無いときは、無いと書く", () => {
    const md = reviewNote(base);
    expect(md).toContain("### 渡した設定資料\n\n- (なし)");
    expect(md).toContain("## 指摘(0件)\n\n(なし)");
  });
});
