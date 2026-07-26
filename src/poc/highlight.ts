/**
 * codex名ハイライト用の CodeMirror 6 拡張。
 *
 * PoC#1 の検証対象(docs/04-design.md §7-1):
 *   ①素の状態での日本語長文入力が安定すること
 *   ②ハイライト装飾が載った文字列上での IME 変換が崩れないこと
 *
 * ②が崩れた場合の緩和策(対応ラダー①)を検証できるよう、
 * 「IME変換中は装飾を外す」モードを切り替えられるようにしている。
 */

import { StateEffect, StateField } from "@codemirror/state";
import { Decoration, DecorationSet, EditorView } from "@codemirror/view";
import { findMentions } from "./mentions";

/** ハイライト対象(codexの名前・別名)を差し替える */
export const setPatterns = StateEffect.define<string[]>();
/** ハイライト自体のON/OFF */
export const setHighlightEnabled = StateEffect.define<boolean>();
/** IME変換中フラグ(対応ラダー①の検証用) */
export const setComposing = StateEffect.define<boolean>();
/** 「IME変換中は装飾を隠す」設定 */
export const setHideWhileComposing = StateEffect.define<boolean>();

type HighlightState = {
  patterns: string[];
  enabled: boolean;
  composing: boolean;
  hideWhileComposing: boolean;
  decorations: DecorationSet;
  /** 直近の計算での一致件数(UI表示用) */
  count: number;
};

const mentionMark = Decoration.mark({ class: "cm-mention" });

function build(doc: string, s: Omit<HighlightState, "decorations" | "count">) {
  if (!s.enabled) return { decorations: Decoration.none, count: 0 };
  const hits = findMentions(doc, s.patterns);
  if (s.composing && s.hideWhileComposing) {
    // 件数だけは数え、描画はしない(隠している間も計測を続けるため)
    return { decorations: Decoration.none, count: hits.length };
  }
  return {
    decorations: Decoration.set(
      hits.map((m) => mentionMark.range(m.from, m.to)),
      true,
    ),
    count: hits.length,
  };
}

export const highlightField = StateField.define<HighlightState>({
  create(state) {
    const base = {
      patterns: [] as string[],
      enabled: true,
      composing: false,
      hideWhileComposing: false,
    };
    return { ...base, ...build(state.doc.toString(), base) };
  },
  update(value, tr) {
    let next = {
      patterns: value.patterns,
      enabled: value.enabled,
      composing: value.composing,
      hideWhileComposing: value.hideWhileComposing,
    };
    let dirty = tr.docChanged;

    for (const e of tr.effects) {
      if (e.is(setPatterns)) {
        next.patterns = e.value;
        dirty = true;
      } else if (e.is(setHighlightEnabled)) {
        next.enabled = e.value;
        dirty = true;
      } else if (e.is(setComposing)) {
        next.composing = e.value;
        // 「隠す」設定が有効なときだけ再計算が必要
        if (next.hideWhileComposing) dirty = true;
      } else if (e.is(setHideWhileComposing)) {
        next.hideWhileComposing = e.value;
        dirty = true;
      }
    }

    if (!dirty) {
      return { ...next, decorations: value.decorations, count: value.count };
    }
    return { ...next, ...build(tr.state.doc.toString(), next) };
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.decorations),
});

/** IME の composition イベントを拾って composing フラグと計測値を更新する */
export function compositionTracker(onEvent: (kind: "start" | "end") => void) {
  return EditorView.domEventHandlers({
    compositionstart(_e, view) {
      onEvent("start");
      view.dispatch({ effects: setComposing.of(true) });
      return false;
    },
    compositionend(_e, view) {
      onEvent("end");
      view.dispatch({ effects: setComposing.of(false) });
      return false;
    },
  });
}

export const highlightTheme = EditorView.baseTheme({
  ".cm-mention": {
    backgroundColor: "rgba(56, 139, 253, 0.18)",
    borderBottom: "2px solid rgba(56, 139, 253, 0.7)",
    borderRadius: "2px",
  },
});

/** UI から現在の一致件数を読む */
export function mentionCount(view: EditorView): number {
  return view.state.field(highlightField).count;
}
