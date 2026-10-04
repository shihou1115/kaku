import { describe, expect, it } from "vitest";
import { clip, frontmatter, notePath, safeFileName, stamp } from "./saveNote";

describe("AIの出力を正本へ残す(03 §4.3)", () => {
  it("ファイル名に使えない文字を落とす", () => {
    expect(safeFileName("架純/悠二:の話")).toBe("架純_悠二_の話");
    expect(safeFileName('a<b>c|d"e?f*g\\h')).toBe("a_b_c_d_e_f_g_h");
  });

  it("空の件名でも壊れない", () => {
    expect(safeFileName("   ")).toBe("無題");
    expect(safeFileName("")).toBe("無題");
  });

  it("長すぎる件名は切る", () => {
    expect(safeFileName("あ".repeat(80))).toHaveLength(40);
  });

  // テスト計画 E4: 依頼の書き出しを件名の既定値にしているので、貼り付けた文の
  // タブや、𠮷・絵文字がちょうど切れ目に来ることがある
  it("切るときに𠮷や絵文字を割らない(片割れのサロゲートを残さない)", () => {
    const name = safeFileName("あ".repeat(39) + "𠮷" + "い");
    expect(Array.from(name)).toHaveLength(40);
    expect(name.endsWith("𠮷")).toBe(true);
    expect(clip("👍👍👍", 2)).toBe("👍👍");
  });

  it("制御文字(タブなど)は名前に残さない", () => {
    expect(safeFileName("前半\t後半")).toBe("前半 後半");
    expect(safeFileName("\t\u0001")).toBe("無題");
  });

  it("日時は並べたときに時系列になる形", () => {
    expect(stamp(new Date(2026, 6, 31, 14, 5))).toBe("2026-07-31-1405");
    // 月・日・時・分がゼロ詰めされること(文字列ソートが時系列と一致する条件)
    expect(stamp(new Date(2026, 0, 1, 0, 0))).toBe("2026-01-01-0000");
  });

  it("保存先のパスを組み立てる", () => {
    expect(notePath("ideas", "展開案", new Date(2026, 6, 31, 14, 35))).toBe(
      "ideas/2026-07-31-1435-展開案.md",
    );
  });

  it("フロントマターは空の値を書かない", () => {
    const got = frontmatter({ title: "x", model: "", source: undefined });
    expect(got).toBe("---\ntitle: x\n---\n");
  });

  it("改行を含む値でYAMLを壊さない", () => {
    // 件名に改行が混ざっても1行に潰す
    const got = frontmatter({ title: "前半\n後半" });
    expect(got).toBe("---\ntitle: 前半 後半\n---\n");
  });

  it("コロンを含む件名は引用符で囲む", () => {
    // 囲まないと「キー: 値: 値」になって構造が壊れる。
    // 残したファイルは他のツールでも開かれる(P-5)
    expect(frontmatter({ title: "架純: 展開案" })).toBe(
      "---\ntitle: '架純: 展開案'\n---\n",
    );
  });

  it("記号で始まる値も囲む", () => {
    expect(frontmatter({ title: "- 先頭がハイフン" })).toBe(
      "---\ntitle: '- 先頭がハイフン'\n---\n",
    );
    expect(frontmatter({ title: "#タグ風" })).toBe(
      "---\ntitle: '#タグ風'\n---\n",
    );
  });

  it("二重引用符を含む値は囲みを外すだけで元に戻る形にする", () => {
    // frontmatter.rs の unquote はエスケープを解かない。
    // 二重引用符+バックスラッシュにすると「\"引用\"」がそのまま表示されてしまう
    expect(frontmatter({ title: '「"引用"」: メモ' })).toBe(
      "---\ntitle: '「\"引用\"」: メモ'\n---\n",
    );
  });

  it("普通の日本語は囲まない(読みやすさを損なわない)", () => {
    expect(frontmatter({ title: "この場面の展開案" })).toBe(
      "---\ntitle: この場面の展開案\n---\n",
    );
  });
});
