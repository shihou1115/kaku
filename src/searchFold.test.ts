import { describe, expect, it } from "vitest";
import { findForSearch, foldForSearch } from "./searchFold";

describe("検索の照合(テスト計画 D3。Rust の search::fold と同じ規則)", () => {
  it("大文字小文字(全角の英字も)を区別せずに見つける", () => {
    expect(findForSearch("前置き。Hello World", "hello")).toBe(4);
    expect(findForSearch("前置き。Hello World", "WORLD")).toBe(10);
    expect(findForSearch("ＡＢＣの話", "ａｂｃ")).toBe(0);
  });

  it("位置は元の本文のまま(サロゲートペアや結合文字の後ろでも)", () => {
    const text = "𠮷か゚Hello";
    const at = findForSearch(text, "hELLO");
    expect(at).toBe(4);
    expect(text.slice(at, at + 5)).toBe("Hello");
  });

  it("全角と半角は寄せない(索引と同じ)", () => {
    expect(findForSearch("Ａbc", "abc")).toBe(-1);
  });

  it("長さが変わる文字は寄せない(位置がずれないように)", () => {
    // İ の小文字は2文字(i + 結合点)になるので、そのまま残す
    expect(foldForSearch("İ")).toBe("İ");
    expect(foldForSearch("ÀΣ")).toBe("àσ");
  });

  it("見つからなければ -1。空の語は探さない", () => {
    expect(findForSearch("本文", "無い")).toBe(-1);
    expect(findForSearch("本文", "")).toBe(-1);
  });
});
