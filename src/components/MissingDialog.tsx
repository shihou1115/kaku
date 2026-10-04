/**
 * 開いているファイルが、アプリの外で削除された(名前が変わった)とき(テスト計画 G3)。
 *
 * 以前は次の自動保存で**黙って作り直していた**(削除が取り消され、名前を変えた場合は
 * 元の名前で二重になった)。作り直すか閉じるかを本人が決める。
 * 決めるまでは書かない。「あとで決める」を選んでも、ヘッダーの「削除済み」から戻れる
 * (外部編集の競合と同じ流れ。ConflictDialog)。
 */

type Props = {
  path: string;
  onRecreate: () => void;
  onClose: () => void;
  onLater: () => void;
};

export function MissingDialog({ path, onRecreate, onClose, onLater }: Props) {
  return (
    <div className="modal-backdrop" onClick={onLater}>
      <div
        className="modal confirm"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label="アプリの外で削除されたファイル"
      >
        <h2>アプリの外で削除されています</h2>
        <p className="confirm-message">
          <code>{path}</code> は、アプリの外(エクスプローラー・同期ソフト・git など)で
          削除されたか、名前が変わりました。アプリ側には保存していない本文があります。
        </p>
        <p className="hint">
          作り直すと、いまの本文でこの場所にファイルを作ります。閉じると、アプリ側の本文は捨てます
          (外で名前を変えただけなら、そちらに元の内容が残っています)。
          決めるまで保存はしません。「あとで決める」を選んでも、ヘッダーの「削除済み」からここへ戻れます。
        </p>
        <div className="modal-actions">
          <button onClick={onLater}>あとで決める</button>
          <span className="spacer" />
          <button className="danger-btn" onClick={onClose}>
            閉じる(本文を捨てる)
          </button>
          <button className="primary" onClick={onRecreate}>
            この内容で作り直す
          </button>
        </div>
      </div>
    </div>
  );
}
