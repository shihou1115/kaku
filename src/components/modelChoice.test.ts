import { describe, expect, it } from "vitest";
import { modelChoices, modelMissing } from "./modelChoice";

const LIST = ["gemma-12b", "qwen-27b"];

describe("設定のモデル欄(画面と実際に送るモデルを食い違わせない)", () => {
  it("保存してあるモデルが一覧にあれば、一覧をそのまま並べる", () => {
    expect(modelChoices(LIST, "qwen-27b").map((c) => c.value)).toEqual(LIST);
    expect(modelMissing(LIST, "qwen-27b")).toBe(false);
  });

  it("一覧に無いモデルも、選ばれた状態で見せる", () => {
    // 並べないと <select> は一覧の先頭を表示し、実際に送るモデルと食い違う
    const got = modelChoices(LIST, "thinkingcap-27b");
    expect(got[0].value).toBe("thinkingcap-27b");
    expect(got[0].label).toContain("一覧にありません");
    expect(got.slice(1).map((c) => c.value)).toEqual(LIST);
    expect(modelMissing(LIST, "thinkingcap-27b")).toBe(true);
  });

  it("未設定なら、選んでいないことが分かる項目を先頭に置く", () => {
    const got = modelChoices(LIST, "");
    expect(got[0]).toEqual({ value: "", label: "(モデルを選んでください)" });
    expect(modelMissing(LIST, "")).toBe(false);
  });

  it("一覧を取れていないときは「無い」と言わない", () => {
    expect(modelMissing([], "qwen-27b")).toBe(false);
  });
});
