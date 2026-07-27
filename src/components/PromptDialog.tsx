/** 名前の入力を受け取る小さなダイアログ(改名・フォルダ作成用)。 */

import { useState } from "react";

type Props = {
  title: string;
  label: string;
  initial: string;
  /** 拡張子を選択範囲から外す(改名時に .md を消しにくくする) */
  selectStem?: boolean;
  onSubmit: (value: string) => void;
  onCancel: () => void;
};

export function PromptDialog({
  title,
  label,
  initial,
  selectStem,
  onSubmit,
  onCancel,
}: Props) {
  const [value, setValue] = useState(initial);

  const submit = () => {
    const v = value.trim();
    if (v) onSubmit(v);
  };

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>{title}</h2>
        <label className="field">
          {label}
          <input
            autoFocus
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onFocus={(e) => {
              if (selectStem) {
                const dot = e.target.value.lastIndexOf(".");
                e.target.setSelectionRange(0, dot > 0 ? dot : e.target.value.length);
              } else {
                e.target.select();
              }
            }}
            onKeyDown={(e) => {
              // IME変換中のEnterで確定させない
              if (e.key === "Enter" && !e.nativeEvent.isComposing) submit();
              if (e.key === "Escape") onCancel();
            }}
          />
        </label>
        <div className="modal-actions">
          <span className="spacer" />
          <button onClick={onCancel}>キャンセル</button>
          <button className="primary" onClick={submit}>
            決定
          </button>
        </div>
      </div>
    </div>
  );
}
