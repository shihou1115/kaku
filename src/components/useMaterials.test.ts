import { describe, expect, it } from "vitest";
import { resolveMaterials } from "./useMaterials";

const A = "codex/characters/架純.md";
const B = "codex/characters/悠二.md";
const C = "codex/locations/青葉高校.md";

describe("渡す資料の選択(U-05 / 04-design §6.2)", () => {
  it("既定では本文で当たった資料をすべて渡す", () => {
    const got = resolveMaterials([A, B], [], []);
    expect(got.autoPaths).toEqual([A, B]);
    expect(got.manualPaths).toEqual([]);
  });

  it("外した資料は渡さない", () => {
    expect(resolveMaterials([A, B], [A], []).autoPaths).toEqual([B]);
  });

  it("外していない資料は本文が変わって新しく当たっても渡る", () => {
    // 「選択リスト」ではなく「除外リスト」を持つ理由。既定ONの意味が保たれる
    expect(resolveMaterials([A, B, C], [A], []).autoPaths).toEqual([B, C]);
  });

  it("当たらなくなった資料の除外は無害", () => {
    // 本文からAの名前が消えた。除外は残るが結果に影響しない
    expect(resolveMaterials([B], [A], []).autoPaths).toEqual([B]);
  });

  it("手動追加は自動と重ならない", () => {
    // 手動で足したあとに本文へ名前を書いた場合。二重に渡さない
    const got = resolveMaterials([A], [], [A, C]);
    expect(got.autoPaths).toEqual([A]);
    expect(got.manualPaths).toEqual([C]);
  });

  it("自動で外した資料が手動側から復活しない", () => {
    // **ここが壊れると「外したのに渡る」という一番まずい形になる**
    const got = resolveMaterials([A], [A], [A]);
    expect(got.autoPaths).toEqual([]);
    expect(got.manualPaths).toEqual([]);
  });

  it("本文から名前が消えたら手動追加として戻る", () => {
    const got = resolveMaterials([], [A], [A]);
    expect(got.autoPaths).toEqual([]);
    expect(got.manualPaths).toEqual([A]);
  });

  it("すべて外した状態を表せる", () => {
    expect(resolveMaterials([A, B], [A, B], []).autoPaths).toEqual([]);
  });
});
