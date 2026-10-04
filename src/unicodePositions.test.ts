/**
 * テスト計画 A1(docs/07-test-plan.md)のフロント側。
 *
 * 位置を返す関数に、数え方が食い違いやすい文字を混ぜて通し、返った位置で切り出した
 * 文字列が期待どおりかを見る。位置はエディタ(CodeMirror)と同じ UTF-16 の単位。
 * Rust 側は src-tauri/tests/unicode_positions.rs。
 */

import { describe, expect, it } from "vitest";
import { findMentions } from "./editor/mentions";
import { canWrap, marks, strip } from "./ruby";

const TRICKY = [
  "𠮷", // サロゲートペア
  "葛\u{E0100}", // 異体字セレクタ
  "か\u{309A}", // 結合文字
  "👨‍👩‍👧", // ZWJ でつないだ絵文字
  "\t",
  "　",
  "é",
];

function bodies(target: string): string[] {
  const out: string[] = [];
  for (const t of TRICKY) {
    out.push(`${t}。${target}は来た。`);
    out.push(`${t}${t}\n${t}本文の途中。${target}がいる。${t}`);
    out.push(`前置き${t}。\n\n${t}、${target}。${t}`);
  }
  out.push(`${TRICKY.join("")}。${target}は来た。`);
  return out;
}

describe("A1: 位置を返す関数に、数え方が食い違う文字を混ぜる", () => {
  it("言及の位置で切り出すと、名前になる", () => {
    for (const body of bodies("佐藤架純")) {
      const hits = findMentions(body, ["佐藤架純", "架純"]);
      expect(hits, body).toHaveLength(1);
      expect(body.slice(hits[0].from, hits[0].to), body).toBe("佐藤架純");
    }
  });

  it("ルビの位置で切り出すと、記法全体になる", () => {
    for (const t of TRICKY) {
      for (const body of [
        `${t}前置き｜漢字《かんじ》${t}`,
        `｜${t}田《よしだ》のあと`,
        `｜吉田《よし${t}だ》のあと`,
      ]) {
        const m = marks(body);
        expect(m, body).toHaveLength(1);
        expect(body.slice(m[0].start, m[0].end).startsWith("｜"), body).toBe(true);
        expect(body.slice(m[0].start, m[0].end).endsWith("》"), body).toBe(true);
      }
    }
  });

  it("ルビの重なり判定が、選んだ範囲の位置どおりに効く", () => {
    for (const t of TRICKY) {
      const body = `${t}${t}｜漢字《かんじ》のあと${t}`;
      const inside = body.indexOf("漢字");
      expect(canWrap(body, inside, inside + 2), body).toBe(false);
      const after = body.indexOf("のあと");
      expect(canWrap(body, after, after + 3), body).toBe(true);
    }
  });

  it("文字数のために記法を外すと、親文字だけが残る", () => {
    for (const t of TRICKY) {
      expect(strip(`${t}｜漢字《かんじ》${t}`)).toBe(`${t}漢字${t}`);
      expect(strip(`｜${t}田《よしだ》`)).toBe(`${t}田`);
    }
  });
});
