import { describe, expect, it } from "vitest";
import { countLabel, fullyChecked } from "./resultLabel";

describe("AIの結果の件数表示(「指摘なし」の誤報告を出さない)", () => {
  it("全部を確かめて0件なら「指摘なし」", () => {
    expect(countLabel(0, { warning: null, unchecked: 0 }, "指摘なし")).toBe(
      "指摘なし",
    );
    expect(fullyChecked({ warning: null, unchecked: 0 })).toBe(true);
  });

  it("警告があれば0件でも「指摘なし」と言わない", () => {
    // 打ち切り・検閲による拒否・形式不備・中止。どれも0件として画面に届く
    for (const warning of [
      "1箇所で応答が途中で打ち切られました",
      "1箇所でモデルが応答を返しませんでした",
      "1箇所で、応答を指定した形式として読み取れませんでした",
      "中止しました。ここまでの結果だけを表示しています",
    ]) {
      const label = countLabel(0, { warning }, "指摘なし");
      expect(label).not.toBe("指摘なし");
      expect(label).toBe("0件(警告あり)");
      expect(fullyChecked({ warning })).toBe(false);
    }
  });

  it("末尾を見ていなければ0件でも「指摘なし」と言わない", () => {
    expect(countLabel(0, { warning: null, unchecked: 1200 }, "指摘なし")).toBe(
      "0件(末尾は未確認)",
    );
    expect(fullyChecked({ warning: null, unchecked: 1200 })).toBe(false);
  });

  it("件数があればそのまま出す(警告は別に表示する)", () => {
    expect(countLabel(3, { warning: "打ち切られました" }, "指摘なし")).toBe("3件");
  });

  it("0件の言い方は機能ごとに変えられる", () => {
    expect(countLabel(0, { warning: null }, "候補なし")).toBe("候補なし");
    expect(countLabel(0, { warning: "読み取れませんでした" }, "候補なし")).toBe(
      "0件(警告あり)",
    );
  });
});
