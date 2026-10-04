//! 通し確認: シーンの自動分割が**実ファイルに対して**安全であること。
//!
//! ここで守りたいのは3つ。どれも壊れると原稿が失われるか、書き手の信頼を失う。
//!
//!  1. **本文が1文字も変わらない**。分割後を連結したら元に戻ること
//!  2. **元のファイルが消えない**。ゴミ箱へ退避され、戻せること
//!  3. **未知のフロントマターが壊れない**(03-data-format §4.1)
//!
//! 加えて、提案を見てから本文が変わった場合に**ずれた位置で切らない**ことも見る。

use std::fs;
use std::path::PathBuf;

use kaku_lib::{frontmatter, project, split};

fn tmp_dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "kaku-split-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

const SCENE: &str = "manuscript/01-転校初日.md";

/// 未知フィールド(pov / progression)を含むフロントマター付きの原稿
fn source() -> String {
    let a = "　転校初日の朝は、雨だった。".repeat(20);
    let b = "　その日の放課後、図書室は静かだった。".repeat(20);
    format!(
        "---\ntype: scene\ntitle: 転校初日\npov: 架純\nprogression:\n  - at: 一章\n    note: 転入\n---\n\n{a}\n{b}\n"
    )
}

fn setup(tag: &str) -> PathBuf {
    let root = tmp_dir(tag);
    project::init(&root).unwrap();
    project::create_file(&root, SCENE, &source()).unwrap();
    root
}

fn accepted() -> Vec<split::AcceptedPoint> {
    vec![split::AcceptedPoint {
        quote: "　その日の放課後、図書室は静かだった。".to_string(),
        title: "図書室".to_string(),
    }]
}

#[test]
fn split_preserves_every_character_of_the_body() {
    let root = setup("lossless");
    let before = project::read_text(&root, SCENE).unwrap();
    let (_, body_before) = frontmatter::split(&before);
    let body_before = body_before.to_string();

    let created = split::apply(&root, SCENE, "転校初日", &accepted()).unwrap();
    // 作られた2ファイル + 退避先
    assert_eq!(created.len(), 3, "{created:?}");

    let joined: String = created[..2]
        .iter()
        .map(|p| {
            let s = project::read_text(&root, p).unwrap();
            frontmatter::split(&s).1.to_string()
        })
        .collect();
    assert_eq!(joined, body_before, "分割で本文が変わった");
}

#[test]
fn split_keeps_unknown_frontmatter_fields() {
    let root = setup("frontmatter");
    let created = split::apply(&root, SCENE, "転校初日", &accepted()).unwrap();

    for (i, path) in created[..2].iter().enumerate() {
        let s = project::read_text(&root, path).unwrap();
        assert!(s.contains("pov: 架純"), "{path}: povが消えた");
        assert!(s.contains("  - at: 一章"), "{path}: 入れ子が消えた");
        assert!(s.contains("note: 転入"), "{path}: 入れ子の値が消えた");
        assert!(s.contains("type: scene"), "{path}: typeが消えた");
        let fm = frontmatter::parse_source(&s);
        assert_eq!(
            fm.title.as_deref(),
            Some(if i == 0 { "転校初日" } else { "図書室" }),
            "{path}: titleが差し替わっていない"
        );
    }
}

#[test]
fn original_goes_to_trash_and_can_be_recovered() {
    let root = setup("trash");
    let before = project::read_text(&root, SCENE).unwrap();

    let created = split::apply(&root, SCENE, "転校初日", &accepted()).unwrap();
    let trashed = created.last().unwrap();

    // 元の場所からは消えている
    assert!(!root.join(SCENE).exists(), "元ファイルが残っている");
    // ゴミ箱に**中身そのまま**で残っている。
    // trash が返すのは案内用の実パス(既存の削除操作と同じ)
    let norm = trashed.replace('\\', "/");
    assert!(norm.contains("/.app/trash/"), "退避先: {trashed}");
    let saved = fs::read_to_string(trashed).unwrap();
    assert_eq!(saved, before, "退避したファイルの中身が変わっている");
}

#[test]
fn file_names_are_numbered_from_the_original() {
    let root = setup("names");
    let created = split::apply(&root, SCENE, "転校初日", &accepted()).unwrap();
    assert_eq!(created[0], "manuscript/01-転校初日-1.md");
    assert_eq!(created[1], "manuscript/01-転校初日-2.md");
}

#[test]
fn refuses_when_the_text_changed_after_the_suggestion() {
    let root = setup("moved");
    // 提案を見たあとに本文を書き換えた状況
    project::write_text(
        &root,
        SCENE,
        "---\ntitle: 転校初日\n---\n\n　すっかり書き直した本文。\n",
    )
    .unwrap();

    let err = split::apply(&root, SCENE, "転校初日", &accepted()).unwrap_err();
    assert!(err.contains("見つかりません"), "{err}");
    // **失敗したときは何も作らない・何も消さない**
    assert!(root.join(SCENE).exists(), "失敗したのに元が消えた");
    assert!(
        !root.join("manuscript/01-転校初日-1.md").exists(),
        "失敗したのにファイルを作った"
    );
}

#[test]
fn refuses_when_a_target_name_is_taken() {
    let root = setup("collision");
    // 分割先と同じ名前が既にある場合。**半分だけ分割された状態を作らない**
    project::create_file(&root, "manuscript/01-転校初日-2.md", "先客").unwrap();

    let err = split::apply(&root, SCENE, "転校初日", &accepted()).unwrap_err();
    assert!(err.contains("既にあります"), "{err}");
    assert!(root.join(SCENE).exists(), "失敗したのに元が消えた");
    assert!(
        !root.join("manuscript/01-転校初日-1.md").exists(),
        "1つ目だけ作られてしまった"
    );
    assert_eq!(
        project::read_text(&root, "manuscript/01-転校初日-2.md").unwrap(),
        "先客",
        "先客を上書きした"
    );
}

/// テスト計画 F5: ゴミ箱の中のファイルは読み取り専用。分割でも書き込めない
/// (分けたファイルも作らず、退避したものをさらに動かさない)
#[test]
fn a_file_in_the_trash_cannot_be_split() {
    let root = setup("trash-ro");
    project::trash(&root, SCENE).unwrap();
    let trash_dir = root.join(".app/trash");
    let inside: Vec<_> = fs::read_dir(&trash_dir).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(inside.len(), 1);
    let rel = format!(
        ".app/trash/{}/{SCENE}",
        inside[0].file_name().unwrap().to_string_lossy()
    );
    let before = project::read_text(&root, &rel).unwrap();
    let files_before = walk_count(&trash_dir);
    assert!(split::apply(&root, &rel, "転校初日", &accepted()).is_err(), "ゴミ箱の中を分割できた");
    assert_eq!(walk_count(&trash_dir), files_before, "ゴミ箱の中にファイルが増えた");
    assert_eq!(project::read_text(&root, &rel).unwrap(), before, "ゴミ箱の中身が変わった");
    // 保存の経路でも書けない
    assert!(project::save_text(&root, &rel, "書き換え").is_err());
}

fn walk_count(dir: &std::path::Path) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                walk_count(&e.path())
            } else {
                1
            }
        })
        .sum()
}
