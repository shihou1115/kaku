/**
 * 右ペインの「参照」タブ。
 *
 * 設定を眺めながら書くための読み取り専用ビュー。
 * 編集用のエディタと分けているのは、同じファイルを2箇所で編集して
 * 上書き合戦になるのを防ぐため(開きたいときは「エディタで開く」)。
 */

import type { CodexEntry } from "../api";

type Props = {
  codex: CodexEntry[];
  /** 本文に登場しているエントリのパス(先頭にまとめて出す) */
  mentionedPaths: string[];
  refPath: string | null;
  refText: string;
  onSelect: (path: string) => void;
  onOpenInEditor: (path: string) => void;
  onClose: () => void;
};

export function ReferencePane({
  codex,
  mentionedPaths,
  refPath,
  refText,
  onSelect,
  onOpenInEditor,
  onClose,
}: Props) {
  const mentioned = codex.filter((c) => mentionedPaths.includes(c.path));
  const others = codex.filter((c) => !mentionedPaths.includes(c.path));
  const current = codex.find((c) => c.path === refPath);

  return (
    <div className="reference-pane">
      <div className="ref-picker">
        <select
          value={refPath ?? ""}
          onChange={(e) => e.target.value && onSelect(e.target.value)}
        >
          <option value="">設定を選ぶ…</option>
          {mentioned.length > 0 && (
            <optgroup label="この本文に登場">
              {mentioned.map((c) => (
                <option key={c.path} value={c.path}>
                  {c.title}
                </option>
              ))}
            </optgroup>
          )}
          {others.length > 0 && (
            <optgroup label="その他">
              {others.map((c) => (
                <option key={c.path} value={c.path}>
                  {c.title}
                </option>
              ))}
            </optgroup>
          )}
        </select>
        {refPath && (
          <button
            className="mini"
            onClick={() => onOpenInEditor(refPath)}
            title="このファイルを編集する"
          >
            エディタで開く
          </button>
        )}
        <button className="mini" onClick={onClose} title="参照を閉じる">
          ×
        </button>
      </div>

      {!refPath && (
        <p className="hint ref-hint">
          本文中の色つきの語を <kbd>Ctrl</kbd>+クリックすると、その設定をここに表示します。
          <br />
          上の一覧から選ぶこともできます。
        </p>
      )}

      {refPath && (
        <>
          <div className="ref-head">
            <strong>{current?.title ?? refPath}</strong>
            <span className="ref-path">{refPath}</span>
          </div>
          <pre className="ref-body">{refText}</pre>
        </>
      )}
    </div>
  );
}
