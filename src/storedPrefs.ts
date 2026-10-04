/**
 * ブラウザ側に覚えておく画面の設定(右ペインのタブ・表示の設定)を読む(テスト計画 F2)。
 *
 * 置き場所(localStorage)は、古い版が書いた値や壊れた値が残りうる。**読んだ値は
 * そのまま使わず、使える値かを確かめる**。以前は右ペインのタブを確かめずに使っていたため、
 * 知らない値が残っていると、どのタブも選ばれず右ペインが空になった。表示の設定も
 * JSON として読めれば型も範囲も見ずにエディタへ渡していた。
 */

import type { ViewSettings } from "./components/ViewMenu";
import type { ThemeChoice } from "./theme";

export const RIGHT_TABS = ["ai", "ref", "proof", "review", "extract"] as const;
export type RightTab = (typeof RIGHT_TABS)[number];

/** 右ペインのタブ。知らない値なら AI相談 */
export function parseRightTab(raw: string | null): RightTab {
  return (RIGHT_TABS as readonly string[]).includes(raw ?? "") ? (raw as RightTab) : "ai";
}

export const DEFAULT_VIEW: ViewSettings = {
  showLineNumbers: true,
  showRuler: false,
  wrapColumns: null,
  // 既定はライト(§5.15)。初回起動でデバイス設定に追従はしない
  theme: "light",
};

/** 折り返し字数として受け付ける範囲(表示メニューの「指定」と同じ) */
export const WRAP_MIN = 10;
export const WRAP_MAX = 200;

const THEMES: readonly ThemeChoice[] = ["light", "dark", "device"];

/** 表示の設定。項目ごとに確かめ、使えない値はその項目だけ既定に戻す */
export function parseView(raw: string | null): ViewSettings {
  let v: unknown;
  try {
    v = raw ? JSON.parse(raw) : null;
  } catch {
    return DEFAULT_VIEW;
  }
  if (typeof v !== "object" || v === null || Array.isArray(v)) return DEFAULT_VIEW;
  const o = v as Record<string, unknown>;
  const bool = (x: unknown, fallback: boolean) => (typeof x === "boolean" ? x : fallback);
  const wrap = o.wrapColumns;
  return {
    showLineNumbers: bool(o.showLineNumbers, DEFAULT_VIEW.showLineNumbers),
    showRuler: bool(o.showRuler, DEFAULT_VIEW.showRuler),
    wrapColumns:
      typeof wrap === "number" && Number.isInteger(wrap) && wrap >= WRAP_MIN && wrap <= WRAP_MAX
        ? wrap
        : null,
    theme: THEMES.includes(o.theme as ThemeChoice) ? (o.theme as ThemeChoice) : DEFAULT_VIEW.theme,
  };
}
