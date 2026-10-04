/** Rust側コマンドの型付きラッパー。UIから直接 invoke を呼ばない。 */

import { invoke, Channel } from "@tauri-apps/api/core";

export type TreeNode = {
  path: string;
  name: string;
  is_dir: boolean;
  title: string | null;
  children: TreeNode[];
};

export type CodexEntry = {
  path: string;
  title: string;
  aliases: string[];
  type_: string | null;
  description: string | null;
};

export type OpenedProject = {
  root: string;
  name: string;
  tree: TreeNode[];
  codex: CodexEntry[];
};

/**
 * フォルダーを開いた(サンプルを作った)結果。
 *
 * `NeedsConfirm` のときは**何も作っていない**。人が置いたものがあるフォルダーに
 * 骨組みを作る前に本人に聞き、了承を得たら `confirmed: true` で呼び直す(テスト計画 B2)
 */
export type OpenOutcome =
  | ({ kind: "Opened" } & OpenedProject)
  | { kind: "NeedsConfirm"; existing: number; will_create: string[] };

export type FileContent = {
  path: string;
  text: string;
  modified_ms: number;
};

export type RustMention = {
  name: string;
  start_byte: number;
  end_byte: number;
  start_utf16: number;
  end_utf16: number;
};

export type ContextEntry = {
  path: string;
  title: string;
  source: "mention" | "manual";
  text: string;
};

export type ContextPreview = {
  body: string;
  body_truncated: boolean;
  entries: ContextEntry[];
  dropped_entries: number;
  total_chars: number;
};

/** 全文検索の1件(ファイル粒度。位置は保存しない=D-7) */
export type SearchHit = {
  path: string;
  title: string;
  snippet: string;
  count: number;
};

export type SearchResult = {
  hits: SearchHit[];
  /** "fts" = trigram索引 / "like" = 3文字未満なので総当たり */
  method: "fts" | "like";
  reindexed: number;
  elapsed_ms: number;
};

export type NotationHit = {
  candidate: string;
  suggestion: string;
  confidence: "high" | "medium";
  reason: string;
  occurrences: { start_utf16: number; end_utf16: number }[];
  candidate_count: number;
  suggestion_count: number;
};

export type AiIssue = {
  quote: string;
  suggestion: string;
  kind: string;
  reason: string;
  found: boolean;
  start_utf16: number | null;
  end_utf16: number | null;
};

export type AiProofreadResult = {
  issues: AiIssue[];
  unchecked_chars: number;
  path: "schema" | "fallback";
  model: string;
  chunks: number;
  elapsed_ms: number;
  tokens_per_sec: number | null;
  warning: string | null;
};

/** シーンの切れ目の候補。**位置は保存せず、適用時に引用から引き直す**(D-7) */
export type SplitPoint = {
  quote: string;
  title: string;
  found: boolean;
  at_utf16: number | null;
  before: string;
  after: string;
  chars: number;
};

export type SplitSuggestion = {
  points: SplitPoint[];
  total_chars: number;
  model: string;
  elapsed_ms: number;
  warning: string | null;
};

/** レビューの観点。**5分類で固定**(02-requirements.md M-05)。増やさない */
export type ReviewAspect =
  | "style"
  | "structure"
  | "character"
  | "consistency"
  | "reader";

export const REVIEW_ASPECTS: { key: ReviewAspect; label: string; hint: string }[] =
  [
    { key: "style", label: "文章品質", hint: "文体・冗長・読みやすさ" },
    { key: "structure", label: "構成・テンポ", hint: "場面の要否・順序・配分" },
    { key: "character", label: "キャラクター", hint: "一貫性・動機・口調" },
    { key: "consistency", label: "設定整合性", hint: "設定資料との矛盾" },
    { key: "reader", label: "読者視点", hint: "分かりにくさ・引き" },
  ];

export type ReviewComment = {
  aspect: ReviewAspect;
  quote: string;
  comment: string;
  /** 直す方向。書き直した本文ではない */
  suggestion: string;
  found: boolean;
  start_utf16: number | null;
  end_utf16: number | null;
};

export type AiReviewResult = {
  comments: ReviewComment[];
  overall: string;
  aspects: ReviewAspect[];
  /** 一緒に渡した設定資料の名前(U-05) */
  materials: string[];
  unchecked_chars: number;
  path: "schema" | "fallback";
  model: string;
  chunks: number;
  elapsed_ms: number;
  tokens_per_sec: number | null;
  /** 1文字も返らなかった。検閲による拒否の疑い */
  refused: boolean;
  /** 応答を指定形式として読み取れなかった。**「指摘なし」ではない**。
   *  このとき overall は生の応答で、引用の照合ができていない */
  unparsed: boolean;
  warning: string | null;
};

export type Candidate = {
  name: string;
  kind: string;
  description: string;
  count: number;
  /** 同じ対象を指す別の呼び名(名寄せの成果) */
  aliases: string[];
  /** ある場合は既存エントリへの別名追加。新規作成ではない */
  existing_path: string | null;
};

export type ExtractResult = {
  candidates: Candidate[];
  /** 複数の対象に結び付いたため、どちらにも付けなかった呼び名 */
  conflicts: string[];
  rejected: number;
  chunks: number;
  elapsed_ms: number;
  warning: string | null;
};

export type TemplateInfo = {
  genre: string;
  kind: string;
};

/** AI相談の依頼テンプレート(M-03)。押すと依頼欄へ文面が入る */
export type PromptTemplate = {
  category: string;
  title: string;
  body: string;
};

export type AiSettings = {
  base_url: string;
  api_key: string | null;
  model: string;
  temperature: number;
  /** 校正で1回に送る本文の文字数 */
  check_chunk_chars: number;
};

/**
 * 会話の1往復(§5.7 会話モード)。
 *
 * **メモリ上にしか持たない。** アプリを閉じれば消える。残したいものは
 * 「この相談を残す」で `ideas/` へ書く(会話は原稿ではなく過程の副産物)。
 */
export type ChatTurn = {
  question: string;
  answer: string;
};

/**
 * 保存の結果。**競合はエラーではない**(T-08)。
 *
 * `Conflict` は「書いていない」ことを意味する。読み込んだ後に外部で
 * 書き換えられていたので、どうするかを人に決めてもらう。
 */
export type SaveOutcome =
  | { kind: "Saved"; modified_ms: number }
  | { kind: "Conflict"; actual_ms: number };

export type ChatEvent =
  | { kind: "Delta"; value: string }
  | { kind: "Done" }
  /** 中止された。完了(Done)とは区別する */
  | { kind: "Cancelled" }
  | { kind: "Error"; value: string };

export const api = {
  openProject: (path: string, confirmed = false) =>
    invoke<OpenOutcome>("open_project", { path, confirmed }),
  /** 前回開いたプロジェクトの場所。無い・消えている場合は null */
  lastProject: () => invoke<string | null>("last_project"),
  createSampleProject: (path: string, confirmed = false) =>
    invoke<OpenOutcome>("create_sample_project", { path, confirmed }),
  refreshProject: () => invoke<OpenedProject>("refresh_project"),
  readFile: (path: string) => invoke<FileContent>("read_file", { path }),
  /**
   * 保存する。`expectedMs` を渡すと、ディスク側がその時刻のままの場合だけ書く。
   * 外部編集を黙って踏み潰さないための楽観ロック(T-08)。
   */
  saveFile: (path: string, text: string, expectedMs: number | null) =>
    invoke<SaveOutcome>("save_file", { path, text, expectedMs }),
  createFile: (path: string, text: string) =>
    invoke<boolean>("create_file", { path, text }),
  checkNotation: (text: string) =>
    invoke<NotationHit[]>("check_notation", { text }),
  extractEntities: (text: string) =>
    invoke<ExtractResult>("extract_entities", { text }),
  createCodexEntries: (candidates: Candidate[]) =>
    invoke<string[]>("create_codex_entries", { candidates }),
  proofreadAi: (text: string) =>
    invoke<AiProofreadResult>("proofread_ai", { text }),
  suggestSceneSplit: (path: string) =>
    invoke<SplitSuggestion>("suggest_scene_split", { path }),
  /** 採用した切れ目で実際に分割する。元ファイルはゴミ箱へ移る */
  applySceneSplit: (
    path: string,
    firstTitle: string,
    points: { quote: string; title: string }[],
  ) => invoke<string[]>("apply_scene_split", { path, firstTitle, points }),
  reviewAi: (
    text: string,
    aspects: ReviewAspect[],
    mentionedPaths: string[],
    manualPaths: string[],
  ) =>
    invoke<AiReviewResult>("review_ai", {
      text,
      aspects,
      mentionedPaths,
      manualPaths,
    }),
  countFiles: (path: string) => invoke<number>("count_files", { path }),
  trashEntry: (path: string) => invoke<string>("trash_entry", { path }),
  renameEntry: (from: string, to: string) =>
    invoke<void>("rename_entry", { from, to }),
  duplicateEntry: (path: string) => invoke<string>("duplicate_entry", { path }),
  createDir: (path: string) => invoke<boolean>("create_dir", { path }),
  revealInExplorer: (path: string) =>
    invoke<void>("reveal_in_explorer", { path }),
  listTemplates: () => invoke<TemplateInfo[]>("list_templates"),
  renderTemplate: (genre: string, kind: string, title: string) =>
    invoke<string>("render_template", { genre, kind, title }),
  openTemplatesDir: () => invoke<string>("open_templates_dir"),
  listPrompts: () => invoke<PromptTemplate[]>("list_prompts"),
  openPromptsDir: () => invoke<string>("open_prompts_dir"),
  searchProject: (query: string) =>
    invoke<SearchResult>("search_project", { query }),
  findMentions: (text: string, patterns: string[]) =>
    invoke<RustMention[]>("find_mentions", { text, patterns }),
  getAiSettings: () => invoke<AiSettings>("get_ai_settings"),
  setAiSettings: (settings: AiSettings) =>
    invoke<void>("set_ai_settings", { settings }),
  listModels: () => invoke<string[]>("list_models"),
  buildContext: (
    body: string,
    mentionedPaths: string[],
    manualPaths: string[],
  ) =>
    invoke<ContextPreview>("build_context", {
      body,
      mentionedPaths,
      manualPaths,
    }),
  /**
   * 実行中のAI処理を中止する。
   *
   * 待っている応答はその場で捨てて止まる(接続も閉じる)。応答の始まりを
   * 待っている間(長い本文の読み込み中など)でも、すぐに止まる。
   */
  cancelAi: () => invoke<void>("cancel_ai"),
  /**
   * 相談を送る。`history` はこれまでの往復(会話モード)。
   * 単発の相談では空配列を渡す(§5.7)。
   */
  askAi: (
    context: ContextPreview,
    question: string,
    history: ChatTurn[],
    onEvent: (e: ChatEvent) => void,
  ) => {
    const channel = new Channel<ChatEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("ask_ai", {
      context,
      question,
      history,
      onEvent: channel,
    });
  },
};
