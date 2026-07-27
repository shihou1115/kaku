/**
 * 言及検出のフロント実装を固定する。
 *
 * Rust側(src-tauri/src/mentions.rs)と同じ leftmost-longest 規則で動く必要がある。
 * ハイライトの表示位置と、AIへ渡す設定資料の選抜が、この結果に依存している。
 */

import { describe, expect, it } from "vitest";
import { findMentions, normalizePatterns } from "./mentions";

const names = (text: string, patterns: string[]) =>
  findMentions(text, patterns).map((m) => m.name);

describe("findMentions", () => {
  it("日本語の名前を位置つきで検出する", () => {
    const got = findMentions("架純は昇降口で悠二とぶつかった。", ["架純", "悠二"]);
    expect(got).toEqual([
      { name: "架純", from: 0, to: 2 },
      { name: "悠二", from: 7, to: 9 },
    ]);
  });

  it("同じ位置では最長の名前を採る(別名で二重にしない)", () => {
    // 「佐藤架純」を「架純」に割ってしまうとハイライトが二重になる
    expect(names("佐藤架純が来た。", ["架純", "佐藤架純"])).toEqual(["佐藤架純"]);
  });

  it("パターンの並び順に結果が左右されない", () => {
    const text = "佐藤架純と架純";
    expect(names(text, ["架純", "佐藤架純"])).toEqual(names(text, ["佐藤架純", "架純"]));
  });

  it("同じ名前の繰り返しをすべて拾う", () => {
    const got = findMentions("架純。架純。架純。", ["架純"]);
    expect(got.map((m) => m.from)).toEqual([0, 3, 6]);
  });

  it("空の入力で落ちない", () => {
    expect(findMentions("", ["架純"])).toEqual([]);
    expect(findMentions("架純", [])).toEqual([]);
    expect(findMentions("架純", ["", "   "])).toEqual([]);
  });

  it("サロゲートペアを含む本文でも位置がずれない", () => {
    // 𠮟 は UTF-16 で2コードユニット。CodeMirror の位置単位に合わせる
    const got = findMentions("𠮟る架純", ["架純"]);
    expect(got).toEqual([{ name: "架純", from: 3, to: 5 }]);
  });

  it("前後の空白を落としたパターンで照合する", () => {
    expect(names("架純が来た", ["  架純  "])).toEqual(["架純"]);
  });
});

describe("normalizePatterns", () => {
  it("空を除き、長い順に並べる(最長一致の前提)", () => {
    expect(normalizePatterns(["架純", "", "佐藤架純", "  "])).toEqual([
      "佐藤架純",
      "架純",
    ]);
  });
});
