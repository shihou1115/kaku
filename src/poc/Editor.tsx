/**
 * PoC#1 用の CodeMirror 6 エディタ。
 *
 * 拡張は意図的に最小限にしている(basicSetup は使わない)。
 * 括弧補完・自動補完・折り返し以外の装飾が入ると、IME の不具合が
 * どの拡張のせいなのか切り分けられなくなるため。
 */

import { useEffect, useRef } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, drawSelection, highlightActiveLine } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { search, searchKeymap } from "@codemirror/search";
import {
  compositionTracker,
  highlightField,
  highlightTheme,
  setHideWhileComposing,
  setHighlightEnabled,
  setPatterns,
} from "./highlight";

export type EditorHandle = {
  view: EditorView | null;
};

type Props = {
  initialDoc: string;
  patterns: string[];
  highlightEnabled: boolean;
  hideWhileComposing: boolean;
  /** docChanged=false は「本文は変わらずハイライトだけ再計算された」場合 */
  onDocChange: (doc: string, mentionCount: number, docChanged: boolean) => void;
  onComposition: (kind: "start" | "end") => void;
  handleRef: EditorHandle;
};

const editorTheme = EditorView.theme({
  "&": {
    height: "100%",
    fontSize: "16px",
  },
  ".cm-content": {
    // 日本語小説の可読性: 行間を広めに、字送りは等幅にしない
    fontFamily:
      '"Yu Gothic", "Hiragino Kaku Gothic ProN", "Noto Sans JP", sans-serif',
    lineHeight: "1.9",
    padding: "16px 20px",
    caretColor: "#0b7285",
  },
  ".cm-scroller": { overflow: "auto" },
  "&.cm-focused": { outline: "none" },
});

export function Editor({
  initialDoc,
  patterns,
  highlightEnabled,
  hideWhileComposing,
  onDocChange,
  onComposition,
  handleRef,
}: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);

  // 初回だけ生成する。props 変更は effect で dispatch して反映する
  // (再生成するとカーソル位置と undo 履歴が失われ、IME検証にならない)
  useEffect(() => {
    if (!hostRef.current) return;

    const state = EditorState.create({
      doc: initialDoc,
      extensions: [
        history(),
        drawSelection(),
        highlightActiveLine(),
        keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap]),
        search({ top: true }),
        EditorView.lineWrapping,
        highlightField,
        highlightTheme,
        editorTheme,
        compositionTracker(onComposition),
        EditorView.updateListener.of((u) => {
          const f = u.state.field(highlightField);
          // 本文変更だけでなく、パターン差し替え等でハイライトが変わった時も通知する
          // (初期表示の件数が 0 のままになるのを防ぐ)
          if (u.docChanged || f !== u.startState.field(highlightField)) {
            onDocChange(u.state.doc.toString(), f.count, u.docChanged);
          }
        }),
      ],
    });

    const view = new EditorView({ state, parent: hostRef.current });
    viewRef.current = view;
    handleRef.view = view;
    view.focus();

    return () => {
      view.destroy();
      viewRef.current = null;
      handleRef.view = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    viewRef.current?.dispatch({ effects: setPatterns.of(patterns) });
  }, [patterns]);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: setHighlightEnabled.of(highlightEnabled),
    });
  }, [highlightEnabled]);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: setHideWhileComposing.of(hideWhileComposing),
    });
  }, [hideWhileComposing]);

  return <div className="editor-host" ref={hostRef} />;
}
