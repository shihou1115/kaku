/**
 * 外部編集との競合(T-08 / 03-data-format.md §5)。
 *
 * 文書が約束しているのは「**別名で保存**」か「**破棄して再読み込み**」の二択で、
 * 自動マージはしない。ここはその二択を出すためだけの窓である。
 *
 * 「あとで決める」も残す。決めるまでアプリ側の本文には手を触れない
 * (勝手に上書きも破棄もしない=D-5)。**閉じても行き止まりにはしない。**
 * 保存・切替・終了などを試みるたびに、この窓へ戻ってくる(saveFlow.shouldPrompt)。
 */

type Props = {
  path: string;
  onSaveAs: () => void;
  onDiscard: () => void;
  onLater: () => void;
};

export function ConflictDialog({ path, onSaveAs, onDiscard, onLater }: Props) {
  return (
    <div className="modal-backdrop" onClick={onLater}>
      <div
        className="modal confirm"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label="外部編集との競合"
      >
        <h2>アプリの外で変更されています</h2>
        <p className="confirm-message">
          <code>{path}</code> は、アプリの外(エディタ・git・同期ソフトなど)で
          書き換えられました。アプリ側にも保存していない変更があります。
        </p>
        <p className="hint">
          どちらを残すかはあなたが決めてください。<strong>自動では混ぜません。</strong>
          決めるまで、アプリ側の本文はそのまま残ります(保存はしません)。
          「あとで決める」を選んでも、ヘッダーの「競合」からここへ戻れます。
        </p>
        <div className="modal-actions">
          <button onClick={onLater}>あとで決める</button>
          <span className="spacer" />
          <button className="danger-btn" onClick={onDiscard}>
            破棄して再読み込み
          </button>
          <button className="primary" onClick={onSaveAs}>
            別名で保存
          </button>
        </div>
      </div>
    </div>
  );
}
