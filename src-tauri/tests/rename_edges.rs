//! テスト計画 B4(docs/07-test-plan.md): 改名の変わった形。
//!
//! フォルダーを自分の中へ / `..` を含む名前 / 既存フォルダーと同じ名前 /
//! フォルダーの大文字小文字だけの改名 / 名前の先頭が同じ兄弟 / 改名直後の検索。
//! 判定: 中身が消える・二重になる・検索が古い場所を返す、原因の分からない文面が出る。

use std::fs;
use std::path::{Path, PathBuf};

use kaku_lib::{project, search};

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "kaku-rename-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    project::init(&p).unwrap();
    p
}

/// フォルダーの中身を、相対パスの一覧で返す(順序は固定)
fn tree(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let rel = e.path().strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            if e.file_type().unwrap().is_dir() {
                out.push(format!("{rel}/"));
                walk(base, &e.path(), out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// フォルダーを自分の中へは移せない。**原因の分かる文面で断り、何も変えない**。
/// 以前は OS の「パラメーターが間違っています」「アクセスが拒否されました」が出て、
/// 行き先の途中のフォルダー(第一章/新)だけが作られて残った
#[test]
fn moving_a_folder_into_itself_is_refused_without_side_effects() {
    let root = tmp("self");
    project::create_file(&root, "manuscript/第一章/01.md", "第一話の本文").unwrap();
    project::create_file(&root, "manuscript/第一章/02.md", "第二話の本文").unwrap();
    let before = tree(&root.join("manuscript"));

    for to in [
        "manuscript/第一章/中",
        "manuscript/第一章/新/中",
        "manuscript/第一章/./新/中",
        // Windows は大文字小文字を区別しないので、これも自分の中
        "MANUSCRIPT/第一章/新",
    ] {
        let err = project::rename(&root, "manuscript/第一章", to).unwrap_err().to_string();
        assert!(err.contains("それ自身の中"), "{to}: 原因の分からない文面: {err}");
        assert_eq!(tree(&root.join("manuscript")), before, "{to}: 何かが変わった");
    }
    // 自分の中ではない、名前の先頭が同じ兄弟へは移せる
    project::rename(&root, "manuscript/第一章", "manuscript/第一章の続き").unwrap();
    assert!(root.join("manuscript/第一章の続き/01.md").is_file());
    fs::remove_dir_all(root).ok();
}

/// `..` は上のフォルダーを表す記号。「末尾が点」の一般の文面ではなく、そう伝える
#[test]
fn dot_dot_is_refused_with_its_own_reason() {
    let root = tmp("dotdot");
    project::create_file(&root, "manuscript/a.md", "本文").unwrap();
    for to in ["manuscript/../a.md", "manuscript/a/.."] {
        let err = project::rename(&root, "manuscript/a.md", to).unwrap_err().to_string();
        assert!(err.contains("上のフォルダー"), "{to}: {err}");
    }
    assert!(root.join("manuscript/a.md").is_file());
    fs::remove_dir_all(root).ok();
}

/// 移せなかったら、行き先のために作ったフォルダーを残さない(他のアプリが開いている場合など)
#[cfg(windows)]
#[test]
fn failed_move_leaves_no_new_folders() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tmp("locked");
    project::create_file(&root, "manuscript/a.md", "本文").unwrap();
    let before = tree(&root.join("manuscript"));
    // 共有なしで開いておくと、ほかからは動かせない(エディタが握っている状態)
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(root.join("manuscript/a.md"))
        .unwrap();
    let r = project::rename(&root, "manuscript/a.md", "manuscript/新章/深い/a.md");
    assert!(r.is_err(), "握られているのに動いた");
    assert_eq!(tree(&root.join("manuscript")), before, "作ったフォルダーが残った");
    drop(held);
    // 離せば動かせる(壊れていない)
    project::rename(&root, "manuscript/a.md", "manuscript/新章/深い/a.md").unwrap();
    assert!(root.join("manuscript/新章/深い/a.md").is_file());
    fs::remove_dir_all(root).ok();
}

/// 既にあるフォルダーと同じ名前には、ファイルもフォルダーも動かさない(上書きしない)
#[test]
fn existing_folder_name_is_not_overwritten() {
    let root = tmp("exists");
    project::create_file(&root, "manuscript/a/x.md", "エー").unwrap();
    project::create_file(&root, "manuscript/b.md", "ビー").unwrap();
    project::create_file(&root, "plot/既存/y.md", "既存").unwrap();
    assert!(project::rename(&root, "manuscript/a", "plot/既存").is_err());
    assert!(project::rename(&root, "manuscript/b.md", "plot/既存").is_err());
    assert_eq!(tree(&root.join("plot/既存")), vec!["y.md".to_string()]);
    assert!(root.join("manuscript/a/x.md").is_file() && root.join("manuscript/b.md").is_file());
    fs::remove_dir_all(root).ok();
}

/// フォルダーの大文字小文字だけの改名と、名前の先頭が同じ兄弟のリンク
#[test]
fn folder_renames_keep_links_right() {
    let root = tmp("links");
    project::create_file(&root, "manuscript/Chapter/01.md", "一話").unwrap();
    project::create_file(&root, "manuscript/a/x.md", "エー").unwrap();
    project::create_file(&root, "manuscript/ab/02.md", "二話").unwrap();
    project::create_file(&root, "manuscript/ab.md", "兄弟").unwrap();
    project::create_file(
        &root,
        "manuscript/目次.md",
        "[一](Chapter/01.md) [二](ab/02.md) [兄](ab.md) [a](a/x.md)\n",
    )
    .unwrap();

    project::rename(&root, "manuscript/Chapter", "manuscript/chapter").unwrap();
    assert!(tree(&root.join("manuscript")).contains(&"chapter/01.md".to_string()));
    project::rename(&root, "manuscript/a", "manuscript/a2").unwrap();
    // 「a」の改名で、先頭が同じ「ab」「ab.md」へのリンクを書き換えない
    assert_eq!(
        project::read_text(&root, "manuscript/目次.md").unwrap(),
        "[一](chapter/01.md) [二](ab/02.md) [兄](ab.md) [a](a2/x.md)\n"
    );
    fs::remove_dir_all(root).ok();
}

/// 改名の直後に検索しても、古い場所を返さない(索引は検索のたびに差分を取り直す)
#[test]
fn search_right_after_a_rename_returns_the_new_place() {
    let root = tmp("search");
    project::create_file(&root, "manuscript/a/x.md", "エーの本文").unwrap();
    let paths = |r: search::SearchResult| r.hits.into_iter().map(|h| h.path).collect::<Vec<_>>();
    assert_eq!(paths(search::search(&root, "エーの本文").unwrap()), vec!["manuscript/a/x.md"]);
    project::rename(&root, "manuscript/a", "plot/動いた").unwrap();
    assert_eq!(paths(search::search(&root, "エーの本文").unwrap()), vec!["plot/動いた/x.md"]);
    fs::remove_dir_all(root).ok();
}
