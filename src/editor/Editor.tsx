/**
 * 原稿・codex 編集用の CodeMirror 6 エディタ。
 *
 * PoC#1(2026-07-26 合格)の構成を引き継ぐ:
 * basicSetup は使わず必要な拡張だけを載せる。日本語IMEでの安定性を優先する。
 *
 * 表示の補助(2026-07-26 追加): 行番号 / 横ルーラー / 任意字数での折り返し。
 * 日本語の全角文字は等幅で並ぶので、全角1文字の送り幅を実測して桁の基準にする。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import {
  EditorView,
  keymap,
  drawSelection,
  highlightActiveLine,
  lineNumbers,
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
  /** 範囲を選択して表示する(校正の指摘へ移動する) */
  selectRange: (from: number, to: number) => void;
  /** 範囲を置き換える(校正の置換。ユーザー操作でのみ呼ぶ) */
  replaceRange: (from: number, to: number, text: string) => void;
};

type Props = {
  patterns: string[];
  highlightEnabled: boolean;
  readOnly: boolean;
  /** 行番号を出すか */
  showLineNumbers: boolean;
  /** 横ルーラーを出すか */
  showRuler: boolean;
  /** 折り返し位置(全角換算の字数)。null なら右端で折り返す */
  wrapColumns: number | null;
  onChange: (text: string) => void;
  onSaveRequest: () => void;
  /** ハイライトされた語を Ctrl/Cmd+クリックしたとき(参照ペインで開く) */
  onMentionActivate: (name: string) => void;
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
    boxSizing: "border-box",
  },
  ".cm-scroller": { overflow: "auto" },
  "&.cm-focused": { outline: "none" },

  // 検索パネル。既定は小さすぎて押しづらいので、アプリの他のボタンと同じ寸法に揃える
  ".cm-panels": { fontSize: "13px", backgroundColor: "#fbfcfd" },
  ".cm-panels.cm-panels-top": { borderBottom: "1px solid #d9dde3" },
  ".cm-panel.cm-search": {
    padding: "8px 40px 8px 10px",
    display: "flex",
    flexWrap: "wrap",
    alignItems: "center",
    gap: "6px",
  },
  ".cm-panel.cm-search br": { display: "none" },
  ".cm-panel.cm-search .cm-textfield": {
    fontSize: "13px",
    fontFamily: "inherit",
    padding: "5px 8px",
    margin: "0",
    // ペインが狭いときは縮み、広いときは伸びる(固定幅だと無駄に折り返す)
    flex: "1 1 180px",
    minWidth: "120px",
    maxWidth: "320px",
    border: "1px solid #d9dde3",
    borderRadius: "4px",
    backgroundColor: "#fff",
  },
  ".cm-panel.cm-search .cm-textfield:focus": {
    outline: "2px solid rgba(11, 114, 133, 0.35)",
    outlineOffset: "-1px",
  },
  ".cm-panel.cm-search .cm-button": {
    fontSize: "12px",
    fontFamily: "inherit",
    padding: "5px 12px",
    margin: "0",
    border: "1px solid #d9dde3",
    borderRadius: "4px",
    backgroundColor: "#fff",
    backgroundImage: "none",
    color: "#1f2328",
    cursor: "pointer",
  },
  ".cm-panel.cm-search .cm-button:hover": { backgroundColor: "#eef1f4" },
  ".cm-panel.cm-search .cm-button:active": { backgroundColor: "#e2e6ea" },
  ".cm-panel.cm-search label": {
    fontSize: "12px",
    display: "inline-flex",
    alignItems: "center",
    gap: "4px",
    margin: "0",
    whiteSpace: "nowrap",
  },
  ".cm-panel.cm-search label input[type=checkbox]": {
    width: "14px",
    height: "14px",
    margin: "0",
  },
  ".cm-panel.cm-search [name=close]": {
    position: "absolute",
    top: "6px",
    right: "8px",
    fontSize: "20px",
    lineHeight: "1",
    padding: "2px 8px",
    border: "1px solid transparent",
    borderRadius: "4px",
    backgroundColor: "transparent",
    color: "#656d76",
    cursor: "pointer",
  },
  ".cm-panel.cm-search [name=close]:hover": {
    backgroundColor: "#eef1f4",
    color: "#1f2328",
  },
  ".cm-gutters": {
    backgroundColor: "transparent",
    borderRight: "1px solid #e6e9ed",
    color: "#a9b1ba",
  },
  ".cm-lineNumbers .cm-gutterElement": {
    padding: "0 6px 0 10px",
    fontSize: "11px",
    fontFamily: "Consolas, monospace",
  },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "#0b7285" },
});

export function Editor({
  patterns,
  highlightEnabled,
  readOnly,
  showLineNumbers,
  showRuler,
  wrapColumns,
  onChange,
  onSaveRequest,
  onMentionActivate,
  handleRef,
}: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const readOnlyComp = useRef(new Compartment());
  const lineNumComp = useRef(new Compartment());
  const extensionsRef = useRef<Extension[]>([]);
  const cbRef = useRef({ onChange, onSaveRequest, onMentionActivate });
  cbRef.current = { onChange, onSaveRequest, onMentionActivate };

  /** 全角1文字の送り幅(px)と、本文左端までのオフセット(ガター+余白) */
  const [metrics, setMetrics] = useState({ charW: 0, offsetLeft: 0, innerW: 0 });

  useEffect(() => {
    if (!hostRef.current) return;

    const extensions: Extension[] = [
      history(),
      drawSelection(),
      highlightActiveLine(),
      lineNumComp.current.of([]),
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
      // 設定名の Ctrl/Cmd+クリックで参照を開く(「定義へ移動」と同じ操作感)。
      // 素のクリックはカーソル移動のままにして、執筆の邪魔をしない
      EditorView.domEventHandlers({
        mousedown(e) {
          if (!e.ctrlKey && !e.metaKey) return false;
          const el = (e.target as HTMLElement | null)?.closest(".cm-mention");
          if (!el?.textContent) return false;
          e.preventDefault();
          cbRef.current.onMentionActivate(el.textContent);
          return true;
        },
      }),
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
      // 差し替え直後の state には現在の設定が入っていないので再適用する
      view.dispatch({
        effects: [
          setPatterns.of(patternsRef.current),
          lineNumComp.current.reconfigure(
            lineNumRef.current ? lineNumbers() : [],
          ),
          readOnlyComp.current.reconfigure(
            EditorState.readOnly.of(readOnlyRef.current),
          ),
        ],
      });
    };
    handleRef.openSearch = () => openSearchPanel(view);
    handleRef.selectRange = (from: number, to: number) => {
      const len = view.state.doc.length;
      const a = Math.max(0, Math.min(from, len));
      const b = Math.max(a, Math.min(to, len));
      view.dispatch({
        selection: { anchor: a, head: b },
        effects: EditorView.scrollIntoView(a, { y: "center" }),
      });
      view.focus();
    };
    handleRef.replaceRange = (from: number, to: number, text: string) => {
      const len = view.state.doc.length;
      const a = Math.max(0, Math.min(from, len));
      const b = Math.max(a, Math.min(to, len));
      view.dispatch({
        changes: { from: a, to: b, insert: text },
        selection: { anchor: a + text.length },
      });
    };
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

  // load() 内から最新の設定を参照するための保持
  const patternsRef = useRef(patterns);
  patternsRef.current = patterns;
  const lineNumRef = useRef(showLineNumbers);
  lineNumRef.current = showLineNumbers;
  const readOnlyRef = useRef(readOnly);
  readOnlyRef.current = readOnly;

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

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: lineNumComp.current.reconfigure(
        showLineNumbers ? lineNumbers() : [],
      ),
    });
  }, [showLineNumbers]);

  /** 実DOMから桁の基準を測る。行番号の桁数や幅の変化に追随する必要がある */
  const measure = useCallback(() => {
    const view = viewRef.current;
    const wrap = wrapRef.current;
    if (!view || !wrap) return;
    const content = view.contentDOM;
    const cs = getComputedStyle(content);

    // 全角文字の送り幅を実測する(日本語は等幅で並ぶ)
    const probe = document.createElement("span");
    probe.style.cssText = `position:absolute;visibility:hidden;white-space:pre;font:${cs.font};`;
    probe.textContent = "あ".repeat(20);
    content.appendChild(probe);
    const charW = probe.getBoundingClientRect().width / 20;
    probe.remove();

    const gutters = view.dom.querySelector<HTMLElement>(".cm-gutters");
    const padL = parseFloat(cs.paddingLeft) || 0;
    const padR = parseFloat(cs.paddingRight) || 0;
    const offsetLeft = (gutters?.offsetWidth ?? 0) + padL;
    const innerW = Math.max(0, content.clientWidth - padL - padR);

    setMetrics((m) =>
      m.charW === charW && m.offsetLeft === offsetLeft && m.innerW === innerW
        ? m
        : { charW, offsetLeft, innerW },
    );
  }, []);

  // 注意: ビューの生成は上の useEffect で行うため、ここも useEffect にする
  // (useLayoutEffect だと生成前に走って observer が張られない)
  useEffect(() => {
    measure();
    const view = viewRef.current;
    if (!view) return;
    const ro = new ResizeObserver(() => measure());
    ro.observe(view.dom);
    // 行数が桁上がりするとガター幅だけが変わる。外枠のサイズは変わらないので個別に見る
    const gutters = view.dom.querySelector(".cm-gutters");
    if (gutters) ro.observe(gutters);
    return () => ro.disconnect();
  }, [measure, showLineNumbers, wrapColumns, showRuler]);

  /** 折り返し幅。null(右端)のときは制限しない */
  const wrapWidth =
    wrapColumns && metrics.charW > 0
      ? `${Math.round(wrapColumns * metrics.charW)}px`
      : "none";

  return (
    <div
      className="editor-wrap"
      ref={wrapRef}
      style={{ ["--wrap-width" as string]: wrapWidth }}
    >
      {showRuler && (
        <Ruler
          charW={metrics.charW}
          offsetLeft={metrics.offsetLeft}
          columns={
            wrapColumns ??
            (metrics.charW > 0 ? Math.floor(metrics.innerW / metrics.charW) : 0)
          }
        />
      )}
      <div className="editor-host" ref={hostRef} onKeyUp={measure} />
    </div>
  );
}

/** 横ルーラー。5字ごとに目盛り、10字ごとに数字を出す */
function Ruler({
  charW,
  offsetLeft,
  columns,
}: {
  charW: number;
  offsetLeft: number;
  columns: number;
}) {
  if (charW <= 0 || columns <= 0) return <div className="ruler" />;
  // 目盛りが密になりすぎないよう、桁数が多いときは間隔を広げる
  const step = columns > 120 ? 10 : 5;
  const labelEvery = step === 5 ? 10 : 20;
  const marks: React.ReactNode[] = [];
  for (let c = step; c <= columns; c += step) {
    const major = c % labelEvery === 0;
    marks.push(
      <span
        key={c}
        className={major ? "tick major" : "tick"}
        style={{ left: c * charW }}
      >
        {major && <em>{c}</em>}
      </span>,
    );
  }
  return (
    <div className="ruler">
      <div className="ruler-track" style={{ marginLeft: offsetLeft }}>
        {marks}
        <span className="tick end" style={{ left: columns * charW }} />
      </div>
    </div>
  );
}
