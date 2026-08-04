/**
 * ルビ記法(03-data-format.md 未決-3 → 2026-08-04 確定)。
 *
 * **記法は `｜漢字《かんじ》` の一形式だけ。** 区切りの `｜` は必須。
 * 半角 `|` は受け付けない(Markdownの表と紛らわしい)。
 *
 * Rust 側(`src-tauri/src/ruby.rs`)と**同じ規則**を実装している。
 * 言及検出(`editor/mentions.ts`)と同じ事情で、
 * 文字数の表示やプレビューは毎入力ごとに要るためIPCへ出せない。
 * **どちらかを直したら両方直すこと。** 規則は両方のテストで固定してある。
 */

export const DELIM = "｜";
export const OPEN = "《";
export const CLOSE = "》";

export type RubySegment =
  | { kind: "text"; text: string }
  | { kind: "ruby"; base: string; reading: string };

/** 本文中のルビ1つ。位置は文字列のインデックス(エディタの選択と同じ座標系) */
export type RubyMark = {
  start: number;
  end: number;
  base: string;
  reading: string;
};

/**
 * 本文からルビの位置を取り出す。
 *
 * 壊れた記法(閉じ忘れ・空・改行またぎ)は**ルビとして扱わず素通しする**。
 * 書きかけの状態でエラーにしても書き手の役に立たない。
 */
export function marks(text: string): RubyMark[] {
  const out: RubyMark[] = [];
  let i = 0;
  while (i < text.length) {
    if (text[i] !== DELIM) {
      i++;
      continue;
    }
    const open = text.indexOf(OPEN, i + 1);
    const close = open < 0 ? -1 : text.indexOf(CLOSE, open + 1);
    const base = open < 0 ? "" : text.slice(i + 1, open);
    const reading = close < 0 ? "" : text.slice(open + 1, close);
    const broken =
      open < 0 ||
      close < 0 ||
      !base ||
      !reading ||
      /[\r\n｜]/.test(base) ||
      /[\r\n《]/.test(reading);
    if (broken) {
      i++;
      continue;
    }
    out.push({ start: i, end: close + CLOSE.length, base, reading });
    i = close + CLOSE.length;
  }
  return out;
}

/** 本文をルビと素のテキストに分ける(プレビュー用) */
export function segments(text: string): RubySegment[] {
  const out: RubySegment[] = [];
  let prev = 0;
  for (const m of marks(text)) {
    if (m.start > prev) out.push({ kind: "text", text: text.slice(prev, m.start) });
    out.push({ kind: "ruby", base: m.base, reading: m.reading });
    prev = m.end;
  }
  if (prev < text.length) out.push({ kind: "text", text: text.slice(prev) });
  return out;
}

/**
 * その範囲に新しくルビを振ってよいか。
 *
 * **既にあるルビと少しでも重なっていたら振れない。** 記法の内側
 * (親文字や読みの途中)を選んで振ると、入れ子になって記法が壊れる:
 * `｜白鏡《しろかがみ》` の「しろかが」に振ると `｜白鏡《し｜ろかが《よみ》み》`。
 * 選択範囲に記法文字が入っているかだけでは、この形を防げない。
 */
export function canWrap(text: string, from: number, to: number): boolean {
  if (from >= to) return false;
  return !marks(text).some((m) => from < m.end && to > m.start);
}

/**
 * 記法を外して素の本文にする(`｜漢字《かんじ》` → `漢字`)。
 *
 * **文字数を数えるときはこれを使う。** 記法込みで数えると投稿サイトの字数と合わない。
 */
export function strip(text: string): string {
  return segments(text)
    .map((s) => (s.kind === "text" ? s.text : s.base))
    .join("");
}

/** 選択された語にルビを付けた文字列(入力補助) */
export function wrap(base: string, reading: string): string {
  return `${DELIM}${base}${OPEN}${reading}${CLOSE}`;
}

/** ルビが1つでも含まれるか(プレビューの導線を出すかの判断に使う) */
export function hasRuby(text: string): boolean {
  return segments(text).some((s) => s.kind === "ruby");
}
