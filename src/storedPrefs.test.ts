import { describe, expect, it } from "vitest";
import { DEFAULT_VIEW, parseRightTab, parseView } from "./storedPrefs";

describe("覚えておいた画面の設定を読む(テスト計画 F2)", () => {
  it("右ペインのタブは、知っている値だけを使う(知らない値で右ペインを空にしない)", () => {
    expect(parseRightTab("review")).toBe("review");
    expect(parseRightTab("settings")).toBe("ai");
    expect(parseRightTab("")).toBe("ai");
    expect(parseRightTab(null)).toBe("ai");
  });

  it("表示の設定は、壊れた JSON なら既定に戻す", () => {
    expect(parseView("{壊れ")).toEqual(DEFAULT_VIEW);
    expect(parseView(null)).toEqual(DEFAULT_VIEW);
    expect(parseView("null")).toEqual(DEFAULT_VIEW);
    expect(parseView("[1,2]")).toEqual(DEFAULT_VIEW);
    expect(parseView("123")).toEqual(DEFAULT_VIEW);
  });

  it("使えない値は、その項目だけ既定に戻す(ほかの項目は残す)", () => {
    const got = parseView(
      JSON.stringify({ showLineNumbers: "yes", showRuler: true, wrapColumns: -5, theme: "blue" }),
    );
    expect(got).toEqual({ ...DEFAULT_VIEW, showRuler: true });
  });

  it("折り返し字数は表示メニューの範囲(10〜200の整数)だけを受け付ける", () => {
    const wrap = (w: unknown) => parseView(JSON.stringify({ wrapColumns: w })).wrapColumns;
    expect(wrap(40)).toBe(40);
    expect(wrap(10)).toBe(10);
    expect(wrap(200)).toBe(200);
    expect(wrap(9)).toBeNull();
    expect(wrap(201)).toBeNull();
    expect(wrap(40.5)).toBeNull();
    expect(wrap("40")).toBeNull();
    expect(wrap(null)).toBeNull();
  });

  it("正しい値はそのまま使う", () => {
    const v = { showLineNumbers: false, showRuler: true, wrapColumns: 42, theme: "dark" };
    expect(parseView(JSON.stringify(v))).toEqual(v);
  });
});
