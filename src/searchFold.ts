/**
 * 検索の照合用に小文字へ寄せる(テスト計画 D3)。
 *
 * Rust 側の `search::fold`(src-tauri/src/search.rs)と**同じ規則**: 1文字ずつ小文字にし、
 * 1文字に寄せられないもの・UTF-16 の長さが変わるものはそのまま残す。
 * 1文字ずつ寄せるので**位置が変わらない**。寄せた本文で見つけた位置を、
 * そのまま元の本文(エディタ)の位置として使える。
 *
 * 検索の索引(FTS5 trigram)は大文字小文字を区別しない(全角の英字も)。
 * 以前は開いた先で `indexOf` の完全一致で探していたため、「hello」で
 * 「Hello」のファイルを開いても一致箇所へ飛ばなかった。
 */
export function foldForSearch(s: string): string {
  let out = "";
  for (const ch of s) {
    const lower = ch.toLowerCase();
    out += [...lower].length === 1 && lower.length === ch.length ? lower : ch;
  }
  return out;
}

/** 本文の中で、検索語が最初に出る位置(UTF-16)。無ければ -1 */
export function findForSearch(text: string, needle: string): number {
  if (!needle) return -1;
  return foldForSearch(text).indexOf(foldForSearch(needle));
}
