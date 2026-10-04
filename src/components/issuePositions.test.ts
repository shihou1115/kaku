import { describe, expect, it } from "vitest";
import { shiftAfterReplace, type Positioned } from "./issuePositions";

function at(body: string, quote: string, nth = 0): Positioned {
  let pos = -1;
  for (let k = 0; k <= nth; k++) pos = body.indexOf(quote, pos + 1);
  return { quote, found: true, start_utf16: pos, end_utf16: pos + quote.length };
}

function replace(body: string, i: Positioned, suggestion: string) {
  const s = i.start_utf16!;
  const e = i.end_utf16!;
  return { newBody: body.slice(0, s) + suggestion + body.slice(e), s, e };
}

describe("置換したあとの残りの指摘の位置(テスト計画 A4)", () => {
  it("後ろの指摘は、長さの差だけ動き、同じ文字列を指したまま", () => {
    const body = "雨だつた。翌日は晴れだつた。";
    const first = at(body, "雨だつた");
    const second = at(body, "晴れだつた");
    const { newBody, s, e } = replace(body, first, "雨だった");
    const [got] = shiftAfterReplace([second], s, e, "雨だった".length, newBody);
    expect(newBody.slice(got.start_utf16!, got.end_utf16!)).toBe("晴れだつた");
  });

  it("同じ文字列が前にあっても、そちらへ飛ばない", () => {
    // 1つ目の「以外と」は正しい用法。誤りは2つ目。先頭から探し直すと1つ目を指してしまう
    const body = "それ以外と比べて。誤字の前に置換する所あり。試験は以外と簡単だった。";
    const fix = at(body, "置換する所");
    const target = at(body, "以外と", 1);
    const { newBody, s, e } = replace(body, fix, "置き換える所");
    const [got] = shiftAfterReplace([target], s, e, "置き換える所".length, newBody);
    expect(got.found).toBe(true);
    expect(newBody.slice(got.end_utf16!, got.end_utf16! + 2)).toBe("簡単");
  });

  it("前にある指摘は動かない", () => {
    const body = "前の誤字。後ろの誤字。";
    const front = at(body, "前の誤字");
    const back = at(body, "後ろの誤字");
    const { newBody, s, e } = replace(body, back, "後ろの語");
    const [got] = shiftAfterReplace([front], s, e, "後ろの語".length, newBody);
    expect(got).toEqual(front);
  });

  it("置換した範囲と重なる指摘は「見つからない」にする", () => {
    const body = "佐藤佳純は来た。";
    const whole = at(body, "佐藤佳純");
    const part = at(body, "佳純は");
    const { newBody, s, e } = replace(body, whole, "佐藤架純");
    const [got] = shiftAfterReplace([part], s, e, "佐藤架純".length, newBody);
    expect(got.found).toBe(false);
    expect(got.start_utf16).toBeNull();
  });

  it("動かした先が引用と違えば「見つからない」にする", () => {
    const body = "一。二。三。";
    const two = at(body, "二");
    // 想定外の編集(長さを偽って渡す)
    const [got] = shiftAfterReplace([two], 0, 1, 5, "いちいち。二。三。");
    expect(got.found).toBe(false);
  });
});
