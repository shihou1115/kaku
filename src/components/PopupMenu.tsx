/**
 * カーソル位置に出す小さなメニューの土台。
 *
 * 中身(項目)は呼び出し側が決める。ここが持つのは
 * **どこに出すか・どう閉じるか**だけ:
 *  - 外側をクリックしたら閉じる
 *  - Escape で閉じる
 *  - 画面外へはみ出さないよう位置を補正する
 *
 * 左ペインの項目メニューとエディタの右クリックで**同じ振る舞いが必要になった**ため
 * 切り出した(「同じコードが2回以上必要になってから共通化する」)。
 */

import { useEffect, useRef } from "react";

export type PopupItem<K extends string> = {
  key: K;
  label: string;
  /** 取り返しのつく操作でも、性質が違うものは色で分ける */
  danger?: boolean;
};

type Props<K extends string> = {
  x: number;
  y: number;
  items: PopupItem<K>[];
  onPick: (key: K) => void;
  onClose: () => void;
};

/** 1項目あたりの高さの目安(画面外へのはみ出しを補正するのに使う) */
const ITEM_H = 28;

export function PopupMenu<K extends string>({
  x,
  y,
  items,
  onPick,
  onClose,
}: Props<K>) {
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

  const left = Math.min(x, window.innerWidth - 200);
  const top = Math.min(y, window.innerHeight - items.length * ITEM_H - 16);

  return (
    <div className="item-menu" style={{ left, top }} ref={ref}>
      {items.map((it) => (
        <button
          key={it.key}
          className={it.danger ? "danger" : ""}
          onClick={() => {
            onPick(it.key);
            onClose();
          }}
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}
