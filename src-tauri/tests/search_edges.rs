//! テスト計画 D1・D2・D3(docs/07-test-plan.md): 検索の変わった語と、改名・削除・分割のあと。
//!
//! 判定: 関係のないファイルに当たる / エラーになる / 当たったのに0回と出る(誤報告)/
//! 消えたファイルが結果に残る。

use std::fs;
use std::path::PathBuf;

use kaku_lib::{project, search, split};

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "kaku-search-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    project::init(&p).unwrap();
    p
}

/// (パス, 出現回数)の一覧。パスは manuscript/ を落とす
fn found(root: &std::path::Path, q: &str) -> Vec<(String, usize)> {
    let r = search::search(root, q).unwrap_or_else(|e| panic!("{q:?} でエラー: {e}"));
    r.hits
        .into_iter()
        .map(|h| (h.path.trim_start_matches("manuscript/").to_string(), h.count))
        .collect()
}

fn project_with_tricky_text(tag: &str) -> PathBuf {
    let root = tmp(tag);
    project::create_file(&root, "manuscript/a.md", "普通の本文。").unwrap();
    project::create_file(&root, "manuscript/b.md", "変数 user_name を使う。100%の力。").unwrap();
    project::create_file(&root, "manuscript/c.md", "Hello World と ＡＢＣ の話。").unwrap();
    project::create_file(
        &root,
        "manuscript/d.md",
        "記号 \"引用\" と a*b と x-y と NEAR と AND と (括弧) と a:b と ^上",
    )
    .unwrap();
    root
}

/// テスト計画 D1: 3文字未満の語の `%` `_` は文字そのものとして探す。
/// 以前は LIKE の「何でもよい」の印になり、「_」「%」で全ファイルが当たった
#[test]
fn percent_and_underscore_are_searched_as_they_are() {
    let root = project_with_tricky_text("like");
    assert_eq!(found(&root, "_"), vec![("b.md".to_string(), 1)]);
    assert_eq!(found(&root, "%"), vec![("b.md".to_string(), 1)]);
    assert_eq!(found(&root, "%の"), vec![("b.md".to_string(), 1)]);
    assert!(found(&root, "e_").is_empty(), "「e」の後ろに何か、で当たった");
    fs::remove_dir_all(root).ok();
}

/// テスト計画 D3: 大文字小文字(全角の英字も)は区別しない。索引がそうなので、
/// 件数・抜き出しもそろえる。以前は当たっても0回と出て、抜き出しは関係のない先頭だった
#[test]
fn case_differences_are_found_and_counted() {
    let root = project_with_tricky_text("case");
    for q in ["hello", "HELLO", "Hello", "ａｂｃ", "ＡＢＣ"] {
        assert_eq!(found(&root, q), vec![("c.md".to_string(), 1)], "{q:?}");
    }
    let hit = &search::search(&root, "hello").unwrap().hits[0];
    assert!(hit.snippet.contains("Hello"), "抜き出しに一致箇所が無い: {}", hit.snippet);
    // 3文字未満(走査)でも同じ扱い
    assert_eq!(found(&root, "he"), vec![("c.md".to_string(), 1)]);
    fs::remove_dir_all(root).ok();
}

/// テスト計画 D3: 検索の記号(" * - NEAR AND 括弧 : ^)を含む語も、そのままの文字列として探す
#[test]
fn search_syntax_characters_are_plain_text() {
    let root = project_with_tricky_text("syntax");
    for q in ["\"引用\"", "a*b", "x-y", "NEAR", "AND", "(括弧)", "a:b", "^上", "\"", "*"] {
        let got = found(&root, q);
        assert_eq!(got.len(), 1, "{q:?}: {got:?}");
        assert_eq!(got[0].0, "d.md", "{q:?}");
    }
    fs::remove_dir_all(root).ok();
}

/// どんな語でも、当たったファイルの出現回数は1以上(当たったのに0回と出さない)
#[test]
fn every_hit_contains_the_query() {
    let root = project_with_tricky_text("count");
    for q in ["_", "%", "e", "の", "本文", "hello", "ａｂｃ", "-", "、", "x"] {
        for (path, count) in found(&root, q) {
            assert!(count >= 1, "{q:?} で {path} が0回のまま当たった");
        }
    }
    fs::remove_dir_all(root).ok();
}

/// テスト計画 D2: ゴミ箱・外部での削除・分割のあとで検索しても、消えたファイルを返さない
#[test]
fn removed_files_never_come_back_in_results() {
    let root = tmp("gone");
    project::create_file(&root, "manuscript/捨てる.md", "合言葉は青い鳥。").unwrap();
    project::create_file(&root, "manuscript/外で消す.md", "合言葉は青い鳥。").unwrap();
    let scene = format!(
        "{}\n{}\n",
        "　合言葉は青い鳥。前半の場面が続く。".repeat(10),
        "　後半の場面が始まる。".repeat(10)
    );
    project::create_file(&root, "manuscript/分ける.md", &scene).unwrap();
    assert_eq!(found(&root, "青い鳥").len(), 3);

    project::trash(&root, "manuscript/捨てる.md").unwrap();
    fs::remove_file(root.join("manuscript/外で消す.md")).unwrap();
    let accepted = vec![split::AcceptedPoint {
        quote: "　後半の場面が始まる。".to_string(),
        title: "後半".to_string(),
    }];
    let created = split::apply(&root, "manuscript/分ける.md", "前半", &accepted).unwrap();

    let got = found(&root, "青い鳥");
    // 分割で作った前半のファイルだけが残る(ゴミ箱の中・消したもの・分割前の元は出ない)
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(format!("manuscript/{}", got[0].0), created[0]);
    fs::remove_dir_all(root).ok();
}
