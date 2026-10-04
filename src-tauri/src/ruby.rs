//! ルビ記法(03-data-format.md 未決-3 → 2026-08-04 確定)。
//!
//! **記法は `｜漢字《かんじ》` の一形式だけ。** 区切りの `｜` を必須にする。
//! 青空文庫式は `｜` を省略して「漢字が続く限り」を対象にできるが、
//! それは**どこからがルビ対象かを推測する**ことになり、長い漢字列で意図とずれる。
//! 常に明示させれば解釈がぶれない。カクヨム・なろう・青空文庫のいずれでも
//! この形はそのまま通る(D-4「コピペ無加工」を守れる)。
//!
//! ## なぜ本文に記法を入れるのか
//!
//! D-4 は「本文内に記法を持ち込まない」だが、**ルビは明示された例外候補**である
//! (03 D-4 の但し書き)。ルビは文字そのものに紐づく情報で、ファイル分割や
//! フロントマターでは表現できない。かつ投稿サイトがこの記法を解釈するため、
//! **本文に書いてあることがそのまま資産になる**。
//!
//! ## 既存機能への波及(ここで潰す)
//!
//! 記法文字が本文に混ざると、文字を数える処理・語を切り出す処理が影響を受ける。
//! - 文字数 → `strip` で記法を外して数える(投稿サイトの字数と合わせる)
//! - 表記ゆれ検出 → `mask` で読み部分を潰してから渡す(カタカナのルビが
//!   登録名と1文字違いになり誤検出する。実際に起こりうる)
//! - AIへ渡す本文 → **外さない**。外すと引用の位置照合ができなくなるため、
//!   プロンプト側で「ルビ記法は誤りではない」と伝える

use serde::Serialize;

/// ルビの区切り(全角縦線)。半角 `|` は受け付けない —
/// Markdownの表と紛らわしく、誤爆すると本文が壊れて見える
pub const DELIM: char = '｜';
pub const OPEN: char = '《';
pub const CLOSE: char = '》';

/// 本文中のルビ1つ。位置はバイトオフセット(**保存はしない**。D-7)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ruby {
    /// `｜` の位置
    pub start: usize,
    /// `》` の次の位置
    pub end: usize,
    /// ルビを振る対象(親文字)
    pub base: String,
    /// 読み
    pub reading: String,
}

/// 本文からルビを取り出す。
///
/// 壊れた記法(閉じ忘れ・空・改行またぎ)は**ルビとして扱わず素通しする**。
/// 書きかけの状態でエラーを出しても書き手の役に立たない。
pub fn parse(text: &str) -> Vec<Ruby> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < text.len() {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(DELIM.to_string().as_bytes()) {
            if let Some(r) = parse_at(text, i) {
                i = r.end;
                out.push(r);
                continue;
            }
        }
        // 次の文字境界へ
        i += text[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    out
}

/// `｜` の位置から1つ読む。読めなければ None
fn parse_at(text: &str, delim_at: usize) -> Option<Ruby> {
    let after_delim = delim_at + DELIM.len_utf8();
    let rest = text.get(after_delim..)?;

    // 親文字: `《` まで。改行や次の `｜` が来たら記法として不成立
    let mut base_len = 0usize;
    for c in rest.chars() {
        if c == OPEN {
            break;
        }
        if c == '\n' || c == '\r' || c == DELIM {
            return None;
        }
        base_len += c.len_utf8();
    }
    let base = rest.get(..base_len)?;
    if base.is_empty() {
        return None;
    }
    let open_at = after_delim + base_len;
    if !text[open_at..].starts_with(OPEN) {
        return None;
    }

    // 読み: `》` まで
    let after_open = open_at + OPEN.len_utf8();
    let tail = text.get(after_open..)?;
    let mut reading_len = 0usize;
    let mut closed = false;
    for c in tail.chars() {
        if c == CLOSE {
            closed = true;
            break;
        }
        if c == '\n' || c == '\r' || c == OPEN {
            return None;
        }
        reading_len += c.len_utf8();
    }
    if !closed {
        return None;
    }
    let reading = tail.get(..reading_len)?;
    if reading.is_empty() {
        return None;
    }

    Some(Ruby {
        start: delim_at,
        end: after_open + reading_len + CLOSE.len_utf8(),
        base: base.to_string(),
        reading: reading.to_string(),
    })
}

/// 記法を外して素の本文にする(`｜漢字《かんじ》` → `漢字`)。
///
/// **文字数を数えるときはこれを使う。** 記法込みで数えると投稿サイトの字数と合わない。
pub fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev = 0usize;
    for r in parse(text) {
        out.push_str(&text[prev..r.start]);
        out.push_str(&r.base);
        prev = r.end;
    }
    out.push_str(&text[prev..]);
    out
}

/// 記法の飾りと読みだけを潰し、**親文字と位置はそのまま残す**。
///
/// 表記ゆれ検出(M-04-02)へ渡す前に通す。カタカナのルビが登録名と1文字違いだと
/// 誤検出するため、読みを語の候補から外す必要がある。
///
/// 潰す文字は全角空白にする。UTF-16 の長さが1で変わらないので、
/// **検出結果の位置が元の本文とそのまま一致する**(位置を写像し直さなくて済む)。
pub fn mask(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev = 0usize;
    for r in parse(text) {
        out.push_str(&text[prev..r.start]);
        out.push('　'); // ｜
        out.push_str(&r.base);
        // 《 + 読み + 》 を全角空白に置き換える。**数えるのは UTF-16 の長さ** —
        // 見つけた位置はエディタの単位(UTF-16)で元の本文へそのまま当てるので、
        // 読みにサロゲートペア(𠮷・絵文字)があると、文字数で数えた分だけ後ろがずれて
        // 「置換」が別の文字を書き換えた(2026-10-04 テスト計画 A1 で見つけた)
        for _ in 0..(r.reading.encode_utf16().count() + 2) {
            out.push('　');
        }
        prev = r.end;
    }
    out.push_str(&text[prev..]);
    out
}

/// 選択された語にルビを付けた文字列を作る(入力補助)
pub fn wrap(base: &str, reading: &str) -> String {
    format!("{DELIM}{base}{OPEN}{reading}{CLOSE}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_single_ruby() {
        let t = "　彼は｜白鏡《しろかがみ》の塔を見た。";
        let got = parse(t);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].base, "白鏡");
        assert_eq!(got[0].reading, "しろかがみ");
        assert_eq!(&t[got[0].start..got[0].end], "｜白鏡《しろかがみ》");
    }

    #[test]
    fn parses_several_in_one_line() {
        let got = parse("｜白鏡《しろかがみ》と｜黒鏡《くろかがみ》");
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].base, "黒鏡");
    }

    #[test]
    fn broken_notation_is_left_alone() {
        // 書きかけを壊れているとは言わない。素通しする
        for t in [
            "｜白鏡",              // 閉じていない
            "｜白鏡《しろかがみ",  // 閉じ括弧が無い
            "｜《しろかがみ》",    // 親文字が無い
            "｜白鏡《》",          // 読みが無い
            "白鏡《しろかがみ》",  // 区切りが無い(｜必須)
            "｜白鏡\n《しろかがみ》", // 改行をまたぐ
        ] {
            assert!(parse(t).is_empty(), "壊れた記法を拾った: {t}");
            assert_eq!(strip(t), t, "壊れた記法で本文が変わった: {t}");
        }
    }

    #[test]
    fn half_width_pipe_is_not_a_delimiter() {
        // Markdownの表と紛らわしいので受け付けない
        assert!(parse("|白鏡《しろかがみ》").is_empty());
    }

    #[test]
    fn strip_leaves_only_the_base() {
        assert_eq!(
            strip("　彼は｜白鏡《しろかがみ》の塔を見た。"),
            "　彼は白鏡の塔を見た。"
        );
        assert_eq!(strip("ルビなしの本文"), "ルビなしの本文");
        assert_eq!(strip(""), "");
    }

    #[test]
    fn char_count_matches_what_a_reader_sees() {
        // 投稿サイトの字数と合わせるための性質
        let t = "｜白鏡《しろかがみ》の塔";
        assert_eq!(t.chars().count(), 12, "記法込みだとこうなる");
        assert_eq!(strip(t).chars().count(), 4, "白鏡の塔");
    }

    #[test]
    fn mask_keeps_positions_in_utf16() {
        // **ここが崩れると表記ゆれの指摘位置がずれる**
        let t = "　架純は｜白鏡《しろかがみ》を見た。";
        let m = mask(t);
        assert_eq!(
            t.chars().count(),
            m.chars().count(),
            "文字数が変わると位置がずれる"
        );
        assert_eq!(
            t.encode_utf16().count(),
            m.encode_utf16().count(),
            "UTF-16長が変わると位置がずれる"
        );
        // 親文字は残り、読みは消える
        assert!(m.contains("白鏡"));
        assert!(!m.contains("しろかがみ"));
        assert!(m.contains("架純"), "ルビ以外は素通し");
    }

    #[test]
    fn mask_removes_katakana_readings_from_candidates() {
        // カタカナのルビは登録名と1文字違いになりうる。候補から外す
        let t = "｜白鏡《ハクキョウ》";
        let m = mask(t);
        assert!(!m.contains("ハクキョウ"));
        assert!(m.contains("白鏡"));
    }

    #[test]
    fn wrap_builds_the_decided_notation() {
        assert_eq!(wrap("白鏡", "しろかがみ"), "｜白鏡《しろかがみ》");
        // 作ったものを読み直せること
        let t = wrap("白鏡", "しろかがみ");
        let got = parse(&t);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].base, "白鏡");
    }

    #[test]
    fn positions_are_byte_offsets_into_the_original() {
        let t = "あ｜漢字《かんじ》い";
        let r = &parse(t)[0];
        assert_eq!(&t[r.start..r.end], "｜漢字《かんじ》");
        assert_eq!(&t[..r.start], "あ");
        assert_eq!(&t[r.end..], "い");
    }
}
