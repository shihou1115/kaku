//! 通し確認: プロジェクト作成 → 執筆 → codex追加 → 言及検出 → AIへ渡す文脈の組み立て。
//!
//! GUI を介さずに MVP の中核経路が繋がっていることを確かめる。

use std::fs;
use std::path::PathBuf;

use kaku_lib::{context, mentions, project};

fn tmp_dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "kaku-flow-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn writes_scene_detects_mentions_and_builds_context() {
    let root = tmp_dir("main");

    // 1. 空フォルダをプロジェクトにする
    project::init(&root).unwrap();

    // 2. 設定を2件作る(片方は title 省略・別名あり)
    project::create_file(
        &root,
        "codex/characters/佐藤架純.md",
        "---\naliases: [架純, かすみん]\ndescription: 主人公。\nid: chr-kasumi\n---\n\n## 外見\n黒髪ロング。\n",
    )
    .unwrap();
    project::create_file(
        &root,
        "codex/locations/県立青葉高校.md",
        "---\ntitle: 県立青葉高校\n---\n\n主人公が転入する高校。\n",
    )
    .unwrap();

    // 3. 本文を書いて保存する
    let scene = "manuscript/01-出会い.md";
    let body = "---\ntitle: 出会い\n---\n\n　佐藤架純は県立青葉高校の昇降口に立っていた。かすみんと呼ぶ者はもういない。\n";
    project::write_text(&root, scene, body).unwrap();

    // 4. codex を読み直すと2件見える。title 省略分はファイル名が使われる
    let codex = project::load_codex(&root).unwrap();
    assert_eq!(codex.len(), 2);
    let kasumi = codex.iter().find(|c| c.title == "佐藤架純").unwrap();
    assert_eq!(kasumi.aliases, vec!["架純", "かすみん"]);
    // type 省略分は種別フォルダから決まる(フロントマターに書くときと同じ単数の名前)
    assert_eq!(kasumi.type_.as_deref(), Some("character"));

    // 5. 本文から言及を検出する(正式名・別名・地名)
    let patterns: Vec<String> = codex.iter().flat_map(|c| c.patterns()).collect();
    let text = project::read_text(&root, scene).unwrap();
    let hits = mentions::find_mentions(&text, &patterns);
    let names: Vec<&str> = hits.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"佐藤架純"), "正式名が拾えていない: {names:?}");
    assert!(names.contains(&"県立青葉高校"));
    assert!(names.contains(&"かすみん"));
    // 「佐藤架純」を「架純」に割ってしまっていないこと
    assert!(!names.contains(&"架純"), "最長一致が効いていない: {names:?}");

    // 6. AIへ渡す文脈を組み立てる
    let mentioned: Vec<String> = codex
        .iter()
        .filter(|c| c.patterns().iter().any(|p| names.contains(&p.as_str())))
        .map(|c| c.path.clone())
        .collect();
    assert_eq!(mentioned.len(), 2);

    let reader = |p: &str| project::read_text(&root, p).ok();
    let ctx = context::build(&text, &codex, &reader, &mentioned, &[]);
    assert_eq!(ctx.entries.len(), 2);
    assert!(!ctx.body_truncated);

    let msg = context::render_user_message(&ctx, "この場面の問題点は?");
    assert!(msg.contains("# 設定資料"));
    assert!(msg.contains("## 佐藤架純"));
    assert!(msg.contains("黒髪ロング"));
    assert!(msg.contains("# 対象本文"));
    assert!(msg.contains("# 依頼"));

    fs::remove_dir_all(root).ok();
}

#[test]
fn second_save_keeps_previous_content_as_backup() {
    let root = tmp_dir("backup");
    project::init(&root).unwrap();
    let path = "manuscript/01-出会い.md";

    project::write_text(&root, path, "初稿").unwrap();
    project::write_text(&root, path, "推敲後").unwrap();

    assert_eq!(
        fs::read_to_string(root.join(".app/backups").join(path)).unwrap(),
        "初稿",
        "保存前の内容が控えられていない"
    );
    assert_eq!(
        fs::read_to_string(root.join(path)).unwrap(),
        "推敲後"
    );

    fs::remove_dir_all(root).ok();
}

#[test]
fn unknown_frontmatter_fields_survive_round_trip() {
    // V1が解釈しないフィールド(id/relations/progression)を壊さないこと
    let root = tmp_dir("roundtrip");
    project::init(&root).unwrap();
    let path = "codex/characters/五十嵐悠二.md";
    let original = "---\nid: chr-yuji\ntitle: 五十嵐悠二\naliases: [悠二]\nrelations:\n  - to: chr-kasumi\n    rel: 幼馴染\nprogression:\n  - at: 第三章\n    note: 隻腕になる\n---\n\n本文\n";
    project::create_file(&root, path, original).unwrap();

    let read = project::read_text(&root, path).unwrap();
    // アプリはフロントマターを再生成しない。読んで書き戻しても同一
    project::write_text(&root, path, &read).unwrap();
    assert_eq!(fs::read_to_string(root.join(path)).unwrap(), original);

    let codex = project::load_codex(&root).unwrap();
    let e = codex.iter().find(|c| c.title == "五十嵐悠二").unwrap();
    assert_eq!(e.aliases, vec!["悠二"]);

    fs::remove_dir_all(root).ok();
}
