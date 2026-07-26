//! 言及検出: 本文中に現れる codex エントリの名前・別名を検出する(M-02の骨格)。
//!
//! 設計上の前提(docs/03-data-format.md D-7):
//! 検出結果は永続化しない。呼ばれた時にメモリ上で算出して返すだけ。
//!
//! オフセットについて:
//! aho-corasick が返すのは UTF-8 バイトオフセットだが、フロントエンド(CodeMirror)が
//! 扱う位置は JS 文字列の UTF-16 コードユニット単位である。日本語(BMP内)は
//! 1文字=3バイト=1コードユニット、絵文字等(サロゲートペア)は 4バイト=2コードユニット
//! となり両者は一致しない。そのため **UTF-16 換算した位置も併せて返す**。

use aho_corasick::{AhoCorasick, MatchKind};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mention {
    /// 一致したパターン(codexの名前または別名)
    pub name: String,
    /// UTF-8 バイトオフセット(Rust側で本文を扱うとき用)
    pub start_byte: usize,
    pub end_byte: usize,
    /// UTF-16 コードユニットオフセット(CodeMirror に渡す用)
    pub start_utf16: usize,
    pub end_utf16: usize,
}

/// 本文から言及を検出する。
///
/// `MatchKind::LeftmostLongest` を使うのは、別名が正式名の一部になる場合
/// (例: 正式名「佐藤架純」・別名「架純」)に短い方だけを拾って二重に
/// ハイライトしないため。同じ位置から始まる候補のうち最長を採る。
pub fn find_mentions(text: &str, patterns: &[String]) -> Vec<Mention> {
    let patterns: Vec<&str> = patterns
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    if patterns.is_empty() || text.is_empty() {
        return Vec::new();
    }

    let ac = match AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostLongest)
        .build(&patterns)
    {
        Ok(ac) => ac,
        // パターンが不正でも本文表示を止めない(寛容に空を返す)
        Err(_) => return Vec::new(),
    };

    let to_utf16 = Utf16Map::new(text);

    ac.find_iter(text)
        .map(|m| Mention {
            name: patterns[m.pattern().as_usize()].to_string(),
            start_byte: m.start(),
            end_byte: m.end(),
            start_utf16: to_utf16.at(m.start()),
            end_utf16: to_utf16.at(m.end()),
        })
        .collect()
}

/// UTF-8 バイトオフセット → UTF-16 コードユニットオフセットの変換表。
///
/// 文字境界ごとの累積値だけを持つ疎な表にして、二分探索で引く。
struct Utf16Map {
    /// (バイトオフセット, その位置までの UTF-16 コードユニット数)
    marks: Vec<(usize, usize)>,
}

impl Utf16Map {
    fn new(text: &str) -> Self {
        let mut marks = Vec::with_capacity(text.len() / 3 + 1);
        let mut utf16 = 0usize;
        for (byte_idx, ch) in text.char_indices() {
            marks.push((byte_idx, utf16));
            utf16 += ch.len_utf16();
        }
        marks.push((text.len(), utf16));
        Self { marks }
    }

    fn at(&self, byte_offset: usize) -> usize {
        match self.marks.binary_search_by_key(&byte_offset, |&(b, _)| b) {
            Ok(i) => self.marks[i].1,
            // 文字境界でないオフセットは来ない想定だが、来ても壊さない
            Err(i) => self.marks[i.saturating_sub(1)].1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pats(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn finds_japanese_names() {
        let text = "架純は昇降口で悠二とぶつかった。";
        let got = find_mentions(text, &pats(&["架純", "悠二"]));
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "架純");
        assert_eq!(got[0].start_utf16, 0);
        assert_eq!(got[0].end_utf16, 2);
        assert_eq!(got[1].name, "悠二");
        // 「架純は昇降口で」= 7文字 → UTF-16 で 7
        assert_eq!(got[1].start_utf16, 7);
    }

    #[test]
    fn prefers_longest_at_same_position() {
        // 別名「架純」が正式名「佐藤架純」の一部。二重に拾わず長い方を採る
        let text = "佐藤架純が来た。";
        let got = find_mentions(text, &pats(&["架純", "佐藤架純"]));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "佐藤架純");
        assert_eq!(got[0].start_utf16, 0);
        assert_eq!(got[0].end_utf16, 4);
    }

    #[test]
    fn utf16_offsets_account_for_surrogate_pairs() {
        // 𠮟(U+20B9F)はサロゲートペア: UTF-8で4バイト、UTF-16で2コードユニット
        let text = "𠮟る架純";
        let got = find_mentions(text, &pats(&["架純"]));
        assert_eq!(got.len(), 1);
        // バイト: 𠮟(4) + る(3) = 7
        assert_eq!(got[0].start_byte, 7);
        // UTF-16: 𠮟(2) + る(1) = 3 ← バイトオフセットとは一致しない
        assert_eq!(got[0].start_utf16, 3);
        assert_eq!(got[0].end_utf16, 5);
    }

    #[test]
    fn empty_inputs_are_safe() {
        assert!(find_mentions("", &pats(&["架純"])).is_empty());
        assert!(find_mentions("架純", &pats(&[])).is_empty());
        assert!(find_mentions("架純", &pats(&["", "  "])).is_empty());
    }

    #[test]
    fn finds_repeated_occurrences() {
        let text = "架純。架純。架純。";
        let got = find_mentions(text, &pats(&["架純"]));
        assert_eq!(got.len(), 3);
        assert_eq!(got[2].start_utf16, 6);
    }
}
