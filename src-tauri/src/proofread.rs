//! 設定DB連携の固有名詞チェック(M-04-02)。
//!
//! **これが既存の校正ツールとの差別化点**。一般の校正ツールは「架純」と「佳純」の
//! どちらが正しいか知らないが、本アプリは codex に正式名と別名を持っているため、
//! 登録された名前と1文字違いの語を「表記ゆれの疑い」として示せる。
//!
//! 原則(docs/06-decision-log.md §2-5): **機械照合が主、LLMは解釈補助**。
//! ここは決定的なアルゴリズムだけで完結させ、LLMには一切依存しない。
//!
//! 誤検出を抑える設計(M-04-04)。次の条件を積み重ねて候補を絞る:
//!  1. 漢字・カタカナだけで構成された2文字以上の語を候補にする
//!     (ひらがなを含めると助詞混じりの語が大量に当たるため、V1では対象外)
//!  2. 登録名と完全一致する語、**登録名を部分に含む語は除く**
//!     (「青葉高校生」は「青葉高校」の誤記ではなく複合語)
//!  3. 登録名との編集距離がちょうど1のものだけを採る
//!  4. 正しい名前が同じ本文中にも出ていれば確度「高」、出ていなければ「中」
//!     (一部だけ誤記されている状態は、типичный な誤変換のかたち)

use serde::Serialize;

use crate::mentions::Utf16Map;

/// 候補として切り出す語の長さ(文字数)
const MIN_LEN: usize = 2;
const MAX_LEN: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Span {
    pub start_utf16: usize,
    pub end_utf16: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotationHit {
    /// 本文に出てきた疑わしい語
    pub candidate: String,
    /// 正しいと思われる登録名
    pub suggestion: String,
    /// "high" = 正しい名前も本文中にある / "medium" = 本文中には無い
    pub confidence: String,
    /// なぜそう判断したかを日本語で(UIにそのまま出す)
    pub reason: String,
    /// 本文での出現位置(UTF-16。永続化はしない)
    pub occurrences: Vec<Span>,
    pub candidate_count: usize,
    pub suggestion_count: usize,
}

/// 名前に使われうる文字か。ひらがなは含めない(§冒頭の理由)
fn is_name_char(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   // CJK統合漢字
        | '\u{3400}'..='\u{4DBF}' // 拡張A
        | '\u{30A1}'..='\u{30FA}' // カタカナ
        | '\u{30FC}'              // 長音符
        | '\u{3005}'              // 々
        | '\u{3006}'              // 〆
    )
}

/// 本文から候補語(漢字・カタカナの連なり)を位置つきで切り出す
fn candidates(text: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut start = 0usize;
    for (idx, c) in text.char_indices() {
        if is_name_char(c) {
            if buf.is_empty() {
                start = idx;
            }
            buf.push(c);
        } else if !buf.is_empty() {
            out.push((std::mem::take(&mut buf), start));
        }
    }
    if !buf.is_empty() {
        out.push((buf, start));
    }
    out.retain(|(s, _)| {
        let n = s.chars().count();
        (MIN_LEN..=MAX_LEN).contains(&n)
    });
    out
}

/// 編集距離。長さの差が1を超える場合は早期に打ち切る(2以上は見ない)
fn edit_distance_le_1(a: &[char], b: &[char]) -> Option<usize> {
    let (la, lb) = (a.len(), b.len());
    if la.abs_diff(lb) > 1 {
        return None;
    }
    if la == lb {
        let diff = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
        return if diff <= 1 { Some(diff) } else { None };
    }
    // 長さが1違う: 短い方が長い方の1文字削除で作れるか
    let (short, long) = if la < lb { (a, b) } else { (b, a) };
    let mut i = 0;
    let mut skipped = false;
    for (j, lc) in long.iter().enumerate() {
        if i < short.len() && short[i] == *lc {
            i += 1;
        } else if skipped {
            return None;
        } else {
            skipped = true;
            let _ = j;
        }
    }
    Some(1)
}

/// 表記ゆれの疑いを検出する。
///
/// `names` は codex の正式名+別名。並び順は結果に影響しない。
pub fn check_notation(text: &str, names: &[String]) -> Vec<NotationHit> {
    let known: Vec<String> = names
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| s.chars().count() >= MIN_LEN)
        .collect();
    if known.is_empty() || text.is_empty() {
        return Vec::new();
    }
    let known_chars: Vec<Vec<char>> = known.iter().map(|s| s.chars().collect()).collect();
    let to_utf16 = Utf16Map::new(text);

    // 登録名が本文に何回出るか(確度の判定に使う)
    let count_of = |needle: &str| -> usize {
        if needle.is_empty() {
            0
        } else {
            text.matches(needle).count()
        }
    };

    // 候補語 -> 出現位置
    let mut grouped: Vec<(String, Vec<usize>)> = Vec::new();
    for (word, byte_pos) in candidates(text) {
        // 登録名そのものは対象外
        if known.iter().any(|k| *k == word) {
            continue;
        }
        match grouped.iter_mut().find(|(w, _)| *w == word) {
            Some((_, positions)) => positions.push(byte_pos),
            None => grouped.push((word, vec![byte_pos])),
        }
    }

    let mut hits = Vec::new();
    for (word, positions) in grouped {
        let wchars: Vec<char> = word.chars().collect();
        // 最も近い登録名を1つだけ選ぶ(複数該当時は先に登録された方)
        let Some(idx) = known_chars
            .iter()
            .position(|k| edit_distance_le_1(&wchars, k) == Some(1))
        else {
            continue;
        };
        let suggestion = known[idx].clone();

        // 登録名に文字が足されただけの語は複合語であって誤記ではない。
        // 例:「青葉高校」+生 =「青葉高校生」、「悠二」+郎 =「悠二郎」。
        //
        // 逆に「五十悠二」は、より長い登録名「五十嵐悠二」から1文字落ちた形で、
        // 登録名そのものは含まれていない。こちらは脱字として拾う必要がある。
        // 「登録名を含む語は一律で除く」としてしまうと後者を取りこぼす。
        if word.contains(&suggestion) {
            continue;
        }

        let suggestion_count = count_of(&suggestion);
        let (confidence, reason) = if suggestion_count > 0 {
            (
                "high",
                format!(
                    "「{suggestion}」が本文に{suggestion_count}回出ています。1文字違いのため誤変換の可能性があります"
                ),
            )
        } else {
            (
                "medium",
                format!("設定に登録された「{suggestion}」と1文字違いです"),
            )
        };

        let occurrences = positions
            .iter()
            .map(|&p| Span {
                start_utf16: to_utf16.at(p),
                end_utf16: to_utf16.at(p + word.len()),
            })
            .collect::<Vec<_>>();

        hits.push(NotationHit {
            candidate_count: occurrences.len(),
            candidate: word,
            suggestion,
            confidence: confidence.to_string(),
            reason,
            occurrences,
            suggestion_count,
        });
    }

    // 確度の高い順、次に出現位置の早い順で並べる。
    // 文字列の辞書順に頼らない("high" < "medium" で意図と逆になる)
    fn rank(confidence: &str) -> u8 {
        match confidence {
            "high" => 0,
            _ => 1,
        }
    }
    hits.sort_by(|a, b| {
        rank(&a.confidence).cmp(&rank(&b.confidence)).then_with(|| {
            a.occurrences
                .first()
                .map(|s| s.start_utf16)
                .cmp(&b.occurrences.first().map(|s| s.start_utf16))
        })
    });
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn detects_wrong_kanji_in_name() {
        let text = "架純は昇降口に立っていた。佳純は振り返らなかった。";
        let hits = check_notation(text, &names(&["架純", "悠二"]));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].candidate, "佳純");
        assert_eq!(hits[0].suggestion, "架純");
        // 正しい名前も本文にあるので確度は高
        assert_eq!(hits[0].confidence, "high");
        assert_eq!(hits[0].suggestion_count, 1);
    }

    #[test]
    fn reports_position_in_utf16() {
        let text = "架純と佳純";
        let hits = check_notation(text, &names(&["架純"]));
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].occurrences,
            vec![Span {
                start_utf16: 3,
                end_utf16: 5
            }]
        );
    }

    #[test]
    fn compound_word_is_not_a_typo() {
        // 「青葉高校生」は「青葉高校」の誤記ではない。登録名を含む語は除く
        let text = "青葉高校生が集まる。";
        let hits = check_notation(text, &names(&["青葉高校"]));
        assert!(hits.is_empty(), "複合語を誤検出した: {hits:?}");
    }

    #[test]
    fn exact_name_is_not_reported() {
        let text = "架純が来た。架純が笑った。";
        assert!(check_notation(text, &names(&["架純"])).is_empty());
    }

    #[test]
    fn two_char_difference_is_ignored() {
        // 距離2以上は誤検出のもとなので拾わない
        let text = "佳澄が来た。";
        assert!(check_notation(text, &names(&["架純"])).is_empty());
    }

    #[test]
    fn detects_missing_or_extra_character() {
        // 脱字(五十嵐悠二 → 五十悠二)
        let text = "五十嵐悠二と五十悠二。";
        let hits = check_notation(text, &names(&["五十嵐悠二"]));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].candidate, "五十悠二");
        assert_eq!(hits[0].confidence, "high");
    }

    #[test]
    fn detects_dropped_char_even_when_a_shorter_name_is_registered() {
        // 「五十悠二」は登録名「悠二」を含むが、実際は「五十嵐悠二」の脱字。
        // 「登録名を含む語は除く」という単純な規則では取りこぼす
        let text = "五十嵐悠二と五十悠二。";
        let hits = check_notation(text, &names(&["五十嵐悠二", "悠二"]));
        assert_eq!(hits.len(), 1, "取りこぼした: {hits:?}");
        assert_eq!(hits[0].candidate, "五十悠二");
        assert_eq!(hits[0].suggestion, "五十嵐悠二");
    }

    #[test]
    fn name_with_extra_char_is_treated_as_compound() {
        // 「悠二郎」は「悠二」に文字が足された別語。誤記として扱わない
        let text = "悠二と悠二郎は別人だ。";
        assert!(check_notation(text, &names(&["悠二"])).is_empty());
    }

    #[test]
    fn exact_name_is_never_reported_even_if_similar_name_exists() {
        // 「悠二」は登録名そのもの。よく似た別の登録名があっても指摘しない
        let text = "悠二が来た。";
        assert!(check_notation(text, &names(&["悠二", "悠三"])).is_empty());
    }

    #[test]
    fn medium_confidence_when_correct_name_absent() {
        // 本文には正しい名前が一度も出ない場合。確度を下げて報告する
        let text = "佳純だけがいた。";
        let hits = check_notation(text, &names(&["架純"]));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].confidence, "medium");
        assert_eq!(hits[0].suggestion_count, 0);
    }

    #[test]
    fn groups_repeated_candidate() {
        let text = "架純。佳純。佳純。";
        let hits = check_notation(text, &names(&["架純"]));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].candidate_count, 2);
        assert_eq!(hits[0].occurrences.len(), 2);
    }

    #[test]
    fn hiragana_is_out_of_scope() {
        // ひらがなは助詞混じりで誤検出が多いためV1では候補にしない
        let text = "かすみんとかすみが歩く。";
        assert!(check_notation(text, &names(&["かすみん"])).is_empty());
    }

    #[test]
    fn katakana_names_are_covered() {
        let text = "ウルスラとウルスナが並ぶ。";
        let hits = check_notation(text, &names(&["ウルスラ"]));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].candidate, "ウルスナ");
    }

    #[test]
    fn high_confidence_comes_first() {
        let text = "架純と佳純。ウルスナだけ。";
        let hits = check_notation(text, &names(&["架純", "ウルスラ"]));
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].confidence, "high");
        assert_eq!(hits[1].confidence, "medium");
    }

    #[test]
    fn empty_inputs_are_safe() {
        assert!(check_notation("", &names(&["架純"])).is_empty());
        assert!(check_notation("架純", &names(&[])).is_empty());
        // 1文字の登録名は候補が広がりすぎるので無視する
        assert!(check_notation("木と本", &names(&["木"])).is_empty());
    }

    #[test]
    fn long_runs_are_bounded() {
        // 極端に長い連なりは名前ではないので候補にしない
        let long: String = "漢".repeat(30);
        let text = format!("{long}が続く。");
        assert!(check_notation(&text, &names(&["漢字表記"])).is_empty());
    }
}
