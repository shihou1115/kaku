//! Tauri アプリのエントリポイント。
//!
//! 方針(docs/04-design.md §2): ドメインロジック(解析・索引・検出)は Rust 側に置き、
//! フロントは表示と編集に徹する。

// ドメインロジックは統合テスト(tests/flow.rs)からも叩けるよう公開する
pub mod ai;
pub mod ailog;
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
            // 知らない項目も、保存時に settings::merge が引き継ぐ
            extra: Default::default(),
        }
    }
}

/// 設定を保存する。**設定ファイルへ書く経路はここ1本に絞る**。
/// 丸ごと書き直す形式なので、部分的な知識で保存すると他の項目が消える(settings::merge)。
///
/// 保存に失敗してもアプリは動かす(次回の起動で既定に戻るだけ)。
/// **書けなかったときは返す**(以前は握りつぶしていたため、読み取り専用の設定ファイルでは
/// 変更が保存されず、次の起動で黙って元に戻った。テスト計画 H1)
fn persist_settings(ai: &AiSettings, last_project: Option<String>) -> std::io::Result<()> {
    let stored = settings::merge(settings::load(), ai.to_stored(), last_project);
    settings::save(&stored)
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
/// 世代番号を進めるだけで、各処理は待っている応答を**その場で捨てて**止まる
/// (`unless_cancelled`。未来を捨てるので接続も閉じる)。以前は「次の塊へ進まない」
/// ところでしか止めず、応答の始まりを待っている間(長い本文の読み込み中など)に
/// 押しても、応答が返るまで最大240秒止まらなかった。
#[tauri::command]
fn cancel_ai(state: State<AppState>) {
    state
        .ai_epoch
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

// ===== AI実行と外界のつなぎ =====

/// AI実行が外界に触れる2点 — 中止の確認と、呼び出しの記録(T-06)。
///
/// コマンド本体から切り離してあるのは**テストのため**。本番では AppState の世代番号と
/// `.app/logs/` につなぎ、テストでは差し替えて、擬似サーバー相手に
/// 「何回投げたか」「何と警告したか」を確かめる。lib.rs はここまでテストが無く、
/// 校正の二重実行も、拒否を打ち切りと取り違える誤診も、この層で起きていた。
///
/// **再試行やフォールバックの手順をここへ集めないこと。** 06 §4 で撤回した
/// 「汎用AI実行エンジン」の入口になる。手順は各コマンドの `run_*` にそのまま書く。
struct AiHooks<'a> {
    /// 中止されたか。各塊の頭で見る
    cancelled: &'a CancelFn<'a>,
    log: &'a LogFn<'a>,
}

/// 寿命を引数に取る。取らないと trait object が `'static` 扱いになり、
/// コマンド内の `State` を借りた中止判定を渡せなくなる
type CancelFn<'a> = dyn Fn() -> bool + Send + Sync + 'a;

/// 1回の呼び出しを記録する(機能名・送ったもの・受けたもの・所要ミリ秒・但し書き)
type LogFn<'a> = dyn Fn(&str, &[ChatMessage], &str, u64, Option<&str>) + Send + Sync + 'a;

/// 本番の中止判定。**開始時の世代番号**を覚え、変わっていたら中止されたとみなす。
///
/// フラグにすると、後から始まった処理が先の中止を打ち消してしまう。
fn live_cancel(app: &AppState) -> impl Fn() -> bool + Send + Sync + '_ {
    let start = app.ai_epoch.load(std::sync::atomic::Ordering::SeqCst);
    move || app.ai_epoch.load(std::sync::atomic::Ordering::SeqCst) != start
}

/// 中止を見に行く間隔。押してから止まるまでの遅れの上限になる
const CANCEL_POLL_MS: u64 = 150;

/// 中止されるまで待つ(中止は世代番号なので、短い間隔で見に行く)
async fn until_cancelled(cancelled: &CancelFn<'_>) {
    while !cancelled() {
        tokio::time::sleep(std::time::Duration::from_millis(CANCEL_POLL_MS)).await;
    }
}

/// 応答を待つ。**中止されたら待つのをやめて `None` を返す**。
///
/// 待っていた未来は捨てる(HTTP の接続も閉じる)。止めるのは待つことだけで、
/// リトライや退避のような仕組みは足さない(06 §4: send/stream/cancel の薄いクライアント)
async fn unless_cancelled<T>(
    cancelled: &CancelFn<'_>,
    fut: impl std::future::Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        r = fut => Some(r),
        _ = until_cancelled(cancelled) => None,
    }
}

/// 本番の記録先(`.app/logs/`)。プロジェクトが開かれていなければ記録しない。
///
/// **プロンプトの実体を残す**ことが目的なので、要約や省略はしない
/// (06 §4 でプロンプトのバージョン管理機構を撤回した根拠がこのログである)。
fn live_log(
    root: Option<PathBuf>,
    model: String,
    base_url: String,
) -> impl Fn(&str, &[ChatMessage], &str, u64, Option<&str>) + Send + Sync {
    move |feature, messages, response, elapsed_ms, note| {
        if let Some(root) = &root {
            ailog::write(
                root, feature, &model, &base_url, messages, response, elapsed_ms, note,
            );
        }
    }
}

fn ms_since(t: std::time::Instant) -> u64 {
    t.elapsed().as_millis() as u64
}

/// モデルが未設定のときの案内(設定はヘッダーの「設定」にある=§5.11)
const NO_MODEL: &str = "モデルが未設定です。ヘッダーの「設定」で接続先とモデルを選んでください";

/// 中止されたことを結果の断り書きに足す。**黙って短い結果を返さない**
/// (「ここまでしか見ていない」と分かる形にする)
fn note_cancelled(warning: Option<String>) -> Option<String> {
    let head = "中止しました。ここまでの結果だけを表示しています".to_string();
    Some(match warning {
        Some(w) => format!("{head} / {w}"),
        None => head,
    })
}

/// すべての塊で問い合わせに失敗したときの文面。**最後の失敗の理由を添える**(テスト計画 E7)。
/// 以前は理由を捨てて一律に「接続設定を確認してください」と出していたため、時間切れ
/// (モデルが遅いだけ)でも接続先やキーを疑わせた
fn all_failed(reason: Option<&str>) -> String {
    match reason {
        Some(r) => format!("AIへの問い合わせに失敗しました。{r}"),
        None => "AIへの問い合わせに失敗しました。接続設定を確認してください".to_string(),
    }
}

/// 一部の塊で失敗したときの断り書き。理由を添える
fn some_failed(failures: usize, what: &str, reason: Option<&str>) -> String {
    let why = reason.map(|r| format!("(理由: {r})")).unwrap_or_default();
    format!("{failures}箇所の{what}に失敗しました{why}。結果は一部のみです")
}

// ===== プロジェクト =====

#[derive(Serialize)]
struct OpenedProject {
    root: String,
    name: String,
    tree: Vec<TreeNode>,
    codex: Vec<CodexEntry>,
}

/// フォルダーを開いた(作った)結果。
///
/// **人が置いたものがあるフォルダーに骨組みを作る前は、本人に聞く**(テスト計画 B2)。
/// `NeedsConfirm` を返したときは何も作っていない。了承を得たら `confirmed: true` で呼び直す
#[derive(Serialize)]
#[serde(tag = "kind")]
enum OpenOutcome {
    Opened(OpenedProject),
    NeedsConfirm {
        existing: usize,
        will_create: Vec<String>,
    },
}

fn needs_confirm(check: project::OpenCheck) -> Option<OpenOutcome> {
    match check {
        project::OpenCheck::Ready => None,
        project::OpenCheck::NeedsConfirm {
            existing,
            will_create,
        } => Some(OpenOutcome::NeedsConfirm {
            existing,
            will_create,
        }),
    }
}

/// 指定フォルダをプロジェクトとして開く。プロジェクトでなければ骨組みを作る
/// (空でないフォルダーなら、先に本人の了承を得る)。
#[tauri::command]
fn open_project(
    path: String,
    confirmed: bool,
    state: State<AppState>,
) -> Result<OpenOutcome, String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("フォルダが見つかりません: {path}"));
    }
    if !confirmed {
        if let Some(ask) = needs_confirm(project::check_open(&root).map_err(to_msg)?) {
            return Ok(ask);
        }
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
        // 覚えられなくても開くことは止めない(次の起動で開き直せないだけ)
        let _ = persist_settings(&ai, Some(root_str.clone()));
    }
    Ok(OpenOutcome::Opened(OpenedProject {
        root: root_str,
        name,
        tree,
        codex,
    }))
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
fn create_sample_project(
    path: String,
    confirmed: bool,
    state: State<AppState>,
) -> Result<OpenOutcome, String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("フォルダが見つかりません: {path}"));
    }
    // 空でなければ先に聞く(本人の原稿のフォルダーを選び間違えると、サンプルが混ざる)
    if !confirmed {
        if let Some(ask) = needs_confirm(project::check_sample(&root).map_err(to_msg)?) {
            return Ok(ask);
        }
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
        // 覚えられなくても開くことは止めない(次の起動で開き直せないだけ)
        let _ = persist_settings(&ai, Some(root_str.clone()));
    }
    Ok(OpenOutcome::Opened(OpenedProject {
        root: root_str,
        name,
        tree: project::scan(&root).map_err(to_msg)?,
        codex: project::load_codex(&root).map_err(to_msg)?,
    }))
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

/// 画面(エディタ・参照ペイン)へ渡す本文。**改行は LF に揃える**(project::to_lf)。
/// 保存時に元の改行コードへ戻す(save_file → project::save_text)
#[tauri::command]
fn read_file(path: String, state: State<AppState>) -> Result<FileContent, String> {
    let root = root_of(&state)?;
    let text = project::to_lf(&project::read_text(&root, &path).map_err(to_msg)?);
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
    project::save_text(&root, &path, &text).map_err(to_msg)?;
    Ok(SaveOutcome::Saved {
        modified_ms: project::modified_ms(&root, &path).unwrap_or(0),
    })
}

#[tauri::command]
fn create_file(path: String, text: String, state: State<AppState>) -> Result<bool, String> {
    let root = root_of(&state)?;
    project::create_file(&root, &path, &text).map_err(to_msg)
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

/// 改名・移動。プロジェクト内のMarkdownリンクも追随する。
/// 戻り値はリンクを書き換えられなかったファイル(改名そのものは済んでいる)
#[tauri::command]
fn rename_entry(from: String, to: String, state: State<AppState>) -> Result<Vec<String>, String> {
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
        return Err(NO_MODEL.to_string());
    }
    let root = root_of(&state)?;
    let codex = project::load_codex(&root).map_err(to_msg)?;
    let names: Vec<String> = codex.iter().flat_map(|c| c.patterns()).collect();

    let cancelled = live_cancel(state.inner());
    let log = live_log(Some(root), s.model.clone(), s.base_url.clone());
    let hooks = AiHooks {
        cancelled: &cancelled,
        log: &log,
    };
    run_proofread(&s, &text, &names, &hooks).await
}

/// 1回の呼び出しの出力トークン数(usage を返さないモデルでは 0)
fn out_tokens(out: &ai::ChatOutcome) -> u64 {
    out.usage.map(|u| u.completion_tokens).unwrap_or(0)
}

/// ログに添える但し書き。打ち切りと拒否は**次にすべきことが逆**なので分けて残す
fn outcome_note(out: &ai::ChatOutcome) -> Option<&'static str> {
    if out.truncated() {
        Some("打ち切られた")
    } else if out.refused() {
        Some("空の応答(拒否の疑い)")
    } else {
        None
    }
}

/// 誤字脱字チェックの本体。
///
/// 1塊ごとの手順:
///  1. 経路A(構造化出力)。接続先が受け付けなければ経路B(スキーマ無し)へ落とす
///  2. **形式として読み取れない**ときだけ経路Bで測り直す。打ち切りなら測り直さない
///     (原因はコンテキスト長で、投げ直しても同じになる)
///  3. **判定は測り直しの後で下す** — 打ち切り / 拒否(空応答)/ 形式不備 / 読めた。
///     経路Aが空でも、測り直しで読めたならそれは拒否ではない
///
/// 打ち切り・拒否・形式不備のどれも「指摘なし」ではない。区別して警告に出す。
async fn run_proofread(
    s: &AiSettings,
    text: &str,
    names: &[String],
    h: &AiHooks<'_>,
) -> Result<AiProofreadResult, String> {
    // 長文はまとめて投げると応答が打ち切られることがあるので分割して順に検査する
    // (PoC#7 §7.1)。分割字数は環境によって最適値が違うため設定可能(§7.2)
    let all_chunks = proofread::split_for_check_with(text, s.check_chunk_chars);
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
    // 最後に失敗した理由。捨てると時間切れまで「接続設定を確認」に化ける(テスト計画 E7)
    let mut last_failure: Option<String> = None;
    // 応答が打ち切られた塊の数。「指摘なし」と取り違えると誤報告になる
    let mut truncated = 0usize;
    // 1文字も返らなかった塊。検閲による拒否でこの形になる(打ち切りとは対処が逆)
    let mut refused = 0usize;
    // 応答は返ったが形式として読み取れなかった塊。これも「誤りなし」ではない
    let mut unparsed = 0usize;
    let mut cancelled = false;

    let starts = proofread::chunk_starts(text, &chunks);

    for (chunk, &chunk_start) in chunks.iter().zip(&starts) {
        if (h.cancelled)() {
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
                content: proofread::build_prompt(chunk, names),
            },
        ];

        // 経路A: 構造化出力。失敗したら経路B(スキーマ無し+寛容パース)へ落とす。
        // どの待ちでも、中止されたらその場でやめる(待っていた応答は捨てる)
        let mut used_fallback = false;
        let call_started = std::time::Instant::now();
        let first = match unless_cancelled(
            h.cancelled,
            ai::chat(
                &s.base_url,
                &s.api_key,
                &s.model,
                &messages,
                0.1,
                Some(proofread::issue_schema()),
            ),
        )
        .await
        {
            None => {
                (h.log)("proofread", &messages, "", ms_since(call_started), Some("中止された"));
                cancelled = true;
                break;
            }
            Some(Ok(out)) => out,
            Some(Err(_)) => {
                used_fallback = true;
                match unless_cancelled(
                    h.cancelled,
                    ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.1, None),
                )
                .await
                {
                    None => {
                        (h.log)("proofread", &messages, "", ms_since(call_started), Some("中止された"));
                        cancelled = true;
                        break;
                    }
                    Some(Ok(out)) => out,
                    Some(Err(e)) => {
                        let note = format!("失敗: {e}");
                        (h.log)("proofread", &messages, "", ms_since(call_started), Some(&note));
                        // 一部が落ちても、取れた分は返す(全部やり直させない)
                        failures += 1;
                        last_failure = Some(e);
                        continue;
                    }
                }
            }
        };
        completion_tokens += out_tokens(&first);
        (h.log)(
            "proofread",
            &messages,
            &first.content,
            ms_since(call_started),
            outcome_note(&first),
        );

        let mut chunk_truncated = first.truncated();
        let mut got_text = !first.content.trim().is_empty();
        let mut issues = proofread::parse_ai_issues(&first.content);
        let mut structured = proofread::looks_structured(&first.content);
        // **指定した形で読み取れない**場合は経路Bで測り直す(レビュー側と同じ条件)。
        // 条件を `issues.is_empty()` にすると、正常な「指摘なし」(`{"issues":[]}`)でも
        // 毎回2回投げることになり、誤字の無い本文ほど時間とトークンが倍かかる。
        // ただし**打ち切られていた場合は再試行しない**。原因はコンテキスト長の
        // 不足であって出力形式ではないため、投げ直しても同じ結果になり時間を捨てるだけ
        if !structured && !used_fallback && !chunk_truncated {
            let retry_started = std::time::Instant::now();
            let retried = unless_cancelled(
                h.cancelled,
                ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.1, None),
            )
            .await;
            // 測り直しの途中で中止されたら、この塊は判定しない(読めたかどうか分からない)
            let Some(retried) = retried else {
                (h.log)("proofread", &messages, "", ms_since(retry_started), Some("中止された"));
                cancelled = true;
                break;
            };
            if let Ok(out) = retried {
                completion_tokens += out_tokens(&out);
                (h.log)(
                    "proofread",
                    &messages,
                    &out.content,
                    ms_since(retry_started),
                    Some(outcome_note(&out).unwrap_or("形式の測り直し(経路B)")),
                );
                got_text |= !out.content.trim().is_empty();
                chunk_truncated |= out.truncated();
                // 経路Bが**読み取れた**なら、0件でもそれが答え(誤字なしと読めている)
                if proofread::looks_structured(&out.content) {
                    used_fallback = true;
                    issues = proofread::parse_ai_issues(&out.content);
                    structured = true;
                }
            }
        }

        // 判定は測り直しの後で下す。どれも「誤りなし」ではない
        if chunk_truncated {
            truncated += 1;
        } else if !structured {
            if got_text {
                unparsed += 1;
            } else {
                refused += 1;
            }
        }
        if used_fallback {
            path = "fallback";
        }
        // 位置は**その塊の中で**付ける(本文全体の最初の一致に当てると、別の塊の
        // 同じ文字列を指してしまう。テスト計画 A4)
        collected.extend(proofread::resolve_issues_in(text, chunk_start, chunk, issues));
    }

    if failures == chunks.len() && !chunks.is_empty() {
        return Err(all_failed(last_failure.as_deref()));
    }

    let elapsed = started.elapsed();
    let elapsed_ms = elapsed.as_millis() as u64;
    let tokens_per_sec = if completion_tokens > 0 && elapsed.as_secs_f64() > 0.0 {
        Some(completion_tokens as f64 / elapsed.as_secs_f64())
    } else {
        None
    };

    // 打ち切り・拒否・形式不備は「指摘なし」と区別して必ず伝える。
    // 校正で「誤りが無い」と誤解させるのは最悪の誤報告になる。
    // (警告は画面にそのまま文字で出るので、Markdown の強調記号は書かない)
    let warning = if truncated > 0 {
        Some(format!(
            "{truncated}箇所で応答が途中で打ち切られました(モデルのコンテキスト長が不足しています)。\
             見落としがある可能性が高いので、LM Studio のコンテキスト長を増やすか、\
             短い範囲に区切って確認してください"
        ))
    } else if refused > 0 {
        Some(format!(
            "{refused}箇所でモデルが応答を返しませんでした。「誤りなし」ではありません。\
             題材によっては検閲で拒否されることがあります。非検閲モデルに切り替えてお試しください"
        ))
    } else if unparsed > 0 {
        Some(format!(
            "{unparsed}箇所で、応答を指定した形式として読み取れませんでした\
             (モデルが形式を守っていません)。「誤りなし」ではありません。\
             別のモデルをお試しください"
        ))
    } else if failures > 0 {
        Some(some_failed(failures, "検査", last_failure.as_deref()))
    } else {
        None
    };

    // 同じ箇所への同じ指摘(測り直しで重なったもの等)だけを1つにまとめ、本文の順に並べる
    let mut issues = proofread::dedupe_issues(collected);
    proofread::order_issues(&mut issues);

    Ok(AiProofreadResult {
        issues,
        unchecked_chars,
        path: path.to_string(),
        model: s.model.clone(),
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
        return Err(NO_MODEL.to_string());
    }
    let root = root_of(&state)?;
    let cancelled = live_cancel(state.inner());
    let log = live_log(Some(root.clone()), s.model.clone(), s.base_url.clone());
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
    let Some(out) = unless_cancelled(
        &cancelled,
        ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.2, None),
    )
    .await
    else {
        log("split", &messages, "", ms_since(started), Some("中止された"));
        return Err("中止しました".to_string());
    };
    let out = out?;
    log(
        "split",
        &messages,
        &out.content,
        ms_since(started),
        outcome_note(&out),
    );
    // 1回きりの呼び出しなので途中では止められない。返ってきた時点で中止されていたら
    // 提案は出さない(ヘッダーの「中止」を押したのに提案が出ると、押した意味が無い)
    if cancelled() {
        return Err("中止しました".to_string());
    }

    let points = split::verify(body, split::parse(&out.content));
    Ok(SplitSuggestion {
        points,
        total_chars: body.chars().count(),
        model: s.model,
        elapsed_ms: started.elapsed().as_millis() as u64,
        warning: split_warning(&out),
    })
}

/// 分割の提案に付ける断り書き。**「切れ目なし」と読めてしまう失敗を黙らない**
/// (打ち切り・拒否・形式不備の3つ。どれも候補0件として画面に届く)
fn split_warning(out: &ai::ChatOutcome) -> Option<String> {
    if out.truncated() {
        Some(
            "応答が途中で打ち切られました。本文が長すぎてモデルが最後まで読めていない可能性があります"
                .to_string(),
        )
    } else if out.refused() {
        Some("モデルが応答を返しませんでした。別のモデルをお試しください".to_string())
    } else if !split::looks_structured(&out.content) {
        Some(
            "応答を指定した形式として読み取れませんでした(モデルが形式を守っていません)。\
             「切れ目なし」ではありません。別のモデルをお試しください"
                .to_string(),
        )
    } else {
        None
    }
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
    /// **実際に渡した**設定資料のパス(materials と同じ順)。頼んだ資料のうち、上限を超えたもの・
    /// 読めなかったものは入らない。講評の記録にはこちらを書く(テスト計画 E4。以前は
    /// 頼んだ資料をそのまま「渡した」と書いていた)
    material_paths: Vec<String>,
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
        return Err(NO_MODEL.to_string());
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

    let cancelled = live_cancel(state.inner());
    let log = live_log(Some(root.clone()), s.model.clone(), s.base_url.clone());
    let hooks = AiHooks {
        cancelled: &cancelled,
        log: &log,
    };
    let mut result = run_review(&s, &text, &picked, &materials, material_names, &hooks).await?;
    result.material_paths = ctx.entries.iter().map(|e| e.path.clone()).collect();
    Ok(result)
}

/// レビューの本体。
///
/// 1塊ごとの手順は校正と同じ形(経路A→B / 形式の測り直し / **判定は測り直しの後**)。
/// 同じ形だが**共通化はしない**(06 §4 の汎用AI実行エンジンの撤回)。
/// その代わり、両方の挙動を擬似サーバー相手のテストで固定している。
///
/// 読み取れなかった応答は捨てずに、生のまま全体講評として見せる(§6.3 の最後の受け皿)。
async fn run_review(
    s: &AiSettings,
    text: &str,
    picked: &[&review::Aspect],
    materials: &[(String, String)],
    material_names: Vec<String>,
    h: &AiHooks<'_>,
) -> Result<AiReviewResult, String> {
    let used_aspects: Vec<String> = picked.iter().map(|a| a.key.to_string()).collect();

    // 長文は分割する。所要時間は実行回数でほぼ決まるので、分割字数は設定に従う(§7.2)
    let all_chunks = proofread::split_for_check_with(text, s.check_chunk_chars);
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
    // 最後に失敗した理由。捨てると時間切れまで「接続設定を確認」に化ける(テスト計画 E7)
    let mut last_failure: Option<String> = None;
    // 応答が打ち切られた塊。「指摘なし」と取り違えると誤報告になる
    let mut truncated = 0usize;
    // 1文字も返らなかった塊。検閲による拒否でこの形になる
    let mut refused = 0usize;
    // 応答は返ったが指定した形で読み取れなかった塊。これも「指摘なし」ではない
    let mut unparsed = 0usize;
    let mut cancelled = false;

    let starts = proofread::chunk_starts(text, &chunks);

    for (chunk, &chunk_start) in chunks.iter().zip(&starts) {
        if (h.cancelled)() {
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
                content: review::build_prompt(chunk, picked, materials),
            },
        ];

        let mut used_fallback = false;
        let call_started = std::time::Instant::now();
        // 温度は校正(0.1)より少し高くする。校正は正解が1つだが、
        // レビューは読み方に幅があり、固めすぎると当たり障りのない指摘に寄る
        //
        // 経路A: 構造化出力。失敗したら経路B(スキーマ無し+寛容パース)へ落とす。
        // どの待ちでも、中止されたらその場でやめる(待っていた応答は捨てる)
        let first = match unless_cancelled(
            h.cancelled,
            ai::chat(
                &s.base_url,
                &s.api_key,
                &s.model,
                &messages,
                0.3,
                Some(review::review_schema()),
            ),
        )
        .await
        {
            None => {
                (h.log)("review", &messages, "", ms_since(call_started), Some("中止された"));
                cancelled = true;
                break;
            }
            Some(Ok(out)) => out,
            Some(Err(_)) => {
                used_fallback = true;
                match unless_cancelled(
                    h.cancelled,
                    ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.3, None),
                )
                .await
                {
                    None => {
                        (h.log)("review", &messages, "", ms_since(call_started), Some("中止された"));
                        cancelled = true;
                        break;
                    }
                    Some(Ok(out)) => out,
                    Some(Err(e)) => {
                        let note = format!("失敗: {e}");
                        (h.log)("review", &messages, "", ms_since(call_started), Some(&note));
                        // 一部が落ちても、取れた分は返す(全部やり直させない)
                        failures += 1;
                        last_failure = Some(e);
                        continue;
                    }
                }
            }
        };
        completion_tokens += out_tokens(&first);
        (h.log)(
            "review",
            &messages,
            &first.content,
            ms_since(call_started),
            outcome_note(&first),
        );

        let mut chunk_truncated = first.truncated();
        let mut got_text = !first.content.trim().is_empty();
        let mut parsed = review::parse(&first.content);
        let mut shown_raw = first.content;
        // 指定した形で読み取れない場合は経路Bで測り直す。
        // ただし**打ち切られていた場合は再試行しない**(原因は出力形式ではなく
        // コンテキスト不足なので、投げ直しても同じ結果になり時間を捨てるだけ)
        if !parsed.structured && !used_fallback && !chunk_truncated {
            let retry_started = std::time::Instant::now();
            let retried = unless_cancelled(
                h.cancelled,
                ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.3, None),
            )
            .await;
            // 測り直しの途中で中止されたら、この塊は判定しない(読めたかどうか分からない)
            let Some(retried) = retried else {
                (h.log)("review", &messages, "", ms_since(retry_started), Some("中止された"));
                cancelled = true;
                break;
            };
            if let Ok(out) = retried {
                completion_tokens += out_tokens(&out);
                (h.log)(
                    "review",
                    &messages,
                    &out.content,
                    ms_since(retry_started),
                    Some(outcome_note(&out).unwrap_or("形式の測り直し(経路B)")),
                );
                chunk_truncated |= out.truncated();
                let retried = review::parse(&out.content);
                if retried.structured {
                    used_fallback = true;
                    parsed = retried;
                    shown_raw = out.content;
                } else if !out.content.trim().is_empty() {
                    got_text = true;
                    // 経路Aが空で測り直しが散文なら、見せるべき生の応答は測り直しの方
                    if shown_raw.trim().is_empty() {
                        shown_raw = out.content;
                    }
                }
            }
        }

        // 判定は測り直しの後で下す。経路Aが空でも、測り直しで読めたならそれは拒否ではない
        if chunk_truncated {
            truncated += 1;
        }
        if !parsed.structured {
            if got_text {
                // 形式を守らない応答(散文・英語・箇条書き等)を捨てない。
                //
                // **捨てると「指摘なし」と区別が付かなくなる**。中身はあるのに読めていない
                // だけなので、生の応答をそのまま講評として見せ、照合できていないことを警告する。
                // これはレビューで最悪の誤報告(問題ありませんでした)を防ぐための処置であり、
                // 経路B(プレーンテキスト+寛容パース)の最後の受け皿にあたる(§6.3)
                unparsed += 1;
                parsed.overall = review::clip_raw(&shown_raw);
            } else if !chunk_truncated {
                refused += 1;
            }
        }

        if used_fallback {
            path = "fallback";
        }
        overalls.push(parsed.overall);
        // 位置は**その塊の中で**付ける(本文全体を前から探すと、前の塊にある同じ一文や
        // 先頭の似た一文に当たる。テスト計画 A5)
        collected.extend(review::resolve_in(text, chunk_start, chunk, parsed.comments));
    }

    if failures == chunks.len() && !chunks.is_empty() {
        return Err(all_failed(last_failure.as_deref()));
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
             「指摘なし」ではありません。読み取れなかった応答はそのまま下に出していますが、\
             引用が本文に実在するかの照合ができていないので、内容は鵜呑みにしないでください。\
             別のモデルをお試しください"
        ))
    } else if failures > 0 {
        Some(some_failed(failures, "レビュー", last_failure.as_deref()))
    } else {
        None
    };

    let mut comments = review::dedupe(collected);
    review::order(&mut comments);

    Ok(AiReviewResult {
        comments,
        overall: review::merge_overall(&overalls),
        aspects: used_aspects,
        materials: material_names,
        // パスは呼び出し元(review_ai)が、実際に渡した資料から埋める
        material_paths: Vec::new(),
        unchecked_chars,
        path: path.to_string(),
        model: s.model.clone(),
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
        return Err(NO_MODEL.to_string());
    }
    let root = root_of(&state)?;
    let codex = project::load_codex(&root).map_err(to_msg)?;

    let cancelled = live_cancel(state.inner());
    let log = live_log(Some(root), s.model.clone(), s.base_url.clone());
    let hooks = AiHooks {
        cancelled: &cancelled,
        log: &log,
    };
    run_extract(&s, &text, &codex, &hooks).await
}

/// 設定抽出の本体。
///
/// **「候補なし」と「見ていない・読めていない」を取り違えないこと。** 校正・レビューと同じく、
/// 打ち切り / 拒否(空応答)/ 形式不備 を区別して警告に出す。さらに抽出は、
/// 長い本文の末尾を見ていないことも黙っていた(校正・レビューは未検査の字数を出していた)。
///
/// 形式の測り直しはしない(抽出は経路Bだけで動いている。新しい手順を足さない)。
async fn run_extract(
    s: &AiSettings,
    text: &str,
    codex: &[CodexEntry],
    h: &AiHooks<'_>,
) -> Result<ExtractResult, String> {
    let known: Vec<String> = codex.iter().flat_map(|c| c.patterns()).collect();

    let all_chunks = proofread::split_for_check_with(text, s.check_chunk_chars);
    let total_chunks = all_chunks.len();
    let chunks: Vec<String> = all_chunks.into_iter().take(proofread::MAX_CHUNKS).collect();
    let unchecked_chars = if total_chunks > chunks.len() {
        text.chars().count() - chunks.iter().map(|c| c.chars().count()).sum::<usize>()
    } else {
        0
    };

    let started = std::time::Instant::now();
    let mut raw_all = Vec::new();
    let mut truncated = 0usize;
    let mut refused = 0usize;
    let mut unparsed = 0usize;
    let mut failures = 0usize;
    // 最後に失敗した理由。捨てると時間切れまで「接続設定を確認」に化ける(テスト計画 E7)
    let mut last_failure: Option<String> = None;
    let mut cancelled = false;

    for chunk in &chunks {
        if (h.cancelled)() {
            cancelled = true;
            break;
        }
        let call_started = std::time::Instant::now();
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
        let Some(answer) = unless_cancelled(
            h.cancelled,
            ai::chat(&s.base_url, &s.api_key, &s.model, &messages, 0.1, None),
        )
        .await
        else {
            (h.log)("extract", &messages, "", ms_since(call_started), Some("中止された"));
            cancelled = true;
            break;
        };
        match answer {
            Ok(out) => {
                (h.log)(
                    "extract",
                    &messages,
                    &out.content,
                    ms_since(call_started),
                    outcome_note(&out),
                );
                if out.truncated() {
                    truncated += 1;
                } else if out.refused() {
                    refused += 1;
                } else if !extract::looks_structured(&out.content) {
                    unparsed += 1;
                }
                raw_all.extend(extract::parse(&out.content));
            }
            Err(e) => {
                let note = format!("失敗: {e}");
                (h.log)("extract", &messages, "", ms_since(call_started), Some(&note));
                failures += 1;
                last_failure = Some(e);
            }
        }
    }

    if failures == chunks.len() && !chunks.is_empty() {
        return Err(all_failed(last_failure.as_deref()));
    }

    let raw_count = raw_all.len();
    // 実在性・重複・名寄せの検証は本文全体に対して行う
    let verified = extract::verify(text, raw_all, codex);
    let rejected = raw_count.saturating_sub(verified.candidates.len());
    let (candidates, conflicts) = (verified.candidates, verified.conflicts);

    let mut notes: Vec<String> = Vec::new();
    if truncated > 0 {
        notes.push(format!(
            "{truncated}箇所で応答が途中で打ち切られました(モデルのコンテキスト長が不足しています)。\
             取りこぼしがある可能性があります"
        ));
    } else if refused > 0 {
        notes.push(format!(
            "{refused}箇所でモデルが応答を返しませんでした。題材によっては検閲で拒否されることがあります。\
             非検閲モデルに切り替えてお試しください"
        ));
    } else if unparsed > 0 {
        notes.push(format!(
            "{unparsed}箇所で、応答を指定した形式として読み取れませんでした(モデルが形式を守っていません)。\
             「候補なし」ではありません。別のモデルをお試しください"
        ));
    } else if failures > 0 {
        notes.push(some_failed(failures, "抽出", last_failure.as_deref()));
    }
    if unchecked_chars > 0 {
        notes.push(format!(
            "本文が長いため、末尾の{unchecked_chars}字は見ていません\
             (1回に送る字数×{}塊まで)。範囲を分けて実行してください",
            proofread::MAX_CHUNKS
        ));
    }
    let warning = if notes.is_empty() {
        None
    } else {
        Some(notes.join(" / "))
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
    // 先に今の状態へ反映する(書けなくても、この起動の間は新しい設定で動く)
    let saved = persist_settings(&settings, None);
    *state.ai.lock().map_err(|_| "状態の更新に失敗")? = settings;
    saved.map_err(|e| {
        format!("設定を保存できませんでした({e})。変更はアプリを閉じるまで有効です")
    })
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
    /// 中止された。**`Done` と区別する** — 区別しないと、1文字目が届く前に止めた相談が
    /// 「検閲で拒否された」と表示され、途中で止めた応答が完結した往復として
    /// 会話の履歴に積まれていた
    Cancelled,
    Error(String),
    /// 実際に送った往復の数(会話モード)。上限を超えた古い往復は送らないので、
    /// 画面が持っている往復の数より少ないことがある(テスト計画 E1)。問い合わせる前に1回流す
    HistorySent(usize),
}

/// 相談の応答の終わり方
#[derive(Debug, PartialEq, Eq)]
enum StreamEnd {
    /// 最後まで受け取った(`[DONE]` か、接続が閉じた)
    Done,
    /// 中止された。それ以降の増分は流していない
    Cancelled,
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
        return Err(NO_MODEL.to_string());
    }
    let messages = context::build_chat_messages(&context, &question, &history);
    // 送らない往復があったことを画面と記録に伝える(黙って落とさない)
    let _ = on_event.send(ChatEvent::HistorySent(context::sent_history(&history).len()));
    // 応答を待っている間に中止されることもあるので、投げる前に世代を覚える
    let cancelled = live_cancel(state.inner());
    let log = live_log(root_of(&state).ok(), s.model.clone(), s.base_url.clone());
    let hooks = AiHooks {
        cancelled: &cancelled,
        log: &log,
    };
    let started = std::time::Instant::now();

    let sent = unless_cancelled(
        &cancelled,
        ai::stream_request(&s.base_url, &s.api_key, &s.model, &messages, s.temperature).send(),
    )
    .await;
    let resp = match sent {
        Some(Ok(r)) => r,
        Some(Err(e)) if !cancelled() => return Err(ai::describe_error(&e, &s.base_url)),
        // 応答の始まりを待つ間に中止された(その後で接続が切れた場合も)。
        // 通信の失敗とは言わない
        _ => {
            log("chat", &messages, "", ms_since(started), Some("中止された"));
            let _ = on_event.send(ChatEvent::Cancelled);
            return Ok(());
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let msg = ai::describe_status(status, &body, &s.base_url);
        log("chat", &messages, "", ms_since(started), Some(&msg));
        let _ = on_event.send(ChatEvent::Error(msg.clone()));
        return Err(msg);
    }

    let mut forward = |d: String| {
        let _ = on_event.send(ChatEvent::Delta(d));
    };
    match consume_stream(resp, &hooks, &mut forward).await {
        Ok((end, answer)) => {
            let note = match end {
                StreamEnd::Cancelled => Some("中止された"),
                StreamEnd::Done if answer.trim().is_empty() => {
                    Some("応答が1文字も返らなかった(拒否の疑い)")
                }
                StreamEnd::Done => None,
            };
            log("chat", &messages, &answer, ms_since(started), note);
            let _ = on_event.send(match end {
                StreamEnd::Done => ChatEvent::Done,
                StreamEnd::Cancelled => ChatEvent::Cancelled,
            });
            Ok(())
        }
        Err((msg, answer)) => {
            log("chat", &messages, &answer, ms_since(started), Some(&msg));
            let _ = on_event.send(ChatEvent::Error(msg.clone()));
            Err(msg)
        }
    }
}

/// 相談の応答を受け取りきる。受け取った増分は `on_delta` へ流し、全文も返す。
///
/// - 受信は**バイト列のまま**行に切る(割れた日本語1文字を化けさせない=`ai::drain_sse_lines`)
/// - 改行で終わらずに切れた最後の行も捨てない(`[DONE]` を送らないサーバがある)
/// - 中止されたら、**それ以降の増分は流さずに** `Cancelled` で返す。
///   中止のあとで受信が途切れた・終わった場合も `Cancelled`(中止を「途中までの完結した
///   応答」や「通信の失敗」と取り違えると、会話の履歴に積まれたり、拒否と表示されたりする)
///
/// 失敗したときも、そこまでに受け取った分を返す(ログに残すため)。
async fn consume_stream(
    resp: reqwest::Response,
    h: &AiHooks<'_>,
    on_delta: &mut (dyn FnMut(String) + Send),
) -> Result<(StreamEnd, String), (String, String)> {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut answer = String::new();
    loop {
        // 次の塊が来ない間(モデルが考えている間など)も、中止されたら待つのをやめる
        let Some(next) = unless_cancelled(h.cancelled, stream.next()).await else {
            return Ok((StreamEnd::Cancelled, answer));
        };
        let Some(chunk) = next else { break };
        let chunk = match chunk {
            Ok(c) => c,
            Err(_) if (h.cancelled)() => return Ok((StreamEnd::Cancelled, answer)),
            Err(e) => return Err((format!("受信が中断されました: {e}"), answer)),
        };
        if (h.cancelled)() {
            return Ok((StreamEnd::Cancelled, answer));
        }
        buf.extend_from_slice(&chunk);
        for line in ai::drain_sse_lines(&mut buf) {
            match ai::parse_sse_line(&line) {
                ai::SseEvent::Delta(d) => {
                    answer.push_str(&d);
                    on_delta(d);
                }
                ai::SseEvent::Done => return Ok((StreamEnd::Done, answer)),
                ai::SseEvent::Ignore => {}
            }
        }
    }
    // 中止を待つ間に、それ以上の増分なしで接続が閉じた
    if (h.cancelled)() {
        return Ok((StreamEnd::Cancelled, answer));
    }
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf).into_owned();
        if let ai::SseEvent::Delta(d) = ai::parse_sse_line(&line) {
            answer.push_str(&d);
            on_delta(d);
        }
    }
    Ok((StreamEnd::Done, answer))
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

    let builder = tauri::Builder::default();
    // 二重起動させない(テスト計画 G1)。2つ目は立ち上がらず、開いている窓を前に出す。
    // 2つの窓で同じ原稿を開くと、片方の古い本文で上書きしかねない(競合の確認は出るが、
    // 「アプリの外で変更」と言われても本人には何のことか分からない)。
    // **最初に登録する**(ほかのプラグインより前でないと効かない、とプラグインの約束にある)。
    // 開発版(npm run tauri dev)には入れない。識別子がインストール版と同じなので、
    // インストール版で執筆しながら開発版を起動できなくなる
    #[cfg(all(desktop, not(debug_assertions)))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        use tauri::Manager;
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }));

    builder
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

/// AIコマンドの本体(`run_*` / `consume_stream`)を、擬似サーバー相手に動かすテスト。
///
/// lib.rs にはここまでテストが1件も無かった。校正の二重実行、拒否を打ち切りと取り違える
/// 誤診、中止が「検閲で拒否」と表示される不具合は、どれもこの層で起きている。
/// 解析関数の単体テストでは捕まらない「何回投げたか」「どの警告を出したか」を固定する。
#[cfg(test)]
mod ai_run_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// 擬似サーバーの応答の台本
    enum Reply {
        /// 非ストリームの chat/completions 応答(JSON本文)
        Json(String),
        /// HTTPエラー。構造化出力を受け付けない接続先などの再現に使う
        Status(u16),
        /// ストリーム。各要素を**間を空けて別々に**書き込む(読み取り単位を割るため)
        Stream(Vec<Vec<u8>>),
        /// 何も返さずに黙り込む(長い本文を読み込んでいるモデル)。中止で待つのをやめられるかを見る
        Hang,
        /// 途中まで流してから黙り込む(次の塊の前に考え込んでいるモデル)
        StreamThenHang(Vec<Vec<u8>>),
    }

    /// 黙り込む長さ。テストはこれより十分短い時間で終わらなければならない
    const HANG: std::time::Duration = std::time::Duration::from_secs(10);

    /// 台本どおりに答える OpenAI 互換の擬似サーバー。
    ///
    /// 受けたリクエストの本文を記録する。**台本が尽きたら 500 を返す** —
    /// 想定より多く投げた(=二重実行)ときに、テストが必ず落ちるようにするため。
    struct Mock {
        base_url: String,
        bodies: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Mock {
        fn count(&self) -> usize {
            self.bodies.lock().unwrap().len()
        }
        fn body(&self, i: usize) -> String {
            self.bodies.lock().unwrap()[i].clone()
        }
    }

    fn mock(script: Vec<Reply>) -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = bodies.clone();
        std::thread::spawn(move || {
            let mut script = script.into_iter();
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { break };
                let body = read_request(&mut conn);
                seen.lock().unwrap().push(body);
                match script.next() {
                    Some(Reply::Json(json)) => respond(&mut conn, 200, &json),
                    Some(Reply::Status(code)) => {
                        respond(&mut conn, code, r#"{"error":"unsupported"}"#)
                    }
                    Some(Reply::Stream(parts)) => write_stream(&mut conn, parts),
                    Some(Reply::Hang) => std::thread::sleep(HANG),
                    Some(Reply::StreamThenHang(parts)) => {
                        write_stream(&mut conn, parts);
                        std::thread::sleep(HANG);
                    }
                    None => respond(
                        &mut conn,
                        500,
                        r#"{"error":"台本切れ(想定より多く呼ばれた)"}"#,
                    ),
                }
            }
        });
        Mock {
            base_url: format!("http://{addr}/v1"),
            bodies,
        }
    }

    fn read_request(conn: &mut TcpStream) -> String {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 8192];
        let header_end = loop {
            let n = conn.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                return String::new();
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let head = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
        let len = head
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while buf.len() < header_end + len {
            let n = conn.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        String::from_utf8_lossy(&buf[header_end..]).into_owned()
    }

    fn write_stream(conn: &mut TcpStream, parts: Vec<Vec<u8>>) {
        let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
        let _ = conn.write_all(head.as_bytes());
        for p in parts {
            let _ = conn.write_all(&p);
            let _ = conn.flush();
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
    }

    fn respond(conn: &mut TcpStream, code: u16, body: &str) {
        let head = format!(
            "HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        );
        let _ = conn.write_all(head.as_bytes());
        let _ = conn.write_all(body.as_bytes());
        let _ = conn.flush();
    }

    /// chat/completions の応答。content が None なら `null`(OpenAIの拒否の形)
    fn chat(content: Option<&str>, finish: &str) -> Reply {
        Reply::Json(
            serde_json::json!({
                "choices": [{
                    "message": {"role": "assistant", "content": content},
                    "finish_reason": finish
                }],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5}
            })
            .to_string(),
        )
    }

    fn sse_delta(text: &str) -> String {
        let v = serde_json::json!({"choices": [{"delta": {"content": text}}]});
        format!("data: {v}\n\n")
    }

    fn settings(m: &Mock) -> AiSettings {
        AiSettings {
            base_url: m.base_url.clone(),
            api_key: None,
            model: "test-model".into(),
            temperature: 0.7,
            check_chunk_chars: 3_000,
        }
    }

    fn never() -> impl Fn() -> bool + Send + Sync {
        || false
    }

    fn no_log() -> impl Fn(&str, &[ChatMessage], &str, u64, Option<&str>) + Send + Sync {
        |_, _, _, _, _| {}
    }

    const TEXT: &str = "　転校初日の朝は、雨だった。佐藤架純は昇降口で靴を履き替える。";

    /// 数塊に分かれる長さの本文(分割字数 500 で使う)
    fn long_text() -> String {
        (0..40)
            .map(|i| format!("{i}行目。これは分割の試験に使う本文で、それなりの長さがある。"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ===== 校正 =====

    /// 2つの塊に分かれる本文。「以外と」が1塊目では正しく(それ以外と比べて)、
    /// 2塊目では誤り(以外と簡単だった=意外と)として出てくる
    fn text_with_a_repeated_phrase() -> String {
        let pad = |tag: &str| {
            (0..12)
                .map(|i| format!("{tag}{i}行目。これは分割の試験に使う本文で、それなりの長さがある。"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        format!(
            "{}\nそれ以外と比べて、ここは静かだった。\n{}\n試験は以外と簡単だった。\n",
            pad("前"),
            pad("後")
        )
    }

    fn u16_at(text: &str, from: usize, to: usize) -> String {
        let units: Vec<u16> = text.encode_utf16().collect();
        String::from_utf16(&units[from..to]).unwrap()
    }

    /// テスト計画 A4: 2塊目の誤りを指摘されたら、**2塊目の箇所**を指す。
    /// 本文全体の最初の一致に当てると、1塊目の正しい「それ以外と」を書き換えてしまう
    #[tokio::test]
    async fn proofread_points_at_the_occurrence_in_the_chunk_it_came_from() {
        let issue = r#"{"issues":[{"quote":"以外と","suggestion":"意外と","kind":"変換ミス","reason":"文脈から"}]}"#;
        let m = mock(vec![chat(Some(r#"{"issues":[]}"#), "stop"), chat(Some(issue), "stop")]);
        let mut s = settings(&m);
        s.check_chunk_chars = 500;
        let text = text_with_a_repeated_phrase();
        let chunks = proofread::split_for_check_with(&text, 500);
        assert_eq!(chunks.len(), 2, "テストの前提: 2塊に分かれること");
        assert!(chunks[0].contains("それ以外と") && chunks[1].contains("以外と簡単"));

        let c = never();
        let l = no_log();
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&s, &text, &[], &h).await.unwrap();
        assert_eq!(r.issues.len(), 1);
        let (st, en) = (r.issues[0].start_utf16.unwrap(), r.issues[0].end_utf16.unwrap());
        assert_eq!(u16_at(&text, st, en), "以外と");
        assert_eq!(
            u16_at(&text, en, en + 2),
            "簡単",
            "指摘が1塊目の正しい「それ以外と」に当たっている"
        );
    }

    /// 同じ誤りが別々の塊にあれば、**どちらも**指摘として残す(重複として1つに潰さない)
    #[tokio::test]
    async fn proofread_keeps_the_same_typo_found_in_different_chunks() {
        let issue = r#"{"issues":[{"quote":"以外と","suggestion":"意外と","kind":"変換ミス","reason":"r"}]}"#;
        let m = mock(vec![chat(Some(issue), "stop"), chat(Some(issue), "stop")]);
        let mut s = settings(&m);
        s.check_chunk_chars = 500;
        let text = text_with_a_repeated_phrase();
        let c = never();
        let l = no_log();
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&s, &text, &[], &h).await.unwrap();
        let mut at: Vec<usize> = r.issues.iter().map(|i| i.start_utf16.unwrap()).collect();
        at.sort();
        at.dedup();
        assert_eq!(at.len(), 2, "別々の箇所の同じ誤りを1つに潰した: {:?}", r.issues);
    }

    /// **誤字が無い本文で2回投げないこと**(外部レビュー指摘6の回帰)。
    /// 正常な「指摘なし」(`{"issues":[]}`)で測り直すと、誤字の無い本文ほど時間が倍かかる
    #[tokio::test]
    async fn proofread_asks_once_when_the_answer_is_no_issues() {
        let m = mock(vec![chat(Some(r#"{"issues":[]}"#), "stop")]);
        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&settings(&m), TEXT, &[], &h).await.unwrap();
        assert_eq!(m.count(), 1, "誤字なしの応答で測り直している");
        assert!(r.issues.is_empty());
        assert!(r.warning.is_none(), "{:?}", r.warning);
        assert_eq!(r.path, "schema");
    }

    /// 散文で返されたら経路Bで1回だけ測り直し、読めたらそれを採る
    #[tokio::test]
    async fn proofread_retries_once_when_the_answer_is_prose() {
        let m = mock(vec![
            chat(Some("誤字は見当たりませんでした。"), "stop"),
            chat(Some(r#"{"issues":[]}"#), "stop"),
        ]);
        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&settings(&m), TEXT, &[], &h).await.unwrap();
        assert_eq!(m.count(), 2);
        assert!(r.warning.is_none(), "測り直しで読めたのに警告している: {:?}", r.warning);
        assert_eq!(r.path, "fallback");
        // 測り直しは構造化出力を付けない(経路B)
        assert!(m.body(0).contains("response_format"));
        assert!(!m.body(1).contains("response_format"));
    }

    /// 測り直しても読めなければ、**「誤りなし」ではなく**読み取れなかったと伝える
    #[tokio::test]
    async fn proofread_reports_unreadable_answers_instead_of_no_issues() {
        let m = mock(vec![
            chat(Some("全体的に良い文章です。"), "stop"),
            chat(Some("特に問題はありません。"), "stop"),
        ]);
        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&settings(&m), TEXT, &[], &h).await.unwrap();
        let w = r.warning.expect("読めなかったのに警告が無い");
        assert!(w.contains("読み取れませんでした"), "{w}");
        assert!(!w.contains("**"), "警告は文字のまま出るので強調記号を書かない: {w}");
    }

    /// **空応答(検閲による拒否)を「コンテキスト長が不足」と誤診しないこと。**
    /// 終了理由 stop の空応答は、打ち切りとは対処が逆(04-design §7.1)
    #[tokio::test]
    async fn proofread_tells_refusal_from_truncation() {
        let m = mock(vec![chat(Some(""), "stop"), chat(Some(""), "stop")]);
        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&settings(&m), TEXT, &[], &h).await.unwrap();
        let w = r.warning.expect("拒否なのに警告が無い(=「誤りなし」と誤報告)");
        assert!(w.contains("検閲"), "{w}");
        assert!(!w.contains("コンテキスト長"), "拒否を打ち切りと誤診している: {w}");
    }

    /// OpenAI は拒否すると `content: null` を返す。解析エラー(=接続の問題)ではなく拒否として扱う
    #[tokio::test]
    async fn proofread_treats_null_content_as_refusal() {
        let m = mock(vec![chat(None, "stop"), chat(None, "stop")]);
        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&settings(&m), TEXT, &[], &h)
            .await
            .expect("null を解析エラーにして、接続の失敗と誤診している");
        assert!(r.warning.unwrap_or_default().contains("検閲"));
    }

    /// 打ち切りは測り直さない(原因はコンテキスト長で、投げ直しても同じになる)
    #[tokio::test]
    async fn proofread_does_not_retry_after_truncation() {
        let m = mock(vec![chat(Some(r#"{"issues":[{"quote":"#), "length")]);
        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&settings(&m), TEXT, &[], &h).await.unwrap();
        assert_eq!(m.count(), 1, "打ち切りで測り直している");
        assert!(r.warning.unwrap_or_default().contains("打ち切られました"));
    }

    /// 構造化出力を受け付けない接続先では経路Bへ落ちる
    #[tokio::test]
    async fn proofread_falls_back_when_the_schema_is_rejected() {
        let m = mock(vec![Reply::Status(400), chat(Some(r#"{"issues":[]}"#), "stop")]);
        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&settings(&m), TEXT, &[], &h).await.unwrap();
        assert_eq!(m.count(), 2);
        assert_eq!(r.path, "fallback");
        assert!(r.warning.is_none(), "{:?}", r.warning);
    }

    /// 中止されたら次の塊へ進まず、ここまでの結果を「中止しました」付きで返す
    #[tokio::test]
    async fn proofread_stops_at_the_next_chunk_when_cancelled() {
        let m = mock(vec![
            chat(Some(r#"{"issues":[]}"#), "stop"),
            chat(Some(r#"{"issues":[]}"#), "stop"),
            chat(Some(r#"{"issues":[]}"#), "stop"),
            chat(Some(r#"{"issues":[]}"#), "stop"),
        ]);
        let mut s = settings(&m);
        s.check_chunk_chars = 500;
        let text = long_text();
        assert!(
            proofread::split_for_check_with(&text, 500).len() >= 2,
            "テストの前提: 複数の塊に分かれること"
        );
        let sent = m.bodies.clone();
        // 1塊目を投げた後で中止された、という状況
        let c = move || !sent.lock().unwrap().is_empty();
        let l = no_log();
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&s, &text, &[], &h).await.unwrap();
        assert_eq!(m.count(), 1, "中止の後も投げている");
        assert!(r.warning.unwrap_or_default().contains("中止しました"));
    }

    /// 中止されたら、終わらない待ちでもすぐにやめる。終わる待ちなら値を返す
    #[tokio::test]
    async fn waiting_stops_as_soon_as_cancelled() {
        let t0 = std::time::Instant::now();
        let c = move || t0.elapsed() > std::time::Duration::from_millis(200);
        assert!(unless_cancelled(&c, std::future::pending::<()>()).await.is_none());
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(2),
            "止まるまでが遅すぎる: {:?}",
            t0.elapsed()
        );
        let n = never();
        assert_eq!(unless_cancelled(&n, async { 7 }).await, Some(7));
    }

    /// 応答の始まりを待っている間(長い本文の読み込み中など)に中止したら、応答を待たずに止まる。
    /// 以前は「次の塊へ進まない」ところでしか止めず、応答が返るまで(最大240秒)止まらなかった
    #[tokio::test]
    async fn proofread_stops_waiting_for_a_silent_model_when_cancelled() {
        let m = mock(vec![Reply::Hang]);
        let t0 = std::time::Instant::now();
        let c = move || t0.elapsed() > std::time::Duration::from_millis(300);
        let l = no_log();
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&settings(&m), TEXT, &[], &h).await.unwrap();
        assert!(
            t0.elapsed() < HANG / 3,
            "黙っているモデルを待ち続けた: {:?}",
            t0.elapsed()
        );
        assert!(r.warning.unwrap_or_default().contains("中止しました"));
        assert_eq!(m.count(), 1, "中止のあとに投げ直した");
    }

    /// テスト計画 E6: 同時に走っている AI 処理は、1回の中止で**すべて**止まる
    /// (中止は世代番号なので、同時に走るどれもが同じ中止を見る)
    #[tokio::test]
    async fn one_cancel_stops_every_running_task() {
        let (m1, m2) = (mock(vec![Reply::Hang]), mock(vec![Reply::Hang]));
        let t0 = std::time::Instant::now();
        let c = move || t0.elapsed() > std::time::Duration::from_millis(300);
        let l = no_log();
        let h = AiHooks { cancelled: &c, log: &l };
        let (s1, s2) = (settings(&m1), settings(&m2));
        let (p, e) = tokio::join!(
            run_proofread(&s1, TEXT, &[], &h),
            run_extract(&s2, TEXT, &[], &h)
        );
        assert!(t0.elapsed() < HANG / 3, "どれかが止まらなかった: {:?}", t0.elapsed());
        assert!(p.unwrap().warning.unwrap_or_default().contains("中止しました"));
        assert!(e.unwrap().warning.unwrap_or_default().contains("中止しました"));
    }

    /// 塊の合間に黙り込んだ(次の塊の前に考えている)間に中止しても、次の塊を待たずに止まる
    #[tokio::test]
    async fn stream_stops_waiting_during_silence_when_cancelled() {
        use std::sync::atomic::AtomicBool;
        let m = mock(vec![Reply::StreamThenHang(vec![sse_delta("一").into_bytes()])]);
        let stopped = Arc::new(AtomicBool::new(false));
        let seen = stopped.clone();
        let c = move || seen.load(Ordering::SeqCst);
        let l = no_log();
        let h = AiHooks {
            cancelled: &c,
            log: &l,
        };
        let t0 = std::time::Instant::now();
        let resp = ai::stream_request(&m.base_url, &None, "test-model", &[], 0.7)
            .send()
            .await
            .unwrap();
        let mut push = |_: String| stopped.store(true, Ordering::SeqCst);
        let (end, answer) = consume_stream(resp, &h, &mut push).await.unwrap();
        assert_eq!(end, StreamEnd::Cancelled);
        assert_eq!(answer, "一");
        assert!(
            t0.elapsed() < HANG / 3,
            "次の塊を待ち続けた: {:?}",
            t0.elapsed()
        );
    }

    /// 測り直しも含めて、**呼び出しのたびに記録が残ること**(T-06)。
    /// 測り直しの記録が無いと、形式不備の調査に要るやりとりが残らない
    #[tokio::test]
    async fn proofread_logs_every_call_including_the_retry() {
        let m = mock(vec![
            chat(Some("散文の応答"), "stop"),
            chat(Some(r#"{"issues":[]}"#), "stop"),
        ]);
        let logged = Arc::new(AtomicUsize::new(0));
        let n = logged.clone();
        let c = never();
        let l = move |feature: &str, _: &[ChatMessage], _: &str, _: u64, _: Option<&str>| {
            assert_eq!(feature, "proofread");
            n.fetch_add(1, Ordering::SeqCst);
        };
        let h = AiHooks { cancelled: &c, log: &l };
        run_proofread(&settings(&m), TEXT, &[], &h).await.unwrap();
        assert_eq!(logged.load(Ordering::SeqCst), 2, "測り直しが記録されていない");
    }

    // ===== レビュー =====

    fn review_json(overall: &str) -> String {
        serde_json::json!({"comments": [], "overall": overall}).to_string()
    }

    async fn review_with(m: &Mock) -> AiReviewResult {
        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        let picked = review::selected_aspects(&[]);
        run_review(&settings(m), TEXT, &picked, &[], vec![], &h)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn review_asks_once_when_the_answer_reads() {
        let m = mock(vec![chat(Some(&review_json("よく書けています")), "stop")]);
        let r = review_with(&m).await;
        assert_eq!(m.count(), 1);
        assert!(!r.refused && !r.unparsed);
        assert!(r.warning.is_none(), "{:?}", r.warning);
    }

    /// **経路Aが空でも、測り直しで読めたなら拒否ではない。**
    /// 以前は最初の空応答で「拒否」を数えたまま残し、結果が出ているのに
    /// 「検閲で拒否されました」と警告していた
    #[tokio::test]
    async fn review_is_not_a_refusal_when_the_retry_reads() {
        let m = mock(vec![
            chat(Some(""), "stop"),
            chat(Some(&review_json("測り直しで読めた講評")), "stop"),
        ]);
        let r = review_with(&m).await;
        assert_eq!(m.count(), 2);
        assert!(!r.refused, "読めたのに拒否として数えている");
        assert!(r.warning.is_none(), "{:?}", r.warning);
        assert!(r.overall.contains("測り直しで読めた講評"));
    }

    /// 2つの塊に分かれ、同じ書き出しの一文が両方の塊にある本文
    fn text_with_a_repeated_opening() -> String {
        let pad = |tag: &str| {
            (0..12)
                .map(|i| format!("{tag}{i}行目。これは分割の試験に使う本文で、それなりの長さがある。"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        format!(
            "{}\n彼女は窓の外を見つめたまま、ため息をついた。\n{}\n彼女は窓の外を見つめたまま、何も答えなかった。\n",
            pad("前"),
            pad("後")
        )
    }

    /// テスト計画 A5: 2塊目への指摘は、2塊目の箇所を指す(引用が言い換えられていても)。
    /// 以前は本文全体を前から探したため、1塊目の似た一文へ「移動」した
    #[tokio::test]
    async fn review_points_at_the_sentence_in_the_chunk_it_came_from() {
        let comment = serde_json::json!({
            "comments": [{"aspect": "style", "quote": "彼女は窓の外を見つめたまま答えなかった", "comment": "沈黙の描写が弱い"}],
            "overall": ""
        })
        .to_string();
        let m = mock(vec![chat(Some(&review_json("")), "stop"), chat(Some(&comment), "stop")]);
        let mut s = settings(&m);
        s.check_chunk_chars = 500;
        let text = text_with_a_repeated_opening();
        let chunks = proofread::split_for_check_with(&text, 500);
        assert_eq!(chunks.len(), 2, "テストの前提: 2塊に分かれること");
        assert!(chunks[0].contains("ため息") && chunks[1].contains("何も答えなかった"));

        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        let picked = review::selected_aspects(&[]);
        let r = run_review(&s, &text, &picked, &[], vec![], &h).await.unwrap();
        assert_eq!(r.comments.len(), 1);
        let cm = &r.comments[0];
        assert!(cm.found);
        let (st, en) = (cm.start_utf16.unwrap(), cm.end_utf16.unwrap());
        assert_eq!(u16_at(&text, st, en), "彼女は窓の外を見つめたまま");
        assert_eq!(u16_at(&text, en, en + 3), "、何も", "指摘が1塊目の一文に当たっている");
    }

    #[tokio::test]
    async fn review_reports_refusal_when_nothing_comes_back() {
        let m = mock(vec![chat(Some(""), "stop"), chat(Some(""), "stop")]);
        let r = review_with(&m).await;
        assert!(r.refused);
        assert!(r.warning.unwrap_or_default().contains("検閲"));
    }

    /// 読めない応答は捨てずに、生のまま講評として見せる(§6.3 の最後の受け皿)
    #[tokio::test]
    async fn review_shows_unreadable_answers_as_they_are() {
        let m = mock(vec![
            chat(Some("全体として冗長な部分があります。"), "stop"),
            chat(Some("全体として冗長な部分があります。"), "stop"),
        ]);
        let r = review_with(&m).await;
        assert!(r.unparsed);
        assert!(!r.refused);
        assert!(r.overall.contains("冗長な部分"));
        let w = r.warning.unwrap_or_default();
        assert!(w.contains("「指摘なし」ではありません"), "{w}");
        assert!(!w.contains("**"), "{w}");
    }

    // ===== 抽出 =====

    async fn extract_with(s: &AiSettings, text: &str) -> ExtractResult {
        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        run_extract(s, text, &[], &h).await.unwrap()
    }

    /// 散文で返されたら「候補なし」ではなく読み取れなかったと伝える
    #[tokio::test]
    async fn extract_reports_unreadable_instead_of_no_candidates() {
        let m = mock(vec![chat(Some("登場人物は佐藤架純です。"), "stop")]);
        let r = extract_with(&settings(&m), TEXT).await;
        assert!(r.candidates.is_empty());
        let w = r.warning.expect("読めなかったのに警告が無い");
        assert!(w.contains("読み取れませんでした"), "{w}");
    }

    #[tokio::test]
    async fn extract_tells_refusal_from_truncation() {
        let m = mock(vec![chat(Some(""), "stop")]);
        let r = extract_with(&settings(&m), TEXT).await;
        let w = r.warning.expect("拒否なのに警告が無い");
        assert!(w.contains("検閲"), "{w}");
        assert!(!w.contains("打ち切られ"), "拒否を打ち切りと誤診している: {w}");
    }

    #[tokio::test]
    async fn extract_reads_an_empty_list_as_no_candidates() {
        let m = mock(vec![chat(Some(r#"{"entities":[]}"#), "stop")]);
        let r = extract_with(&settings(&m), TEXT).await;
        assert!(r.warning.is_none(), "{:?}", r.warning);
    }

    /// **見ていない末尾を黙らないこと。** 校正・レビューは未検査の字数を出していたが、
    /// 抽出だけ黙っており、長い原稿で「候補なし」と「見ていない」が区別できなかった
    #[tokio::test]
    async fn extract_says_so_when_the_tail_was_not_read() {
        let replies = (0..proofread::MAX_CHUNKS)
            .map(|_| chat(Some(r#"{"entities":[]}"#), "stop"))
            .collect();
        let m = mock(replies);
        let mut s = settings(&m);
        s.check_chunk_chars = 500;
        let text = (0..6).map(|_| long_text()).collect::<Vec<_>>().join("\n");
        assert!(proofread::split_for_check_with(&text, 500).len() > proofread::MAX_CHUNKS);
        let r = extract_with(&s, &text).await;
        let w = r.warning.expect("末尾を見ていないのに黙っている");
        assert!(w.contains("見ていません"), "{w}");
    }

    /// テスト計画 E7: すべての塊で失敗したら、**失敗の理由をそのまま伝える**。
    /// 以前は理由を捨てて一律に「接続設定を確認してください」と出していた
    /// (時間切れ=モデルが遅いだけ、でも接続先やキーを疑わせた)
    #[tokio::test]
    async fn every_run_tells_why_it_failed() {
        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        // APIキーの拒否(経路A・Bとも)
        let m = mock(vec![Reply::Status(401), Reply::Status(401)]);
        let err = run_proofread(&settings(&m), TEXT, &[], &h).await.err().expect("失敗するはず");
        assert!(err.contains("APIキー"), "校正: {err}");
        let m = mock(vec![Reply::Status(401), Reply::Status(401)]);
        let picked = review::selected_aspects(&[]);
        let err = run_review(&settings(&m), TEXT, &picked, &[], vec![], &h).await.err().expect("失敗するはず");
        assert!(err.contains("APIキー"), "レビュー: {err}");
        let m = mock(vec![Reply::Status(401), Reply::Status(401)]);
        let err = run_extract(&settings(&m), TEXT, &[], &h).await.err().expect("失敗するはず");
        assert!(err.contains("APIキー"), "抽出: {err}");

        // 接続先に何も居ない
        let closed = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            drop(l);
            port
        };
        let mut s = settings(&mock(vec![]));
        s.base_url = format!("http://127.0.0.1:{closed}/v1");
        let err = run_proofread(&s, TEXT, &[], &h).await.err().expect("失敗するはず");
        assert!(err.contains("接続できませんでした"), "{err}");
    }

    /// 一部の塊だけ失敗したときも、断り書きに理由を添える
    #[tokio::test]
    async fn partial_failure_note_tells_why() {
        let m = mock(vec![
            chat(Some(r#"{"issues":[]}"#), "stop"),
            Reply::Status(401),
            Reply::Status(401),
        ]);
        let mut s = settings(&m);
        s.check_chunk_chars = 500;
        let text = text_with_a_repeated_phrase();
        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        let r = run_proofread(&s, &text, &[], &h).await.unwrap();
        let w = r.warning.expect("一部失敗したのに断りが無い");
        assert!(w.contains("1箇所の検査に失敗しました") && w.contains("APIキー"), "{w}");
    }

    /// テスト計画 E3: 塊の上限を超えた本文では、校正もレビューも見ていない末尾の字数を返す
    /// (画面はこれを見て「指摘なし」と言わない)。字数は、見た塊の分を引いた残りと一致する
    #[tokio::test]
    async fn proofread_and_review_report_the_unread_tail() {
        let text = (0..6).map(|_| long_text()).collect::<Vec<_>>().join("\n");
        let chunks = proofread::split_for_check_with(&text, 500);
        assert!(chunks.len() > proofread::MAX_CHUNKS, "テストの前提: 上限を超えること");
        let read: usize = chunks[..proofread::MAX_CHUNKS].iter().map(|c| c.chars().count()).sum();
        let expected = text.chars().count() - read;

        let (c, l) = (never(), no_log());
        let h = AiHooks { cancelled: &c, log: &l };
        let m = mock((0..proofread::MAX_CHUNKS).map(|_| chat(Some(r#"{"issues":[]}"#), "stop")).collect());
        let mut s = settings(&m);
        s.check_chunk_chars = 500;
        let r = run_proofread(&s, &text, &[], &h).await.unwrap();
        assert_eq!(r.unchecked_chars, expected, "校正");

        let m = mock((0..proofread::MAX_CHUNKS).map(|_| chat(Some(&review_json("")), "stop")).collect());
        let mut s = settings(&m);
        s.check_chunk_chars = 500;
        let picked = review::selected_aspects(&[]);
        let r = run_review(&s, &text, &picked, &[], vec![], &h).await.unwrap();
        assert_eq!(r.unchecked_chars, expected, "レビュー");
    }

    /// テスト計画 E3: 同じ人物が2つの塊で挙がっても、候補は1つにまとまる(二重に出さない)
    #[tokio::test]
    async fn extract_merges_the_same_entity_from_two_chunks() {
        let pad = |tag: &str| {
            (0..12)
                .map(|i| format!("{tag}{i}行目。これは分割の試験に使う本文で、それなりの長さがある。"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let text = format!("{}\n佐藤架純が来た。\n{}\n佐藤架純が笑った。\n", pad("前"), pad("後"));
        assert_eq!(proofread::split_for_check_with(&text, 500).len(), 2, "テストの前提");
        let entity = |desc: &str| {
            serde_json::json!({"entities": [{"name": "佐藤架純", "kind": "character", "description": desc, "aliases": []}]})
                .to_string()
        };
        let m = mock(vec![chat(Some(&entity("主人公")), "stop"), chat(Some(&entity("高校生")), "stop")]);
        let mut s = settings(&m);
        s.check_chunk_chars = 500;
        let r = extract_with(&s, &text).await;
        let names: Vec<&str> = r.candidates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["佐藤架純"], "同じ人物が二重に出た: {names:?}");
    }

    // ===== 相談(ストリーミング) =====

    async fn stream_with(
        m: &Mock,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> (Result<(StreamEnd, String), (String, String)>, Vec<String>) {
        let l = no_log();
        let h = AiHooks { cancelled, log: &l };
        let resp = ai::stream_request(&m.base_url, &None, "test-model", &[], 0.7)
            .send()
            .await
            .unwrap();
        let mut got = Vec::new();
        let mut push = |d: String| got.push(d);
        let r = consume_stream(resp, &h, &mut push).await;
        (r, got)
    }

    /// **読み取りの切れ目が日本語1文字の途中に落ちても化けないこと**(外部レビュー指摘3)
    #[tokio::test]
    async fn stream_keeps_a_character_split_across_reads() {
        let line = sse_delta("あい").into_bytes();
        let cut = line
            .windows(3)
            .position(|w| w == "あ".as_bytes())
            .unwrap()
            + 1;
        let m = mock(vec![Reply::Stream(vec![
            line[..cut].to_vec(),
            line[cut..].to_vec(),
            b"data: [DONE]\n\n".to_vec(),
        ])]);
        let c = never();
        let (r, got) = stream_with(&m, &c).await;
        let (end, answer) = r.unwrap();
        assert_eq!(end, StreamEnd::Done);
        assert_eq!(answer, "あい");
        assert!(!answer.contains(char::REPLACEMENT_CHARACTER));
        assert_eq!(got.concat(), "あい");
    }

    /// 改行で終わらずに切れた最後の行も捨てない
    #[tokio::test]
    async fn stream_keeps_the_last_line_without_a_newline() {
        let last = sse_delta("最後").trim_end().as_bytes().to_vec();
        let m = mock(vec![Reply::Stream(vec![sse_delta("最初と").into_bytes(), last])]);
        let c = never();
        let (r, _) = stream_with(&m, &c).await;
        assert_eq!(r.unwrap().1, "最初と最後");
    }

    /// 中止されたら `Cancelled` で返し、**それ以降の増分は流さない**。
    /// `Done` で返すと、画面は「検閲で拒否された」と誤表示し、会話の履歴に途中の応答が積まれる
    #[tokio::test]
    async fn stream_reports_cancel_as_cancelled_not_done() {
        use std::sync::atomic::AtomicBool;
        let m = mock(vec![Reply::Stream(vec![
            sse_delta("一").into_bytes(),
            sse_delta("二").into_bytes(),
            sse_delta("三").into_bytes(),
            b"data: [DONE]\n\n".to_vec(),
        ])]);
        // 1文字目が画面に出た直後に「中止」が押された、という状況。
        // (中止を見る回数で決めると、待っている間にも見るようになった今は早すぎる)
        let stopped = Arc::new(AtomicBool::new(false));
        let seen = stopped.clone();
        let c = move || seen.load(Ordering::SeqCst);
        let l = no_log();
        let h = AiHooks {
            cancelled: &c,
            log: &l,
        };
        let resp = ai::stream_request(&m.base_url, &None, "test-model", &[], 0.7)
            .send()
            .await
            .unwrap();
        let mut got = Vec::new();
        let mut push = |d: String| {
            got.push(d);
            stopped.store(true, Ordering::SeqCst);
        };
        let (end, answer) = consume_stream(resp, &h, &mut push).await.unwrap();
        assert_eq!(end, StreamEnd::Cancelled);
        assert_eq!(answer, "一");
        assert_eq!(got, vec!["一".to_string()], "中止の後も流している");
    }

    /// 中止のあと、それ以上の増分なしで接続が閉じても `Cancelled` で返す。
    /// `Done` で返すと、途中までの応答が完結した往復として会話の履歴に積まれ、
    /// 1文字も来ていなければ「拒否の疑い」と表示される(実機確認で見つけた経路)
    #[tokio::test]
    async fn stream_reports_cancel_when_the_connection_just_closes_after_it() {
        use std::sync::atomic::AtomicBool;
        // [DONE] を送らずに閉じるサーバー
        let m = mock(vec![Reply::Stream(vec![sse_delta("一").into_bytes()])]);
        let stopped = Arc::new(AtomicBool::new(false));
        let seen = stopped.clone();
        let c = move || seen.load(Ordering::SeqCst);
        let l = no_log();
        let h = AiHooks {
            cancelled: &c,
            log: &l,
        };
        let resp = ai::stream_request(&m.base_url, &None, "test-model", &[], 0.7)
            .send()
            .await
            .unwrap();
        let mut got = Vec::new();
        // 1文字目を受け取った直後に「中止」が押された
        let mut push = |d: String| {
            got.push(d);
            stopped.store(true, Ordering::SeqCst);
        };
        let (end, answer) = consume_stream(resp, &h, &mut push).await.unwrap();
        assert_eq!(end, StreamEnd::Cancelled, "中止のあとの終端を完了と取り違えた");
        assert_eq!(answer, "一");
    }

    fn outcome(content: &str, finish: Option<&str>) -> ai::ChatOutcome {
        ai::ChatOutcome {
            content: content.to_string(),
            usage: None,
            finish_reason: finish.map(str::to_string),
        }
    }

    /// 分割の提案で、候補0件に見える失敗を黙らないこと。
    /// 散文で返されると以前は警告なしで「切れ目は見つかりませんでした」と出ていた
    #[test]
    fn split_warns_when_the_answer_could_not_be_read() {
        assert!(split_warning(&outcome(r#"{"points":[]}"#, Some("stop"))).is_none());
        let prose = split_warning(&outcome("この本文は一つの場面です。", Some("stop")))
            .expect("散文なのに警告が無い(=「切れ目なし」と誤報告)");
        assert!(prose.contains("読み取れませんでした"), "{prose}");
        let refused = split_warning(&outcome("", Some("stop"))).expect("拒否なのに警告が無い");
        assert!(refused.contains("応答を返しませんでした"), "{refused}");
        let cut = split_warning(&outcome(r#"{"points":[{"quo"#, Some("length")))
            .expect("打ち切りなのに警告が無い");
        assert!(cut.contains("打ち切られました"), "{cut}");
    }
}
