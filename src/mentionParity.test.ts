/**
 * テスト計画 A3(docs/07-test-plan.md): 言及検出の TS 版が、Rust 版
 * (src-tauri/src/mentions.rs)と同じ結果を返すか。
 *
 * 同じ規則を2か所で実装しているので、**共通の入力表**を両方のテストから読む。
 * 表と期待値(Rust 版の結果)は src-tauri/tests/mention_parity.rs が作る。
 */

import { describe, expect, it } from "vitest";
import table from "../src-tauri/tests/data/mention_parity.json";
import { findMentions } from "./editor/mentions";

describe("A3: 言及検出は Rust 版と同じ結果を返す", () => {
  it("共通の入力表のすべての組で一致する", () => {
    expect(table.cases.length).toBeGreaterThan(300);
    const mismatches: string[] = [];
    for (const c of table.cases) {
      const got = findMentions(c.text, c.patterns).map((m) => ({
        name: m.name,
        from: m.from,
        to: m.to,
      }));
      if (JSON.stringify(got) !== JSON.stringify(c.expected)) {
        mismatches.push(
          `本文 ${JSON.stringify(c.text)} / 名前 ${JSON.stringify(c.patterns)}\n` +
            `  Rust ${JSON.stringify(c.expected)}\n  TS   ${JSON.stringify(got)}`,
        );
      }
    }
    expect(mismatches, mismatches.slice(0, 5).join("\n")).toEqual([]);
  });
});
