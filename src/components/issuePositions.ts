/**
 * 指摘を1件「置換」したあとの、残りの指摘の位置(校正タブ)。
 *
 * **探し直さずに、置換でずれた分だけ動かす。** 以前は残りの引用を本文の先頭から
 * 探し直していたため、同じ文字列が前にもあるとそちらへ飛び、次の「置換」が
 * 別の箇所(正しい用法のことが多い)を書き換えた(テスト計画 A4)。
 *
 * - 置換した範囲より前の指摘はそのまま、後ろの指摘は長さの差だけ動かす
 * - 置換した範囲と重なる指摘は、もう同じ箇所を指せないので「見つからない」にする
 * - 動かした先の文字列が引用と違えば(想定していない編集)、同じく「見つからない」にする
 */

export type Positioned = {
  quote: string;
  found: boolean;
  start_utf16: number | null;
  end_utf16: number | null;
};

export function shiftAfterReplace<T extends Positioned>(
  issues: T[],
  from: number,
  to: number,
  insertedLength: number,
  newBody: string,
): T[] {
  const delta = insertedLength - (to - from);
  return issues.map((i) => {
    if (!i.found || i.start_utf16 === null || i.end_utf16 === null) return i;
    let start = i.start_utf16;
    let end = i.end_utf16;
    if (end <= from) {
      // 前にある。動かさない
    } else if (start >= to) {
      start += delta;
      end += delta;
    } else {
      return { ...i, found: false, start_utf16: null, end_utf16: null };
    }
    if (newBody.slice(start, end) !== i.quote) {
      return { ...i, found: false, start_utf16: null, end_utf16: null };
    }
    return { ...i, start_utf16: start, end_utf16: end };
  });
}
