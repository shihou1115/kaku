//! 抽出の候補を codex へ反映する(`extract::apply`)。テスト計画 P2 で控えた P3 の項目。
//!
//! 以前は名前を検査に通さずに保存先を決めていたため、予約名(`CON`)などで作成が**まとめて**
//! 止まった。やり直しても同じ候補で止まり、後ろの候補はいつまでも反映されなかった。
//! 既存のエントリへ別名を足すときも、読めないファイル(Shift_JIS)1つで全体が止まった。

use std::fs;
use std::path::PathBuf;

use kaku_lib::{extract, frontmatter, project};

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "kaku-codex-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    project::init(&p).unwrap();
    p
}

fn cand(name: &str, kind: &str, aliases: &[&str], existing: Option<&str>) -> extract::Candidate {
    extract::Candidate {
        name: name.to_string(),
        kind: kind.to_string(),
        description: "説明".to_string(),
        count: 1,
        aliases: aliases.iter().map(|a| a.to_string()).collect(),
        existing_path: existing.map(str::to_string),
    }
}

fn title_of(root: &std::path::Path, path: &str) -> Option<String> {
    frontmatter::parse_source(&project::read_text(root, path).unwrap()).title
}

#[test]
fn one_candidate_that_cannot_be_written_does_not_stop_the_rest() {
    let root = tmp("rest");
    // 別名を足す先が Shift_JIS(読めない)。「---\ntitle: 旧\n---\n」を Shift_JIS で書いたもの
    fs::write(
        root.join("codex/characters/旧.md"),
        b"---\ntitle: \x8B\x8C\n---\n\x96\x7B\x95\xB6\n",
    )
    .unwrap();
    // 作ろうとする名前のファイルが、別の題名で既にある
    project::create_file(&root, "codex/characters/重なり.md", "---\ntitle: 別人\n---\n").unwrap();
    // フロントマターが閉じていない(本人が手で直している途中など)
    project::create_file(&root, "codex/characters/壊れ.md", "---\ntitle: 壊れ\n本文\n").unwrap();

    let got = extract::apply(
        &root,
        &[
            cand("黒木", "character", &["教授"], None),
            cand("旧", "character", &["旧名"], Some("codex/characters/旧.md")),
            cand("CON", "character", &[], None),
            cand("重なり", "character", &[], None),
            cand(".NET", "term", &[], None),
            cand("壊れ", "character", &["こわれ"], Some("codex/characters/壊れ.md")),
            cand("龍一", "character", &[], None),
        ],
    );

    // 読めない・既にある・壊れている、の3件は理由つきで返し、それ以外はすべて作る
    assert_eq!(got.failed, ["旧", "重なり", "壊れ"], "{:?}", got.reasons);
    assert!(got.reasons[0].starts_with("旧(") && got.reasons[0].contains("UTF-8"), "{:?}", got.reasons);
    assert!(got.reasons[1].contains("既にある"), "{:?}", got.reasons);
    assert!(got.reasons[2].contains("閉じていない"), "{:?}", got.reasons);
    assert_eq!(
        got.touched,
        [
            "codex/characters/黒木.md",
            "codex/characters/CON_.md",
            "codex/terms/_.NET.md",
            "codex/characters/龍一.md",
        ]
    );
    // 保存先の名前を変えても、名前そのもの(title)は変えない
    assert_eq!(title_of(&root, "codex/characters/CON_.md").as_deref(), Some("CON"));
    assert_eq!(title_of(&root, "codex/terms/_.NET.md").as_deref(), Some(".NET"));
    // 点で始まる名前のまま作ると、設定として読まれない(隠しファイル扱い)。作ったものは読まれる
    let codex = project::load_codex(&root).unwrap();
    for name in ["黒木", "CON", ".NET", "龍一"] {
        assert!(codex.iter().any(|e| e.title == name), "{name} が設定に出ない");
    }
    // 反映できなかったファイルには触れていない
    assert_eq!(
        project::read_text(&root, "codex/characters/重なり.md").unwrap(),
        "---\ntitle: 別人\n---\n"
    );
    assert_eq!(
        project::read_text(&root, "codex/characters/壊れ.md").unwrap(),
        "---\ntitle: 壊れ\n本文\n"
    );
    fs::remove_dir_all(root).ok();
}

/// 別名が登録済みなら何もしない(失敗でも、書き込みでもない)
#[test]
fn aliases_already_there_are_neither_written_nor_failures() {
    let root = tmp("same");
    project::create_file(&root, "codex/characters/黒木.md", "---\ntitle: 黒木\naliases: [教授]\n---\n").unwrap();
    let got = extract::apply(
        &root,
        &[cand("黒木", "character", &["教授"], Some("codex/characters/黒木.md"))],
    );
    assert!(got.touched.is_empty() && got.failed.is_empty(), "{got:?}");
    fs::remove_dir_all(root).ok();
}
