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

export type Candidate = {
  name: string;
  kind: string;
  description: string;
  count: number;
};

export type ExtractResult = {
  candidates: Candidate[];
  rejected: number;
  chunks: number;
  elapsed_ms: number;
  warning: string | null;
};

export type TemplateInfo = {
  genre: string;
  kind: string;
};

export type AiSettings = {
  base_url: string;
  api_key: string | null;
  model: string;
  temperature: number;
  /** 校正で1回に送る本文の文字数 */
  check_chunk_chars: number;
};

export type ChatEvent =
  | { kind: "Delta"; value: string }
  | { kind: "Done" }
  | { kind: "Error"; value: string };

export const api = {
  openProject: (path: string) => invoke<OpenedProject>("open_project", { path }),
  refreshProject: () => invoke<OpenedProject>("refresh_project"),
  readFile: (path: string) => invoke<FileContent>("read_file", { path }),
  saveFile: (path: string, text: string) =>
    invoke<number>("save_file", { path, text }),
  createFile: (path: string, text: string) =>
    invoke<boolean>("create_file", { path, text }),
  fileModifiedMs: (path: string) =>
    invoke<number>("file_modified_ms", { path }),
  checkNotation: (text: string) =>
    invoke<NotationHit[]>("check_notation", { text }),
  extractEntities: (text: string) =>
    invoke<ExtractResult>("extract_entities", { text }),
  createCodexEntries: (candidates: Candidate[]) =>
    invoke<string[]>("create_codex_entries", { candidates }),
  proofreadAi: (text: string) =>
    invoke<AiProofreadResult>("proofread_ai", { text }),
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
  askAi: (
    context: ContextPreview,
    question: string,
    onEvent: (e: ChatEvent) => void,
  ) => {
    const channel = new Channel<ChatEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("ask_ai", { context, question, onEvent: channel });
  },
};
