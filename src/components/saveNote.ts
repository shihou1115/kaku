/**
 * AIの出力を正本へ残すときの共通部品(M-03の `ideas/` / M-05の `reviews/`)。
 *
 * 方針(03-data-format.md §4.3): **AIの出力も人間可読な資産として残す**。
 * どちらも普通のMarkdownなので、あとから読み返せるし手で直せる。
 *
 * **自動保存はしない**(未決-5 / §5.7 A案)。相談も講評も、残すと決めたものだけを
 * 正本へ置く。全部自動で置くと採用しなかったものまで正本を汚す。
 *
 * ここで共通化するのは「保存先のパスの作り方」だけ。中身の組み立ては
 * 相談と講評で別物なので別々に持つ(`ideaNote.ts` / `reviewNote.ts`。同じコードが
 * 2回以上必要になってから共通部を抽出する、の原則どおり)。
 */

/**
 * 先頭から n 文字(**コードポイント単位**)。`slice` は UTF-16 の単位で切るので、
 * 𠮷や絵文字の途中で切ると片割れのサロゲートが残り、保存の呼び出しが壊れる
 * (テスト計画 E4)
 */
export function clip(s: string, n: number): string {
  return Array.from(s).slice(0, n).join("");
}

/** ファイル名に使えない文字(制御文字を含む)を落とす。長すぎる件名も切る */
export function safeFileName(s: string): string {
  // 制御文字(タブなど)は保存の前の名前の検査で断られる。依頼の書き出しを件名の
  // 既定値にしているので、貼り付けた本文のタブがそのまま入りうる
  const cleaned = s.replace(/[\\/:*?"<>|]/g, "_").replace(/\p{Cc}/gu, " ");
  return clip(cleaned.trim(), 40).trim() || "無題";
}

/** `2026-07-31-1435`。ファイル名の先頭に置くと一覧が時系列に並ぶ */
export function stamp(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}-${p(
    d.getHours(),
  )}${p(d.getMinutes())}`;
}

/** `ideas/2026-07-31-1435-件名.md` の形 */
export function notePath(dir: string, name: string, d: Date): string {
  return `${dir}/${stamp(d)}-${safeFileName(name)}.md`;
}

/**
 * フロントマターを組み立てる。**空の値は書かない**(読むときのノイズになる)。
 *
 * V1がアプリとして解釈するのは type/title/aliases/description の4つだけで
 * (03 §4.1)、ここで足す created/model/source は**人が読むための記録**である。
 * 未知フィールドは無改変で保持されるので、あとから増やしても壊れない。
 */
export function frontmatter(fields: Record<string, string | undefined>): string {
  const lines = Object.entries(fields)
    .filter(([, v]) => v !== undefined && v.trim() !== "")
    .map(([k, v]) => `${k}: ${yamlValue(v!)}`);
  return ["---", ...lines, "---", ""].join("\n");
}

/**
 * YAMLの値として安全な形にする。
 *
 * 件名は人が入力するので、コロンや先頭の記号が入りうる。そのまま書くと
 * 「架純: 展開案」のように**キーと値の区切りが二重になって**構造が壊れる。
 * 本アプリのパーサは寛容なので読めてしまうが、**残したファイルは他のツールでも
 * 開かれる**(P-5)ため、必要なときだけ引用符で囲む。
 */
function yamlValue(raw: string): string {
  // 改行は1行に潰す(複数行のスカラーは書かない)
  const v = raw.replace(/\r?\n/g, " ").trim();
  const needsQuote = /[:#]/.test(v) || /^[-?[\]{}&*!|>'"%@`]/.test(v);
  if (!needsQuote) return v;
  // **単一引用符を使う**。二重引用符だとバックスラッシュのエスケープが要るが、
  // 読む側(frontmatter.rs の unquote)はエスケープを解かないので
  // 「\"引用\"」がそのまま表示されてしまう。単一引用符なら囲みを外すだけで元に戻る
  return `'${v.replace(/'/g, "''")}'`;
}
