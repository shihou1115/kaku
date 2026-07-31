/**
 * MVP(M1)の本体。3ペイン構成。
 *
 * 2026-07-26 ドッグフーディングの指摘を反映:
 *  - 保存忘れによる編集内容の消失 → **自動保存**(入力が止まった時/ファイル切替時/フォーカスを失った時)
 *  - ペース配分をユーザーが決められるよう **境界のドラッグでリサイズ**
 *  - 右ペインは常時表示ではなく **開閉可能**。AI専用にせず **参照タブ** を持つ
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  api,
  type AiSettings,
  type CodexEntry,
  type OpenedProject,
  type TreeNode,
} from "./api";
import { Editor, type EditorHandle } from "./editor/Editor";
import { findMentions } from "./editor/mentions";
import { FileTree } from "./components/FileTree";
import { AiPanel } from "./components/AiPanel";
import { NewFileDialog } from "./components/NewFileDialog";
import { ReferencePane } from "./components/ReferencePane";
import { ProofreadPane } from "./components/ProofreadPane";
import { ReviewPane } from "./components/ReviewPane";
import { ExtractPane } from "./components/ExtractPane";
import { ItemMenu, type MenuAction } from "./components/ItemMenu";
import { ViewMenu, type ViewSettings } from "./components/ViewMenu";
import { folderLabel } from "./components/folderLabels";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { PromptDialog } from "./components/PromptDialog";
import "./App.css";

/** テンプレートを使わない場合の中身 */
const PLAIN_SCENE = "---\ntitle: \n---\n\n";
const PLAIN_CODEX = "---\ntitle: \naliases: []\n---\n\n";

/** 入力が止まってから自動保存するまで */
const AUTOSAVE_DELAY_MS = 1200;

const LS = {
  leftW: "kaku.leftW",
  rightW: "kaku.rightW",
  rightOpen: "kaku.rightOpen",
  rightTab: "kaku.rightTab",
  view: "kaku.view",
};

const DEFAULT_VIEW: ViewSettings = {
  showLineNumbers: true,
  showRuler: false,
  wrapColumns: null,
};

function storedView(): ViewSettings {
  try {
    const raw = localStorage.getItem(LS.view);
    if (!raw) return DEFAULT_VIEW;
    const v = JSON.parse(raw) as Partial<ViewSettings>;
    return { ...DEFAULT_VIEW, ...v };
  } catch {
    return DEFAULT_VIEW;
  }
}

function storedNum(key: string, fallback: number): number {
  const v = Number(localStorage.getItem(key));
  return Number.isFinite(v) && v > 0 ? v : fallback;
}

const clamp = (v: number, lo: number, hi: number) =>
  Math.min(hi, Math.max(lo, v));

/**
 * 境界のドラッグ用。
 *
 * ドラッグ中は window でイベントを受ける。6pxの細い帯からポインタが外れても
 * 追従を切らさないため(素早く動かすと必ず外れる)。
 */
function Splitter({ onDrag }: { onDrag: (dx: number) => void }) {
  const last = useRef(0);
  const cb = useRef(onDrag);
  cb.current = onDrag;

  const begin = useCallback((clientX: number) => {
    last.current = clientX;
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - last.current;
      last.current = ev.clientX;
      if (dx !== 0) cb.current(dx);
    };
    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      document.body.classList.remove("resizing");
    };
    document.body.classList.add("resizing");
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
  }, []);

  return (
    <div
      className="splitter"
      role="separator"
      aria-orientation="vertical"
      onPointerDown={(e) => {
        e.preventDefault();
        begin(e.clientX);
      }}
    />
  );
}

type RightTab = "ai" | "ref" | "proof" | "review" | "extract";

export default function App() {
  const handleRef = useRef<EditorHandle>({
    view: null,
    load: () => {},
    openSearch: () => {},
    scrollTo: () => {},
    selectRange: () => {},
    replaceRange: () => {},
  });

  const [project, setProject] = useState<OpenedProject | null>(null);
  const [currentPath, setCurrentPath] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [savedText, setSavedText] = useState("");
  const [modifiedMs, setModifiedMs] = useState(0);
  const [status, setStatus] = useState("");
  const [highlightEnabled, setHighlightEnabled] = useState(true);
  const [view, setView] = useState<ViewSettings>(storedView);
  const [viewMenuOpen, setViewMenuOpen] = useState(false);
  const [newFileDir, setNewFileDir] = useState<string | null>(null);
  /** 項目メニュー */
  const [menu, setMenu] = useState<{ node: TreeNode; x: number; y: number } | null>(
    null,
  );
  /** 削除確認 */
  const [trashTarget, setTrashTarget] = useState<{
    node: TreeNode;
    count: number;
  } | null>(null);
  /** 名前入力(改名 / 新規フォルダー) */
  const [prompt, setPrompt] = useState<{
    mode: "rename" | "newFolder";
    node: TreeNode;
  } | null>(null);

  // ペインの幅と開閉
  const [leftW, setLeftW] = useState(() => storedNum(LS.leftW, 240));
  const [rightW, setRightW] = useState(() => storedNum(LS.rightW, 360));
  const [rightOpen, setRightOpen] = useState(
    () => localStorage.getItem(LS.rightOpen) !== "0",
  );
  const [rightTab, setRightTab] = useState<RightTab>(
    () => (localStorage.getItem(LS.rightTab) as RightTab) || "ai",
  );

  // 参照ペインの表示対象
  const [refPath, setRefPath] = useState<string | null>(null);
  const [refText, setRefText] = useState("");

  // AI設定はここで一元管理する。
  // 右ペインの各タブを常駐させるため、複数のペインが設定の写しを持つと
  // 古い値で上書きし合う。持ち主を1つにして防ぐ
  const [aiSettings, setAiSettings] = useState<AiSettings | null>(null);

  useEffect(() => {
    api.getAiSettings().then(setAiSettings).catch(() => {});
  }, []);

  const patchAiSettings = useCallback((p: Partial<AiSettings>) => {
    setAiSettings((prev) => {
      if (!prev) return prev;
      const next = { ...prev, ...p };
      api.setAiSettings(next).catch(() => {});
      return next;
    });
  }, []);

  // ===== AIの状態(ヘッダーに出す) =====
  //
  // 接続状態と実行状態は複数のペインに散らばると食い違うので、ここで一元管理する。
  // 実行中の表示があることで「動いているのか止まっているのか」が常に分かる

  const [aiConn, setAiConn] = useState<"unknown" | "ok" | "error">("unknown");
  const [models, setModels] = useState<string[]>([]);
  const [connDetail, setConnDetail] = useState("");
  /** 実行中の処理。複数同時に走りうるので発生源ごとに持つ */
  const [busyMap, setBusyMap] = useState<Record<string, string>>({});

  const setAiBusy = useCallback((source: string, label: string | null) => {
    setBusyMap((prev) => {
      if (label === null) {
        if (!(source in prev)) return prev;
        const next = { ...prev };
        delete next[source];
        return next;
      }
      return { ...prev, [source]: label };
    });
  }, []);

  const checkConnection = useCallback(async () => {
    setAiConn("unknown");
    setConnDetail("確認中…");
    try {
      const list = await api.listModels();
      setModels(list);
      setAiConn("ok");
      setConnDetail(`${list.length}モデル`);
      return list;
    } catch (e) {
      setAiConn("error");
      setConnDetail(String(e));
      return [];
    }
  }, []);

  // 起動時に一度だけ疎通を見る(接続先が応答しなければ即座に失敗する)
  useEffect(() => {
    if (!aiSettings) return;
    void checkConnection();
    // 設定の読み込み完了時に一度だけ
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [aiSettings !== null]);

  const busyLabels = Object.values(busyMap);
  const aiStatus = busyLabels.length > 0
    ? {
        kind: "busy" as const,
        text:
          busyLabels.length === 1
            ? busyLabels[0]
            : `${busyLabels[0]} 他${busyLabels.length - 1}件`,
      }
    : aiConn === "error"
      ? { kind: "error" as const, text: "未接続" }
      : !aiSettings?.model
        ? { kind: "warn" as const, text: "モデル未選択" }
        : aiConn === "ok"
          ? { kind: "ok" as const, text: "接続OK" }
          : { kind: "warn" as const, text: "未確認" };

  const dirty = text !== savedText;
  const codex: CodexEntry[] = project?.codex ?? [];

  useEffect(() => {
    localStorage.setItem(LS.leftW, String(leftW));
  }, [leftW]);
  useEffect(() => {
    localStorage.setItem(LS.rightW, String(rightW));
  }, [rightW]);
  useEffect(() => {
    localStorage.setItem(LS.rightOpen, rightOpen ? "1" : "0");
  }, [rightOpen]);
  useEffect(() => {
    localStorage.setItem(LS.rightTab, rightTab);
  }, [rightTab]);
  useEffect(() => {
    localStorage.setItem(LS.view, JSON.stringify(view));
  }, [view]);

  /** codexの正式名+別名をまとめたハイライト対象 */
  const patterns = useMemo(
    () => codex.flatMap((c) => [c.title, ...c.aliases]),
    [codex],
  );

  /** 本文に実際に登場したエントリ */
  const mentionedPaths = useMemo(() => {
    if (patterns.length === 0 || !text) return [];
    const names = new Set(findMentions(text, patterns).map((m) => m.name));
    return codex
      .filter((c) => [c.title, ...c.aliases].some((n) => names.has(n)))
      .map((c) => c.path);
  }, [text, patterns, codex]);

  // ===== 保存 =====

  // 保存処理から最新値を読むための箱(依存で関数を作り直さない)
  const live = useRef({ currentPath, text, savedText });
  live.current = { currentPath, text, savedText };
  /** 実行中の保存。切替時はこれを待ってから次に進む */
  const inflight = useRef<Promise<void> | null>(null);

  /** 未保存なら保存する。silent=true なら控えめに通知する */
  const flushSave = useCallback(async (silent: boolean): Promise<void> => {
    // 実行中の保存があれば必ず待つ(待たずに切り替えると保存が取りこぼされる)
    if (inflight.current) await inflight.current;
    const { currentPath: p, text: t, savedText: s } = live.current;
    if (!p || t === s) return;

    const task = (async () => {
      try {
        const ms = await api.saveFile(p, t);
        // 保存中に別ファイルへ移っていたら、その画面の状態は触らない
        if (live.current.currentPath === p) {
          setSavedText(t);
          setModifiedMs(ms);
        }
        setStatus(
          `${silent ? "自動保存" : "保存"}しました(${new Date().toLocaleTimeString()})`,
        );
      } catch (e) {
        setStatus(`保存に失敗しました: ${e}`);
      }
    })();
    inflight.current = task;
    try {
      await task;
    } finally {
      if (inflight.current === task) inflight.current = null;
    }
  }, []);

  /** 明示的な保存。ツリーの表示名やcodexの別名も更新する */
  const saveNow = useCallback(async () => {
    await flushSave(false);
    try {
      setProject(await api.refreshProject());
    } catch {
      /* 一覧の更新に失敗しても保存自体は済んでいる */
    }
  }, [flushSave]);

  // 入力が止まったら自動保存する(保存前に1世代のバックアップが残る)
  useEffect(() => {
    if (!currentPath || !dirty) return;
    const t = setTimeout(() => void flushSave(true), AUTOSAVE_DELAY_MS);
    return () => clearTimeout(t);
  }, [text, dirty, currentPath, flushSave]);

  // ウィンドウからフォーカスが外れたら保存(別アプリで作業して戻る流れを守る)
  useEffect(() => {
    const onBlur = () => void flushSave(true);
    window.addEventListener("blur", onBlur);
    return () => window.removeEventListener("blur", onBlur);
  }, [flushSave]);

  // 閉じる操作でも取りこぼさない。保存を待ってからウィンドウを破棄する
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    try {
      const w = getCurrentWindow();
      void w
        .onCloseRequested(async (e) => {
          if (live.current.text === live.current.savedText && !inflight.current) {
            return;
          }
          e.preventDefault();
          await flushSave(true);
          await w.destroy();
        })
        .then((f) => {
          unlisten = f;
        })
        .catch(() => {
          /* 権限が無い等。閉じる動作自体は妨げない */
        });
    } catch {
      // Tauri 外(ブラウザで開いた開発時)ではウィンドウAPIが無い
    }
    return () => unlisten?.();
  }, [flushSave]);

  // ===== ファイル操作 =====

  const openProject = useCallback(async () => {
    await flushSave(true);
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
      setRefPath(null);
      setRefText("");
      setStatus(`「${p.name}」を開きました`);
    } catch (e) {
      setStatus(String(e));
    }
  }, [flushSave]);

  const openFile = useCallback(
    async (path: string) => {
      // 切り替え前に必ず保存する(ここが編集内容を失う最大の場面だった)
      await flushSave(true);
      try {
        const f = await api.readFile(path);
        setCurrentPath(f.path);
        setText(f.text);
        setSavedText(f.text);
        setModifiedMs(f.modified_ms);
        handleRef.current.load(f.text);
        setStatus("");
        // 別名やタイトルの変更をハイライトへ反映する
        try {
          setProject(await api.refreshProject());
        } catch {
          /* noop */
        }
      } catch (e) {
        setStatus(String(e));
      }
    },
    [flushSave],
  );

  const createFile = useCallback(
    async (
      dirPath: string,
      fileName: string,
      genre: string | null,
      kind: string | null,
    ) => {
      const path = `${dirPath}/${fileName}`;
      const title = fileName.replace(/\.md$/, "");
      let content: string;
      if (genre && kind) {
        try {
          content = await api.renderTemplate(genre, kind, title);
        } catch (e) {
          setStatus(`テンプレートを使えませんでした(${e})`);
          content = dirPath.startsWith("codex") ? PLAIN_CODEX : PLAIN_SCENE;
        }
      } else {
        content = dirPath.startsWith("codex") ? PLAIN_CODEX : PLAIN_SCENE;
      }
      try {
        const created = await api.createFile(path, content);
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

  // ===== 項目メニューの操作 =====

  const handleMenu = useCallback(
    async (action: MenuAction, node: TreeNode) => {
      switch (action) {
        case "newFile":
          setNewFileDir(node.path);
          return;
        case "newFolder":
        case "rename":
          setPrompt({ mode: action, node });
          return;
        case "reveal":
          try {
            await api.revealInExplorer(node.path);
          } catch (e) {
            setStatus(String(e));
          }
          return;
        case "duplicate":
          try {
            const created = await api.duplicateEntry(node.path);
            setProject(await api.refreshProject());
            setStatus(`複製しました: ${created}`);
          } catch (e) {
            setStatus(String(e));
          }
          return;
        case "trash":
          try {
            const count = await api.countFiles(node.path);
            setTrashTarget({ node, count });
          } catch (e) {
            setStatus(String(e));
          }
          return;
      }
    },
    [],
  );

  const doTrash = useCallback(async () => {
    if (!trashTarget) return;
    const { node } = trashTarget;
    setTrashTarget(null);
    try {
      // 削除対象を開いていたら、保存を済ませてから閉じる
      const openedInside =
        currentPath === node.path ||
        (node.is_dir && currentPath?.startsWith(`${node.path}/`));
      if (openedInside) {
        await flushSave(true);
        setCurrentPath(null);
        setText("");
        setSavedText("");
        handleRef.current.load("");
      }
      const dest = await api.trashEntry(node.path);
      if (refPath === node.path) {
        setRefPath(null);
        setRefText("");
      }
      setProject(await api.refreshProject());
      setStatus(`ゴミ箱へ移しました(復元元: ${dest})`);
    } catch (e) {
      setStatus(String(e));
    }
  }, [trashTarget, currentPath, refPath, flushSave]);

  const doPrompt = useCallback(
    async (value: string) => {
      if (!prompt) return;
      const { mode, node } = prompt;
      setPrompt(null);
      try {
        if (mode === "newFolder") {
          const path = `${node.path}/${value}`;
          if (!(await api.createDir(path))) {
            setStatus("同名のフォルダーが既にあります");
            return;
          }
          setProject(await api.refreshProject());
          setStatus(`フォルダーを作りました: ${path}`);
          return;
        }
        // 改名: 拡張子を書かなかった場合はファイルなら .md を補う
        let name = value;
        if (!node.is_dir && !/\.[^./\\]+$/.test(name)) name = `${name}.md`;
        const dir = node.path.includes("/")
          ? node.path.slice(0, node.path.lastIndexOf("/"))
          : "";
        const to = dir ? `${dir}/${name}` : name;
        if (to === node.path) return;

        const wasOpen =
          currentPath === node.path ||
          (node.is_dir && currentPath?.startsWith(`${node.path}/`));
        if (wasOpen) await flushSave(true);

        await api.renameEntry(node.path, to);
        setProject(await api.refreshProject());

        if (currentPath === node.path) {
          setCurrentPath(to);
        } else if (node.is_dir && currentPath?.startsWith(`${node.path}/`)) {
          setCurrentPath(currentPath.replace(node.path, to));
        }
        setStatus(`名前を変更しました: ${to}`);
      } catch (e) {
        setStatus(String(e));
      }
    },
    [prompt, currentPath, flushSave],
  );

  // ===== 参照ペイン =====

  const showReference = useCallback(async (path: string) => {
    try {
      const f = await api.readFile(path);
      setRefPath(path);
      setRefText(f.text);
      setRightOpen(true);
      setRightTab("ref");
    } catch (e) {
      setStatus(String(e));
    }
  }, []);

  /** 本文中の設定名を Ctrl+クリックしたとき */
  const activateMention = useCallback(
    (name: string) => {
      const hit = codex.find((c) => c.title === name || c.aliases.includes(name));
      if (hit) void showReference(hit.path);
    },
    [codex, showReference],
  );

  // 参照中のファイルを編集した場合に備え、保存後は読み直す
  useEffect(() => {
    if (refPath && refPath === currentPath) setRefText(savedText);
  }, [savedText, refPath, currentPath]);

  // ===== 外部編集の検知(常駐監視はせず、フォーカス復帰時のみ) =====

  useEffect(() => {
    const onFocus = async () => {
      const p = live.current.currentPath;
      if (!p) return;
      try {
        const ms = await api.fileModifiedMs(p);
        if (ms === modifiedMs) return;
        if (live.current.text !== live.current.savedText) {
          setStatus(
            "このファイルはアプリ外で変更されました。未保存の変更があるため自動では読み込みません",
          );
          return;
        }
        const f = await api.readFile(p);
        setText(f.text);
        setSavedText(f.text);
        setModifiedMs(f.modified_ms);
        handleRef.current.load(f.text);
        setStatus("アプリ外の変更を読み込みました");
      } catch {
        /* ファイルが消えた等。次の操作でエラーになる */
      }
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [modifiedMs]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "s") {
        e.preventDefault();
        void saveNow();
      }
      // Ctrl+\ で右ペインの開閉
      if ((e.ctrlKey || e.metaKey) && e.key === "\\") {
        e.preventDefault();
        setRightOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [saveNow]);

  const charCount = text.replace(/\s/g, "").length;

  const gridTemplate = rightOpen
    ? `${leftW}px 6px 1fr 6px ${rightW}px`
    : `${leftW}px 6px 1fr`;

  return (
    <div className="app">
      <header className="app-header">
        <div className="title">
          <strong>{project ? project.name : "AI小説執筆支援ツール"}</strong>
          {currentPath && (
            <>
              <span className="path">{currentPath}</span>
              <button
                className={`savechip${dirty ? " dirty" : ""}`}
                onClick={saveNow}
                title="クリックで保存(Ctrl+S)。入力が止まると自動でも保存します"
              >
                {dirty ? "未保存" : "保存済み"}
              </button>
            </>
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
          <button
            className={`ai-status ${aiStatus.kind}`}
            title={
              connDetail
                ? `AI: ${aiStatus.text}(${connDetail})クリックで設定へ`
                : "クリックでAI設定へ"
            }
            onClick={() => {
              setRightOpen(true);
              setRightTab("ai");
            }}
          >
            <span className="ai-dot" />
            {aiStatus.text}
          </button>
          <div className="view-menu-anchor">
            <button
              className={viewMenuOpen ? "toggled" : ""}
              onClick={() => setViewMenuOpen((v) => !v)}
              title="行番号・ルーラー・折り返しの設定"
            >
              表示
            </button>
            {viewMenuOpen && (
              <ViewMenu
                value={view}
                onChange={setView}
                onClose={() => setViewMenuOpen(false)}
              />
            )}
          </div>
          <button onClick={() => handleRef.current.openSearch()}>検索</button>
          <button
            className={rightOpen ? "toggled" : ""}
            onClick={() => setRightOpen((v) => !v)}
            title="右ペインの表示切替(Ctrl+\)"
          >
            {rightOpen ? "▶ 閉じる" : "◀ 参照/AI"}
          </button>
          <button className="primary" onClick={openProject}>
            プロジェクトを開く
          </button>
        </div>
      </header>

      <div className="main" style={{ gridTemplateColumns: gridTemplate }}>
        <aside className="pane left">
          <FileTree
            tree={project?.tree ?? []}
            currentPath={currentPath}
            dirty={dirty}
            onOpen={openFile}
            onCreate={setNewFileDir}
            onMenu={(node, x, y) => setMenu({ node, x, y })}
          />
        </aside>

        <Splitter onDrag={(dx) => setLeftW((w) => clamp(w + dx, 160, 480))} />

        <section className="pane center">
          {/* エディタは常に描画しておく。display:none で隠すと寸法が測れず、
              ルーラーや折り返し幅の計算ができなくなるため */}
          {!project && (
            <div className="empty-state-overlay">
              <div className="empty-state">
                <h2>プロジェクトを開いてください</h2>
                <p>
                  空のフォルダーを選ぶと 原稿 / 設定 / プロット などの構成を作ります。
                  <br />
                  データはすべて普通のMarkdownファイルなので、他のエディタからも編集できます。
                </p>
                <button className="primary" onClick={openProject}>
                  フォルダを選ぶ
                </button>
              </div>
            </div>
          )}
          <Editor
              patterns={patterns}
              highlightEnabled={highlightEnabled}
              readOnly={!currentPath}
              showLineNumbers={view.showLineNumbers}
              showRuler={view.showRuler}
              wrapColumns={view.wrapColumns}
              onChange={setText}
              onSaveRequest={saveNow}
            onMentionActivate={activateMention}
            handleRef={handleRef.current}
          />
        </section>

        {rightOpen && (
          <>
            <Splitter
              onDrag={(dx) => setRightW((w) => clamp(w - dx, 260, 700))}
            />
            <aside className="pane right">
              <div className="tabs">
                <button
                  className={rightTab === "ai" ? "tab active" : "tab"}
                  onClick={() => setRightTab("ai")}
                >
                  AI相談
                </button>
                <button
                  className={rightTab === "ref" ? "tab active" : "tab"}
                  onClick={() => setRightTab("ref")}
                >
                  参照
                </button>
                <button
                  className={rightTab === "proof" ? "tab active" : "tab"}
                  onClick={() => setRightTab("proof")}
                >
                  校正
                </button>
                <button
                  className={rightTab === "review" ? "tab active" : "tab"}
                  onClick={() => setRightTab("review")}
                >
                  レビュー
                </button>
                <button
                  className={rightTab === "extract" ? "tab active" : "tab"}
                  onClick={() => setRightTab("extract")}
                >
                  抽出
                </button>
              </div>
              {/* 各タブは常に描画しておき、CSSで表示を切り替える。
                  条件描画にすると切替のたびに破棄され、実行中のAI処理の結果と
                  「実行中」の表示が失われる(60秒待った結果が消える) */}
              <div className="tab-body">
                <div
                  className="tab-pane"
                  style={{ display: rightTab === "ai" ? "block" : "none" }}
                >
                  <AiPanel
                    body={text}
                    mentionedPaths={mentionedPaths}
                    codex={codex}
                    disabled={!project}
                    settings={aiSettings}
                    onPatchSettings={patchAiSettings}
                    models={models}
                    connDetail={connDetail}
                    onCheckConnection={checkConnection}
                    onBusy={(l) => setAiBusy("chat", l)}
                    onShowReference={showReference}
                    onSaved={async (path, err) => {
                      if (err || !path) {
                        setStatus(err ?? "保存できませんでした");
                        return;
                      }
                      try {
                        setProject(await api.refreshProject());
                      } catch {
                        /* 一覧の更新に失敗しても保存自体は済んでいる */
                      }
                      setStatus(`着想に残しました: ${path}`);
                    }}
                  />
                </div>
                <div
                  className="tab-pane"
                  style={{ display: rightTab === "ref" ? "block" : "none" }}
                >
                  <ReferencePane
                    codex={codex}
                    mentionedPaths={mentionedPaths}
                    refPath={refPath}
                    refText={refText}
                    onSelect={showReference}
                    onOpenInEditor={(p) => void openFile(p)}
                    onClose={() => setRightOpen(false)}
                  />
                </div>
                <div
                  className="tab-pane"
                  style={{ display: rightTab === "extract" ? "block" : "none" }}
                >
                  <ExtractPane
                    body={text}
                    disabled={!currentPath}
                    onBusy={(l) => setAiBusy("extract", l)}
                    onCreated={async (paths) => {
                      try {
                        setProject(await api.refreshProject());
                        setStatus(
                          paths.length > 0
                            ? `${paths.length}件を設定に追加しました`
                            : "追加できるものがありませんでした(同名が既にあります)",
                        );
                      } catch (e) {
                        setStatus(String(e));
                      }
                    }}
                  />
                </div>
                <div
                  className="tab-pane"
                  style={{ display: rightTab === "review" ? "block" : "none" }}
                >
                  <ReviewPane
                    body={text}
                    disabled={!currentPath}
                    codex={codex}
                    mentionedPaths={mentionedPaths}
                    onBusy={(l) => setAiBusy("review", l)}
                    onJump={(from, to) =>
                      handleRef.current.selectRange(from, to)
                    }
                  />
                </div>
                <div
                  className="tab-pane"
                  style={{ display: rightTab === "proof" ? "block" : "none" }}
                >
                  <ProofreadPane
                    body={text}
                    disabled={!currentPath}
                    settings={aiSettings}
                    onPatchSettings={patchAiSettings}
                    onBusy={(l) => setAiBusy("proof", l)}
                    onJump={(from, to) =>
                      handleRef.current.selectRange(from, to)
                    }
                    onReplace={(from, to, t) =>
                      handleRef.current.replaceRange(from, to, t)
                    }
                  />
                </div>
              </div>
            </aside>
          </>
        )}
      </div>

      {menu && (
        <ItemMenu
          x={menu.x}
          y={menu.y}
          isDir={menu.node.is_dir}
          onPick={(action) => void handleMenu(action, menu.node)}
          onClose={() => setMenu(null)}
        />
      )}

      {trashTarget && (
        <ConfirmDialog
          title="削除の確認"
          message={
            trashTarget.node.is_dir
              ? `フォルダー「${folderLabel(trashTarget.node.path, trashTarget.node.name)}」を削除します(中のファイル ${trashTarget.count} 件も一緒に移動します)。`
              : `「${trashTarget.node.title || trashTarget.node.name}」を削除します。`
          }
          note={
            "完全には消えません。プロジェクト内の .app/trash/ へ移すので、必要ならエクスプローラーから元に戻せます。"
          }
          confirmLabel="ゴミ箱へ移す"
          danger
          onConfirm={() => void doTrash()}
          onCancel={() => setTrashTarget(null)}
        />
      )}

      {prompt && (
        <PromptDialog
          title={prompt.mode === "rename" ? "名前を変更" : "新しいフォルダー"}
          label={prompt.mode === "rename" ? "新しい名前" : "フォルダー名"}
          initial={prompt.mode === "rename" ? prompt.node.name : ""}
          selectStem={prompt.mode === "rename" && !prompt.node.is_dir}
          onSubmit={(v) => void doPrompt(v)}
          onCancel={() => setPrompt(null)}
        />
      )}

      {newFileDir !== null && (
        <NewFileDialog
          dirPath={newFileDir}
          onCancel={() => setNewFileDir(null)}
          onCreate={(fileName, genre, kind) => {
            const dir = newFileDir;
            setNewFileDir(null);
            void createFile(dir, fileName, genre, kind);
          }}
        />
      )}

      <footer className="status">{status}</footer>
    </div>
  );
}
