//! テスト計画 E8: 本人が文例・テンプレートの Markdown を壊した場合(空・見出しなし・BOM・
//! 巨大・Shift_JIS)。
//!
//! 判定: 画面が開かない、ほかの文例まで消える。加えて、**読めないものを黙って消さない**
//! (以前は Shift_JIS で保存し直した文例ファイルを黙って飛ばし、そのカテゴリの文例が
//! 理由も分からずに消えて見えた)。

use std::fs;
use std::path::PathBuf;

use kaku_lib::{prompts, templates};

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "kaku-broken-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

/// 「## 見出し\n本文\n」を Shift_JIS で書いたもの(UTF-8 としては読めない)
const SJIS: &[u8] = b"## \x8C\xA9\x8F\x6F\x82\xB5\n\x96\x7B\x95\xB6\n";

#[test]
fn a_broken_prompt_file_neither_hides_the_others_nor_disappears_silently() {
    let dir = tmp("prompts");
    fs::write(dir.join("01-普通.md"), "# 普通\n\n## 展開案\n案を3つ。\n\n## What if\nもし。\n").unwrap();
    fs::write(dir.join("02-空.md"), "").unwrap();
    fs::write(dir.join("03-見出しなし.md"), "見出しの無い説明だけ。\n").unwrap();
    // メモ帳などで保存すると先頭に BOM が付く。1行目の見出しを落とさない
    fs::write(dir.join("04-BOM.md"), "\u{feff}## 先頭\n本文。\n").unwrap();
    fs::write(dir.join("05-sjis.md"), SJIS).unwrap();
    let huge: String = (0..2000).map(|i| format!("## 文例{i}\n{}\n", "長い本文。".repeat(500))).collect();
    fs::write(dir.join("06-巨大.md"), &huge).unwrap();

    let got = prompts::list_in(&dir).unwrap();
    let titles = |cat: &str| -> Vec<String> {
        got.templates.iter().filter(|t| t.category == cat).map(|t| t.title.clone()).collect()
    };
    assert_eq!(titles("普通"), ["展開案", "What if"]);
    assert_eq!(titles("BOM"), ["先頭"], "BOM の後ろの見出しを読んでいない");
    assert_eq!(titles("巨大").len(), 2000);
    assert!(titles("空").is_empty() && titles("見出しなし").is_empty());
    // 読めなかったものは、名前と直し方を返す(画面に出す)
    assert_eq!(got.unreadable.len(), 1, "{:?}", got.unreadable);
    assert!(got.unreadable[0].starts_with("05-sjis.md"), "{:?}", got.unreadable);
    assert!(got.unreadable[0].contains("UTF-8"), "{:?}", got.unreadable);
    fs::remove_dir_all(dir).ok();
}

/// 全部読めなくても、読めないことは返す(画面はこれを見て、文例の置き場へ行く道を出す)
#[test]
fn every_prompt_file_unreadable_is_still_reported() {
    let dir = tmp("all-broken");
    fs::write(dir.join("01-a.md"), SJIS).unwrap();
    fs::write(dir.join("02-b.md"), SJIS).unwrap();
    // 文字コード以外の理由で読めないもの(.md という名前のフォルダー)も黙らない
    fs::create_dir_all(dir.join("03-c.md")).unwrap();
    let got = prompts::list_in(&dir).unwrap();
    assert!(got.templates.is_empty());
    assert_eq!(got.unreadable.len(), 3, "{:?}", got.unreadable);
    assert!(got.unreadable[2].starts_with("03-c.md("), "{:?}", got.unreadable);
    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_broken_template_says_how_to_fix_it() {
    let base = tmp("templates");
    fs::create_dir_all(base.join("検証")).unwrap();
    fs::write(base.join("検証/character.md"), SJIS).unwrap();
    fs::write(base.join("検証/empty.md"), "").unwrap();
    fs::write(base.join("検証/nofm.md"), "本文だけ\n").unwrap();

    let err = templates::render_in(&base, "検証", "character", "架純").unwrap_err();
    assert!(err.contains("検証/character.md") && err.contains("UTF-8 で保存し直す"), "{err}");
    assert!(!err.contains("stream did not contain"), "OS の英語の文面のまま: {err}");
    // 空・フロントマターの無いテンプレートは、そのまま使う(題名は足さない)
    assert_eq!(templates::render_in(&base, "検証", "empty", "架純").unwrap(), "");
    assert_eq!(templates::render_in(&base, "検証", "nofm", "架純").unwrap(), "本文だけ\n");
    // 無いテンプレート(一覧を出した後に消された)も、どれのことか分かる文面で返す
    let missing = templates::render_in(&base, "検証", "無い", "架純").unwrap_err();
    assert!(missing.contains("検証/無い.md"), "{missing}");
    fs::remove_dir_all(base).ok();
}
