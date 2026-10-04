//! テスト計画 A6(docs/07-test-plan.md): ルビ記法の崩れた形(`｜` の無い《》・閉じ忘れ・
//! 入れ子・改行をまたぐ など)を、Rust 版(`ruby.rs`)と TS 版(`src/ruby.ts`)が同じに読むか。
//!
//! 同じ規則を2か所で実装している(Rust は表記ゆれの前処理、TS は字数・プレビュー・
//! ルビを振れるかの判断)。**共通の入力表**(`tests/data/ruby_parity.json`)を両方の
//! テストから読む(TS 側は `src/rubyParity.test.ts`)。期待値は Rust 版の結果。
//!
//! あわせて、表記ゆれの前処理(`mask`)が**本文を消さず、位置を変えない**ことを確かめる
//! (見つけた位置を元の本文へそのまま当てるため)。
//!
//! 規則を変えたときは表を作り直す:
//!
//! ```text
//! cargo test --test ruby_parity -- --ignored
//! ```

mod common;

use common::{ascii_json, utf16_at, Rng};
use kaku_lib::ruby;
use serde::{Deserialize, Serialize};

const TABLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/ruby_parity.json");
const SEED: u64 = 20261004;
const RANDOM_CASES: usize = 400;

/// TS の `marks` と同じ形。位置は UTF-16(エディタの単位)
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Mark {
    start: usize,
    end: usize,
    base: String,
    reading: String,
}

#[derive(Serialize, Deserialize)]
struct Case {
    text: String,
    marks: Vec<Mark>,
    /// 記法を外した本文(字数を数えるのに使う)
    stripped: String,
}

#[derive(Serialize, Deserialize)]
struct Table {
    seed: u64,
    cases: Vec<Case>,
}

/// 本文を組み立てる部品。記法の文字を多めに混ぜ、崩れた形が出やすいようにする
const PIECES: &[&str] = &[
    "｜", "｜", "《", "《", "》", "》", "漢字", "吉", "かんじ", "よし", "。",
    "\n", "\r\n", " ", "\u{3000}", "a",
    "𠮷",                     // サロゲートペア
    "👨\u{200D}👩",           // ZWJ
    "葛\u{E0100}",            // 異体字セレクタ
    "|", "<<", ">>",          // 半角の似た記号(記法ではない)
    "｜漢字《かんじ》",       // 正しい形
    "｜𠮷田《よしだ》",
];

fn chosen_cases() -> Vec<String> {
    [
        "｜漢字《かんじ》",
        "漢字《かんじ》",                     // 区切りが無い
        "｜漢字《かんじ",                     // 閉じ忘れ
        "｜漢字《》",                         // 読みが空
        "｜《かんじ》",                       // 親文字が空
        "｜漢\n字《かんじ》",                 // 親文字が改行をまたぐ
        "｜漢字《かん\nじ》",                 // 読みが改行をまたぐ
        "｜漢字《かん\r\nじ》",
        "｜漢字《か《ん》じ》",               // 読みの中に《
        "｜漢｜字《かんじ》",                 // 親文字の中に｜(後ろの｜から読む)
        "｜漢字《かんじ》》",                 // 閉じが余る
        "｜漢》字《かんじ》",                 // 親文字の中に》
        "｜白鏡《し｜ろかが《よみ》み》",     // 入れ子(振り直しで壊れた形)
        "|漢字《かんじ》",                    // 半角の区切り
        "｜漢字《かんじ》｜漢字《かんじ》",   // 連続
        "｜𠮷田《よし𠮷だ》",                 // 読みにサロゲートペア
        "｜漢字《かんじ》\r\n｜漢字《かんじ》",
        "｜｜漢字《かんじ》",                 // 区切りが2つ
        "《かんじ》｜漢字",                   // 順序が逆
        "",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn random_text(rng: &mut Rng) -> String {
    let len = rng.below(25);
    (0..len).map(|_| rng.pick(PIECES)).collect()
}

fn rust_marks(text: &str) -> Vec<Mark> {
    ruby::parse(text)
        .into_iter()
        .map(|r| Mark {
            start: utf16_at(text, r.start),
            end: utf16_at(text, r.end),
            base: r.base,
            reading: r.reading,
        })
        .collect()
}

#[test]
#[ignore]
fn regenerate_the_shared_table() {
    let mut rng = Rng(SEED);
    let mut texts = chosen_cases();
    texts.extend((0..RANDOM_CASES).map(|_| random_text(&mut rng)));
    let cases = texts
        .into_iter()
        .map(|text| Case {
            marks: rust_marks(&text),
            stripped: ruby::strip(&text),
            text,
        })
        .collect();
    std::fs::create_dir_all(std::path::Path::new(TABLE).parent().unwrap()).unwrap();
    std::fs::write(TABLE, ascii_json(&Table { seed: SEED, cases })).unwrap();
}

fn read_table() -> Table {
    let raw = std::fs::read_to_string(TABLE).expect("表が無い。--ignored で作る");
    serde_json::from_str(&raw).unwrap()
}

/// 表の期待値どおりに Rust 版が読む(TS 版は src/rubyParity.test.ts で同じ表を見る)
#[test]
fn rust_matches_the_shared_table() {
    let table = read_table();
    assert!(table.cases.len() > RANDOM_CASES, "表が小さすぎる");
    for case in &table.cases {
        assert_eq!(rust_marks(&case.text), case.marks, "本文 {:?}", case.text);
        assert_eq!(ruby::strip(&case.text), case.stripped, "本文 {:?}", case.text);
    }
}

/// 表記ゆれの前処理は、**本文を消さず、位置を変えない**。
/// UTF-16 の長さが同じで、ルビの外はそのまま、親文字は元の位置に残る
#[test]
fn mask_keeps_every_position() {
    for case in &read_table().cases {
        let text = &case.text;
        let masked = ruby::mask(text);
        let t: Vec<u16> = text.encode_utf16().collect();
        let m: Vec<u16> = masked.encode_utf16().collect();
        assert_eq!(m.len(), t.len(), "長さが変わった: {text:?} → {masked:?}");
        let mut inside = vec![false; t.len()];
        for mark in &case.marks {
            inside[mark.start..mark.end].iter_mut().for_each(|x| *x = true);
            // 親文字は `｜` の次から、元の位置のまま
            let base: Vec<u16> = mark.base.encode_utf16().collect();
            let at = mark.start + 1;
            assert_eq!(&m[at..at + base.len()], &base[..], "親文字が動いた: {text:?}");
        }
        for (i, (a, b)) in t.iter().zip(&m).enumerate() {
            if !inside[i] {
                assert_eq!(a, b, "ルビの外が変わった({i}): {text:?} → {masked:?}");
            }
        }
    }
}
