/**
 * PoC#1 検証ハーネス(M0 / docs/05-roadmap.md)。
 *
 * 目的: CodeMirror 6 が Windows 日本語 IME で実用に耐えるかを判定する。
 * 合格条件(docs/04-design.md §7-1):
 *   ① 素の状態での日本語長文入力が安定すること
 *   ② codex名ハイライト(単純装飾)が載った文字列上での IME 変換が崩れないこと
 *
 * このハーネス自体は製品コードではない。判定が済んだら MVP 実装に置き換える。
 */

import { useCallback, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Editor, type EditorHandle } from "./poc/Editor";
import { findMentions } from "./poc/mentions";
import "./App.css";

const SAMPLE_TEXT = `　転校初日の朝は、雨だった。
　佐藤架純は昇降口で靴を履き替えながら、傘の水滴が上履きに落ちるのを見ていた。県立青葉高校の廊下は、前の学校よりずっと薄暗い。
「……最悪」
　小さくつぶやいたその声に、誰かが振り向いた気配がした。五十嵐悠二だった。彼は架純を一瞥すると、何も言わずに歩き去っていく。
　かすみんと呼ばれていたのは、もう遠い町の話だ。ここでは誰も、彼女の名前を知らない。
`;

const SAMPLE_PATTERNS = [
  "佐藤架純",
  "架純",
  "かすみん",
  "五十嵐悠二",
  "悠二",
  "県立青葉高校",
];

const CHECKLIST = [
  { id: "c1", label: "①素の状態(ハイライトOFF)で1000字以上を変換入力して、文字の欠落・重複・並び替えが起きない" },
  { id: "c2", label: "①変換確定後にカーソルが意図した位置に残る(先頭や別行へ飛ばない)" },
  { id: "c3", label: "①再変換(確定後にもう一度変換)しても本文が壊れない" },
  { id: "c4", label: "①Ctrl+Z / Ctrl+Y の undo/redo が変換単位で妥当に戻る" },
  { id: "c5", label: "②ハイライトONで、ハイライトされた語の直後・直前に変換入力しても表示が崩れない" },
  { id: "c6", label: "②ハイライトされた語の内部にカーソルを置いて変換・削除しても崩れない" },
  { id: "c7", label: "②変換中(未確定)の文字にハイライトが誤って掛からない/ちらつかない" },
  { id: "c8", label: "②長文(サンプルを10回追加)でも入力の体感遅延がない" },
];

type Metrics = {
  compositionStart: number;
  compositionEnd: number;
  docChanges: number;
  docLength: number;
  mentionCount: number;
};

export default function App() {
  const handleRef = useRef<EditorHandle>({ view: null });
  const [patternText, setPatternText] = useState(SAMPLE_PATTERNS.join("\n"));
  const [highlightEnabled, setHighlightEnabled] = useState(true);
  const [hideWhileComposing, setHideWhileComposing] = useState(false);
  const [metrics, setMetrics] = useState<Metrics>({
    compositionStart: 0,
    compositionEnd: 0,
    docChanges: 0,
    docLength: SAMPLE_TEXT.length,
    mentionCount: 0,
  });
  const [checked, setChecked] = useState<Record<string, boolean | null>>({});
  const [note, setNote] = useState("");
  const [rustCheck, setRustCheck] = useState<string>("未実行");

  const patterns = useMemo(
    () =>
      patternText
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean),
    [patternText],
  );

  const onDocChange = useCallback(
    (doc: string, mentionCount: number, docChanged: boolean) => {
      setMetrics((m) => ({
        ...m,
        docChanges: docChanged ? m.docChanges + 1 : m.docChanges,
        docLength: doc.length,
        mentionCount,
      }));
    },
    [],
  );

  const onComposition = useCallback((kind: "start" | "end") => {
    setMetrics((m) =>
      kind === "start"
        ? { ...m, compositionStart: m.compositionStart + 1 }
        : { ...m, compositionEnd: m.compositionEnd + 1 },
    );
  }, []);

  /** Rust(aho-corasick)とJS実装の検出結果が一致するかを突き合わせる */
  const compareWithRust = useCallback(async () => {
    const view = handleRef.current.view;
    if (!view) return;
    const text = view.state.doc.toString();
    try {
      const rust = await invoke<
        { name: string; start_utf16: number; end_utf16: number }[]
      >("find_mentions", { text, patterns });
      const js = findMentions(text, patterns);
      const rKey = rust
        .map((r) => `${r.start_utf16}-${r.end_utf16}:${r.name}`)
        .join("|");
      const jKey = js.map((j) => `${j.from}-${j.to}:${j.name}`).join("|");
      setRustCheck(
        rKey === jKey
          ? `一致 (${rust.length}件) — Rust/JS の位置計算が揃っている`
          : `不一致!\nRust: ${rKey}\nJS  : ${jKey}`,
      );
    } catch (e) {
      setRustCheck(`Tauriコマンド呼び出し失敗: ${String(e)}`);
    }
  }, [patterns]);

  const appendSample = useCallback(() => {
    const view = handleRef.current.view;
    if (!view) return;
    view.dispatch({
      changes: { from: view.state.doc.length, insert: "\n" + SAMPLE_TEXT },
    });
  }, []);

  const resultMarkdown = useMemo(() => {
    const lines = CHECKLIST.map((c) => {
      const v = checked[c.id];
      const mark = v === true ? "OK" : v === false ? "NG" : "未";
      return `- [${mark}] ${c.label}`;
    });
    const ngCount = CHECKLIST.filter((c) => checked[c.id] === false).length;
    const okCount = CHECKLIST.filter((c) => checked[c.id] === true).length;
    return [
      "## PoC#1 結果 (CodeMirror 6 × Windows日本語IME)",
      "",
      `- 判定: ${
        ngCount > 0
          ? "不合格(対応ラダーへ)"
          : okCount === CHECKLIST.length
            ? "合格"
            : "未完了"
      }`,
      `- OK ${okCount} / NG ${ngCount} / 未 ${CHECKLIST.length - okCount - ngCount}`,
      `- 計測: composition開始 ${metrics.compositionStart} / 終了 ${metrics.compositionEnd} / doc変更 ${metrics.docChanges} / 文字数 ${metrics.docLength} / 一致 ${metrics.mentionCount}`,
      `- Rust照合: ${rustCheck.split("\n")[0]}`,
      "",
      ...lines,
      "",
      "### メモ",
      note || "(なし)",
    ].join("\n");
  }, [checked, metrics, note, rustCheck]);

  return (
    <div className="app">
      <header className="app-header">
        <h1>PoC#1 — CodeMirror 6 × 日本語IME 検証</h1>
        <p className="sub">
          M0のゲート。合格条件は「素の状態での日本語長文入力」と「ハイライト装飾上でのIME変換」が崩れないこと。
        </p>
      </header>

      <div className="main">
        <section className="pane editor-pane">
          <div className="pane-head">
            <strong>本文</strong>
            <span className="metrics">
              {metrics.docLength}字 / 一致 {metrics.mentionCount}件 / IME{" "}
              {metrics.compositionStart}→{metrics.compositionEnd}
            </span>
          </div>
          <Editor
            initialDoc={SAMPLE_TEXT}
            patterns={patterns}
            highlightEnabled={highlightEnabled}
            hideWhileComposing={hideWhileComposing}
            onDocChange={onDocChange}
            onComposition={onComposition}
            handleRef={handleRef.current}
          />
        </section>

        <aside className="pane side-pane">
          <div className="block">
            <h2>操作</h2>
            <label className="check">
              <input
                type="checkbox"
                checked={highlightEnabled}
                onChange={(e) => setHighlightEnabled(e.target.checked)}
              />
              ハイライトを有効にする(②の検証はON、①はOFF)
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={hideWhileComposing}
                onChange={(e) => setHideWhileComposing(e.target.checked)}
              />
              IME変換中は装飾を隠す(対応ラダー①の効果確認)
            </label>
            <div className="row">
              <button onClick={appendSample}>サンプル文を末尾に追加</button>
              <button onClick={compareWithRust}>Rustの検出と照合</button>
            </div>
            <pre className="rust-check">{rustCheck}</pre>
          </div>

          <div className="block">
            <h2>codex名・別名(1行1件)</h2>
            <textarea
              className="patterns"
              value={patternText}
              onChange={(e) => setPatternText(e.target.value)}
              spellCheck={false}
            />
            <p className="hint">
              「佐藤架純」と別名「架純」が両方あっても二重にハイライトされない(最長一致)ことを確認する。
            </p>
          </div>

          <div className="block">
            <h2>チェックリスト</h2>
            {CHECKLIST.map((c) => (
              <div key={c.id} className="checkitem">
                <div className="checkbuttons">
                  <button
                    className={checked[c.id] === true ? "ok active" : "ok"}
                    onClick={() => setChecked((s) => ({ ...s, [c.id]: true }))}
                  >
                    OK
                  </button>
                  <button
                    className={checked[c.id] === false ? "ng active" : "ng"}
                    onClick={() => setChecked((s) => ({ ...s, [c.id]: false }))}
                  >
                    NG
                  </button>
                </div>
                <span>{c.label}</span>
              </div>
            ))}
            <textarea
              className="note"
              placeholder="気づいたこと(NGの再現手順など)"
              value={note}
              onChange={(e) => setNote(e.target.value)}
            />
            <button
              className="primary"
              onClick={() => navigator.clipboard.writeText(resultMarkdown)}
            >
              結果をMarkdownでコピー
            </button>
          </div>
        </aside>
      </div>
    </div>
  );
}
