/**
 * 左ペインの項目メニュー(右クリック / ⋯ ボタン)。
 *
 * 出し方・閉じ方は PopupMenu が持つ。ここが決めるのは**何が並ぶか**だけ。
 *
 * 削除は**消さずにゴミ箱へ移す**方式なので、確認は「取り返しがつかない操作」への
 * 警告ではなく「どこへ移るか」の告知にしている(ダイアログを機械的に押す癖を
 * つけさせないため、脅し文句にはしない)。
 */

import { PopupMenu, type PopupItem } from "./PopupMenu";

export type MenuAction =
  | "newFile"
  | "newFolder"
  | "rename"
  | "duplicate"
  | "reveal"
  | "split"
  | "trash";

type Props = {
  x: number;
  y: number;
  isDir: boolean;
  onPick: (action: MenuAction) => void;
  onClose: () => void;
};

const FILE_ITEMS: PopupItem<MenuAction>[] = [
  { key: "rename", label: "名前を変更…" },
  { key: "duplicate", label: "複製" },
  { key: "split", label: "シーンに分割…" },
  { key: "reveal", label: "エクスプローラーで表示" },
  { key: "trash", label: "削除…", danger: true },
];

const DIR_ITEMS: PopupItem<MenuAction>[] = [
  { key: "newFile", label: "新しいファイル…" },
  { key: "newFolder", label: "新しいフォルダー…" },
  { key: "rename", label: "名前を変更…" },
  { key: "reveal", label: "エクスプローラーで表示" },
  { key: "trash", label: "削除…", danger: true },
];

export function ItemMenu({ x, y, isDir, onPick, onClose }: Props) {
  return (
    <PopupMenu
      x={x}
      y={y}
      items={isDir ? DIR_ITEMS : FILE_ITEMS}
      onPick={onPick}
      onClose={onClose}
    />
  );
}
