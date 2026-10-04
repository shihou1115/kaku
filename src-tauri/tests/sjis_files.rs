//! テスト計画 B8(docs/07-test-plan.md): Shift_JIS のファイルが混ざったプロジェクト。
//!
//! V1 は UTF-8 だけを読む(03 §4.4。判別を誤ると本文が静かに壊れるため)。
//! 判定: 開く・一覧・設定の読み込み・検索の索引・改名のリンク追随のどれかが止まる、
//! または Shift_JIS のファイルを書き換える。

use std::fs;
use std::path::PathBuf;

use kaku_lib::{project, search};

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "kaku-sjis-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    project::init(&p).unwrap();
    p
}

/// 「古い原稿です」(Shift_JIS。JS の TextDecoder("shift_jis") で確かめたバイト列)
const SJIS_BODY: &[u8] = &[0x8C, 0xC3, 0x82, 0xA2, 0x8C, 0xB4, 0x8D, 0x65, 0x82, 0xC5, 0x82, 0xB7];
/// 「参照[a](a.md)\n」(Shift_JIS)。UTF-8 のファイルへのリンクを持つ
const SJIS_LINK: &[u8] = &[
    0x8E, 0x51, 0x8F, 0xC6, 0x5B, 0x61, 0x5D, 0x28, 0x61, 0x2E, 0x6D, 0x64, 0x29, 0x0A,
];
/// 「---\ntitle: 旧人物\n---\n」(Shift_JIS の設定ファイル)
const SJIS_CODEX: &[u8] = &[
    0x2D, 0x2D, 0x2D, 0x0A, 0x74, 0x69, 0x74, 0x6C, 0x65, 0x3A, 0x20, 0x8B, 0x8C, 0x90, 0x6C,
    0x95, 0xA8, 0x0A, 0x2D, 0x2D, 0x2D, 0x0A,
];

#[test]
fn shift_jis_files_never_stop_anything_and_are_never_rewritten() {
    let root = tmp("mix");
    fs::write(root.join("manuscript/古い原稿.md"), SJIS_BODY).unwrap();
    fs::write(root.join("manuscript/古い目次.md"), SJIS_LINK).unwrap();
    fs::write(root.join("codex/characters/旧人物.md"), SJIS_CODEX).unwrap();
    project::create_file(&root, "manuscript/a.md", "新しい原稿の本文").unwrap();
    project::create_file(&root, "manuscript/目次.md", "[古](古い原稿.md)\n").unwrap();
    project::create_file(&root, "codex/characters/架純.md", "---\ntitle: 架純\n---\n").unwrap();

    // 一覧・設定の読み込み・検索は止まらない(読めないものは飛ばす)
    let mut names = Vec::new();
    fn walk(nodes: &[project::TreeNode], out: &mut Vec<String>) {
        for n in nodes {
            out.push(n.name.clone());
            walk(&n.children, out);
        }
    }
    walk(&project::scan(&root).unwrap(), &mut names);
    assert!(names.contains(&"古い原稿.md".to_string()), "一覧に出ない: {names:?}");
    let codex = project::load_codex(&root).unwrap();
    assert_eq!(codex.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["架純"]);
    let hits = search::search(&root, "新しい原稿").unwrap().hits;
    assert_eq!(hits.len(), 1, "UTF-8 のファイルが索引に入らない");

    // 開こうとすると、理由と直し方の分かる文面で断る
    let err = project::read_text(&root, "manuscript/古い原稿.md").unwrap_err().to_string();
    assert!(err.contains("UTF-8"), "理由の分からない文面: {err}");

    // 改名のリンク追随: Shift_JIS のファイルは書き換えない。UTF-8 のファイルのリンクは追随する
    project::rename(&root, "manuscript/a.md", "manuscript/b.md").unwrap();
    assert_eq!(fs::read(root.join("manuscript/古い目次.md")).unwrap(), SJIS_LINK);
    project::rename(&root, "manuscript/古い原稿.md", "manuscript/昔の原稿.md").unwrap();
    assert_eq!(fs::read(root.join("manuscript/昔の原稿.md")).unwrap(), SJIS_BODY);
    assert_eq!(
        project::read_text(&root, "manuscript/目次.md").unwrap(),
        "[古](昔の原稿.md)\n"
    );
    // 複製・ゴミ箱も中身をそのまま運ぶ
    let copy = project::duplicate(&root, "manuscript/昔の原稿.md").unwrap();
    assert_eq!(fs::read(root.join(&copy)).unwrap(), SJIS_BODY);
    let trashed = project::trash(&root, &copy).unwrap();
    assert_eq!(fs::read(trashed).unwrap(), SJIS_BODY);
    assert_eq!(fs::read(root.join("codex/characters/旧人物.md")).unwrap(), SJIS_CODEX);
    fs::remove_dir_all(root).ok();
}
