/**
 * lint は最小構成。**入れた理由は react-hooks の依存チェック1点**である。
 *
 * このプロジェクトが実際に踏んだ事故(2026-08-04)は
 * 「親がインラインで渡したコールバックを子が useEffect の依存に入れ、
 *  呼ぶ→再描画→別の関数になる→また呼ぶ、でAIへリクエストを撃ち続ける」だった。
 * exhaustive-deps はまさにこの形を指す。
 *
 * 型の誤りは CI の `tsc` が既に見ているので、eslint に型系ルールは持たせない
 * (依存を増やす割に重複する)。整形も入れない — 一度も通したことがない
 * `cargo fmt` / prettier を今かけると、変更の中身が差分に埋もれる。
 */

import parser from "@typescript-eslint/parser";
import reactHooks from "eslint-plugin-react-hooks";

export default [
  {
    files: ["src/**/*.ts", "src/**/*.tsx"],
    languageOptions: {
      parser,
      parserOptions: {
        ecmaVersion: "latest",
        sourceType: "module",
        ecmaFeatures: { jsx: true },
      },
    },
    plugins: { "react-hooks": reactHooks },
    rules: {
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
    },
  },
];
