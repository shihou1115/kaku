/**
 * 表示設定のポップオーバー(行番号 / ルーラー / 折り返し)。
 *
 * ヘッダーに項目を並べると混むので、まとめてここに入れる。
 */

import { useEffect, useRef, useState } from "react";

export type ViewSettings = {
  showLineNumbers: boolean;
  showRuler: boolean;
  /** 折り返し字数(全角換算)。null は右端で折り返す */
  wrapColumns: number | null;
};

type Props = {
  value: ViewSettings;
  onChange: (v: ViewSettings) => void;
  onClose: () => void;
};

/** 日本語の原稿でよく使う字詰め */
const PRESETS = [20, 30, 40, 42, 50];

export function ViewMenu({ value, onChange, onClose }: Props) {
  const ref = useRef<HTMLDivElement | null>(null);
  const [custom, setCustom] = useState(
    value.wrapColumns && !PRESETS.includes(value.wrapColumns)
      ? String(value.wrapColumns)
      : "",
  );

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  const selectValue =
    value.wrapColumns === null
      ? "edge"
      : PRESETS.includes(value.wrapColumns)
        ? String(value.wrapColumns)
        : "custom";

  return (
    <div className="view-menu" ref={ref}>
      <label className="check">
        <input
          type="checkbox"
          checked={value.showLineNumbers}
          onChange={(e) =>
            onChange({ ...value, showLineNumbers: e.target.checked })
          }
        />
        行番号を表示
      </label>
      <label className="check">
        <input
          type="checkbox"
          checked={value.showRuler}
          onChange={(e) => onChange({ ...value, showRuler: e.target.checked })}
        />
        ルーラーを表示
      </label>

      <div className="view-sep" />

      <label className="field">
        折り返し
        <select
          value={selectValue}
          onChange={(e) => {
            const v = e.target.value;
            if (v === "edge") onChange({ ...value, wrapColumns: null });
            else if (v === "custom") {
              const n = Number(custom) || 40;
              setCustom(String(n));
              onChange({ ...value, wrapColumns: n });
            } else onChange({ ...value, wrapColumns: Number(v) });
          }}
        >
          <option value="edge">右端で折り返す</option>
          {PRESETS.map((n) => (
            <option key={n} value={n}>
              {n}字で折り返す
            </option>
          ))}
          <option value="custom">字数を指定…</option>
        </select>
      </label>

      {selectValue === "custom" && (
        <label className="field">
          字数(全角換算)
          <input
            type="number"
            min={10}
            max={200}
            value={custom}
            onChange={(e) => {
              setCustom(e.target.value);
              const n = Number(e.target.value);
              if (n >= 10 && n <= 200) onChange({ ...value, wrapColumns: n });
            }}
          />
        </label>
      )}

      <p className="hint">
        全角1文字ぶんの幅を実測して桁を出しています。半角が混じると目盛りとずれます。
      </p>
    </div>
  );
}
