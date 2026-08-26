//! シーンの自動分割(P-8 / docs/05-roadmap.md §5.9-D)。
//!
//! 長い原稿を場面の切れ目で分ける支援。現在は執筆者が手でファイルを分けている。
//!
//! **提案型であること。** 分割は破壊的操作なので、機械が勝手に切らない。
//! 切れ目の候補を出し、前後を見せ、**採用したものだけ**を適用する
//! (P-8: 自動化してよいのは「気づくこと」であって「決めること」ではない)。
//!
//! **本文は1文字も変えない。** ここでAIがするのは「どこで切るか」の提案だけで、
//! 文章を書いたり直したりはしない(C-01の境界 = docs/06-decision-log.md §2-1)。
//! 分割の前後を連結すると元の本文に戻ることを、テストで固定してある。
//!
//! **位置は保存しない**(D-7)。適用するときに引用から引き直す。提案を見てから
//! 適用するまでの間に本文が変わっていたら、**誤った位置で切るより失敗させる**。

use serde::{Deserialize, Serialize};

use crate::mentions::{locate_quote, Utf16Map};

/// これより短い断片は作らない(文字数)。
///
/// 場面として成立しない切れ端がファイルとして増えると、
/// ツリーが散らかって「分けて良かった」と思えなくなる。
pub const MIN_SEGMENT_CHARS: usize = 200;

/// LLMが返した切れ目(検証前)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPoint {
    pub quote: String,
    pub title: String,
}

/// 検証後の切れ目。**この位置は表示用**で、適用時には引用から引き直す
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SplitPoint {
    /// 切れ目の直後にくる一文(本文からの引用)
    pub quote: String,
    /// ここから始まる場面の見出し
    pub title: String,
    /// 本文に実在したか。しないものは幻覚の疑い
    pub found: bool,
    /// 表示用の位置(UTF-16)
    pub at_utf16: Option<usize>,
    /// 切れ目の直前の抜粋(プレビュー用)
    pub before: String,
    /// 切れ目の直後の抜粋(プレビュー用)
    pub after: String,
    /// この切れ目から次の切れ目までの文字数
    pub chars: usize,
}

/// 採用された切れ目(フロントから返ってくる)
#[derive(Debug, Clone, Deserialize)]
pub struct AcceptedPoint {
    pub quote: String,
    pub title: String,
}

/// 分割後の1ファイル分
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Segment {
    pub title: String,
    /// 本文(フロントマターは含まない)
    pub text: String,
    pub chars: usize,
}

pub const SYSTEM_PROMPT: &str = "\
あなたは小説の編集者です。原稿を場面ごとに分ける手伝いをします。

- **本文を書き換えたり要約したりしません。** どこで切るかを示すだけです。
- 日本語で答えます。
- 指示された出力形式(JSON)だけを返します。前置きを付けません。";

/// 切れ目を挙げさせるプロンプト
pub fn build_prompt(body: &str) -> String {
    let mut s = String::new();
    s.push_str(
        "次の小説本文を、場面(シーン)の切れ目で分けたいと考えています。\n\
         **切れ目になる箇所**を挙げてください。\n\n\
         場面が変わるのは、たとえば次のようなときです:\n\
         - 時間が飛ぶ(その日の夜、翌朝、数日後)\n\
         - 場所が変わる(教室から屋上へ、屋内から屋外へ)\n\
         - 視点人物が変わる\n\
         - 語りの区切りがある(回想の開始と終了など)\n\n\
         守ること:\n\
         - quote には、**切れ目の直後に来る一文を本文からそのまま書き写して**ください\n  \
           (15〜40字程度)。要約・言い換えをせず、前後も変えないでください\n\
         - title には、そこから始まる場面の**短い見出し**(10字程度)を付けてください\n\
         - **切りすぎないでください。** 会話の途中や、同じ場面の中の段落境界は切れ目ではありません\n\
         - 切れ目が無いと判断したら、空の一覧を返してください\n\
         - **本文の文章そのものは書き換えないでください**\n\n",
    );
    s.push_str("出力は次のJSONだけを返してください(説明文は不要です):\n");
    s.push_str(
        "{\"points\":[{\"quote\":\"切れ目の直後の一文\",\"title\":\"場面の見出し\"}]}\n\n",
    );
    s.push_str("--- 本文ここから ---\n");
    s.push_str(body);
    s.push_str("\n--- 本文ここまで ---\n");
    s
}

/// 応答の寛容パース(レビュー・校正と同じ方針)
pub fn parse(raw: &str) -> Vec<RawPoint> {
    let blob = crate::ai::extract_json_blob(raw);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(blob) else {
        return Vec::new();
    };
    let array = value
        .get("points")
        .or_else(|| value.get("scenes"))
        .or_else(|| value.get("items"))
        .and_then(|v| v.as_array())
        .or_else(|| value.as_array());
    let Some(array) = array else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|item| {
            let quote = item
                .get("quote")
                .or_else(|| item.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if quote.is_empty() {
                return None;
            }
            let title = item
                .get("title")
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            Some(RawPoint { quote, title })
        })
        .collect()
}

/// 位置を**行頭へ寄せる**。
///
/// 段落の途中で切ると原稿が壊れて見える。引用が行の途中から始まっていても、
/// その行の先頭を切れ目にする。
fn snap_to_line_start(body: &str, at: usize) -> usize {
    body[..at].rfind('\n').map(|p| p + 1).unwrap_or(0)
}

/// 前後の抜粋を作る(プレビュー用。保存はしない)
fn excerpt(body: &str, at: usize, before: bool, chars: usize) -> String {
    let slice = if before { &body[..at] } else { &body[at..] };
    let taken: String = if before {
        let v: Vec<char> = slice.chars().collect();
        v[v.len().saturating_sub(chars)..].iter().collect()
    } else {
        slice.chars().take(chars).collect()
    };
    taken.replace('\n', " ").trim().to_string()
}

/// LLMの候補を機械側で検証する。
///
/// 落とすもの:
///  - 本文に実在しない引用(幻覚)
///  - 先頭(位置0)の切れ目 — 空のファイルができるだけ
///  - 直前の切れ目と近すぎるもの(§MIN_SEGMENT_CHARS)
///  - 重複
pub fn verify(body: &str, raw: Vec<RawPoint>) -> Vec<SplitPoint> {
    let to_utf16 = Utf16Map::new(body);

    // まず位置を解決して並べ替える(LLMの並び順は当てにしない)
    let mut resolved: Vec<(usize, RawPoint)> = Vec::new();
    let mut missing: Vec<RawPoint> = Vec::new();
    for p in raw {
        match locate_quote(body, &p.quote) {
            Some((at, _)) => {
                let at = snap_to_line_start(body, at);
                if at == 0 {
                    continue; // 冒頭は切れ目にならない
                }
                if resolved.iter().any(|(a, _)| *a == at) {
                    continue;
                }
                resolved.push((at, p));
            }
            None => missing.push(p),
        }
    }
    resolved.sort_by_key(|(at, _)| *at);

    // 近すぎる切れ目を落とす。前の採用位置からの距離で見る
    let mut kept: Vec<(usize, RawPoint)> = Vec::new();
    let mut prev = 0usize;
    for (at, p) in resolved {
        if body[prev..at].chars().count() < MIN_SEGMENT_CHARS {
            continue;
        }
        prev = at;
        kept.push((at, p));
    }
    // 末尾が短すぎる場合は最後の切れ目を落とす(切れ端を作らない)
    if let Some((last, _)) = kept.last() {
        if body[*last..].chars().count() < MIN_SEGMENT_CHARS {
            kept.pop();
        }
    }

    let mut out: Vec<SplitPoint> = Vec::new();
    for (i, (at, p)) in kept.iter().enumerate() {
        let end = kept.get(i + 1).map(|(a, _)| *a).unwrap_or(body.len());
        out.push(SplitPoint {
            quote: p.quote.clone(),
            title: p.title.clone(),
            found: true,
            at_utf16: Some(to_utf16.at(*at)),
            before: excerpt(body, *at, true, 40),
            after: excerpt(body, *at, false, 40),
            chars: body[*at..end].chars().count(),
        });
    }
    // 見つからなかったものも消さずに後ろへ置く(モデルの癖を見えるようにする)
    for p in missing {
        out.push(SplitPoint {
            quote: p.quote,
            title: p.title,
            found: false,
            at_utf16: None,
            before: String::new(),
            after: String::new(),
            chars: 0,
        });
    }
    out
}

/// 採用された切れ目で本文を切る。
///
/// **引用から位置を引き直す**ので、提案を見たあとに本文が変わっていても
/// ずれた位置では切らない。1つでも解決できなければ全体を失敗させる —
/// 一部だけ切れた原稿を残すほうが困る。
pub fn segments(
    body: &str,
    first_title: &str,
    accepted: &[AcceptedPoint],
) -> Result<Vec<Segment>, String> {
    let mut cuts: Vec<(usize, String)> = Vec::new();
    for a in accepted {
        let Some((at, _)) = locate_quote(body, &a.quote) else {
            return Err(format!(
                "「{}」が本文に見つかりません。本文が変わった可能性があるので、提案し直してください",
                a.quote.chars().take(20).collect::<String>()
            ));
        };
        let at = snap_to_line_start(body, at);
        if at == 0 {
            return Err("冒頭では分割できません".to_string());
        }
        cuts.push((at, a.title.clone()));
    }
    cuts.sort_by_key(|(at, _)| *at);
    cuts.dedup_by_key(|(at, _)| *at);
    if cuts.is_empty() {
        return Err("切れ目が選ばれていません".to_string());
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    let mut title = first_title.to_string();
    for (at, next_title) in cuts {
        let text = body[start..at].to_string();
        out.push(Segment {
            chars: text.chars().count(),
            title: std::mem::replace(&mut title, next_title),
            text,
        });
        start = at;
    }
    let text = body[start..].to_string();
    out.push(Segment {
        chars: text.chars().count(),
        title,
        text,
    });
    Ok(out)
}

/// 元ファイルのフロントマター区画をそのまま返す(未知フィールドを温存するため)
fn header_of(source: &str) -> String {
    match crate::frontmatter::split(source).0 {
        Some(fm) => format!("---\n{fm}---\n"),
        None => String::new(),
    }
}

/// 実際に分割する。**元ファイルはゴミ箱へ退避**し、消さない。
///
/// 戻り値は「作ったファイル群 + 退避先」。呼び出し側はこれを案内に使う。
///
/// 安全のための順序:
///  1. 位置を**引用から引き直す**(提案時の位置は信用しない)
///  2. 書き込み先が全部空いているか**先に**確かめる
///     — 途中で衝突して「半分だけ分割された」状態を作らない
///  3. 作ってから、最後に元を退避する
pub fn apply(
    root: &std::path::Path,
    path: &str,
    first_title: &str,
    accepted: &[AcceptedPoint],
) -> Result<Vec<String>, String> {
    let source = crate::project::read_text(root, path).map_err(|e| e.to_string())?;
    let (_, body) = crate::frontmatter::split(&source);
    let segs = segments(body, first_title, accepted)?;
    let header = header_of(&source);

    let targets: Vec<String> = (0..segs.len()).map(|i| file_name_for(path, i)).collect();
    for t in &targets {
        if crate::project::resolve(root, t)
            .map_err(|e| e.to_string())?
            .exists()
        {
            return Err(format!("{t} が既にあります。先に整理してください"));
        }
    }

    for (i, (target, seg)) in targets.iter().zip(segs.iter()).enumerate() {
        // 元のフロントマターを引き継ぎ、title だけ差し替える(未知フィールドを壊さない)
        let content =
            crate::frontmatter::set_title(&format!("{header}{}", seg.text), &seg.title);
        if let Err(e) = crate::project::create_file(root, target, &content) {
            // **途中で失敗したら、このコールで作った分を消して元へ戻す。**
            // 上の事前確認で「空いている」と確かめた場所だけなので、消して安全。
            // 元原稿にはまだ触れていない(ゴミ箱への退避はこのループの後)
            for done in &targets[..i] {
                if let Ok(p) = crate::project::resolve(root, done) {
                    let _ = std::fs::remove_file(p);
                }
            }
            return Err(e.to_string());
        }
    }

    let trashed = crate::project::trash(root, path).map_err(|e| e.to_string())?;
    let mut out = targets;
    out.push(trashed);
    Ok(out)
}

/// 分割後のファイル名を作る。元の名前を残して連番を付ける。
///
/// 見出しをファイル名にすると、既存ファイルと衝突したり使えない文字が入ったりする。
/// **名前は機械的に、見出しはフロントマターの title に置く**(ツリーには title が出る)。
pub fn file_name_for(original_path: &str, index: usize) -> String {
    let (dir, file) = match original_path.rfind('/') {
        Some(i) => (&original_path[..i], &original_path[i + 1..]),
        None => ("", original_path),
    };
    let stem = file.strip_suffix(".md").unwrap_or(file);
    let name = format!("{stem}-{}.md", index + 1);
    if dir.is_empty() {
        name
    } else {
        format!("{dir}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3場面ぶんの本文。**各場面は MIN_SEGMENT_CHARS を超える長さにする**
    /// (超えないと verify 自身の「切れ端を作らない」規則で落ちる)
    fn body() -> String {
        let a = "　転校初日の朝は、雨だった。".repeat(20); // 280字
        let b = "　その日の放課後、図書室は静かだった。".repeat(20); // 360字
        let c = "　翌朝、廊下で悠二とすれ違った。".repeat(20); // 320字
        format!("{a}\n{b}\n{c}\n")
    }

    fn raw(v: &[(&str, &str)]) -> Vec<RawPoint> {
        v.iter()
            .map(|(q, t)| RawPoint {
                quote: q.to_string(),
                title: t.to_string(),
            })
            .collect()
    }

    #[test]
    fn parses_points() {
        let got = parse(r#"{"points":[{"quote":"その日の放課後","title":"図書室"}]}"#);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "図書室");
    }

    #[test]
    fn broken_response_is_safe() {
        assert!(parse("切れ目はありません").is_empty());
        assert!(parse("").is_empty());
        assert!(parse(r#"{"points":[{"title":"見出しだけ"}]}"#).is_empty());
    }

    #[test]
    fn finds_and_orders_split_points() {
        let b = body();
        // わざと逆順で渡す。LLMの並び順は当てにしない
        let got = verify(
            &b,
            raw(&[
                ("　翌朝、廊下で悠二とすれ違った。", "翌朝"),
                ("　その日の放課後、図書室は静かだった。", "図書室"),
            ]),
        );
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].title, "図書室");
        assert_eq!(got[1].title, "翌朝");
        assert!(got[0].at_utf16.unwrap() < got[1].at_utf16.unwrap());
        assert!(got.iter().all(|p| p.found));
    }

    #[test]
    fn preview_shows_both_sides() {
        let b = body();
        let got = verify(&b, raw(&[("　その日の放課後、図書室は静かだった。", "図書室")]));
        assert!(got[0].before.contains("雨だった"), "直前が見えない");
        assert!(got[0].after.contains("図書室"), "直後が見えない");
        assert!(!got[0].before.contains('\n'), "改行が残ると一覧が崩れる");
    }

    #[test]
    fn hallucinated_quote_is_kept_but_marked() {
        let b = body();
        let got = verify(&b, raw(&[("本文にはまったく存在しない一文である", "幻")]));
        assert_eq!(got.len(), 1);
        assert!(!got[0].found);
        assert!(got[0].at_utf16.is_none());
    }

    #[test]
    fn rejects_split_at_the_very_beginning() {
        let b = body();
        // 冒頭で切ると空のファイルができる
        let got = verify(&b, raw(&[("　転校初日の朝は、雨だった。", "冒頭")]));
        assert!(got.is_empty());
    }

    #[test]
    fn drops_points_that_would_make_slivers() {
        // 短い本文の中で2箇所を切ろうとしても、切れ端は作らない
        let b = "　一行目です。\n　二行目です。\n　三行目です。\n";
        let got = verify(b, raw(&[("　二行目です。", "二"), ("　三行目です。", "三")]));
        assert!(got.is_empty(), "短すぎる断片を作ってしまった: {got:?}");
    }

    #[test]
    fn snaps_to_the_start_of_the_line() {
        // 行の途中の引用でも、切れ目は行頭に寄せる(段落の途中で切らない)
        let b = "　一行目。\n　二行目のとちゅうから始まる引用です。\n";
        let line2 = b.find("　二行目").unwrap();
        let got = verify(b, raw(&[("とちゅうから始まる引用です", "二")]));
        // MIN_SEGMENT_CHARS に満たないので候補としては落ちるが、位置の寄せ方を直接見る
        assert_eq!(snap_to_line_start(b, b.find("とちゅう").unwrap()), line2);
        let _ = got;
    }

    // ===== 分割そのもの =====

    fn accepted(v: &[(&str, &str)]) -> Vec<AcceptedPoint> {
        v.iter()
            .map(|(q, t)| AcceptedPoint {
                quote: q.to_string(),
                title: t.to_string(),
            })
            .collect()
    }

    #[test]
    fn splitting_never_changes_the_text() {
        // **これが一番大事**。分割して連結したら元に戻ること
        let b = body();
        let segs = segments(
            &b,
            "転校初日",
            &accepted(&[
                ("　その日の放課後、図書室は静かだった。", "図書室"),
                ("　翌朝、廊下で悠二とすれ違った。", "翌朝"),
            ]),
        )
        .unwrap();
        assert_eq!(segs.len(), 3);
        assert_eq!(
            segs.iter().map(|s| s.text.as_str()).collect::<String>(),
            b,
            "分割で本文が変わった"
        );
        assert_eq!(
            segs.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(),
            vec!["転校初日", "図書室", "翌朝"]
        );
        assert!(segs.iter().all(|s| s.chars > 0));
    }

    #[test]
    fn split_order_does_not_depend_on_input_order() {
        let b = body();
        let a = segments(
            &b,
            "1",
            &accepted(&[
                ("　翌朝、廊下で悠二とすれ違った。", "3"),
                ("　その日の放課後、図書室は静かだった。", "2"),
            ]),
        )
        .unwrap();
        assert_eq!(
            a.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(),
            vec!["1", "2", "3"]
        );
    }

    #[test]
    fn fails_loudly_when_the_text_moved_on() {
        // 提案を見てから本文が変わった場合。**ずれた位置で切るより失敗させる**
        let b = body();
        let err = segments(&b, "1", &accepted(&[("もう存在しない一文である", "2")])).unwrap_err();
        assert!(err.contains("見つかりません"), "{err}");
    }

    #[test]
    fn nothing_accepted_is_an_error_not_an_empty_split() {
        let b = body();
        assert!(segments(&b, "1", &[]).is_err());
    }

    #[test]
    fn file_names_keep_the_original_and_add_a_number() {
        assert_eq!(
            file_name_for("manuscript/01-転校初日.md", 0),
            "manuscript/01-転校初日-1.md"
        );
        assert_eq!(
            file_name_for("manuscript/01-転校初日.md", 2),
            "manuscript/01-転校初日-3.md"
        );
        assert_eq!(file_name_for("単体.md", 0), "単体-1.md");
    }

    #[test]
    fn prompt_forbids_rewriting_the_text() {
        // プロダクトの境界(AIは本文を書かない)を、分割の依頼からも崩さない
        let p = build_prompt("本文");
        assert!(p.contains("本文の文章そのものは書き換えないでください"));
        assert!(p.contains("切りすぎないでください"));
        assert!(SYSTEM_PROMPT.contains("書き換えたり要約したりしません"));
    }
}
