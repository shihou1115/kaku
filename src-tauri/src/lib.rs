//! Tauri アプリのエントリポイント。
//!
//! 方針(docs/04-design.md §2): ドメインロジック(解析・索引・検出)は Rust 側に置き、
//! フロントは表示と編集に徹する。コマンドは機能別モジュールに分ける(uggg の流儀を踏襲)。

mod mentions;

use mentions::Mention;

/// 本文から codex の名前・別名の出現箇所を返す(M-02 言及検出の骨格)。
///
/// 位置は永続化せず、呼ばれるたびに算出する(docs/03-data-format.md D-7)。
#[tauri::command]
fn find_mentions(text: String, patterns: Vec<String>) -> Vec<Mention> {
    mentions::find_mentions(&text, &patterns)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![find_mentions])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
