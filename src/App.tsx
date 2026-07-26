/**
 * MVP(M1)の本体。3ペイン構成。
 *
 * 実装範囲は docs/05-roadmap.md §2 の6要素:
 *  1. CM6でファイルを開いて編集・保存  2. ファイルツリー  3. codex名ハイライト
 *  4. AI相談(送信内容を事前提示)      5. 保存時1世代バックアップ(Rust側)
 *  6. ファイル内検索(Ctrl+F)
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { api, type CodexEntry, type OpenedProject } from "./api";
import { Editor, type EditorHandle } from "./editor/Editor";
import { findMentions } from "./editor/mentions";
import { FileTree } from "./components/FileTree";
import { AiPanel } from "./components/AiPanel";
import "./App.css";

const NEW_SCENE = "---\ntitle: \n---\n\n";
const NEW_CODEX = "---\ntitle: \naliases: []\n---\n\n";

export default function App() {
  const handleRef = useRef<EditorHandle>({
    view: null,
    load: () => {},
    openSearch: () => {},
    scrollTo: () => {},
  });

  const [project, setProject] = useState<OpenedProject | null>(null);
  const [currentPath, setCurrentPath] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [savedText, setSavedText] = useState("");
  const [modifiedMs, setModifiedMs] = useState(0);
  const [status, setStatus] = useState("");
  const [highlightEnabled, setHighlightEnabled] = useState(true);

  const dirty = text !== savedText;
  const codex: CodexEntry[] = project?.codex ?? [];

  /** codexの正式名+別名をまとめたハイライト対象 */
  const patterns = useMemo(
    () => codex.flatMap((c) => [c.title, ...c.aliases]),
    [codex],
  );

  /** 本文に実際に登場したエントリ(AIへ渡す候補) */
  const mentionedPaths = useMemo(() => {
    if (patterns.length === 0 || !text) return [];
    const names = new Set(findMentions(text, patterns).map((m) => m.name));
    return codex
      .filter((c) => [c.title, ...c.aliases].some((n) => names.has(n)))
      .map((c) => c.path);
  }, [text, patterns, codex]);

  const openProject = useCallback(async () => {
    const picked = await openDialog({
      directory: true,
      title: "小説プロジェクトのフォルダを選ぶ(空フォルダなら新規作成)",
    });
    if (typeof picked !== "string") return;
    try {
      const p = await api.openProject(picked);
      setProject(p);
      setCurrentPath(null);
      setText("");
      setSavedText("");
      setStatus(`「${p.name}」を開きました`);
    } catch (e) {
      setStatus(String(e));
    }
  }, []);

  const openFile = useCallback(
    async (path: string) => {
      if (dirty && !confirm("未保存の変更があります。破棄して開きますか?")) {
        return;
      }
      try {
        const f = await api.readFile(path);
        setCurrentPath(f.path);
        setText(f.text);
        setSavedText(f.text);
        setModifiedMs(f.modified_ms);
        handleRef.current.load(f.text);
        setStatus("");
      } catch (e) {
        setStatus(String(e));
      }
    },
    [dirty],
  );

  const save = useCallback(async () => {
    if (!currentPath) return;
    try {
      const ms = await api.saveFile(currentPath, text);
      setSavedText(text);
      setModifiedMs(ms);
      setStatus(`保存しました(${new Date().toLocaleTimeString()})`);
      // タイトル変更やcodex追加を反映する
      const p = await api.refreshProject();
      setProject(p);
    } catch (e) {
      setStatus(String(e));
    }
  }, [currentPath, text]);

  const createFile = useCallback(
    async (dirPath: string) => {
      const name = prompt(
        `${dirPath} に作るファイル名(.md は省略可)`,
        dirPath.startsWith("codex") ? "新しい設定" : "01-新しいシーン",
      );
      if (!name) return;
      const file = name.endsWith(".md") ? name : `${name}.md`;
      const path = `${dirPath}/${file}`;
      try {
        const created = await api.createFile(
          path,
          dirPath.startsWith("codex") ? NEW_CODEX : NEW_SCENE,
        );
        if (!created) {
          setStatus("同名のファイルが既にあります");
          return;
        }
        setProject(await api.refreshProject());
        await openFile(path);
      } catch (e) {
        setStatus(String(e));
      }
    },
    [openFile],
  );

  /** 外部編集の検知: 常駐監視はせず、ウィンドウにフォーカスが戻った時だけ確認する */
  useEffect(() => {
    const onFocus = async () => {
      if (!currentPath) return;
      try {
        const ms = await api.fileModifiedMs(currentPath);
        if (ms === modifiedMs) return;
        if (dirty) {
          setStatus(
            "このファイルはアプリ外で変更されました。未保存の変更があるため自動では読み込みません(別名で保存するか、破棄して開き直してください)",
          );
          return;
        }
        const f = await api.readFile(currentPath);
        setText(f.text);
        setSavedText(f.text);
        setModifiedMs(f.modified_ms);
        handleRef.current.load(f.text);
        setStatus("アプリ外の変更を読み込みました");
      } catch {
        /* ファイルが消えた等。次の操作でエラーを出す */
      }
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [currentPath, modifiedMs, dirty]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "s") {
        e.preventDefault();
        void save();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [save]);

  const charCount = text.replace(/\s/g, "").length;

  return (
    <div className="app">
      <header className="app-header">
        <div className="title">
          <strong>{project ? project.name : "AI小説執筆支援ツール"}</strong>
          {currentPath && (
            <span className="path">
              {currentPath}
              {dirty && " *"}
            </span>
          )}
        </div>
        <div className="tools">
          <label className="check">
            <input
              type="checkbox"
              checked={highlightEnabled}
              onChange={(e) => setHighlightEnabled(e.target.checked)}
            />
            設定名を強調
          </label>
          <span className="count">{charCount}字</span>
          <button onClick={() => handleRef.current.openSearch()}>検索</button>
          <button onClick={save} disabled={!currentPath || !dirty}>
            保存
          </button>
          <button className="primary" onClick={openProject}>
            プロジェクトを開く
          </button>
        </div>
      </header>

      <div className="main">
        <aside className="pane left">
          <FileTree
            tree={project?.tree ?? []}
            currentPath={currentPath}
            dirty={dirty}
            onOpen={openFile}
            onCreate={createFile}
          />
        </aside>

        <section className="pane center">
          {!project && (
            <div className="empty-state">
              <h2>プロジェクトを開いてください</h2>
              <p>
                空のフォルダを選ぶと manuscript / codex / plot などの構成を作ります。
                <br />
                データはすべて普通のMarkdownファイルなので、他のエディタからも編集できます。
              </p>
              <button className="primary" onClick={openProject}>
                フォルダを選ぶ
              </button>
            </div>
          )}
          <div style={{ display: project ? "contents" : "none" }}>
            <Editor
              patterns={patterns}
              highlightEnabled={highlightEnabled}
              readOnly={!currentPath}
              onChange={setText}
              onSaveRequest={save}
              handleRef={handleRef.current}
            />
          </div>
        </section>

        <aside className="pane right">
          <AiPanel
            body={text}
            mentionedPaths={mentionedPaths}
            codex={codex}
            disabled={!project}
          />
        </aside>
      </div>

      <footer className="status">{status}</footer>
    </div>
  );
}
