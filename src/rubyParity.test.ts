/**
 * テスト計画 A6(docs/07-test-plan.md): ルビ記法の崩れた形を、TS 版が Rust 版
 * (src-tauri/src/ruby.rs)と同じに読むか。
 *
 * 同じ規則を2か所で実装しているので、**共通の入力表**を両方のテストから読む。
 * 表と期待値(Rust 版の結果)は src-tauri/tests/ruby_parity.rs が作る。
 */

import { describe, expect, it } from "vitest";
import table from "../src-tauri/tests/data/ruby_parity.json";
import { marks, strip } from "./ruby";

describe("A6: ルビ記法は Rust 版と同じに読む", () => {
  it("共通の入力表のすべての本文で、記法の位置と字数用の本文が一致する", () => {
    expect(table.cases.length).toBeGreaterThan(400);
    const mismatches: string[] = [];
    for (const c of table.cases) {
      const got = marks(c.text);
      const stripped = strip(c.text);
      if (JSON.stringify(got) !== JSON.stringify(c.marks) || stripped !== c.stripped) {
        mismatches.push(
          `本文 ${JSON.stringify(c.text)}\n` +
            `  Rust ${JSON.stringify(c.marks)} / ${JSON.stringify(c.stripped)}\n` +
            `  TS   ${JSON.stringify(got)} / ${JSON.stringify(stripped)}`,
        );
      }
    }
    expect(mismatches, mismatches.slice(0, 5).join("\n")).toEqual([]);
  });
});
