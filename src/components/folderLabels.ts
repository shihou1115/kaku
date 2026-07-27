/**
 * 既定フォルダーの表示名(docs/05-roadmap.md §5.8 案A)。
 *
 * 実体のフォルダー名は英語のまま変えない。UIでの表示だけを日本語にする。
 * - 目的(日本語で書いている最中に英語を読ませない)は表示で達成できる
 * - 実体を触らないので、あとから「実体ごと日本語化」へ移る道を塞がない
 * - ディスク上の名前は隠さない。ツールチップには実パスを出す
 *
 * 対応表はプロジェクト直下の既定フォルダーにだけ効かせる。
 * ユーザーが自分で掘ったサブフォルダーは、そのままの名前で出す
 * (もともと好きな名前を日本語で付けられるため)。
 */

const LABELS: Record<string, string> = {
  manuscript: "原稿",
  codex: "設定",
  "codex/characters": "人物",
  "codex/locations": "場所",
  "codex/items": "物品",
  "codex/terms": "用語",
  "codex/notes": "メモ",
  plot: "プロット",
  ideas: "着想",
  reviews: "講評",
  exports: "出力",
};

/**
 * フォルダーの表示名を返す。対応表に無ければ実体名をそのまま返す。
 *
 * @param path プロジェクトルートからの相対パス(区切りは `/`)
 * @param name 実体のフォルダー名
 */
export function folderLabel(path: string, name: string): string {
  return LABELS[path] ?? name;
}

/** 表示名が実体名と異なるか(異なるときだけ実体名を併記する) */
export function isRelabeled(path: string): boolean {
  return path in LABELS;
}

/**
 * フォルダーのパスを表示用にする。既定フォルダーの部分だけ日本語に置き換える。
 * 例: `codex/characters/主要` → `設定 / 人物 / 主要`
 */
export function folderDisplayPath(path: string): string {
  const segs = path.split("/").filter(Boolean);
  const out: string[] = [];
  let prefix = "";
  for (const seg of segs) {
    prefix = prefix ? `${prefix}/${seg}` : seg;
    out.push(LABELS[prefix] ?? seg);
  }
  return out.join(" / ");
}
