/**
 * 原稿・codex 編集用の CodeMirror 6 エディタ。
 *
 * PoC#1(2026-07-26 合格)の構成を引き継ぐ:
 * basicSetup は使わず必要な拡張だけを載せる。日本語IMEでの安定性を優先する。
 */

import { useEffect, useRef } from "react";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import {
  EditorView,
  keymap,
  drawSelection,
  highlightActiveLine,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { search, searchKeymap, openSearchPanel } from "@codemirror/search";
import {
  highlightField,
  highlightTheme,
  setPatterns,
  setHighlightEnabled,
} from "./highlight";

export type EditorHandle = {
  view: EditorView | null;
  /** ファイル切替。undo履歴も作り直す(前のファイルの undo が効くと事故になる) */
  load: (text: string) => void;
  openSearch: () => void;
  scrollTo: (pos: number) => void;
};

type Props = {
  patterns: string[];
  highlightEnabled: boolean;
  readOnly: boolean;
  onChange: (text: string) => void;
  onSaveRequest: () => void;
  handleRef: EditorHandle;
};

const editorTheme = EditorView.theme({
  "&": { height: "100%", fontSize: "16px" },
  ".cm-content": {
    fontFamily:
      '"Yu Gothic", "Hiragino Kaku Gothic ProN", "Noto Sans JP", sans-serif',
    lineHeight: "1.9",
    padding: "18px 24px",
    caretColor: "#0b7285",
  },
  ".cm-scroller": { overflow: "auto" },
  "&.cm-focused": { outline: "none" },
  ".cm-panels": { fontSize: "12px" },
});

export function Editor({
  patterns,
  highlightEnabled,
  readOnly,
  onChange,
  onSaveRequest,
  handleRef,
}: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const readOnlyComp = useRef(new Compartment());
  const extensionsRef = useRef<Extension[]>([]);
  // 最新のコールバックを参照する(エディタ自体は作り直さない)
  const cbRef = useRef({ onChange, onSaveRequest });
  cbRef.current = { onChange, onSaveRequest };

  useEffect(() => {
    if (!hostRef.current) return;

    const extensions: Extension[] = [
      history(),
      drawSelection(),
      highlightActiveLine(),
      keymap.of([
        {
          key: "Mod-s",
          preventDefault: true,
          run: () => {
            cbRef.current.onSaveRequest();
            return true;
          },
        },
        ...defaultKeymap,
        ...historyKeymap,
        ...searchKeymap,
      ]),
      search({ top: true }),
      EditorView.lineWrapping,
      highlightField,
      highlightTheme,
      editorTheme,
      readOnlyComp.current.of(EditorState.readOnly.of(false)),
      EditorView.updateListener.of((u) => {
        if (u.docChanged) cbRef.current.onChange(u.state.doc.toString());
      }),
    ];
    extensionsRef.current = extensions;

    const view = new EditorView({
      state: EditorState.create({ doc: "", extensions }),
      parent: hostRef.current,
    });

    viewRef.current = view;
    handleRef.view = view;
    handleRef.load = (text: string) => {
      view.setState(
        EditorState.create({ doc: text, extensions: extensionsRef.current }),
      );
      // 差し替え直後の state には現在のパターン設定が入っていないので再適用する
      view.dispatch({ effects: setPatterns.of(patternsRef.current) });
    };
    handleRef.openSearch = () => openSearchPanel(view);
    handleRef.scrollTo = (pos: number) => {
      const clamped = Math.max(0, Math.min(pos, view.state.doc.length));
      view.dispatch({
        selection: { anchor: clamped },
        effects: EditorView.scrollIntoView(clamped, { y: "center" }),
      });
      view.focus();
    };

    return () => {
      view.destroy();
      viewRef.current = null;
      handleRef.view = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // load() 内から最新のパターンを参照するための保持
  const patternsRef = useRef(patterns);
  patternsRef.current = patterns;

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
      effects: readOnlyComp.current.reconfigure(
        EditorState.readOnly.of(readOnly),
      ),
    });
  }, [readOnly]);

  return <div className="editor-host" ref={hostRef} />;
}
