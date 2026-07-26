/**
 * 言及検出(フロント側)。
 *
 * Rust側(src-tauri/src/mentions.rs)と同じ leftmost-longest 規則を実装する。
 * 装飾の再計算は IME 変換のたびに走るため、PoC では非同期(Tauri IPC)を挟まず
 * 同期で計算する。IPC を挟むと「装飾が遅れて付く」ことが IME 不具合と
 * 見分けられなくなるため、切り分けのために意図的に同期実装を使う。
 *
 * 位置は JS 文字列の UTF-16 コードユニット単位 = CodeMirror の位置単位。
 */

export type Mention = {
  name: string;
  from: number;
  to: number;
};

/** 空行を除き、長い順に並べたパターン配列を作る */
export function normalizePatterns(raw: string[]): string[] {
  return raw
    .map((p) => p.trim())
    .filter((p) => p.length > 0)
    .sort((a, b) => b.length - a.length);
}

/**
 * text から patterns の出現箇所を返す。
 * 同じ位置に複数該当する場合は最長のものを採用し、重複ハイライトを避ける
 * (例: 正式名「佐藤架純」と別名「架純」の両方が登録されている場合)。
 */
export function findMentions(text: string, patterns: string[]): Mention[] {
  const pats = normalizePatterns(patterns);
  if (pats.length === 0 || text.length === 0) return [];

  const out: Mention[] = [];
  let i = 0;
  while (i < text.length) {
    let hit: string | null = null;
    for (const p of pats) {
      // pats は長い順なので、最初に一致したものが最長
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
