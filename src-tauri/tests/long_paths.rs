//! テスト計画 B3(docs/07-test-plan.md): 深い階層と長い日本語名で、パス全体が
//! Windows の 260 文字(MAX_PATH)を超える場合。
//!
//! 保存・保存前のバックアップ(`.app/backups/…`)・ゴミ箱(`.app/trash/日時/…`)・改名・
//! 複製・検索の索引・ツリーの走査が動くか。判定: どれかが失敗する、黙って保存されない。

use std::fs;
use std::path::PathBuf;

use kaku_lib::{project, search};

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "kaku-long-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    project::init(&p).unwrap();
    p
}

/// 長い日本語名のフォルダーを重ねた相対パス(フォルダーまで)
fn deep_dir() -> String {
    let level = "第一部・王都編の長い長い章のまとまりを表すフォルダー名";
    let mut rel = String::from("manuscript");
    for i in 0..8 {
        rel.push_str(&format!("/{level}{i}"));
    }
    rel
}

#[test]
fn every_file_operation_works_past_max_path() {
    let root = tmp("ops");
    let dir = deep_dir();
    let rel = format!("{dir}/場面.md");
    let full = root.join(&rel);
    let full_len = full.to_string_lossy().encode_utf16().count();
    assert!(full_len > 260, "テストの前提: 260 文字を超えること({full_len})");

    // 作る・読む
    assert!(project::create_file(&root, &rel, "長い場所の本文").unwrap());
    assert_eq!(project::read_text(&root, &rel).unwrap(), "長い場所の本文");
    // 保存(2回目からは保存前のバックアップを取る)
    project::save_text(&root, &rel, "書き換えた本文").unwrap();
    project::save_text(&root, &rel, "さらに書き換えた本文").unwrap();
    assert_eq!(project::read_text(&root, &rel).unwrap(), "さらに書き換えた本文");
    // バックアップは本文よりさらに長い場所(.app/backups/…)に書かれる
    let backup = root.join(".app/backups").join(&rel);
    assert_eq!(fs::read_to_string(&backup).unwrap(), "書き換えた本文", "バックアップが無い");
    assert!(project::modified_ms(&root, &rel).is_ok());
    // ツリーと検索
    let tree = format!("{:?}", project::scan(&root).unwrap());
    assert!(tree.contains("場面"), "ツリーに出ない");
    let hits = search::search(&root, "さらに書き換えた").unwrap().hits;
    assert_eq!(hits.len(), 1, "索引に入らない");
    // 複製・改名(リンク追随つき)・ゴミ箱
    let copy = project::duplicate(&root, &rel).unwrap();
    let moved = format!("{dir}/場面・改.md");
    project::rename(&root, &rel, &moved).unwrap();
    assert_eq!(project::read_text(&root, &moved).unwrap(), "さらに書き換えた本文");
    let trashed = project::trash(&root, &copy).unwrap();
    assert!(std::path::Path::new(&trashed).exists(), "ゴミ箱に無い: {trashed}");
    assert!(!project::resolve(&root, &copy).unwrap().exists(), "元の場所に残った");
    fs::remove_dir_all(&root).ok();
}
