//! テスト計画 G3(docs/07-test-plan.md): 開いているファイルをアプリの外で削除・改名・
//! 書き換えてから、アプリで保存する。
//!
//! 判定: 黙ってファイルを作り直す(削除が取り消される・改名で二重になる)、
//! 外部の変更を黙って踏み潰す。

use std::fs;
use std::path::PathBuf;

use kaku_lib::project::{self, SaveCheck};

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "kaku-external-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    project::init(&p).unwrap();
    p
}

/// 開いた状態: ファイルを作り、読み込んだときの時刻を返す
fn opened(root: &std::path::Path, rel: &str) -> u64 {
    project::create_file(root, rel, "元の本文").unwrap();
    project::modified_ms(root, rel).unwrap()
}

#[test]
fn unchanged_file_is_saved() {
    let root = tmp("plain");
    let ms = opened(&root, "manuscript/a.md");
    let r = project::save_checked(&root, "manuscript/a.md", "打った本文", Some(ms)).unwrap();
    assert!(matches!(r, SaveCheck::Saved(_)), "{r:?}");
    assert_eq!(project::read_text(&root, "manuscript/a.md").unwrap(), "打った本文");
    fs::remove_dir_all(root).ok();
}

/// アプリの外で書き換えられていたら、書かずに競合を返す(外部の内容はそのまま)
#[test]
fn external_edit_is_not_overwritten() {
    let root = tmp("edit");
    let ms = opened(&root, "manuscript/a.md");
    let path = root.join("manuscript/a.md");
    fs::write(&path, "外で書いた本文").unwrap();
    // 時刻を確実にずらす(同じミリ秒に収まると、時刻では見分けられない)
    let later = std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms + 5_000);
    fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();

    let r = project::save_checked(&root, "manuscript/a.md", "打った本文", Some(ms)).unwrap();
    assert!(matches!(r, SaveCheck::Conflict(_)), "{r:?}");
    assert_eq!(project::read_text(&root, "manuscript/a.md").unwrap(), "外で書いた本文");
    fs::remove_dir_all(root).ok();
}

/// アプリの外で削除されていたら、**書かずに** Missing を返す(黙って作り直さない)
#[test]
fn externally_deleted_file_is_not_recreated() {
    let root = tmp("delete");
    let ms = opened(&root, "manuscript/a.md");
    fs::remove_file(root.join("manuscript/a.md")).unwrap();

    let r = project::save_checked(&root, "manuscript/a.md", "打った本文", Some(ms)).unwrap();
    assert_eq!(r, SaveCheck::Missing);
    assert!(!root.join("manuscript/a.md").exists(), "削除を取り消した(作り直した)");
    fs::remove_dir_all(root).ok();
}

/// アプリの外で名前を変えられていたら、元の名前で作り直さない(二重にしない)
#[test]
fn externally_renamed_file_is_not_duplicated() {
    let root = tmp("rename");
    let ms = opened(&root, "manuscript/a.md");
    fs::rename(root.join("manuscript/a.md"), root.join("manuscript/b.md")).unwrap();

    let r = project::save_checked(&root, "manuscript/a.md", "打った本文", Some(ms)).unwrap();
    assert_eq!(r, SaveCheck::Missing);
    assert!(!root.join("manuscript/a.md").exists(), "元の名前で作り直した(二重になる)");
    assert_eq!(project::read_text(&root, "manuscript/b.md").unwrap(), "元の本文");
    fs::remove_dir_all(root).ok();
}

/// 本人が「この内容で作り直す」を選んだときは、時刻を渡さずに書く(作り直せる)。
/// 時刻が読めなかった印(0)のときも、照合できないので書く(以前と同じ)
#[test]
fn recreating_is_possible_when_asked_for() {
    let root = tmp("recreate");
    opened(&root, "manuscript/a.md");
    fs::remove_file(root.join("manuscript/a.md")).unwrap();
    let r = project::save_checked(&root, "manuscript/a.md", "作り直した本文", None).unwrap();
    assert!(matches!(r, SaveCheck::Saved(_)), "{r:?}");
    assert_eq!(project::read_text(&root, "manuscript/a.md").unwrap(), "作り直した本文");

    fs::remove_file(root.join("manuscript/a.md")).unwrap();
    let r = project::save_checked(&root, "manuscript/a.md", "時刻なし", Some(0)).unwrap();
    assert!(matches!(r, SaveCheck::Saved(_)), "{r:?}");
    fs::remove_dir_all(root).ok();
}
