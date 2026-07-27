/**
 * 左ペインの項目メニュー(右クリック / ⋯ ボタン)。
 *
 * 削除は**消さずにゴミ箱へ移す**方式なので、確認は「取り返しがつかない操作」への
 * 警告ではなく「どこへ移るか」の告知にしている(ダイアログを機械的に押す癖を
 * つけさせないため、脅し文句にはしない)。
 */

import { useEffect, useRef } from "react";

export type MenuAction =
  | "newFile"
  | "newFolder"
  | "rename"
  | "duplicate"
  | "reveal"
  | "trash";

type Props = {
  x: number;
  y: number;
  isDir: boolean;
  onPick: (action: MenuAction) => void;
  onClose: () => void;
};

const FILE_ITEMS: { action: MenuAction; label: string }[] = [
  { action: "rename", label: "名前を変更…" },
  { action: "duplicate", label: "複製" },
  { action: "reveal", label: "エクスプローラーで表示" },
  { action: "trash", label: "削除…" },
];

const DIR_ITEMS: { action: MenuAction; label: string }[] = [
  { action: "newFile", label: "新しいファイル…" },
  { action: "newFolder", label: "新しいフォルダー…" },
  { action: "rename", label: "名前を変更…" },
  { action: "reveal", label: "エクスプローラーで表示" },
  { action: "trash", label: "削除…" },
];

export function ItemMenu({ x, y, isDir, onPick, onClose }: Props) {
  const ref = useRef<HTMLDivElement | null>(null);

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

  const items = isDir ? DIR_ITEMS : FILE_ITEMS;
  // 画面外へはみ出さないよう軽く補正する
  const left = Math.min(x, window.innerWidth - 200);
  const top = Math.min(y, window.innerHeight - items.length * 28 - 16);

  return (
    <div className="item-menu" style={{ left, top }} ref={ref}>
      {items.map((it) => (
        <button
          key={it.action}
          className={it.action === "trash" ? "danger" : ""}
          onClick={() => {
            onPick(it.action);
            onClose();
          }}
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}
