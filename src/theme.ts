/**
 * 配色テーマの解決と適用(docs/05-roadmap.md §5.15)。
 *
 * ユーザーが選ぶのは **ライト / ダーク / デバイス設定依存** の3つ。
 * 「デバイス設定依存」は JS 側で `prefers-color-scheme` を見て light/dark へ
 * 解決し、`<html data-theme="...">` に落とす。
 *
 * **CSSに書くのは light と dark の2つだけで済む。** メディアクエリで
 * ダークの定義を二重に書くと、値を直すときに片方だけ直す事故が起きる。
 */

export type ThemeChoice = "light" | "dark" | "device";
export type ResolvedTheme = "light" | "dark";

const QUERY = "(prefers-color-scheme: dark)";

/** 選択とOSの設定から、実際に当てるテーマを決める */
export function resolveTheme(choice: ThemeChoice): ResolvedTheme {
  if (choice !== "device") return choice;
  return typeof window !== "undefined" && window.matchMedia?.(QUERY).matches
    ? "dark"
    : "light";
}

/**
 * 実際にDOMへ当てる。
 *
 * `color-scheme` も一緒に指定する。これが無いとスクロールバーや
 * ネイティブの入力部品が明るいままで、そこだけ浮く。
 */
export function applyTheme(choice: ThemeChoice): ResolvedTheme {
  const resolved = resolveTheme(choice);
  const root = document.documentElement;
  root.dataset.theme = resolved;
  root.style.colorScheme = resolved;
  return resolved;
}

/**
 * 「デバイス設定依存」のときだけ、OSの切り替えに追従する。
 *
 * 戻り値は購読解除。ライト/ダークを明示している間は購読しない
 * (明示した選択をOSの都合で上書きしない)。
 */
export function watchDeviceTheme(
  choice: ThemeChoice,
  onChange: (resolved: ResolvedTheme) => void,
): () => void {
  if (choice !== "device" || typeof window === "undefined" || !window.matchMedia) {
    return () => {};
  }
  const mq = window.matchMedia(QUERY);
  const handler = () => onChange(applyTheme("device"));
  mq.addEventListener("change", handler);
  return () => mq.removeEventListener("change", handler);
}
