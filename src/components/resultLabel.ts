/**
 * AIの結果の件数をどう見せるか(校正・レビュー・抽出で共通)。
 *
 * **「指摘なし」と言えるのは、送った本文を全部読めたときだけ。**
 * 打ち切り・拒否・形式不備・失敗・中止・未確認の末尾が残っているのに「指摘なし」と出すと、
 * 校正なら「誤りが無い」、レビューなら「問題が無い」と信じさせる。このアプリで一番まずい
 * 誤報告で、Rust 側では区別して警告を返していたのに、画面の件数表示と
 * 「見つかりませんでした」の一文がそれを捨てていた(2026-10-04)。
 */

export type Coverage = {
  /** 打ち切り・拒否・形式不備・失敗・中止の断り書き(Rust 側の warning) */
  warning: string | null;
  /** 長すぎて見ていない末尾の字数 */
  unchecked?: number;
};

/** 0件のとき、全部を確かめたうえでの0件と言ってよいか */
export function fullyChecked(c: Coverage): boolean {
  return !c.warning && (c.unchecked ?? 0) === 0;
}

/** 件数の見出し。`none` は全部を確かめて0件だったときの言い方 */
export function countLabel(count: number, c: Coverage, none: string): string {
  if (count > 0) return `${count}件`;
  if (c.warning) return "0件(警告あり)";
  if ((c.unchecked ?? 0) > 0) return "0件(末尾は未確認)";
  return none;
}
