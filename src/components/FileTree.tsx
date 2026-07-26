/** 左ペイン: manuscript/ と codex/ のファイルツリー(MVP要素2)。 */

import { useState } from "react";
import type { TreeNode } from "../api";

type Props = {
  tree: TreeNode[];
  currentPath: string | null;
  dirty: boolean;
  onOpen: (path: string) => void;
  /** 新規作成の要求。実際の作成はダイアログで条件を決めてから行う */
  onCreate: (dirPath: string) => void;
};

function Node({
  node,
  depth,
  currentPath,
  dirty,
  onOpen,
  onCreate,
}: {
  node: TreeNode;
  depth: number;
} & Omit<Props, "tree">) {
  const [open, setOpen] = useState(depth < 1);

  if (node.is_dir) {
    return (
      <div>
        <div
          className="tree-row dir"
          style={{ paddingLeft: 6 + depth * 12 }}
          onClick={() => setOpen((v) => !v)}
        >
          <span className="twisty">{open ? "▾" : "▸"}</span>
          <span className="name">{node.name}</span>
          <button
            className="mini"
            title="このフォルダに新規ファイル"
            onClick={(e) => {
              e.stopPropagation();
              onCreate(node.path);
            }}
          >
            +
          </button>
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
      title={node.path}
    >
      <span className="name">{node.title || node.name}</span>
      {active && dirty && <span className="dot" title="未保存" />}
    </div>
  );
}

export function FileTree({
  tree,
  currentPath,
  dirty,
  onOpen,
  onCreate,
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
        />
      ))}
    </div>
  );
}
