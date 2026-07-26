/**
 * 言及検出(フロント側)。
 *
 * Rust側(src-tauri/src/mentions.rs)と同じ leftmost-longest 規則。
 * 装飾は入力のたびに再計算するため、IPCを挟まず同期で計算する。
 * PoC#1 で 311 件・12,000字規模でも Rust と結果が一致することを確認済み。
 *
 * 位置は JS 文字列の UTF-16 コードユニット単位 = CodeMirror の位置単位。
 */

export type Mention = {
  name: string;
  from: number;
  to: number;
};

export function normalizePatterns(raw: string[]): string[] {
  return raw
    .map((p) => p.trim())
    .filter((p) => p.length > 0)
    .sort((a, b) => b.length - a.length);
}

/**
 * 同じ位置に複数該当する場合は最長のものを採用する
 * (正式名「佐藤架純」と別名「架純」が両方登録されていても二重にしない)。
 */
export function findMentions(text: string, patterns: string[]): Mention[] {
  const pats = normalizePatterns(patterns);
  if (pats.length === 0 || text.length === 0) return [];

  const out: Mention[] = [];
  let i = 0;
  while (i < text.length) {
    let hit: string | null = null;
    for (const p of pats) {
      if (text.startsWith(p, i)) {
        hit = p;
        break;
      }
    }
    if (hit) {
      out.push({ name: hit, from: i, to: i + hit.length });
      i += hit.length;
    } else {
      i += 1;
    }
  }
  return out;
}
