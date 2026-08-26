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
pub mod prompts;
pub mod proofread;
pub mod review;
pub mod ruby;
pub mod sample;
pub mod search;
pub mod settings;
pub mod split;
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
    /// AIの中止(T-10撤回後も V1 の約束に残っている `cancel`)。
    ///
    /// 実行中フラグではなく**世代番号**にする。中止は「いま走っているものを止める」
    /// 操作なので、フラグ方式だと後から始まった処理が先の中止を打ち消してしまう。
    /// 各実行は開始時の番号を覚え、番号が変わっていたら自分は中止されたと判断する。
    ai_epoch: std::sync::atomic::AtomicU64,
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
            // 前回のプロジェクトは接続設定とは別の話。保存時に settings::merge が引き継ぐ
            last_project: None,
        }
    }
}

/// 設定を保存する。**設定ファイルへ書く経路はここ1本に絞る**。
/// 丸ごと書き直す形式なので、部分的な知識で保存すると他の項目が消える(settings::merge)。
///
/// 保存に失敗してもアプリは動かす(次回の起動で既定に戻るだけ)。
fn persist_settings(ai: &AiSettings, last_project: Option<String>) {
    let stored = settings::merge(settings::load(), ai.to_stored(), last_project);
    let _ = settings::save(&stored);
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

/// 実行中のAI処理を中止する。
///
/// 走っているリクエスト自体は途中で切らない(HTTPの中断まで持ち込むと
/// 薄いクライアントでなくなる)。**次の塊へ進まない**ところで止める。
/// 校正は最大4分割×240秒あるので、これだけで待ち時間の上限が大きく下がる。
#[tauri::command]
fn cancel_ai(state: State<AppState>) {
    state
        .ai_epoch
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// 開始時の世代番号。これが変わったら中止された
fn ai_epoch(state: &State<AppState>) -> u64 {
    state.ai_epoch.load(std::sync::atomic::Ordering::SeqCst)
}

fn ai_cancelled(state: &State<AppState>, epoch: u64) -> bool {
    ai_epoch(state) != epoch
}

/// 中止されたことを結果の断り書きに足す。**黙って短い結果を返さない**
/// (「ここまでしか見ていない」と分かる形にする)
fn note_cancelled(warning: Option<String>) -> Option<String> {
    let head = "中止しました。ここまでの結果だけを表示しています".to_string();
    Some(match warning {
        Some(w) => format!("{head} / {w}"),
        None => head,
    })
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
    let root_str = root.to_string_lossy().to_string();
    // 次の起動で開き直せるように覚える
    if let Ok(ai) = state.ai.lock() {
        persist_settings(&ai, Some(root_str.clone()));
    }
    Ok(OpenedProject {
        root: root_str,
        name,
        tree,
        codex,
    })
}

/// 前回開いたプロジェクトの場所。起動時に開き直すために使う。
///
/// **返すのは「すでにプロジェクトであるフォルダ」だけ**。フォルダが消えていたり、
/// `manuscript/` が無くなっていたら `None` を返す。
/// `open_project` は `manuscript/` が無いフォルダを新規プロジェクトとして初期化するので、
/// ここで絞らないと**起動しただけでフォルダが作られる**ことになる(初期化は人の操作でだけ起こす)。
#[tauri::command]
fn last_project() -> Option<String> {
    let path = settings::load()?.last_project?;
    let root = PathBuf::from(&path);
    if root.join("manuscript").is_dir() {
        Some(path)
    } else {
        None
    }
}

/// サンプルプロジェクトを作って開く(M-08)。
///
/// 空のプロジェクトでは何ができるアプリなのか分からないので、
/// **触りながら学べる素材**を用意する。既存ファイルは上書きしない。
#[tauri::command]
fn create_sample_project(path: String, state: State<AppState>) -> Result<OpenedProject, String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("フォルダが見つかりません: {path}"));
    }
    sample::create(&root).map_err(to_msg)?;
    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    *state.root.lock().map_err(|_| "状態の更新に失敗")? = Some(root.clone());
    let root_str = root.to_string_lossy().to_string();
    // サンプルも「開いたプロジェクト」なので同じように覚える
    if let Ok(ai) = state.ai.lock() {
        persist_settings(&ai, Some(root_str.clone()));
    }
    Ok(OpenedProject {
        root: root_str,
        name,
        tree: project::scan(&root).map_err(to_msg)?,
        codex: project::load_codex(&root).map_err(to_msg)?,
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

/// 保存の結果。**競合はエラーではない**ので Ok で返す。
///
/// Err にすると自動保存の失敗通知と混ざり、フロントが文字列で見分ける羽目になる。
#[derive(Serialize)]
#[serde(tag = "kind")]
enum SaveOutcome {
    /// 書けた。値は保存後の更新時刻
    Saved { modified_ms: u64 },
    /// **書いていない。** 読み込んだ後に外部で書き換えられていた(T-08)
    Conflict { actual_ms: u64 },
}

/// 保存(保存前に1世代のバックアップを取る)。
///
/// `expected_ms` を渡すと、**ディスク側がその時刻のままの場合だけ書く**(楽観ロック)。
/// 外部エディタやgitでの書き換えを黙って踏み潰さないため(T-08 / D-5「勝手に上書きしない」)。
/// 新規作成直後など、まだ時刻を持っていない経路は `None` で素通しできる。
///
/// 戻り値の更新時刻は、**書けたあとに読めなくても保存は失敗ではない**ので 0 を返す。
/// ここで Err にすると、フロントは「保存に失敗した」と判断して切替や終了を止めてしまう。
#[tauri::command]
fn save_file(
    path: String,
    text: String,
    expected_ms: Option<u64>,
    state: State<AppState>,
) -> Result<SaveOutcome, String> {
    let root = root_of(&state)?;
    if let Ok(actual) = project::modified_ms(&root, &path) {
        if project::is_stale(actual, expected_ms) {
            return Ok(SaveOutcome::Conflict { actual_ms: actual });
        }
    }
    project::write_text(&root, &path, &text).map_err(to_msg)?;
    Ok(SaveOutcome::Saved {
        modified_ms: project::modified_ms(&root, &path).unwrap_or(0),
    })
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

/// AI相談の依頼テンプレート(M-03)。押すと依頼欄へ入る文例の一覧
#[tauri::command]
fn list_prompts() -> Result<Vec<prompts::PromptTemplate>, String> {
    prompts::ensure_defaults().map_err(|e| e.to_string())?;
    prompts::list().map_err(|e| e.to_string())
}

/// 文例の置き場をエクスプローラで開く(自分の口癖に書き換えられる)
#[tauri::command]
fn open_prompts_dir(app: tauri::AppHandle) -> Result<String, String> {
    use tauri_plugin_opener::OpenerExt;
    prompts::ensure_defaults().map_err(|e| e.to_string())?;
    let dir = prompts::prompts_dir();
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().to_string())
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

// ===== 全文検索 =====

/// プロジェクト全体を検索する(PoC#2で方式を確定)。
///
/// 索引は使い捨て(D-1)。**検索のたびに変更分だけ索引し直す**ので、
/// ファイル監視や常駐処理は持たない。
#[tauri::command]
fn search_project(query: String, state: State<AppState>) -> Result<search::SearchResult, String> {
    let root = root_of(&state)?;
    search::search(&root, &query).map_err(to_msg)
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
    // ルビの読みは語の候補から外す。カタカナのルビが登録名と1文字違いだと
    // 誤検出する(`｜白鏡《ハクキョウ》`)。**位置は変わらない**ので指摘先はずれない
    Ok(proofread::check_notation(&ruby::mask(&text), &names))
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
    let epoch = ai_epoch(&state);
    let mut cancelled = false;

    for chunk in &chunks {
        if ai_cancelled(&state, epoch) {
            cancelled = true;
            break;
        }
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
        // **指定した形で読み取れない**場合は経路Bで測り直す(レビュー側と同じ条件)。
        // 条件を `issues.is_empty()` にすると、正常な「指摘なし」(`{"issues":[]}`)でも
        // 毎回2回投げることになり、誤字の無い本文ほど時間とトークンが倍かかる。
        // ただし**打ち切られていた場合は再試行しない**。原因はコンテキスト長の
        // 不足であって出力形式ではないため、投げ直しても同じ結果になり時間を捨てるだけ
        if !structured && !used_fallback && !chunk_truncated {
            if let Ok(out) = ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.1, None).await
            {
                completion_tokens += out.usage.map(|u| u.completion_tokens).unwrap_or(0);
                // 経路Bが**読み取れた**なら、0件でもそれが答え(誤字なしと読めている)
                if proofread::looks_structured(&out.content) {
                    used_fallback = true;
                    issues = proofread::parse_ai_issues(&out.content);
                    structured = true;
                }
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
        warning: if cancelled { note_cancelled(warning) } else { warning },
    })
}

// ===== シーンの自動分割 =====

#[derive(Serialize)]
struct SplitSuggestion {
    points: Vec<split::SplitPoint>,
    /// 分割前の本文の字数(プレビューで規模を見せる)
    total_chars: usize,
    model: String,
    elapsed_ms: u64,
    warning: Option<String>,
}

/// 場面の切れ目を提案する(P-8 / §5.9-D)。**提案するだけで切らない。**
#[tauri::command]
async fn suggest_scene_split(
    path: String,
    state: State<'_, AppState>,
) -> Result<SplitSuggestion, String> {
    let s = state.ai.lock().map_err(|_| "状態の取得に失敗")?.clone();
    if s.model.trim().is_empty() {
        return Err("モデルが未設定です。AI相談タブの設定で接続してください".to_string());
    }
    let root = root_of(&state)?;
    let source = project::read_text(&root, &path).map_err(to_msg)?;
    let (_, body) = frontmatter::split(&source);
    if body.chars().count() < split::MIN_SEGMENT_CHARS * 2 {
        return Err(format!(
            "本文が短すぎます。分けるには最低でも{}字ほど必要です",
            split::MIN_SEGMENT_CHARS * 2
        ));
    }

    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: split::SYSTEM_PROMPT.to_string(),
        },
        ChatMessage {
            role: "user".into(),
            content: split::build_prompt(body),
        },
    ];

    let started = std::time::Instant::now();
    // 分割は**全体を通して読まないと切れ目が分からない**ので、分割送信はしない。
    // 長すぎてコンテキストに入らない場合は打ち切りとして正直に伝える
    let out = ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.2, None)
        .await
        .map_err(|e| e)?;

    let warning = if out.finish_reason.as_deref() == Some("length") {
        Some(
            "応答が途中で打ち切られました。本文が長すぎてモデルが最後まで読めていない可能性があります"
                .to_string(),
        )
    } else if out.refused() {
        Some("モデルが応答を返しませんでした。別のモデルをお試しください".to_string())
    } else {
        None
    };

    let points = split::verify(body, split::parse(&out.content));
    Ok(SplitSuggestion {
        points,
        total_chars: body.chars().count(),
        model: s.model,
        elapsed_ms: started.elapsed().as_millis() as u64,
        warning,
    })
}

/// 採用された切れ目で実際に分割する(中身は split::apply)。
///
/// **元ファイルはゴミ箱へ退避する**(削除と同じ流儀)。消さないので戻せる。
/// 位置は保存しておらず、**適用時に引用から引き直す**ので、提案を見てから
/// 本文が変わっていた場合はずれた位置で切らずに失敗する。
#[tauri::command]
fn apply_scene_split(
    path: String,
    first_title: String,
    points: Vec<split::AcceptedPoint>,
    state: State<AppState>,
) -> Result<Vec<String>, String> {
    let root = root_of(&state)?;
    split::apply(&root, &path, &first_title, &points)
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
    let epoch = ai_epoch(&state);
    let mut cancelled = false;

    for chunk in &chunks {
        if ai_cancelled(&state, epoch) {
            cancelled = true;
            break;
        }
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
        warning: if cancelled { note_cancelled(warning) } else { warning },
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
    let epoch = ai_epoch(&state);
    let mut cancelled = false;

    for chunk in &chunks {
        if ai_cancelled(&state, epoch) {
            cancelled = true;
            break;
        }
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
        warning: if cancelled { note_cancelled(warning) } else { warning },
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
    persist_settings(&settings, None);
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
    // これまでの往復(会話モード)。単発の相談では空で来る(§5.7)
    history: Vec<context::ChatTurn>,
    on_event: Channel<ChatEvent>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let s = state.ai.lock().map_err(|_| "状態の取得に失敗")?.clone();
    if s.model.trim().is_empty() {
        return Err("モデルが未設定です。設定でモデルを選んでください".to_string());
    }
    let messages = context::build_chat_messages(&context, &question, &history);
    // 応答を待っている間に中止されることもあるので、投げる前に覚える
    let epoch = ai_epoch(&state);

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
    // **バイト列のまま持つ。** チャンクごとに文字列化すると、割れた日本語1文字が化ける
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("受信が中断されました: {e}");
                let _ = on_event.send(ChatEvent::Error(msg.clone()));
                return Err(msg);
            }
        };
        if ai_cancelled(&state, epoch) {
            let _ = on_event.send(ChatEvent::Done);
            return Ok(());
        }
        buf.extend_from_slice(&chunk);
        // 行単位で処理し、途中で切れた行は次のチャンクへ持ち越す(ai::drain_sse_lines)
        for line in ai::drain_sse_lines(&mut buf) {
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
    // 改行で終わらずに切れた最後の行も捨てない([DONE] を送らないサーバがある)
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf).into_owned();
        if let ai::SseEvent::Delta(d) = ai::parse_sse_line(&line) {
            let _ = on_event.send(ChatEvent::Delta(d));
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
            last_project,
            create_sample_project,
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
            list_prompts,
            open_prompts_dir,
            find_mentions,
            search_project,
            check_notation,
            proofread_ai,
            review_ai,
            suggest_scene_split,
            apply_scene_split,
            extract_entities,
            create_codex_entries,
            get_ai_settings,
            set_ai_settings,
            list_models,
            build_context,
            ask_ai,
            cancel_ai,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
