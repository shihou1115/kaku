//! 通し確認: 全文検索の索引が**実ファイルに対して**正しく育つこと。
//!
//! 単体テストでは切り出した純粋関数しか見られないが、ここで怖いのは
//! **差分更新**(03-data-format §5-4)である。書き換えたのに古い内容が引ける、
//! 消したファイルが検索に残る、といった壊れ方は索引を持つ設計に固有のもので、
//! 「ファイルが正」(D-1)の建前を裏切る。
//!
//! 索引は使い捨てなので、壊れていたら作り直せることも併せて確かめる。

use std::fs;
use std::path::PathBuf;

use kaku_lib::{project, sample, search};

fn tmp_dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "kaku-search-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn paths(result: &search::SearchResult) -> Vec<&str> {
    result.hits.iter().map(|h| h.path.as_str()).collect()
}

#[test]
fn indexes_sample_and_finds_japanese_text() {
    let root = tmp_dir("basic");
    sample::create(&root).unwrap();

    // 3文字以上: trigram索引で解決する
    let got = search::search(&root, "昇降口").unwrap();
    assert_eq!(got.method, "fts");
    assert!(got.reindexed > 0, "初回は全件を索引する");
    assert_eq!(paths(&got), vec!["manuscript/01-転校初日.md"]);
    assert!(got.hits[0].snippet.contains("昇降口"));
    assert_eq!(got.hits[0].title, "転校初日", "表示名はフロントマターのtitle");

    // 2文字: trigramでは当たらないのでLIKEへ回る(PoC#2)
    let got = search::search(&root, "架純").unwrap();
    assert_eq!(got.method, "like");
    assert!(
        got.hits.len() >= 3,
        "本文と設定の両方から引けること: {:?}",
        paths(&got)
    );

    // 出現の多い順に並ぶ
    let counts: Vec<usize> = got.hits.iter().map(|h| h.count).collect();
    let mut sorted = counts.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(counts, sorted, "出現回数の多い順になっていない");

    // 当たってはいけない語
    assert!(search::search(&root, "存在しない語句").unwrap().hits.is_empty());
}

#[test]
fn second_search_reuses_the_index() {
    let root = tmp_dir("incremental");
    sample::create(&root).unwrap();

    search::search(&root, "昇降口").unwrap();
    let again = search::search(&root, "昇降口").unwrap();
    assert_eq!(
        again.reindexed, 0,
        "変更が無いのに索引し直している(mtime+サイズの比較が効いていない)"
    );
    assert_eq!(paths(&again), vec!["manuscript/01-転校初日.md"]);
}

#[test]
fn edits_are_reflected_and_old_text_disappears() {
    let root = tmp_dir("edit");
    sample::create(&root).unwrap();
    let scene = "manuscript/01-転校初日.md";

    assert!(!search::search(&root, "昇降口").unwrap().hits.is_empty());

    // 書き換える。**古い語は引けなくなり、新しい語が引ける**こと
    project::write_text(
        &root,
        scene,
        "---\ntitle: 転校初日\n---\n\n　図書室の窓は曇っていた。給水塔が見えた。\n",
    )
    .unwrap();

    let after = search::search(&root, "昇降口").unwrap();
    assert!(
        after.hits.iter().all(|h| h.path != scene),
        "書き換えたのに古い本文が引ける: {:?}",
        paths(&after)
    );
    let fresh = search::search(&root, "給水塔").unwrap();
    assert!(
        fresh.hits.iter().any(|h| h.path == scene),
        "書き換えた本文が引けない: {:?}",
        paths(&fresh)
    );
}

#[test]
fn deleted_files_leave_the_index() {
    let root = tmp_dir("delete");
    sample::create(&root).unwrap();
    let scene = "manuscript/02-図書室.md";
    assert!(search::search(&root, "図書室")
        .unwrap()
        .hits
        .iter()
        .any(|h| h.path == scene));

    // アプリのゴミ箱へ移す(= 元の場所からは消える)
    project::trash(&root, scene).unwrap();

    let after = search::search(&root, "図書室").unwrap();
    assert!(
        after.hits.iter().all(|h| h.path != scene),
        "消したファイルが検索に残っている: {:?}",
        paths(&after)
    );
    // ゴミ箱(.app/配下)を拾ってしまっていないこと
    assert!(
        after.hits.iter().all(|h| !h.path.starts_with(".app")),
        "アプリ専用領域を索引している: {:?}",
        paths(&after)
    );
}

#[test]
fn broken_index_is_rebuilt_instead_of_failing() {
    let root = tmp_dir("broken");
    sample::create(&root).unwrap();
    search::search(&root, "昇降口").unwrap();

    // 索引を壊す。**検索が止まってはいけない**(索引は使い捨て=D-1)
    let index = root.join(".app").join("index.sqlite");
    fs::write(&index, b"this is not a database").unwrap();

    let got = search::search(&root, "昇降口").expect("壊れた索引で検索が落ちた");
    assert_eq!(paths(&got), vec!["manuscript/01-転校初日.md"]);
}
