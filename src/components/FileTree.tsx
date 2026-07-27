/** 左ペイン: manuscript/ と codex/ のファイルツリー(MVP要素2)。 */

import { useState } from "react";
import type { TreeNode } from "../api";
import { folderLabel, isRelabeled } from "./folderLabels";

type Props = {
  tree: TreeNode[];
  currentPath: string | null;
  dirty: boolean;
  onOpen: (path: string) => void;
  /** 新規作成の要求。実際の作成はダイアログで条件を決めてから行う */
  onCreate: (dirPath: string) => void;
  /** 項目メニューを開く(右クリック / ⋯ ボタン) */
  onMenu: (node: TreeNode, x: number, y: number) => void;
};

function Node({
  node,
  depth,
  currentPath,
  dirty,
  onOpen,
  onCreate,
  onMenu,
}: {
  node: TreeNode;
  depth: number;
} & Omit<Props, "tree">) {
  const [open, setOpen] = useState(depth < 1);

  const menuButton = (
    <button
      className="mini menu-btn"
      title="メニュー"
      onClick={(e) => {
        e.stopPropagation();
        const r = e.currentTarget.getBoundingClientRect();
        onMenu(node, r.left, r.bottom + 2);
      }}
    >
      ⋯
    </button>
  );

  if (node.is_dir) {
    return (
      <div>
        <div
          className="tree-row dir"
          style={{ paddingLeft: 6 + depth * 12 }}
          onClick={() => setOpen((v) => !v)}
          onContextMenu={(e) => {
            e.preventDefault();
            onMenu(node, e.clientX, e.clientY);
          }}
          // 表示名を変えた場合でも、ディスク上の名前は隠さない
          title={isRelabeled(node.path) ? `${node.path}/` : undefined}
        >
          <span className="twisty">{open ? "▾" : "▸"}</span>
          <span className="name">{folderLabel(node.path, node.name)}</span>
          <button
            className="mini"
            title="このフォルダーに新規ファイル"
            onClick={(e) => {
              e.stopPropagation();
              onCreate(node.path);
            }}
          >
            +
          </button>
          {menuButton}
        </div>
        {open &&
          node.children.map((c) => (
            <Node
              key={c.path}
              node={c}
              depth={depth + 1}
              currentPath={currentPath}
              dirty={dirty}
              onOpen={onOpen}
              onCreate={onCreate}
              onMenu={onMenu}
            />
          ))}
      </div>
    );
  }

  const active = node.path === currentPath;
  return (
    <div
      className={`tree-row file${active ? " active" : ""}`}
      style={{ paddingLeft: 18 + depth * 12 }}
      onClick={() => onOpen(node.path)}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu(node, e.clientX, e.clientY);
      }}
      title={node.path}
    >
      <span className="name">{node.title || node.name}</span>
      {active && dirty && <span className="dot" title="未保存" />}
      {menuButton}
    </div>
  );
}

export function FileTree({
  tree,
  currentPath,
  dirty,
  onOpen,
  onCreate,
  onMenu,
}: Props) {
  if (tree.length === 0) {
    return <p className="empty">プロジェクトを開くとファイルが表示されます。</p>;
  }
  return (
    <div className="tree">
      {tree.map((n) => (
        <Node
          key={n.path}
          node={n}
          depth={0}
          currentPath={currentPath}
          dirty={dirty}
          onOpen={onOpen}
          onCreate={onCreate}
          onMenu={onMenu}
        />
      ))}
    </div>
  );
}
