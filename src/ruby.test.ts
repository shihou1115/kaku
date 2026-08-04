import { describe, expect, it } from "vitest";
import { canWrap, hasRuby, marks, segments, strip, wrap } from "./ruby";

/**
 * Rust 側(`src-tauri/src/ruby.rs`)と**同じ規則**であることを固定する。
 * 向こうのテストと同じ入力を並べてあるので、片方だけ直すと落ちる。
 */
describe("ルビ記法(未決-3 → 2026-08-04 確定)", () => {
  it("1つのルビを読み取る", () => {
    const got = segments("　彼は｜白鏡《しろかがみ》の塔を見た。");
    expect(got).toEqual([
      { kind: "text", text: "　彼は" },
      { kind: "ruby", base: "白鏡", reading: "しろかがみ" },
      { kind: "text", text: "の塔を見た。" },
    ]);
  });

  it("1行に複数あっても読み取る", () => {
    const got = segments("｜白鏡《しろかがみ》と｜黒鏡《くろかがみ》");
    expect(got.filter((s) => s.kind === "ruby")).toHaveLength(2);
  });

  it("壊れた記法は素通しする(書きかけを壊れているとは言わない)", () => {
    for (const t of [
      "｜白鏡",
      "｜白鏡《しろかがみ",
      "｜《しろかがみ》",
      "｜白鏡《》",
      "白鏡《しろかがみ》",
      "｜白鏡\n《しろかがみ》",
    ]) {
      expect(segments(t).some((s) => s.kind === "ruby")).toBe(false);
      expect(strip(t)).toBe(t);
    }
  });

  it("半角の縦線は区切りにしない(Markdownの表と紛らわしい)", () => {
    expect(hasRuby("|白鏡《しろかがみ》")).toBe(false);
  });

  it("記法を外すと親文字だけが残る", () => {
    expect(strip("　彼は｜白鏡《しろかがみ》の塔を見た。")).toBe(
      "　彼は白鏡の塔を見た。",
    );
    expect(strip("ルビなしの本文")).toBe("ルビなしの本文");
    expect(strip("")).toBe("");
  });

  it("文字数は読者が見る字数と一致する", () => {
    // 投稿サイトの字数と合わせるための性質
    const t = "｜白鏡《しろかがみ》の塔";
    expect([...t].length).toBe(12);
    expect([...strip(t)].length).toBe(4);
  });

  it("作った記法を読み直せる", () => {
    const t = wrap("白鏡", "しろかがみ");
    expect(t).toBe("｜白鏡《しろかがみ》");
    expect(segments(t)).toEqual([
      { kind: "ruby", base: "白鏡", reading: "しろかがみ" },
    ]);
  });

  it("ルビの有無を判定する", () => {
    expect(hasRuby("｜白鏡《しろかがみ》")).toBe(true);
    expect(hasRuby("ただの本文")).toBe(false);
  });

  it("位置を返す", () => {
    const t = "あ｜漢字《かんじ》い";
    expect(marks(t)).toEqual([
      { start: 1, end: 9, base: "漢字", reading: "かんじ" },
    ]);
    expect(t.slice(1, 9)).toBe("｜漢字《かんじ》");
  });
});

/**
 * 既にルビがある場所へ重ねて振れてしまうと、記法が入れ子になって壊れる。
 * `｜白鏡《しろかがみ》` の「しろかが」に振ると
 * `｜白鏡《し｜ろかが《よみ》み》` になる — 選択範囲に記法文字が入っているかだけでは防げない。
 */
describe("ルビを振ってよい範囲か", () => {
  // 0        1
  // 0123456789...
  // あ｜漢字《かんじ》い
  const T = "あ｜漢字《かんじ》い";

  it("ルビの外なら振れる", () => {
    expect(canWrap(T, 0, 1)).toBe(true); // 「あ」
    expect(canWrap(T, 9, 10)).toBe(true); // 「い」
    expect(canWrap("ただの本文", 0, 3)).toBe(true);
  });

  it("読みの途中は振れない", () => {
    expect(canWrap(T, 5, 7)).toBe(false); // 「かん」
  });

  it("親文字の途中は振れない", () => {
    expect(canWrap(T, 2, 3)).toBe(false); // 「漢」
  });

  it("ルビ全体を選んでも振れない", () => {
    expect(canWrap(T, 1, 9)).toBe(false);
  });

  it("ルビをまたぐ選択も振れない", () => {
    // またいだまま振ると外側の記法が内側の｜で壊れる
    expect(canWrap(T, 0, 10)).toBe(false);
    expect(canWrap(T, 0, 4)).toBe(false);
    expect(canWrap(T, 6, 10)).toBe(false);
  });

  it("隣接しているだけなら振れる", () => {
    // ルビの直前・直後で切れている選択は重なっていない
    expect(canWrap(T, 0, 1)).toBe(true);
    expect(canWrap(T, 9, 10)).toBe(true);
  });

  it("選択が空なら振れない", () => {
    expect(canWrap(T, 3, 3)).toBe(false);
  });

  it("壊れた記法は障害物にならない", () => {
    // 書きかけの「｜漢字」はルビとして成立していないので、普通の文字として扱う
    expect(canWrap("｜漢字だけ", 1, 3)).toBe(true);
  });
});
