//! テスト計画 B6(docs/07-test-plan.md): 読み取り専用のファイル、他のアプリが開いている
//! (ロックされた)ファイルへの保存・改名・ゴミ箱。
//!
//! 判定: 失敗したのに成功に見える、本文が消える、元に戻しても保存できないままになる、
//! 改名が途中で止まって他のファイルのリンクまで切れる。

use std::fs;
use std::path::{Path, PathBuf};

use kaku_lib::project;

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "kaku-locked-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    project::init(&p).unwrap();
    p
}

fn set_readonly(path: &Path, on: bool) {
    let mut perm = fs::metadata(path).unwrap().permissions();
    perm.set_readonly(on);
    fs::set_permissions(path, perm).unwrap();
}

/// 後片付け: 読み取り専用のまま残すと消せない
fn cleanup(root: &Path) {
    fn walk(dir: &Path) {
        for e in fs::read_dir(dir).into_iter().flatten().flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                walk(&e.path());
            } else if fs::metadata(e.path()).map(|m| m.permissions().readonly()).unwrap_or(false) {
                set_readonly(&e.path(), false);
            }
        }
    }
    walk(root);
    fs::remove_dir_all(root).ok();
}

/// 読み取り専用の原稿は、理由の分かる文面で断り、中身に触れない
#[test]
fn readonly_file_is_refused_with_a_clear_reason() {
    let root = tmp("ro");
    let rel = "manuscript/a.md";
    project::create_file(&root, rel, "元の本文").unwrap();
    set_readonly(&root.join(rel), true);
    let err = project::save_text(&root, rel, "書き換え").unwrap_err().to_string();
    assert!(err.contains("読み取り専用"), "原因の分からない文面: {err}");
    assert_eq!(project::read_text(&root, rel).unwrap(), "元の本文");
    // 書き込めるように戻せば保存できる
    set_readonly(&root.join(rel), false);
    project::save_text(&root, rel, "書き換え").unwrap();
    assert_eq!(project::read_text(&root, rel).unwrap(), "書き換え");
    cleanup(&root);
}

/// 読み取り専用の控え(前の版が残したもの)があっても、保存は止まらない。
/// 以前は控えを取る fs::copy が読み取り専用の属性まで写したため、一度でも読み取り専用の
/// 原稿を控えると、原稿を書き込めるように戻しても「アクセスが拒否されました」で保存できなかった
#[test]
fn readonly_backup_does_not_block_saving() {
    let root = tmp("robackup");
    let rel = "manuscript/a.md";
    project::create_file(&root, rel, "一回目").unwrap();
    project::save_text(&root, rel, "二回目").unwrap();
    let backup = root.join(".app/backups").join(rel);
    set_readonly(&backup, true);
    project::save_text(&root, rel, "三回目").unwrap();
    assert_eq!(project::read_text(&root, rel).unwrap(), "三回目");
    assert_eq!(fs::read_to_string(&backup).unwrap(), "二回目", "控えが更新されていない");
    assert!(!fs::metadata(&backup).unwrap().permissions().readonly(), "控えが読み取り専用のまま");
    cleanup(&root);
}

/// リンクを書き換えられないファイル(読み取り専用)があっても、**ほかのファイルは書き換え**、
/// 書き換えられなかったものを返す。改名そのものは済んでいるので失敗にしない。
/// 以前は最初の失敗で止まり、書き換えられたはずの目次2まで切れたまま、失敗と表示された
#[test]
fn rename_rewrites_every_writable_link_and_reports_the_rest() {
    let root = tmp("rolink");
    project::create_file(&root, "manuscript/a.md", "本文").unwrap();
    project::create_file(&root, "manuscript/目次.md", "[a](a.md)\n").unwrap();
    project::create_file(&root, "manuscript/目次2.md", "[a](a.md)\n").unwrap();
    project::create_file(&root, "plot/流れ.md", "[a](../manuscript/a.md)\n").unwrap();
    set_readonly(&root.join("manuscript/目次.md"), true);

    let failed = project::rename(&root, "manuscript/a.md", "manuscript/b.md").unwrap();
    assert_eq!(failed, vec!["manuscript/目次.md".to_string()]);
    assert!(root.join("manuscript/b.md").is_file() && !root.join("manuscript/a.md").exists());
    assert_eq!(project::read_text(&root, "manuscript/目次2.md").unwrap(), "[a](b.md)\n");
    assert_eq!(project::read_text(&root, "plot/流れ.md").unwrap(), "[a](../manuscript/b.md)\n");
    // 読み取り専用のファイルには触れていない
    assert_eq!(project::read_text(&root, "manuscript/目次.md").unwrap(), "[a](a.md)\n");
    // 読み取り専用の原稿から取った控えも、書き込める状態で置く(次の控えで上書きできるように)
    let backup = root.join(".app/backups/manuscript/目次.md");
    assert!(!fs::metadata(&backup).unwrap().permissions().readonly(), "控えが読み取り専用のまま");
    cleanup(&root);
}

/// ほかのアプリが共有なしで開いている原稿: 保存は失敗として返り、中身はそのまま
#[cfg(windows)]
#[test]
fn locked_file_is_not_reported_as_saved() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tmp("lock");
    let rel = "manuscript/a.md";
    project::create_file(&root, rel, "元の本文").unwrap();
    let held = fs::OpenOptions::new().read(true).share_mode(0).open(root.join(rel)).unwrap();
    assert!(project::save_text(&root, rel, "書き換え").is_err(), "握られているのに保存できたことになった");
    assert!(project::trash(&root, rel).is_err(), "握られているのにゴミ箱へ移せたことになった");
    drop(held);
    assert_eq!(project::read_text(&root, rel).unwrap(), "元の本文");
    cleanup(&root);
}

/// 読み取り専用の原稿もゴミ箱へは移せる(消さずに退避するだけなので、止める理由がない)
#[test]
fn readonly_file_can_still_be_moved_to_the_trash() {
    let root = tmp("rotrash");
    let rel = "manuscript/a.md";
    project::create_file(&root, rel, "本文").unwrap();
    set_readonly(&root.join(rel), true);
    let trashed = project::trash(&root, rel).unwrap();
    assert!(Path::new(&trashed).is_file());
    assert!(!root.join(rel).exists());
    cleanup(&root);
}
