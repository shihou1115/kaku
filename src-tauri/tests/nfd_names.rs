//! テスト計画 B7(docs/07-test-plan.md): 濁点が分離した名前(NFD。Mac で作ったファイル)。
//!
//! 表記ゆれの側は proofread.rs のテスト(語を割らない・登録名と同じに扱う)。
//! ここではファイル名が NFD のときの、作る・ツリー・改名・リンク追随を確かめる。

use std::fs;
use std::path::PathBuf;

use kaku_lib::project;

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "kaku-nfd-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    project::init(&p).unwrap();
    p
}

/// 「ガイドブック」の NFD(カ + 濁点 + イ + ト + 濁点 + ブ…)
const NFD_GUIDE: &str = "カ\u{3099}イト\u{3099}ブック";

#[test]
fn nfd_file_names_show_up_and_follow_renames() {
    let root = tmp("files");
    let nfd = format!("manuscript/{NFD_GUIDE}.md");
    project::create_file(&root, &nfd, "Mac で作った名前のファイル").unwrap();
    project::create_file(&root, "manuscript/目次.md", &format!("[案内]({NFD_GUIDE}.md)\n")).unwrap();

    // ({:?} で文字列にすると濁点が \u{3099} と書かれるので、名前を直接集める)
    fn names(nodes: &[project::TreeNode], out: &mut Vec<String>) {
        for n in nodes {
            out.push(n.name.clone());
            names(&n.children, out);
        }
    }
    let mut all = Vec::new();
    names(&project::scan(&root).unwrap(), &mut all);
    assert!(all.iter().any(|n| n.contains(NFD_GUIDE)), "ツリーに出ない: {all:?}");
    assert_eq!(project::read_text(&root, &nfd).unwrap(), "Mac で作った名前のファイル");

    // NFC の名前へ改名すると、NFD で書かれたリンクも追随する
    let failed = project::rename(&root, &nfd, "manuscript/ガイドブック.md").unwrap();
    assert!(failed.is_empty());
    assert_eq!(
        project::read_text(&root, "manuscript/目次.md").unwrap(),
        "[案内](ガイドブック.md)\n"
    );
    fs::remove_dir_all(root).ok();
}
