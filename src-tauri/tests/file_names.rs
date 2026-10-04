//! テスト計画 B1(docs/07-test-plan.md): Windows で使えない名前を、作る・改名する・
//! フォルダーを作る、のどの経路でも書く前に断ること。
//!
//! 検証しないと、`第1話: 出会い.md` は「第1話」という空のファイルと代替データストリームに
//! なって本文がツリーから消え(この機械で確認)、末尾の点・空白や予約名(`con.md`)は
//! 多くのツールで開けないファイルになる。

use std::fs;
use std::path::PathBuf;

use kaku_lib::project;

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "kaku-names-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    project::init(&p).unwrap();
    p
}

const BAD: &[&str] = &[
    "第1話: 出会い.md",
    "なぜ?.md",
    "星*印.md",
    "a<b.md",
    "a>b.md",
    "a|b.md",
    "引用\"符.md",
    "制御\u{1}文字.md",
    "末尾の点.md.",
    "末尾の空白.md ",
    "con.md",
    "NUL",
    "aux.txt",
    "Com1.md",
    "lpt9",
];

fn names_in(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn names_windows_cannot_hold_are_refused_on_every_path() {
    let root = tmp("bad");
    project::create_file(&root, "manuscript/元.md", "本文").unwrap();
    for bad in BAD {
        let rel = format!("manuscript/{bad}");
        let made = project::create_file(&root, &rel, "x");
        assert!(made.is_err(), "作れてしまった: {bad:?} -> {made:?}");
        assert!(project::create_dir(&root, &rel).is_err(), "フォルダーを作れてしまった: {bad:?}");
        assert!(
            project::rename(&root, "manuscript/元.md", &rel).is_err(),
            "改名できてしまった: {bad:?}"
        );
    }
    // 断った名前のどれも、隠れた形でも書かれていない
    assert_eq!(names_in(&root.join("manuscript")), vec!["元.md".to_string()]);
    fs::remove_dir_all(root).ok();
}

#[test]
fn ordinary_names_still_work() {
    let root = tmp("ok");
    for ok in [
        "第1話 出会い.md",
        "第1話\u{FF1A}出会い.md", // 全角のコロン(:)は使える
        "なぜ\u{FF1F}.md",        // 全角の疑問符(?)も使える
        "conquest.md",     // 予約名で始まるだけの語
        "コン.md",
        "a.b.c.md",
        "第一章",
    ] {
        let rel = format!("manuscript/{ok}");
        if ok.ends_with(".md") {
            assert!(project::create_file(&root, &rel, "x").unwrap(), "{ok}");
        } else {
            assert!(project::create_dir(&root, &rel).unwrap(), "{ok}");
        }
    }
    fs::remove_dir_all(root).ok();
}

#[test]
fn the_reason_is_written_in_words_the_writer_can_act_on() {
    let root = tmp("msg");
    let e = project::create_file(&root, "manuscript/第1話: 出会い.md", "x")
        .unwrap_err()
        .to_string();
    assert!(e.contains("使えない文字"), "{e}");
    assert!(e.contains("全角"), "代わりに使える文字を示していない: {e}");
    let e = project::create_file(&root, "manuscript/con.md", "x")
        .unwrap_err()
        .to_string();
    assert!(e.contains("予約"), "{e}");
    fs::remove_dir_all(root).ok();
}
