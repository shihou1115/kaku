/**
 * ルビの見え方を確認する画面(03-data-format 未決-3)。
 *
 * **エディタ本体には手を入れない。** 執筆中の本文にルビを乗せると
 * CodeMirror の装飾とIMEが相互作用し、PoC#1 で確認したリスク領域に入る
 * (04-design §3.2 が「相応の実装コスト」と警告している)。
 * 確認は書いている途中ではなく「投稿前に見る」用途なので、別画面で足りる。
 *
 * 将来の縦書きプレビュー(S-09 / PoC#6)も、置き場所はここになる想定。
 */

import { segments, strip } from "../ruby";

type Props = {
  title: string;
  text: string;
  onClose: () => void;
};

export function RubyPreview({ title, text, onClose }: Props) {
  const segs = segments(text);
  const rubyCount = segs.filter((s) => s.kind === "ruby").length;
  const plainChars = [...strip(text).replace(/\s/g, "")].length;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal preview-modal"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label="プレビュー"
      >
        <div className="help-head">
          <h2>プレビュー{title && ` — ${title}`}</h2>
          <button onClick={onClose}>閉じる</button>
        </div>

        <p className="pf-meta preview-meta">
          {/* 記法込みの字数ではなく**読者が見る字数**を出す。投稿サイトの制限はこちら */}
          {plainChars.toLocaleString()}字(記法を除く)
          {rubyCount > 0 && ` / ルビ${rubyCount}件`}
        </p>

        <div className="preview-body">
          {text.trim() ? (
            segs.map((s, i) =>
              s.kind === "text" ? (
                <span key={i}>{s.text}</span>
              ) : (
                <ruby key={i}>
                  {s.base}
                  <rt>{s.reading}</rt>
                </ruby>
              ),
            )
          ) : (
            <p className="empty">本文がありません。</p>
          )}
        </div>

        {rubyCount === 0 && text.trim() && (
          <p className="hint preview-hint">
            ルビはまだありません。本文で語を選んでヘッダーの「ルビ」を押すと、
            <code>｜漢字《かんじ》</code> の形で入ります。
            この記法はカクヨム・なろう・青空文庫でそのまま通ります。
          </p>
        )}
      </div>
    </div>
  );
}
