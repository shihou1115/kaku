//! Tauri アプリのエントリポイント。
//!
//! 方針(docs/04-design.md §2): ドメインロジック(解析・索引・検出)は Rust 側に置き、
//! フロントは表示と編集に徹する。

// ドメインロジックは統合テスト(tests/flow.rs)からも叩けるよう公開する
pub mod ai;
pub mod context;
pub mod extract;
pub mod frontmatter;
pub mod mentions;
pub mod project;
pub mod proofread;
pub mod review;
pub mod settings;
pub mod templates;

use std::path::PathBuf;
use std::sync::Mutex;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;

use ai::ChatMessage;
use context::ContextPreview;
use mentions::Mention;
use project::{CodexEntry, ProjectError, TreeNode};

// ===== 状態 =====

#[derive(Default)]
struct AppState {
    /// 開いているプロジェクトのルート。未選択なら None
    root: Mutex<Option<PathBuf>>,
    ai: Mutex<AiSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AiSettings {
    base_url: String,
    api_key: Option<String>,
    model: String,
    temperature: f32,
    /// 校正で1回に送る本文の文字数(PoC#7 §7.2)。
    /// 小さいと実行回数が増えて遅く、大きいとコンテキストを超えて打ち切られる
    #[serde(default = "default_chunk_chars")]
    check_chunk_chars: usize,
}

fn default_chunk_chars() -> usize {
    proofread::CHUNK_CHARS
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            base_url: ai::DEFAULT_BASE_URL.to_string(),
            api_key: None,
            model: String::new(),
            temperature: 0.7,
            check_chunk_chars: proofread::CHUNK_CHARS,
        }
    }
}

impl AiSettings {
    /// 保存済みの設定を反映する。**APIキーは保存されないのでメモリ上の値を保つ**
    fn from_stored(stored: settings::StoredSettings) -> Self {
        Self {
            base_url: stored.base_url,
            api_key: None,
            model: stored.model,
            temperature: stored.temperature,
            check_chunk_chars: settings::clamp_chunk_chars(stored.check_chunk_chars),
        }
    }

    fn to_stored(&self) -> settings::StoredSettings {
        settings::StoredSettings {
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            temperature: self.temperature,
            check_chunk_chars: self.check_chunk_chars,
        }
    }
}

fn root_of(state: &State<AppState>) -> Result<PathBuf, String> {
    state
        .root
        .lock()
        .map_err(|_| "状態の取得に失敗しました".to_string())?
        .clone()
        .ok_or_else(|| "プロジェクトが開かれていません".to_string())
}

fn to_msg(e: ProjectError) -> String {
    e.to_string()
}

// ===== プロジェクト =====

#[derive(Serialize)]
struct OpenedProject {
    root: String,
    name: String,
    tree: Vec<TreeNode>,
    codex: Vec<CodexEntry>,
}

/// 指定フォルダをプロジェクトとして開く。空フォルダなら初期構成を作る。
#[tauri::command]
fn open_project(path: String, state: State<AppState>) -> Result<OpenedProject, String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("フォルダが見つかりません: {path}"));
    }
    // manuscript/ が無ければ新規プロジェクトとして初期化する
    if !root.join("manuscript").is_dir() {
        project::init(&root).map_err(to_msg)?;
    }
    let tree = project::scan(&root).map_err(to_msg)?;
    let codex = project::load_codex(&root).map_err(to_msg)?;
    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    *state.root.lock().map_err(|_| "状態の更新に失敗")? = Some(root.clone());
    Ok(OpenedProject {
        root: root.to_string_lossy().to_string(),
        name,
        tree,
        codex,
    })
}

/// ツリーと codex を読み直す(ファイル作成後などに呼ぶ)
#[tauri::command]
fn refresh_project(state: State<AppState>) -> Result<OpenedProject, String> {
    let root = root_of(&state)?;
    Ok(OpenedProject {
        root: root.to_string_lossy().to_string(),
        name: root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        tree: project::scan(&root).map_err(to_msg)?,
        codex: project::load_codex(&root).map_err(to_msg)?,
    })
}

#[derive(Serialize)]
struct FileContent {
    path: String,
    text: String,
    modified_ms: u64,
}

#[tauri::command]
fn read_file(path: String, state: State<AppState>) -> Result<FileContent, String> {
    let root = root_of(&state)?;
    let text = project::read_text(&root, &path).map_err(to_msg)?;
    let modified_ms = project::modified_ms(&root, &path).map_err(to_msg)?;
    Ok(FileContent {
        path,
        text,
        modified_ms,
    })
}

/// 保存(保存前に1世代のバックアップを取る)
#[tauri::command]
fn save_file(path: String, text: String, state: State<AppState>) -> Result<u64, String> {
    let root = root_of(&state)?;
    project::write_text(&root, &path, &text).map_err(to_msg)?;
    project::modified_ms(&root, &path).map_err(to_msg)
}

#[tauri::command]
fn create_file(path: String, text: String, state: State<AppState>) -> Result<bool, String> {
    let root = root_of(&state)?;
    project::create_file(&root, &path, &text).map_err(to_msg)
}

/// 外部編集の検知用(常駐監視はしない。フロントがフォーカス復帰時に呼ぶ)
#[tauri::command]
fn file_modified_ms(path: String, state: State<AppState>) -> Result<u64, String> {
    let root = root_of(&state)?;
    project::modified_ms(&root, &path).map_err(to_msg)
}

/// 削除対象の件数(確認ダイアログで「何が消えるか」を見せるため)
#[tauri::command]
fn count_files(path: String, state: State<AppState>) -> Result<usize, String> {
    let root = root_of(&state)?;
    project::count_files(&root, &path).map_err(to_msg)
}

/// 削除。**消さずにプロジェクト内のゴミ箱へ移す**。戻り値は退避先(UIで案内する)
#[tauri::command]
fn trash_entry(path: String, state: State<AppState>) -> Result<String, String> {
    let root = root_of(&state)?;
    project::trash(&root, &path).map_err(to_msg)
}

/// 改名・移動。プロジェクト内のMarkdownリンクも追随する
#[tauri::command]
fn rename_entry(from: String, to: String, state: State<AppState>) -> Result<(), String> {
    let root = root_of(&state)?;
    project::rename(&root, &from, &to).map_err(to_msg)
}

#[tauri::command]
fn duplicate_entry(path: String, state: State<AppState>) -> Result<String, String> {
    let root = root_of(&state)?;
    project::duplicate(&root, &path).map_err(to_msg)
}

#[tauri::command]
fn create_dir(path: String, state: State<AppState>) -> Result<bool, String> {
    let root = root_of(&state)?;
    project::create_dir(&root, &path).map_err(to_msg)
}

/// エクスプローラで開く(「ファイルが正」を体感させる導線)
#[tauri::command]
fn reveal_in_explorer(
    path: String,
    app: tauri::AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let root = root_of(&state)?;
    let target = project::resolve(&root, &path).map_err(to_msg)?;
    // ファイルなら親フォルダを開く
    let dir = if target.is_dir() {
        target
    } else {
        target.parent().map(|p| p.to_path_buf()).unwrap_or(root)
    };
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

// ===== テンプレート =====

/// 利用可能なテンプレート一覧(ユーザーが足したジャンルも含む)
#[tauri::command]
fn list_templates() -> Result<Vec<templates::TemplateInfo>, String> {
    templates::ensure_defaults().map_err(|e| e.to_string())?;
    templates::list().map_err(|e| e.to_string())
}

/// テンプレートを適用した新規ファイルの中身を返す
#[tauri::command]
fn render_template(genre: String, kind: String, title: String) -> Result<String, String> {
    templates::render(&genre, &kind, &title).map_err(|e| {
        format!("テンプレートを読めませんでした({genre}/{kind}): {e}")
    })
}

/// テンプレート置き場をエクスプローラで開く(ユーザーが自由に編集・追加できる)
#[tauri::command]
fn open_templates_dir(app: tauri::AppHandle) -> Result<String, String> {
    use tauri_plugin_opener::OpenerExt;
    templates::ensure_defaults().map_err(|e| e.to_string())?;
    let dir = templates::templates_dir();
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().to_string())
}

// ===== 言及検出 =====

/// 本文から codex の名前・別名の出現箇所を返す(M-02)。
/// 位置は永続化せず、呼ばれるたびに算出する(docs/03-data-format.md D-7)。
#[tauri::command]
fn find_mentions(text: String, patterns: Vec<String>) -> Vec<Mention> {
    mentions::find_mentions(&text, &patterns)
}

// ===== 校正 =====

/// 設定DB連携の固有名詞チェック(M-04-02)。
///
/// **機械照合のみでLLMを使わない**(docs/06-decision-log.md §2-5)。
/// 位置は永続化せず、呼ばれるたびに算出する。
#[tauri::command]
fn check_notation(
    text: String,
    state: State<AppState>,
) -> Result<Vec<proofread::NotationHit>, String> {
    let root = root_of(&state)?;
    let codex = project::load_codex(&root).map_err(to_msg)?;
    let names: Vec<String> = codex.iter().flat_map(|c| c.patterns()).collect();
    Ok(proofread::check_notation(&text, &names))
}

#[derive(Serialize)]
struct AiProofreadResult {
    issues: Vec<proofread::AiIssue>,
    /// 本文が長くて末尾を切り捨てた場合の未検査文字数(0なら全文を見た)
    unchecked_chars: usize,
    /// "schema" = 構造化出力が通った / "fallback" = 寛容パースで拾った
    path: String,
    model: String,
    /// 検査した塊の数(PoC#7: 長文は分割しないと検知率が落ちる)
    chunks: usize,
    /// 所要時間。ローカルLLMの速度をユーザーが判断できるようにする
    elapsed_ms: u64,
    /// 出力トークン/秒(usageを返さないモデルでは None)
    tokens_per_sec: Option<f64>,
    /// 一部の塊で失敗した場合の断り書き
    warning: Option<String>,
}

/// 誤字脱字チェック(M-04-01)。
///
/// 経路A(json_schema)で試し、失敗したら経路B(スキーマ無し+寛容パース)へ落とす
/// (docs/04-design.md §6.3)。どちらで取れたかは結果に含めて可視化する。
#[tauri::command]
async fn proofread_ai(
    text: String,
    state: State<'_, AppState>,
) -> Result<AiProofreadResult, String> {
    let s = state.ai.lock().map_err(|_| "状態の取得に失敗")?.clone();
    if s.model.trim().is_empty() {
        return Err("モデルが未設定です。AI相談タブの設定で接続してください".to_string());
    }
    let root = root_of(&state)?;
    let codex = project::load_codex(&root).map_err(to_msg)?;
    let names: Vec<String> = codex.iter().flat_map(|c| c.patterns()).collect();

    // 長文はまとめて投げると応答が打ち切られることがあるので分割して順に検査する
    // (PoC#7 §7.1)。分割字数は環境によって最適値が違うため設定可能(§7.2)
    let all_chunks = proofread::split_for_check_with(&text, s.check_chunk_chars);
    let total_chunks = all_chunks.len();
    let chunks: Vec<String> = all_chunks
        .into_iter()
        .take(proofread::MAX_CHUNKS)
        .collect();
    let unchecked_chars = if total_chunks > chunks.len() {
        text.chars().count() - chunks.iter().map(|c| c.chars().count()).sum::<usize>()
    } else {
        0
    };

    let started = std::time::Instant::now();
    let mut path = "schema";
    let mut collected = Vec::new();
    let mut completion_tokens = 0u64;
    let mut failures = 0usize;
    // 応答が打ち切られた塊の数。「指摘なし」と取り違えると誤報告になる
    let mut truncated = 0usize;
    // 応答は返ったが形式として読み取れなかった塊。これも「誤りなし」ではない
    let mut unparsed = 0usize;

    for chunk in &chunks {
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content:
                    "あなたは日本語の小説を校正する編集者です。指示に従い、JSONだけを返します。"
                        .into(),
            },
            ChatMessage {
                role: "user".into(),
                content: proofread::build_prompt(chunk, &names),
            },
        ];

        // 経路A: 構造化出力。失敗したら経路B(スキーマ無し+寛容パース)へ落とす
        let mut used_fallback = false;
        let mut chunk_truncated = false;
        let raw = match ai::chat(
            &s.base_url,
            &s.api_key,
            &s.model,
            &messages,
            0.1,
            Some(proofread::issue_schema()),
        )
        .await
        {
            Ok(out) => {
                completion_tokens += out.usage.map(|u| u.completion_tokens).unwrap_or(0);
                if out.truncated() {
                    truncated += 1;
                    chunk_truncated = true;
                }
                out.content
            }
            Err(_) => {
                used_fallback = true;
                match ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.1, None).await {
                    Ok(out) => {
                        completion_tokens += out.usage.map(|u| u.completion_tokens).unwrap_or(0);
                        if out.truncated() {
                            truncated += 1;
                            chunk_truncated = true;
                        }
                        out.content
                    }
                    Err(_) => {
                        // 一部が落ちても、取れた分は返す(全部やり直させない)
                        failures += 1;
                        continue;
                    }
                }
            }
        };

        let mut issues = proofread::parse_ai_issues(&raw);
        let mut structured = proofread::looks_structured(&raw);
        // 構造化出力が通ったのに中身が取れない場合は経路Bで測り直す。
        // ただし**打ち切られていた場合は再試行しない**。原因はコンテキスト長の
        // 不足であって出力形式ではないため、投げ直しても同じ結果になり時間を捨てるだけ
        if issues.is_empty() && !used_fallback && !chunk_truncated {
            if let Ok(out) = ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.1, None).await
            {
                completion_tokens += out.usage.map(|u| u.completion_tokens).unwrap_or(0);
                let retried = proofread::parse_ai_issues(&out.content);
                if !retried.is_empty() {
                    used_fallback = true;
                    issues = retried;
                }
                structured = structured || proofread::looks_structured(&out.content);
            }
        }
        // 応答は返ったのに形式を守っていない塊。**「誤りなし」ではない**
        if !structured && !chunk_truncated && !raw.trim().is_empty() {
            unparsed += 1;
        }
        if used_fallback {
            path = "fallback";
        }
        collected.extend(issues);
    }

    if failures == chunks.len() && !chunks.is_empty() {
        return Err("AIへの問い合わせに失敗しました。接続設定を確認してください".to_string());
    }

    let elapsed = started.elapsed();
    let elapsed_ms = elapsed.as_millis() as u64;
    let tokens_per_sec = if completion_tokens > 0 && elapsed.as_secs_f64() > 0.0 {
        Some(completion_tokens as f64 / elapsed.as_secs_f64())
    } else {
        None
    };

    // 打ち切りは「指摘なし」と区別して必ず伝える。
    // 校正で「誤りが無い」と誤解させるのは最悪の誤報告になる
    let warning = if truncated > 0 {
        Some(format!(
            "{truncated}箇所で応答が途中で打ち切られました(モデルのコンテキスト長が不足しています)。\
             見落としがある可能性が高いので、LM Studio のコンテキスト長を増やすか、\
             短い範囲に区切って確認してください"
        ))
    } else if unparsed > 0 {
        Some(format!(
            "{unparsed}箇所で、応答を指定した形式として読み取れませんでした\
             (モデルが形式を守っていません)。**「誤りなし」ではありません**。\
             別のモデルをお試しください"
        ))
    } else if failures > 0 {
        Some(format!(
            "{failures}箇所の検査に失敗しました。結果は一部のみです"
        ))
    } else {
        None
    };

    // 位置は本文全体に対して引き直す(塊ごとのずれを持ち込まない)
    let issues = proofread::resolve_issues(&text, proofread::dedupe_issues(collected));

    Ok(AiProofreadResult {
        issues,
        unchecked_chars,
        path: path.to_string(),
        model: s.model,
        chunks: chunks.len(),
        elapsed_ms,
        tokens_per_sec,
        warning,
    })
}

// ===== レビュー =====

#[derive(Serialize)]
struct AiReviewResult {
    comments: Vec<review::ReviewComment>,
    /// 全体講評。引用に紐づかない講評はここに出る
    overall: String,
    /// 実際に見た観点(キー)
    aspects: Vec<String>,
    /// 一緒に渡した設定資料の名前(U-05: 何を渡したかを見せる)
    materials: Vec<String>,
    /// 本文が長くて末尾を切り捨てた場合の未検査文字数(0なら全文を見た)
    unchecked_chars: usize,
    /// "schema" = 構造化出力が通った / "fallback" = 寛容パースで拾った
    path: String,
    model: String,
    chunks: usize,
    elapsed_ms: u64,
    tokens_per_sec: Option<f64>,
    /// 1文字も返らなかった塊がある。**検閲による拒否の疑い**(04-design §8.1)
    refused: bool,
    /// 応答を指定した形式として読み取れなかった。**「指摘なし」ではない**。
    /// このとき overall には生の応答が入っており、引用の照合はできていない
    unparsed: bool,
    warning: Option<String>,
}

/// シーン/章のレビュー(M-05)。
///
/// 校正(proofread_ai)と同じ骨格: 分割ループ / 経路A→B / 打ち切り検出 /
/// 一部が失敗しても取れた分は返す。**「指摘なし」と誤報告しないこと**が最重要。
///
/// レビュー固有の扱いとして、**1文字も返らない応答**(検閲の疑い)を
/// 打ち切りと区別して伝える(§8.1)。
#[tauri::command]
async fn review_ai(
    text: String,
    aspects: Vec<String>,
    mentioned_paths: Vec<String>,
    manual_paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<AiReviewResult, String> {
    let s = state.ai.lock().map_err(|_| "状態の取得に失敗")?.clone();
    if s.model.trim().is_empty() {
        return Err("モデルが未設定です。AI相談タブの設定で接続してください".to_string());
    }
    if text.trim().is_empty() {
        return Err("本文がありません".to_string());
    }
    let root = root_of(&state)?;
    let codex = project::load_codex(&root).map_err(to_msg)?;

    // コンテキストは3系統だけ(docs/04-design.md §6.2)。組み立ては ask_ai と同じ経路を使う
    let reader = |p: &str| project::read_text(&root, p).ok();
    let ctx = context::build(&text, &codex, &reader, &mentioned_paths, &manual_paths);
    let materials: Vec<(String, String)> = ctx
        .entries
        .iter()
        .map(|e| (e.title.clone(), e.text.clone()))
        .collect();
    let material_names: Vec<String> = ctx.entries.iter().map(|e| e.title.clone()).collect();

    let picked = review::selected_aspects(&aspects);
    let used_aspects: Vec<String> = picked.iter().map(|a| a.key.to_string()).collect();

    // 長文は分割する。所要時間は実行回数でほぼ決まるので、分割字数は設定に従う(§7.2)
    let all_chunks = proofread::split_for_check_with(&text, s.check_chunk_chars);
    let total_chunks = all_chunks.len();
    let chunks: Vec<String> = all_chunks.into_iter().take(proofread::MAX_CHUNKS).collect();
    let unchecked_chars = if total_chunks > chunks.len() {
        text.chars().count() - chunks.iter().map(|c| c.chars().count()).sum::<usize>()
    } else {
        0
    };

    let started = std::time::Instant::now();
    let mut path = "schema";
    let mut collected: Vec<review::ReviewComment> = Vec::new();
    let mut overalls: Vec<String> = Vec::new();
    let mut completion_tokens = 0u64;
    let mut failures = 0usize;
    // 応答が打ち切られた塊。「指摘なし」と取り違えると誤報告になる
    let mut truncated = 0usize;
    // 1文字も返らなかった塊。検閲による拒否でこの形になる
    let mut refused = 0usize;
    // 応答は返ったが指定した形で読み取れなかった塊。これも「指摘なし」ではない
    let mut unparsed = 0usize;

    for chunk in &chunks {
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: review::SYSTEM_PROMPT.to_string(),
            },
            ChatMessage {
                role: "user".into(),
                content: review::build_prompt(chunk, &picked, &materials),
            },
        ];

        let mut used_fallback = false;
        let mut chunk_truncated = false;
        // 温度は校正(0.1)より少し高くする。校正は正解が1つだが、
        // レビューは読み方に幅があり、固めすぎると当たり障りのない指摘に寄る
        //
        // 経路A: 構造化出力。失敗したら経路B(スキーマ無し+寛容パース)へ落とす
        let raw = match ai::chat(
            &s.base_url,
            &s.api_key,
            &s.model,
            &messages,
            0.3,
            Some(review::review_schema()),
        )
        .await
        {
            Ok(out) => {
                completion_tokens += out.usage.map(|u| u.completion_tokens).unwrap_or(0);
                if out.finish_reason.as_deref() == Some("length") {
                    truncated += 1;
                    chunk_truncated = true;
                } else if out.refused() {
                    refused += 1;
                }
                out.content
            }
            Err(_) => {
                used_fallback = true;
                match ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.3, None).await {
                    Ok(out) => {
                        completion_tokens += out.usage.map(|u| u.completion_tokens).unwrap_or(0);
                        if out.finish_reason.as_deref() == Some("length") {
                            truncated += 1;
                            chunk_truncated = true;
                        } else if out.refused() {
                            refused += 1;
                        }
                        out.content
                    }
                    Err(_) => {
                        // 一部が落ちても、取れた分は返す(全部やり直させない)
                        failures += 1;
                        continue;
                    }
                }
            }
        };

        let mut parsed = review::parse(&raw);
        let mut shown_raw = raw;
        // 指定した形で読み取れない場合は経路Bで測り直す。
        // ただし**打ち切られていた場合は再試行しない**(原因は出力形式ではなく
        // コンテキスト不足なので、投げ直しても同じ結果になり時間を捨てるだけ)
        if !parsed.structured && !used_fallback && !chunk_truncated {
            if let Ok(out) = ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.3, None).await
            {
                completion_tokens += out.usage.map(|u| u.completion_tokens).unwrap_or(0);
                let retried = review::parse(&out.content);
                if retried.structured {
                    used_fallback = true;
                    parsed = retried;
                    shown_raw = out.content;
                }
            }
        }

        // 形式を守らない応答(散文・英語・箇条書き等)を捨てない。
        //
        // **捨てると「指摘なし」と区別が付かなくなる**。中身はあるのに読めていない
        // だけなので、生の応答をそのまま講評として見せ、照合できていないことを警告する。
        // これはレビューで最悪の誤報告(問題ありませんでした)を防ぐための処置であり、
        // 経路B(プレーンテキスト+寛容パース)の最後の受け皿にあたる(§6.3)
        if !parsed.structured && !shown_raw.trim().is_empty() {
            unparsed += 1;
            parsed.overall = review::clip_raw(&shown_raw);
        }

        if used_fallback {
            path = "fallback";
        }
        overalls.push(parsed.overall);
        collected.extend(parsed.comments);
    }

    if failures == chunks.len() && !chunks.is_empty() {
        return Err("AIへの問い合わせに失敗しました。接続設定を確認してください".to_string());
    }

    let elapsed = started.elapsed();
    let elapsed_ms = elapsed.as_millis() as u64;
    let tokens_per_sec = if completion_tokens > 0 && elapsed.as_secs_f64() > 0.0 {
        Some(completion_tokens as f64 / elapsed.as_secs_f64())
    } else {
        None
    };

    // 打ち切り・拒否は「指摘なし」と必ず区別して伝える。
    // レビューで「問題ありません」と誤解させるのは校正と同じく最悪の誤報告になる
    let warning = if truncated > 0 {
        Some(format!(
            "{truncated}箇所で応答が途中で打ち切られました(モデルのコンテキスト長が不足しています)。\
             見落としがある可能性が高いので、LM Studio のコンテキスト長を増やすか、\
             1回に送る字数を小さくしてください"
        ))
    } else if refused > 0 {
        Some(format!(
            "{refused}箇所でモデルが応答を返しませんでした。題材によっては検閲で拒否されることがあります。\
             非検閲モデルに切り替えてお試しください"
        ))
    } else if unparsed > 0 {
        Some(format!(
            "{unparsed}箇所で、応答を指定した形式として読み取れませんでした(モデルが形式を守っていません)。\
             **「指摘なし」ではありません**。読み取れなかった応答はそのまま下に出していますが、\
             引用が本文に実在するかの照合ができていないので、内容は鵜呑みにしないでください。\
             別のモデルをお試しください"
        ))
    } else if failures > 0 {
        Some(format!(
            "{failures}箇所のレビューに失敗しました。結果は一部のみです"
        ))
    } else {
        None
    };

    // 位置は本文全体に対して引き直す(塊ごとのずれを持ち込まない)
    let comments = review::resolve(&text, review::dedupe(collected));

    Ok(AiReviewResult {
        comments,
        overall: review::merge_overall(&overalls),
        aspects: used_aspects,
        materials: material_names,
        unchecked_chars,
        path: path.to_string(),
        model: s.model,
        chunks: chunks.len(),
        elapsed_ms,
        tokens_per_sec,
        refused: refused > 0,
        unparsed: unparsed > 0,
        warning,
    })
}

#[derive(Serialize)]
struct ExtractResult {
    candidates: Vec<extract::Candidate>,
    /// 複数の対象に結び付いたため、どちらにも付けなかった呼び名
    conflicts: Vec<String>,
    /// LLMが挙げたが機械側の検証で落とした数(幻覚・一般名詞・登録済み)
    rejected: usize,
    chunks: usize,
    elapsed_ms: u64,
    warning: Option<String>,
}

/// 本文からの設定自動抽出(M-09)。**手動実行のみ**。
///
/// 提案するだけで登録はしない。登録は create_codex_entries を別途呼ぶ。
#[tauri::command]
async fn extract_entities(
    text: String,
    state: State<'_, AppState>,
) -> Result<ExtractResult, String> {
    let s = state.ai.lock().map_err(|_| "状態の取得に失敗")?.clone();
    if s.model.trim().is_empty() {
        return Err("モデルが未設定です。AI相談タブの設定で接続してください".to_string());
    }
    let root = root_of(&state)?;
    let codex = project::load_codex(&root).map_err(to_msg)?;
    let known: Vec<String> = codex.iter().flat_map(|c| c.patterns()).collect();

    let chunks = proofread::split_for_check_with(&text, s.check_chunk_chars);
    let chunks: Vec<String> = chunks.into_iter().take(proofread::MAX_CHUNKS).collect();

    let started = std::time::Instant::now();
    let mut raw_all = Vec::new();
    let mut truncated = 0usize;
    let mut failures = 0usize;

    for chunk in &chunks {
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: "あなたは小説の設定を整理する編集者です。指示に従い、JSONだけを返します。"
                    .into(),
            },
            ChatMessage {
                role: "user".into(),
                content: extract::build_prompt(chunk, &known),
            },
        ];
        match ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.1, None).await {
            Ok(out) => {
                if out.truncated() {
                    truncated += 1;
                }
                raw_all.extend(extract::parse(&out.content));
            }
            Err(_) => failures += 1,
        }
    }

    if failures == chunks.len() && !chunks.is_empty() {
        return Err("AIへの問い合わせに失敗しました。接続設定を確認してください".to_string());
    }

    let raw_count = raw_all.len();
    // 実在性・重複・名寄せの検証は本文全体に対して行う
    let verified = extract::verify(&text, raw_all, &codex);
    let rejected = raw_count.saturating_sub(verified.candidates.len());
    let (candidates, conflicts) = (verified.candidates, verified.conflicts);

    let warning = if truncated > 0 {
        Some(format!(
            "{truncated}箇所で応答が途中で打ち切られました。取りこぼしがある可能性があります"
        ))
    } else if failures > 0 {
        Some(format!("{failures}箇所の抽出に失敗しました。結果は一部のみです"))
    } else {
        None
    };

    Ok(ExtractResult {
        candidates,
        conflicts,
        rejected,
        chunks: chunks.len(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        warning,
    })
}

/// 採用した候補をcodexへ反映する(ユーザーが選んだものだけ)。
///
/// `existing_path` があれば**既存ファイルへ別名を追加**し、無ければ新規作成する。
/// 別名追加は `aliases:` の行だけを書き換え、他の行はそのまま残す
/// (フロントマターを再シリアライズしない原則=03 §4.1)。
#[tauri::command]
fn create_codex_entries(
    candidates: Vec<extract::Candidate>,
    state: State<AppState>,
) -> Result<Vec<String>, String> {
    let root = root_of(&state)?;
    let mut touched = Vec::new();
    for c in candidates {
        match &c.existing_path {
            Some(path) => {
                let source = project::read_text(&root, path).map_err(to_msg)?;
                let updated = frontmatter::add_aliases(&source, &c.aliases);
                if updated != source {
                    project::write_text(&root, path, &updated).map_err(to_msg)?;
                    touched.push(path.clone());
                }
            }
            None => {
                let safe = c
                    .name
                    .replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_");
                let path = format!("{}/{}.md", extract::folder_for(&c.kind), safe);
                match project::create_file(&root, &path, &extract::entry_markdown(&c)) {
                    Ok(true) => touched.push(path),
                    // 同名が既にある場合は黙って飛ばす(上書きしない)
                    Ok(false) => {}
                    Err(e) => return Err(to_msg(e)),
                }
            }
        }
    }
    Ok(touched)
}

// ===== AI =====

#[tauri::command]
fn get_ai_settings(state: State<AppState>) -> Result<AiSettings, String> {
    Ok(state.ai.lock().map_err(|_| "状態の取得に失敗")?.clone())
}

#[tauri::command]
fn set_ai_settings(settings: AiSettings, state: State<AppState>) -> Result<(), String> {
    let mut settings = settings;
    settings.check_chunk_chars = settings::clamp_chunk_chars(settings.check_chunk_chars);
    // 保存に失敗してもアプリは動かす(次回の起動で既定に戻るだけ)
    let _ = settings::save(&settings.to_stored());
    *state.ai.lock().map_err(|_| "状態の更新に失敗")? = settings;
    Ok(())
}

#[tauri::command]
async fn list_models(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let s = state.ai.lock().map_err(|_| "状態の取得に失敗")?.clone();
    ai::list_models(&s.base_url, &s.api_key).await
}

/// 送信前プレビュー: 何をAIに渡すかを組み立てて返す(U-05の透明性)。
#[tauri::command]
fn build_context(
    body: String,
    mentioned_paths: Vec<String>,
    manual_paths: Vec<String>,
    state: State<AppState>,
) -> Result<ContextPreview, String> {
    let root = root_of(&state)?;
    let codex = project::load_codex(&root).map_err(to_msg)?;
    let reader = |p: &str| project::read_text(&root, p).ok();
    Ok(context::build(
        &body,
        &codex,
        &reader,
        &mentioned_paths,
        &manual_paths,
    ))
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", content = "value")]
enum ChatEvent {
    Delta(String),
    Done,
    Error(String),
}

/// ストリーミングでAIに問い合わせる。応答は Channel でフロントへ逐次流す。
#[tauri::command]
async fn ask_ai(
    context: ContextPreview,
    question: String,
    on_event: Channel<ChatEvent>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let s = state.ai.lock().map_err(|_| "状態の取得に失敗")?.clone();
    if s.model.trim().is_empty() {
        return Err("モデルが未設定です。設定でモデルを選んでください".to_string());
    }
    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: context::SYSTEM_PROMPT.to_string(),
        },
        ChatMessage {
            role: "user".into(),
            content: context::render_user_message(&context, &question),
        },
    ];

    let resp = ai::stream_request(&s.base_url, &s.api_key, &s.model, &messages, s.temperature)
        .send()
        .await
        .map_err(|e| ai::describe_error(&e, &s.base_url))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let msg = format!("APIエラー: status={status} body={body}");
        let _ = on_event.send(ChatEvent::Error(msg.clone()));
        return Err(msg);
    }

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("受信が中断されました: {e}");
                let _ = on_event.send(ChatEvent::Error(msg.clone()));
                return Err(msg);
            }
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));
        // 行単位で処理し、途中で切れた行は次のチャンクへ持ち越す
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            match ai::parse_sse_line(&line) {
                ai::SseEvent::Delta(d) => {
                    let _ = on_event.send(ChatEvent::Delta(d));
                }
                ai::SseEvent::Done => {
                    let _ = on_event.send(ChatEvent::Done);
                    return Ok(());
                }
                ai::SseEvent::Ignore => {}
            }
        }
    }
    let _ = on_event.send(ChatEvent::Done);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 前回の接続設定を復元する(APIキーは保存していないので毎回入力)
    let state = AppState::default();
    if let Some(stored) = settings::load() {
        if let Ok(mut ai) = state.ai.lock() {
            *ai = AiSettings::from_stored(stored);
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            open_project,
            refresh_project,
            read_file,
            save_file,
            create_file,
            file_modified_ms,
            count_files,
            trash_entry,
            rename_entry,
            duplicate_entry,
            create_dir,
            reveal_in_explorer,
            list_templates,
            render_template,
            open_templates_dir,
            find_mentions,
            check_notation,
            proofread_ai,
            review_ai,
            extract_entities,
            create_codex_entries,
            get_ai_settings,
            set_ai_settings,
            list_models,
            build_context,
            ask_ai,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
