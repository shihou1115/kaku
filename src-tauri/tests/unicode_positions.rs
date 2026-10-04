//! テスト計画 A1(docs/07-test-plan.md): 位置を返すすべての機能に、
//! **数え方が食い違いやすい文字**を混ぜて通す。
//!
//! 位置はエディタの単位(UTF-16)で返る。返った位置で本文を切り出し、
//! 期待した文字列と一致するかを見る。CRLF の原稿で「置換」が別の文字を
//! 書き換えた不具合(2026-10-04)と同じ型を、ほかの機能で探すためのもの。

use kaku_lib::{mentions, proofread, review, ruby, split};

/// UTF-8 のバイト数・char の数・UTF-16 の数が、それぞれ食い違う文字
const TRICKY: &[&str] = &[
    "𠮷",                         // サロゲートペア(UTF-16 で2)
    "葛\u{E0100}",                // 異体字セレクタ(char で2・UTF-16 で3)
    "か\u{309A}",                 // 結合文字(半濁点)
    "👨\u{200D}👩\u{200D}👧",     // ZWJ でつないだ絵文字
    "\t",                         // タブ
    "　",                         // 全角空白
    "é",                          // 合成済みのラテン文字(UTF-8 で2バイト)
];

fn u16_slice(text: &str, from: usize, to: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    assert!(to <= units.len(), "範囲外: {from}..{to} / {}", units.len());
    String::from_utf16(&units[from..to])
        .unwrap_or_else(|_| panic!("UTF-16 の途中で切れている: {from}..{to}"))
}

/// 対象の語を、TRICKY の文字のあとのいろいろな場所に置いた本文
fn bodies(target: &str) -> Vec<String> {
    let mut out = Vec::new();
    for t in TRICKY {
        out.push(format!("{t}。{target}は来た。"));
        out.push(format!("{t}{t}\n{t}本文の途中。{target}がいる。{t}"));
        out.push(format!("前置き{t}。\n\n{t}、{target}。{t}"));
    }
    out.push(format!("{}。{target}は来た。", TRICKY.concat()));
    out
}

#[test]
fn mentions_point_at_the_name() {
    for body in bodies("佐藤架純") {
        let hits = mentions::find_mentions(&body, &["佐藤架純".to_string()]);
        assert_eq!(hits.len(), 1, "見つからない: {body:?}");
        for h in hits {
            assert_eq!(
                u16_slice(&body, h.start_utf16, h.end_utf16),
                "佐藤架純",
                "{body:?}"
            );
        }
    }
}

#[test]
fn notation_hits_point_at_the_variant() {
    for body in bodies("佐藤佳純") {
        let hits = proofread::check_notation(&body, &["佐藤架純".to_string()]);
        assert_eq!(hits.len(), 1, "見つからない: {body:?}");
        for o in &hits[0].occurrences {
            assert_eq!(
                u16_slice(&body, o.start_utf16, o.end_utf16),
                "佐藤佳純",
                "{body:?}"
            );
        }
    }
}

/// 表記ゆれは、ルビの読みを潰した本文(`ruby::mask`)で探し、その位置を
/// **元の本文**にそのまま当てる。潰した本文と元の本文で位置がずれてはいけない
#[test]
fn notation_positions_survive_ruby_masking() {
    for t in TRICKY {
        // 読みにも、親文字にも、数え方の食い違う文字を入れる
        for ruby_text in [
            format!("｜吉田《よし{t}だ》"),
            format!("｜{t}田《よしだ》"),
            format!("｜葛城《かつら{t}ぎ》"),
        ] {
            let body = format!("{ruby_text}が来た。佐藤佳純は鞄を持った。");
            let masked = ruby::mask(&body);
            let hits = proofread::check_notation(&masked, &["佐藤架純".to_string()]);
            assert_eq!(hits.len(), 1, "見つからない: {body:?}");
            let o = &hits[0].occurrences[0];
            assert_eq!(
                u16_slice(&body, o.start_utf16, o.end_utf16),
                "佐藤佳純",
                "潰した本文と元の本文で位置がずれた: {body:?}"
            );
        }
    }
}

#[test]
fn proofread_issues_point_at_the_quote() {
    for body in bodies("佐藤佳純") {
        let issue = proofread::AiIssue {
            quote: "佐藤佳純".into(),
            suggestion: "佐藤架純".into(),
            kind: "変換ミス".into(),
            reason: String::new(),
            found: false,
            start_utf16: None,
            end_utf16: None,
        };
        let got = proofread::resolve_issues(&body, vec![issue]);
        let (s, e) = (got[0].start_utf16.unwrap(), got[0].end_utf16.unwrap());
        assert_eq!(u16_slice(&body, s, e), "佐藤佳純", "{body:?}");
    }
}

#[test]
fn review_comments_point_at_the_quote() {
    for body in bodies("佐藤架純") {
        let c = review::ReviewComment {
            aspect: "style".into(),
            quote: "佐藤架純".into(),
            comment: "c".into(),
            suggestion: String::new(),
            found: false,
            start_utf16: None,
            end_utf16: None,
        };
        let got = review::resolve(&body, vec![c]);
        let (s, e) = (got[0].start_utf16.unwrap(), got[0].end_utf16.unwrap());
        assert_eq!(u16_slice(&body, s, e), "佐藤架純", "{body:?}");
    }
}

#[test]
fn split_points_land_on_the_line_start() {
    for t in TRICKY {
        // 切れ目の前後とも MIN_SEGMENT_CHARS を超える長さにする
        let filler: String = format!("{t}転校初日の朝は、雨だった。").repeat(30);
        let quote = "その日の放課後、図書室は静かだった。";
        let body = format!("{filler}\n{quote}{filler}\n");
        let got = split::verify(
            &body,
            vec![split::RawPoint {
                quote: quote.into(),
                title: "図書室".into(),
            }],
        );
        let at = got[0].at_utf16.expect("切れ目が見つからない");
        let quote_len = quote.encode_utf16().count();
        assert_eq!(u16_slice(&body, at, at + quote_len), quote, "{t:?}");
    }
}

#[test]
fn ruby_spans_cover_the_whole_notation() {
    for t in TRICKY {
        let body = format!("{t}前置き｜漢字《かんじ》{t}");
        let rubies = ruby::parse(&body);
        assert_eq!(rubies.len(), 1, "{body:?}");
        assert_eq!(&body[rubies[0].start..rubies[0].end], "｜漢字《かんじ》");
    }
}
