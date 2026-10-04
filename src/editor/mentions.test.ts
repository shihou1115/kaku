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

/** 速くする前の素朴な形(位置ごとに、長い順の名前をすべて試す)。結果の正解として使う */
function reference(text: string, patterns: string[]) {
  const pats = normalizePatterns(patterns);
  const out: { name: string; from: number; to: number }[] = [];
  if (pats.length === 0) return out;
  let i = 0;
  while (i < text.length) {
    const hit = pats.find((p) => text.startsWith(p, i));
    if (hit) {
      out.push({ name: hit, from: i, to: i + hit.length });
      i += hit.length;
    } else {
      i += 1;
    }
  }
  return out;
}

describe("findMentions の速い形(テスト計画 F1)", () => {
  // 10万字・名前200個で1打鍵ごとに約30ms(エディタと App で2回)かかっていたので、
  // 名前を先頭の1単位で分けて引くようにした。**結果は素朴な形と同じであること**
  it("乱数の本文と名前で、素朴な形と同じ結果を返す", () => {
    const pieces = ["架", "純", "佐", "藤", "𠮷", "あ", " "];
    // xorshift32(掛け算の LCG は JS の数の精度を超えて、下の桁が死ぬ)
    let seed = 20261005;
    const next = (n: number) => {
      seed ^= seed << 13;
      seed >>>= 0;
      seed ^= seed >>> 17;
      seed ^= seed << 5;
      seed >>>= 0;
      return seed % n;
    };
    const word = (len: number) =>
      Array.from({ length: len }, () => pieces[next(pieces.length)]).join("");
    let hits = 0;
    for (let round = 0; round < 2000; round++) {
      const text = word(next(40));
      // 名前は本文の一部を切り出して作る(当たる名前・先頭が同じ名前・重なる名前が出る)
      const chars = Array.from(text);
      const patterns = Array.from({ length: next(6) }, () => {
        const at = next(chars.length + 1);
        return chars.slice(at, at + 1 + next(3)).join("") || word(1);
      });
      const got = findMentions(text, patterns);
      expect(got).toEqual(reference(text, patterns));
      hits += got.length;
    }
    expect(hits).toBeGreaterThan(1000); // 当たりの無い入力ばかりでは比べたことにならない
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
