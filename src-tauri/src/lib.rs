//! Tauri アプリのエントリポイント。
//!
//! 方針(docs/04-design.md §2): ドメインロジック(解析・索引・検出)は Rust 側に置き、
//! フロントは表示と編集に徹する。

// ドメインロジックは統合テスト(tests/flow.rs)からも叩けるよう公開する
pub mod ai;
pub mod context;
pub mod frontmatter;
pub mod mentions;
pub mod project;
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
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            base_url: ai::DEFAULT_BASE_URL.to_string(),
            api_key: None,
            model: String::new(),
            temperature: 0.7,
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

// ===== AI =====

#[tauri::command]
fn get_ai_settings(state: State<AppState>) -> Result<AiSettings, String> {
    Ok(state.ai.lock().map_err(|_| "状態の取得に失敗")?.clone())
}

#[tauri::command]
fn set_ai_settings(settings: AiSettings, state: State<AppState>) -> Result<(), String> {
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
        .map_err(|e| format!("接続できませんでした: {e}"))?;

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
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            open_project,
            refresh_project,
            read_file,
            save_file,
            create_file,
            file_modified_ms,
            list_templates,
            render_template,
            open_templates_dir,
            find_mentions,
            get_ai_settings,
            set_ai_settings,
            list_models,
            build_context,
            ask_ai,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
