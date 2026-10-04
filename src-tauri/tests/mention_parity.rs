//! テスト計画 A3(docs/07-test-plan.md): 言及検出の Rust 版(`mentions::find_mentions`)と
//! TS 版(`src/editor/mentions.ts`)が、同じ入力に同じ結果を返すか。
//!
//! 同じ規則を2か所で実装している。**共通の入力表**(`tests/data/mention_parity.json`)を
//! 両方のテストから読み、同じ期待値で確かめる(TS 側は `src/mentionParity.test.ts`)。
//!
//! 表は、手で選んだ組と、乱数(固定シード)で作った組でできている。期待値は Rust 版の結果。
//! 規則を変えたときは作り直す:
//!
//! ```text
//! cargo test --test mention_parity -- --ignored
//! ```

mod common;

use common::{ascii_json, Rng};
use kaku_lib::mentions;
use serde::{Deserialize, Serialize};

const TABLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/mention_parity.json");
const SEED: u64 = 20261004;
const RANDOM_CASES: usize = 300;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Hit {
    name: String,
    /// UTF-16 の位置(エディタと同じ単位)
    from: usize,
    to: usize,
}

#[derive(Serialize, Deserialize)]
struct Case {
    text: String,
    patterns: Vec<String>,
    expected: Vec<Hit>,
}

#[derive(Serialize, Deserialize)]
struct Table {
    seed: u64,
    cases: Vec<Case>,
}

/// 本文を組み立てる部品。数え方が食い違いやすい文字と、名前らしい語を混ぜる
const PIECES: &[&str] = &[
    "架", "純", "佐藤", "悠二", "佐藤架純", "の", "は", "。", "「", "」",
    "𠮷",                     // サロゲートペア
    "葛\u{E0100}",            // 異体字セレクタ
    "か\u{309A}",             // 結合文字
    "👨\u{200D}👩\u{200D}👧", // ZWJ
    "\t", "\u{3000}", "\n", "\r\n", " ",
    "a", "A", "é", "e\u{301}", // 合成済みと分解された é
    "\u{FEFF}",               // BOM(ゼロ幅のノーブレークスペース)
    "\u{85}",                 // NEL
    "\u{A0}",                 // ノーブレークスペース
];

/// 名前の前後に付きうる空白の類。両方の版で「名前の前後の空白を除く」規則が同じかを見る
const PADS: &[&str] = &["", "", "", " ", "\u{3000}", "\t", "\n", "\u{FEFF}", "\u{85}", "\u{A0}"];

fn random_case(rng: &mut Rng) -> (String, Vec<String>) {
    let len = rng.below(30);
    let text: String = (0..len).map(|_| rng.pick(PIECES)).collect();
    let chars: Vec<char> = text.chars().collect();
    let count = 1 + rng.below(5);
    let patterns = (0..count)
        .map(|_| {
            // 半分は本文の一部(必ずどこかに当たる)、残りは部品の組み合わせ
            let core: String = if !chars.is_empty() && rng.below(2) == 0 {
                let from = rng.below(chars.len());
                let to = (from + 1 + rng.below(4)).min(chars.len());
                chars[from..to].iter().collect()
            } else {
                (0..1 + rng.below(2)).map(|_| rng.pick(PIECES)).collect()
            };
            format!("{}{core}{}", rng.pick(PADS), rng.pick(PADS))
        })
        .collect();
    (text, patterns)
}

/// 手で選んだ組。乱数では当たりにくい形を確実に入れる
fn chosen_cases() -> Vec<(String, Vec<String>)> {
    let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    vec![
        // 別名が正式名の一部(長い方を採る)
        ("佐藤架純と架純".into(), p(&["架純", "佐藤架純"])),
        // 名前の前後の空白の類
        ("架純が来た".into(), p(&["\u{FEFF}架純"])),
        ("架純が来た".into(), p(&["架純\u{85}"])),
        ("架純が来た".into(), p(&["\u{3000}架純\u{A0}"])),
        // 空白だけの名前は無視する
        ("架純".into(), p(&["", " ", "\u{3000}", "\u{85}", "\u{FEFF}"])),
        // サロゲートペア・異体字・結合文字のあと
        ("𠮷葛\u{E0100}か\u{309A}架純".into(), p(&["架純", "𠮷"])),
        // 重なる名前(左から、長い方)
        ("abcdef".into(), p(&["abc", "bcdef"])),
        ("abcdef".into(), p(&["bcd", "abcde"])),
    ]
}

fn rust_hits(text: &str, patterns: &[String]) -> Vec<Hit> {
    mentions::find_mentions(text, patterns)
        .into_iter()
        .map(|m| Hit {
            name: m.name,
            from: m.start_utf16,
            to: m.end_utf16,
        })
        .collect()
}

#[test]
#[ignore]
fn regenerate_the_shared_table() {
    let mut rng = Rng(SEED);
    let mut inputs = chosen_cases();
    inputs.extend((0..RANDOM_CASES).map(|_| random_case(&mut rng)));
    let cases = inputs
        .into_iter()
        .map(|(text, patterns)| Case {
            expected: rust_hits(&text, &patterns),
            text,
            patterns,
        })
        .collect();
    let table = Table { seed: SEED, cases };
    std::fs::create_dir_all(std::path::Path::new(TABLE).parent().unwrap()).unwrap();
    std::fs::write(TABLE, ascii_json(&table)).unwrap();
}

/// 表の期待値どおりに Rust 版が返す(TS 版は src/mentionParity.test.ts で同じ表を見る)
#[test]
fn rust_matches_the_shared_table() {
    let raw = std::fs::read_to_string(TABLE).expect("表が無い。--ignored で作る");
    let table: Table = serde_json::from_str(&raw).unwrap();
    assert!(table.cases.len() > RANDOM_CASES, "表が小さすぎる");
    for case in &table.cases {
        assert_eq!(
            rust_hits(&case.text, &case.patterns),
            case.expected,
            "本文 {:?} / 名前 {:?}",
            case.text,
            case.patterns
        );
    }
}
