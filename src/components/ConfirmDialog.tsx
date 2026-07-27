/** 破壊的に見える操作の前に挟む確認ダイアログ。 */

import { useEffect, useRef } from "react";

type Props = {
  title: string;
  message: string;
  /** 補足(復旧方法など)。太字にせず落ち着いて伝える */
  note?: string;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
};

export function ConfirmDialog({
  title,
  message,
  note,
  confirmLabel,
  danger,
  onConfirm,
  onCancel,
}: Props) {
  const okRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    // 既定のフォーカスは「キャンセル」寄りにしたいので、Enterでの即決は避ける
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal confirm" onClick={(e) => e.stopPropagation()}>
        <h2>{title}</h2>
        <p className="confirm-message">{message}</p>
        {note && <p className="hint">{note}</p>}
        <div className="modal-actions">
          <span className="spacer" />
          <button autoFocus onClick={onCancel}>
            キャンセル
          </button>
          <button
            ref={okRef}
            className={danger ? "danger-btn" : "primary"}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
