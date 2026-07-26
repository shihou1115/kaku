/**
 * codex名ハイライト用の CodeMirror 6 拡張(M-02 言及検出の表示側)。
 *
 * 位置は保存しない。ドキュメントが変わるたびにメモリ上で算出する
 * (docs/03-data-format.md D-7)。
 *
 * PoC#1(2026-07-26)で、装飾が載った文字列上でのIME変換が崩れないことを
 * 実機確認済みのため、変換中に装飾を隠す緩和策は入れていない。
 */

import { StateEffect, StateField } from "@codemirror/state";
import { Decoration, DecorationSet, EditorView } from "@codemirror/view";
import { findMentions } from "./mentions";

export const setPatterns = StateEffect.define<string[]>();
export const setHighlightEnabled = StateEffect.define<boolean>();

type HighlightState = {
  patterns: string[];
  enabled: boolean;
  decorations: DecorationSet;
  count: number;
};

const mentionMark = Decoration.mark({ class: "cm-mention" });

function build(doc: string, patterns: string[], enabled: boolean) {
  if (!enabled || patterns.length === 0) {
    return { decorations: Decoration.none, count: 0 };
  }
  const hits = findMentions(doc, patterns);
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
    return {
      patterns: [],
      enabled: true,
      ...build(state.doc.toString(), [], true),
    };
  },
  update(value, tr) {
    let patterns = value.patterns;
    let enabled = value.enabled;
    let dirty = tr.docChanged;

    for (const e of tr.effects) {
      if (e.is(setPatterns)) {
        patterns = e.value;
        dirty = true;
      } else if (e.is(setHighlightEnabled)) {
        enabled = e.value;
        dirty = true;
      }
    }

    if (!dirty) {
      return { ...value, patterns, enabled };
    }
    return {
      patterns,
      enabled,
      ...build(tr.state.doc.toString(), patterns, enabled),
    };
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.decorations),
});

export const highlightTheme = EditorView.baseTheme({
  ".cm-mention": {
    backgroundColor: "rgba(56, 139, 253, 0.16)",
    borderBottom: "2px solid rgba(56, 139, 253, 0.6)",
    borderRadius: "2px",
  },
});

export function mentionCount(view: EditorView): number {
  return view.state.field(highlightField).count;
}
