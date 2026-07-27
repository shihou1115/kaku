import { describe, expect, it } from "vitest";
import { folderDisplayPath, folderLabel, isRelabeled } from "./folderLabels";

describe("folderLabel", () => {
  it("既定フォルダーを日本語で表示する", () => {
    expect(folderLabel("manuscript", "manuscript")).toBe("原稿");
    expect(folderLabel("codex", "codex")).toBe("設定");
    expect(folderLabel("codex/characters", "characters")).toBe("人物");
    expect(folderLabel("plot", "plot")).toBe("プロット");
  });

  it("ユーザーが作ったフォルダーは実体名のまま出す", () => {
    // 好きな名前を日本語で付けられるので、置き換える必要がない
    expect(folderLabel("codex/characters/主要", "主要")).toBe("主要");
    expect(folderLabel("manuscript/01-第一章", "01-第一章")).toBe("01-第一章");
  });

  it("同名でも階層が違えば置き換えない", () => {
    // 既定の codex/characters だけが「人物」。深い階層の同名は触らない
    expect(folderLabel("codex/characters/characters", "characters")).toBe(
      "characters",
    );
  });

  it("置き換えたかどうかを判別できる(実パス併記の判断に使う)", () => {
    expect(isRelabeled("codex/characters")).toBe(true);
    expect(isRelabeled("codex/characters/主要")).toBe(false);
  });
});

describe("folderDisplayPath", () => {
  it("既定フォルダーの部分だけ置き換える", () => {
    expect(folderDisplayPath("codex/characters")).toBe("設定 / 人物");
    expect(folderDisplayPath("codex/characters/主要")).toBe("設定 / 人物 / 主要");
    expect(folderDisplayPath("manuscript")).toBe("原稿");
  });

  it("未知のパスはそのまま連結する", () => {
    expect(folderDisplayPath("foo/bar")).toBe("foo / bar");
  });

  it("空文字や余分な区切りで壊れない", () => {
    expect(folderDisplayPath("")).toBe("");
    expect(folderDisplayPath("codex//characters")).toBe("設定 / 人物");
  });
});
