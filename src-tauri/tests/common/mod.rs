//! 結合テストで共有する道具。
//!
//! 同じ規則を Rust と TS の2か所で持つもの(言及検出・ルビ)を突き合わせる表を作るのに使う
//! (テスト計画 A3・A6)。依存を増やさないため、乱数も自前で持つ。

// 使う道具はテストごとに違う(使わない関数を警告にしない)
#![allow(dead_code)]

use serde::Serialize;

/// 乱数(SplitMix64)。シードを決めれば毎回同じ並びになる
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    pub fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

/// 表の JSON を ASCII だけで書く(BOM や NEL を生のまま置くと、エディタや差分の表示で化ける)
pub fn ascii_json<T: Serialize>(value: &T) -> String {
    let raw = serde_json::to_string_pretty(value).unwrap();
    let mut out = String::with_capacity(raw.len() * 2);
    for c in raw.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out.push('\n');
    out
}

/// バイト位置を UTF-16 の位置(エディタの単位)にする
pub fn utf16_at(text: &str, byte: usize) -> usize {
    text[..byte].encode_utf16().count()
}
