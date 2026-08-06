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
import { ProjectSearch } from "./components/ProjectSearch";
import { AiPanel } from "./components/AiPanel";
import { NewFileDialog } from "./components/NewFileDialog";
import { ReferencePane } from "./components/ReferencePane";
import { ProofreadPane } from "./components/ProofreadPane";
import { ReviewPane } from "./components/ReviewPane";
import { ExtractPane } from "./components/ExtractPane";
import { ItemMenu, type MenuAction } from "./components/ItemMenu";
import { PopupMenu } from "./components/PopupMenu";
import { ViewMenu, type ViewSettings } from "./components/ViewMenu";
import { applyTheme, watchDeviceTheme } from "./theme";
import { folderLabel, isTrashPath } from "./components/folderLabels";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { PromptDialog } from "./components/PromptDialog";
import { HelpDialog } from "./components/HelpDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { RubyPreview } from "./components/RubyPreview";
import {
  canWrap as canWrapRuby,
  strip as stripRuby,
  wrap as wrapRuby,
} from "./ruby";
import { SplitDialog } from "./components/SplitDialog";
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
  /** 設定名の強調。表示の設定と同じ置き場所にする(§5.11 の集約で保存対象へ入れた) */
  highlight: "kaku.highlight",
};

const DEFAULT_VIEW: ViewSettings = {
  showLineNumbers: true,
  showRuler: false,
  wrapColumns: null,
  // 既定はライト(§5.15)。初回起動でデバイス設定に追従はしない
  theme: "light",
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
    toggleSearch: () => {},
    scrollTo: () => {},
    selectRange: () => {},
    replaceRange: () => {},
    getSelection: () => null,
  });

  const [project, setProject] = useState<OpenedProject | null>(null);
  const [currentPath, setCurrentPath] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [savedText, setSavedText] = useState("");
  const [modifiedMs, setModifiedMs] = useState(0);
  const [status, setStatus] = useState("");
  // 起動のたびに戻ると「切ったつもりが戻っている」になる(§5.11 論点3)
  const [highlightEnabled, setHighlightEnabled] = useState(
    () => localStorage.getItem(LS.highlight) !== "0",
  );
  const [view, setView] = useState<ViewSettings>(storedView);
  const [viewMenuOpen, setViewMenuOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  /** 本文で選択して右クリックしたときのメニュー位置 */
  const [selMenu, setSelMenu] = useState<{ x: number; y: number } | null>(null);
  /** ルビの入力補助。読みを聞いている間、対象の範囲を保持する */
  const [rubyTarget, setRubyTarget] = useState<{
    from: number;
    to: number;
    base: string;
  } | null>(null);
  /** シーン分割の対象。開いている間だけ提案を出す */
  const [splitTarget, setSplitTarget] = useState<TreeNode | null>(null);
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

  /**
   * 実行中表示のコールバック。**毎回作り直さない**。
   *
   * インラインの矢印関数で渡すと、再描画のたびに別の関数になる。
   * 受け取った側がそれを useEffect の依存に含めていると
   * 「呼ぶ → 親が再描画 → 別物になる → 依存が変わる → また呼ぶ」の無限ループになり、
   * AIへリクエストを撃ち続ける(2026-08-03 にシーン分割で実際に起きた)。
   * 渡す側で identity を固定しておけば、この形の事故は起きない。
   */
  const busy = useMemo(
    () => ({
      chat: (l: string | null) => setAiBusy("chat", l),
      proof: (l: string | null) => setAiBusy("proof", l),
      review: (l: string | null) => setAiBusy("review", l),
      extract: (l: string | null) => setAiBusy("extract", l),
      split: (l: string | null) => setAiBusy("split", l),
    }),
    [setAiBusy],
  );

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

  /**
   * ゴミ箱の中を開いているか。**アプリからは読み取り専用**にする。
   *
   * 退避しておいたものが書き換わると、取っておいた意味が消える。
   * 書き込み自体は Rust 側(project::is_app_area)でも塞いであるが、
   * **編集できてしまう画面を見せてから保存で断る**のは最悪の体験なので、
   * 入口から編集させない。
   */
  const inTrash = currentPath !== null && isTrashPath(currentPath);
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
  useEffect(() => {
    localStorage.setItem(LS.highlight, highlightEnabled ? "1" : "0");
  }, [highlightEnabled]);

  // 配色を当てる。「デバイス設定」を選んでいる間だけOSの切替に追従する
  useEffect(() => {
    applyTheme(view.theme);
    return watchDeviceTheme(view.theme, () => {});
  }, [view.theme]);

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

  /**
   * 起動時に前回のプロジェクトを開き直す。
   *
   * 毎回フォルダを選び直させない(P-8: ツール自体の作業負荷を最小にする)。
   * **開けなければ黙って空状態のまま**にする — 起動を止めてまで伝えることではないし、
   * フォルダを移した/消した本人にとっては警告が出る方が煩わしい。
   * 一度だけ走らせる(ref の門番)。ここを依存つきにすると開き直しが繰り返される
   */
  const restoredRef = useRef(false);
  useEffect(() => {
    if (restoredRef.current) return;
    restoredRef.current = true;
    (async () => {
      try {
        const path = await api.lastProject();
        if (!path) return;
        const p = await api.openProject(path);
        setProject(p);
        setStatus(`「${p.name}」を開きました`);
      } catch {
        // 覚えていない / 開けなくなっている / Tauri外(ブラウザでの開発時)
      }
    })();
  }, []);

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

  /**
   * サンプルを作って開く(M-08)。
   *
   * 空のプロジェクトでは何ができるアプリなのか分からない。
   * **AI未接続でも試せる**素材(ハイライトと表記ゆれ検出)を入れてある
   */
  const openSample = useCallback(async () => {
    await flushSave(true);
    const picked = await openDialog({
      directory: true,
      title: "サンプルを作るフォルダを選ぶ(空のフォルダを推奨)",
    });
    if (typeof picked !== "string") return;
    try {
      const p = await api.createSampleProject(picked);
      setProject(p);
      setCurrentPath(null);
      setText("");
      setSavedText("");
      setRefPath(null);
      setRefText("");
      setStatus(
        "サンプルを作りました。左の「はじめに」から試してみてください",
      );
    } catch (e) {
      setStatus(String(e));
    }
  }, [flushSave]);

  /** ファイルを開く。開いた本文を返す(検索から一致箇所へ飛ぶのに使う) */
  const openFile = useCallback(
    async (path: string): Promise<string | null> => {
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
        return f.text;
      } catch (e) {
        setStatus(String(e));
        return null;
      }
    },
    [flushSave],
  );

  /**
   * 検索結果から開く。開いたうえで**最初の一致箇所へ飛ぶ**。
   *
   * 位置は索引に持たない(D-7)ので、開いた本文から都度探す。
   * 索引と本文がずれていても、ここでずれた位置へ飛ぶことはない
   */
  const openFromSearch = useCallback(
    async (path: string, needle: string) => {
      const text = await openFile(path);
      if (!text || !needle) return;
      const at = text.indexOf(needle);
      if (at >= 0) handleRef.current.selectRange(at, at + needle.length);
    },
    [openFile],
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
        case "split":
          setSplitTarget(node);
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

  /** AIの出力を正本へ残したあと(相談=ideas / 講評=reviews で共通) */
  const noteSaved = useCallback(
    async (label: string, path: string | null, err?: string) => {
      if (err || !path) {
        setStatus(err ?? "保存できませんでした");
        return;
      }
      try {
        setProject(await api.refreshProject());
      } catch {
        /* 一覧の更新に失敗しても保存自体は済んでいる */
      }
      setStatus(`${label}に残しました: ${path}`);
    },
    [],
  );

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

  // 記法込みで数えると投稿サイトの字数と合わない(ルビは読者が読む字数には入らない)
  const charCount = stripRuby(text).replace(/\s/g, "").length;

  /**
   * 選択した語にルビを振る(入力補助)。
   *
   * 選択が無ければ何もできないので、その旨だけ伝える。
   * 読みは入力してもらってから、選択範囲を `｜漢字《かんじ》` に置き換える。
   */
  const startRuby = useCallback(() => {
    if (!currentPath || inTrash) return;
    const sel = handleRef.current.getSelection();
    if (!sel) {
      setStatus("ルビを振る語を本文で選んでください");
      return;
    }
    // **既にあるルビと重なっていたら振らせない。** 重ねると記法が入れ子になって壊れる
    if (!canWrapRuby(text, sel.from, sel.to)) {
      setStatus("すでにルビが振られている箇所には重ねられません");
      return;
    }
    setRubyTarget({ from: sel.from, to: sel.to, base: sel.text });
  }, [currentPath, inTrash, text]);

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
              {inTrash ? (
                <span
                  className="savechip readonly"
                  title="ゴミ箱の中のファイルです。アプリからは編集できません(エクスプローラーから元に戻せます)"
                >
                  読み取り専用
                </span>
              ) : (
                <button
                  className={`savechip${dirty ? " dirty" : ""}`}
                  onClick={saveNow}
                  title="クリックで保存(Ctrl+S)。入力が止まると自動でも保存します"
                >
                  {dirty ? "未保存" : "保存済み"}
                </button>
              )}
            </>
          )}
        </div>
        <div className="tools">
          <span className="count">{charCount}字</span>
          <button
            className={`ai-status ${aiStatus.kind}`}
            title={
              connDetail
                ? `AI: ${aiStatus.text}(${connDetail})クリックで設定へ`
                : "クリックでAI設定へ"
            }
            onClick={() => setSettingsOpen(true)}
          >
            <span className="ai-dot" />
            {aiStatus.text}
          </button>
          {/* アプリの動作に関する設定はここ1つに集約する(§5.11 案A) */}
          <button
            onClick={() => setSettingsOpen(true)}
            title="AI接続・校正・本文の表示などの設定"
          >
            設定
          </button>
          <div className="view-menu-anchor">
            <button
              className={viewMenuOpen ? "toggled" : ""}
              onClick={() => setViewMenuOpen((v) => !v)}
              title="行番号・ルーラー・折り返しの設定(書きながら変えるもの)"
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
          <button
            onClick={() => handleRef.current.toggleSearch()}
            title="開いているファイル内を検索(Ctrl+F)。もう一度押すと閉じます"
          >
            検索
          </button>
          <button
            onClick={startRuby}
            disabled={!currentPath || inTrash}
            title="選んだ語にルビを振る(｜漢字《かんじ》)"
          >
            ルビ
          </button>
          <button
            onClick={() => setPreviewOpen(true)}
            disabled={!currentPath}
            title="ルビの見え方を確認する"
          >
            プレビュー
          </button>
          <button
            onClick={() => setHelpOpen(true)}
            title="使い方・AIの接続・知っておくこと"
            aria-label="ヘルプ"
          >
            ?
          </button>
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
          {/* ファイル内検索(Ctrl+F)とは別物。あちらは開いている原稿の中、
              こちらは「どのファイルにあるか」を探す */}
          <ProjectSearch
            disabled={!project}
            onOpen={(p, needle) => void openFromSearch(p, needle)}
          />
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
                <div className="empty-actions">
                  <button className="primary" onClick={openProject}>
                    フォルダを選ぶ
                  </button>
                  {/* 空状態がチュートリアルを兼ねる(U-08)。
                      何ができるアプリなのかは、触らないと分からない */}
                  <button onClick={() => void openSample()}>
                    サンプルを試す
                  </button>
                </div>
                <p className="empty-note">
                  はじめてなら「サンプルを試す」から。
                  <strong>AIに接続していなくても</strong>
                  ハイライトと表記ゆれの検出は動きます。
                  <br />
                  使い方は右上の「?」にまとめてあります。
                </p>
              </div>
            </div>
          )}
          <Editor
              patterns={patterns}
              highlightEnabled={highlightEnabled}
              readOnly={!currentPath || inTrash}
              showLineNumbers={view.showLineNumbers}
              showRuler={view.showRuler}
              wrapColumns={view.wrapColumns}
              onChange={setText}
              onSaveRequest={saveNow}
            onMentionActivate={activateMention}
            onSelectionMenu={(x, y) => {
              // 振れない場所では**メニューを出さない**。
              // 出してから断るのは、押せる操作が押せなかったのと同じで紛らわしい
              const sel = handleRef.current.getSelection();
              if (sel && !canWrapRuby(text, sel.from, sel.to)) {
                setStatus("すでにルビが振られている箇所には重ねられません");
                return;
              }
              setSelMenu({ x, y });
            }}
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
                    currentPath={currentPath}
                    mentionedPaths={mentionedPaths}
                    codex={codex}
                    disabled={!project}
                    settings={aiSettings}
                    onBusy={busy.chat}
                    onShowReference={showReference}
                    onSaved={(p, e) => void noteSaved("着想", p, e)}
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
                    onBusy={busy.extract}
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
                    currentPath={currentPath}
                    disabled={!currentPath}
                    codex={codex}
                    mentionedPaths={mentionedPaths}
                    onBusy={busy.review}
                    onJump={(from, to) =>
                      handleRef.current.selectRange(from, to)
                    }
                    onSaved={(p, e) => void noteSaved("講評", p, e)}
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
                    onBusy={busy.proof}
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

      {/* 本文を選んで右クリック。ヘッダーまで戻らずにルビを振れるようにする */}
      {selMenu && (
        <PopupMenu
          x={selMenu.x}
          y={selMenu.y}
          items={[{ key: "ruby", label: "ルビを挿入する…" }]}
          onPick={() => startRuby()}
          onClose={() => setSelMenu(null)}
        />
      )}

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

      {helpOpen && <HelpDialog onClose={() => setHelpOpen(false)} />}

      {settingsOpen && (
        <SettingsDialog
          settings={aiSettings}
          onPatch={patchAiSettings}
          models={models}
          connDetail={connDetail}
          onCheckConnection={checkConnection}
          highlightEnabled={highlightEnabled}
          onHighlightChange={setHighlightEnabled}
          onClose={() => setSettingsOpen(false)}
        />
      )}

      {previewOpen && (
        <RubyPreview
          title={currentPath ?? ""}
          text={text}
          onClose={() => setPreviewOpen(false)}
        />
      )}

      {rubyTarget && (
        <PromptDialog
          title="ルビを振る"
          label={`「${rubyTarget.base}」の読み`}
          initial=""
          onSubmit={(reading) => {
            const t = rubyTarget;
            setRubyTarget(null);
            handleRef.current.replaceRange(
              t.from,
              t.to,
              wrapRuby(t.base, reading),
            );
          }}
          onCancel={() => setRubyTarget(null)}
        />
      )}

      {splitTarget && (
        <SplitDialog
          path={splitTarget.path}
          title={splitTarget.title || splitTarget.name.replace(/\.md$/, "")}
          onBusy={busy.split}
          onClose={() => setSplitTarget(null)}
          onDone={async (created, err) => {
            if (err || !created) {
              setStatus(err ?? "分割できませんでした");
              return;
            }
            // 元ファイルを開いていたら閉じる(ゴミ箱へ移っている)
            if (currentPath === splitTarget.path) {
              setCurrentPath(null);
              setText("");
              setSavedText("");
              handleRef.current.load("");
            }
            try {
              setProject(await api.refreshProject());
            } catch {
              /* 一覧の更新に失敗しても分割自体は済んでいる */
            }
            setStatus(
              `${created.length - 1}個に分けました(元は ${created[created.length - 1]} へ)`,
            );
          }}
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
