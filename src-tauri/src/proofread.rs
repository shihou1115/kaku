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
//!     (一部だけ誤記されている状態は、典型的な誤変換のかたち)

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
        if known.contains(&word) {
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

// ===== M-04-01 誤字脱字チェック(LLM) =====
//
// 表記ゆれ(上記)と違い、ここはLLMに頼らざるを得ない。
// ただし**判定材料はできるだけ機械側で用意する**:
//  - codexの登録名を渡し、固有名詞を「誤字」と誤判定させない
//  - 引用された箇所が本文に実在するかを機械側で照合する(幻覚の除去)

/// 1件の指摘。位置は保持せず、引用文字列で本文を検索して解決する(D-7)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiIssue {
    /// 本文からの引用(そのままの表記)
    pub quote: String,
    /// 直した形
    pub suggestion: String,
    /// 誤字 / 脱字 / 衍字 / 変換ミス / その他
    pub kind: String,
    pub reason: String,
    /// 引用が本文中に見つかったか。見つからないものは幻覚の疑いが強い
    pub found: bool,
    /// 見つかった場合の位置(UTF-16)。保存はしない
    pub start_utf16: Option<usize>,
    pub end_utf16: Option<usize>,
}

/// 1回のリクエストで送る本文の長さ(文字数)。
///
/// PoC#7(2026-07-26)の実測: **長文になると検知率が有意に低下し、3000字程度までなら
/// 実用に耐える**。単純に切り捨てると未検査部分が生まれるため、この長さで分割して
/// 順に検査する。「単純な切り捨てから始め、必要になってから高度化する」の
/// 高度化の条件(実測による裏づけ)が揃ったための変更。
pub const CHUNK_CHARS: usize = 3_000;

/// 1回の実行で検査する塊の上限。
/// ローカルLLMは30トークン/秒程度なので、際限なく投げると待ち時間が現実的でなくなる
pub const MAX_CHUNKS: usize = 4;

/// 1回の実行で検査できる最大の文字数
pub const MAX_CHECK_CHARS: usize = CHUNK_CHARS * MAX_CHUNKS;

/// 本文を検査単位に分ける。**行の途中では切らない**(文が割れると検知精度が落ちる)。
///
/// 1行が長すぎる場合だけ、やむを得ず文字数で切る。
/// `chunk_chars` は設定可能(既定 CHUNK_CHARS)。コンテキストに余裕がある環境では
/// 大きくした方が速い(所要時間は本文長よりも実行回数で決まる。§7.2)。
pub fn split_for_check_with(body: &str, chunk_chars: usize) -> Vec<String> {
    let chunk_chars = chunk_chars.max(100);
    if body.trim().is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for line in body.split_inclusive('\n') {
        let line_len = line.chars().count();

        // 1行だけで上限を超える場合は、その行を文字数で分割する
        if line_len > chunk_chars {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
                current_len = 0;
            }
            let mut buf = String::new();
            let mut n = 0usize;
            for c in line.chars() {
                buf.push(c);
                n += 1;
                if n == chunk_chars {
                    chunks.push(std::mem::take(&mut buf));
                    n = 0;
                }
            }
            if !buf.is_empty() {
                current = buf;
                current_len = n;
            }
            continue;
        }

        if current_len + line_len > chunk_chars && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_len = 0;
        }
        current.push_str(line);
        current_len += line_len;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// 既定の分割字数で分ける
pub fn split_for_check(body: &str) -> Vec<String> {
    split_for_check_with(body, CHUNK_CHARS)
}

/// 同じ指摘の重複を落とす(塊をまたいで同じ語が指摘されることがある)
pub fn dedupe_issues(issues: Vec<AiIssue>) -> Vec<AiIssue> {
    // **位置も鍵に入れる。** 別々の箇所にある同じ誤字は別の指摘で、1つに潰すと
    // 2つ目が黙って見落とされる(位置が付く前なら従来どおり引用と提案だけで比べる)
    let mut seen: Vec<(String, String, Option<usize>)> = Vec::new();
    let mut out = Vec::new();
    for i in issues {
        let key = (i.quote.clone(), i.suggestion.clone(), i.start_utf16);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(i);
    }
    out
}

/// 構造化出力(経路A)のスキーマ
pub fn issue_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "json_schema",
        "json_schema": {
            "name": "proofread_result",
            "strict": true,
            "schema": {
                "type": "object",
                "properties": {
                    "issues": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "quote": { "type": "string" },
                                "suggestion": { "type": "string" },
                                "kind": { "type": "string" },
                                "reason": { "type": "string" }
                            },
                            "required": ["quote", "suggestion", "kind", "reason"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["issues"],
                "additionalProperties": false
            }
        }
    })
}

/// 小説の本文であることを前提にした校正プロンプト(M-04-04)。
///
/// 意図的な崩しや会話文の口語を「誤り」と言わせないことが要点。
pub fn build_prompt(body: &str, names: &[String]) -> String {
    let mut s = String::new();
    s.push_str(
        "次の日本語の小説本文から、**明らかな誤字・脱字・衍字(余分な文字)・変換ミス**だけを抜き出してください。\n\n\
         守ること:\n\
         - 小説の本文です。会話文の口語、方言、意図的なひらがな表記、体言止め、倒置、\n  三点リーダや棒線の使い方は**誤りではありません**。指摘しないでください。\n\
         - 文体や表現の good/bad は評価しないでください。ここでは誤記だけを見ます。\n\
         - **「｜漢字《かんじ》」はルビ(振り仮名)の記法です。誤字ではありません。**\n  \
           記号を消したり、読みを本文の一部として直そうとしないでください。\n\
         - 迷ったら指摘しないでください。**確実なものだけ**を挙げます。\n\
         - quote には本文に現れる文字列を**そのまま**書き写してください(前後を変えない)。\n\
         - 誤りが無ければ空の一覧を返してください。\n\n",
    );
    if !names.is_empty() {
        s.push_str(
            "次の語はこの作品の固有名詞です。**正しい表記なので誤字として指摘しないでください**:\n",
        );
        for n in names.iter().take(80) {
            s.push_str("- ");
            s.push_str(n);
            s.push('\n');
        }
        s.push('\n');
    }
    s.push_str("出力は次のJSONだけを返してください(説明文は不要です):\n");
    s.push_str(
        "{\"issues\":[{\"quote\":\"本文からの引用\",\"suggestion\":\"直した形\",\"kind\":\"誤字|脱字|衍字|変換ミス\",\"reason\":\"理由\"}]}\n\n",
    );
    s.push_str("--- 本文ここから ---\n");
    s.push_str(body);
    s.push_str("\n--- 本文ここまで ---\n");
    s
}

/// 応答から指摘を取り出す(経路A/B共通の寛容パース)。
///
/// スキーマ強制が効かないモデルでも拾えるよう、次を受け入れる:
///  - <code>```json</code> で囲まれたもの
///  - {"issues":[...]} / {"items":[...]} / 素の配列
///  - kind や reason の欠落
pub fn parse_ai_issues(raw: &str) -> Vec<AiIssue> {
    let blob = crate::ai::extract_json_blob(raw);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(blob) else {
        return Vec::new();
    };
    let array = value
        .get("issues")
        .or_else(|| value.get("items"))
        .or_else(|| value.get("results"))
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
            let suggestion = item
                .get("suggestion")
                .or_else(|| item.get("fixed"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let kind = item
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("その他")
                .trim()
                .to_string();
            let reason = item
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            Some(AiIssue {
                quote,
                suggestion,
                kind: if kind.is_empty() { "その他".into() } else { kind },
                reason,
                found: false,
                start_utf16: None,
                end_utf16: None,
            })
        })
        .collect()
}

/// 応答が「指定した形」として読み取れたか。
///
/// **`false` と「誤りなし」は違う。** 前者はモデルが形式を守らなかった状態
/// (散文で講評を返す等)で、中身の有無は分からない。これを「誤字なし」と
/// 報告するのが校正で最悪の失敗なので、呼び出し側が区別できるようにする
/// (M-05の実装で同じ穴が見つかったため、こちらにも入れた)。
pub fn looks_structured(raw: &str) -> bool {
    crate::ai::has_list(raw, &["issues", "items", "results"])
}

/// 引用が本文に実在するかを照合し、位置を埋める。
///
/// 見つからない指摘は幻覚の疑いが強いので `found=false` にして後ろへ回す
/// (消しはしない。モデルの癖を見えるようにしておく)。
pub fn resolve_issues(body: &str, issues: Vec<AiIssue>) -> Vec<AiIssue> {
    let mut issues = resolve_issues_in(body, 0, body, issues);
    order_issues(&mut issues);
    issues
}

/// 1つの塊から返った指摘に、本文全体での位置を付ける。**探すのはその塊の中だけ。**
///
/// 本文全体の最初の一致に当てると、別の塊にある同じ文字列(正しい用法のことが多い)を
/// 指してしまい、「置換」が正しい文を書き換える(「それ以外と比べて」と「以外と簡単」。
/// テスト計画 A4)。モデルは自分の塊しか見ていないので、引用は塊の中にあるはずで、
/// 無ければ取り違え(見つからない)として扱う。
/// 同じ塊の中に複数あるときは最初の1つ(04-design §6.4 の割り切り)。
///
/// `chunk_start` は塊が本文のどこ(バイト位置)から始まるか。
pub fn resolve_issues_in(
    body: &str,
    chunk_start: usize,
    chunk: &str,
    mut issues: Vec<AiIssue>,
) -> Vec<AiIssue> {
    let to_utf16 = Utf16Map::new(body);
    for issue in issues.iter_mut() {
        if issue.quote.is_empty() {
            continue;
        }
        if let Some(pos) = chunk.find(&issue.quote) {
            let at = chunk_start + pos;
            issue.found = true;
            issue.start_utf16 = Some(to_utf16.at(at));
            issue.end_utf16 = Some(to_utf16.at(at + issue.quote.len()));
        }
    }
    // 変更のない提案(quote == suggestion)は指摘として意味がないので落とす
    issues.retain(|i| i.suggestion != i.quote);
    issues
}

/// 見つかったものを本文の順に、見つからないものを後ろに並べる
pub fn order_issues(issues: &mut [AiIssue]) {
    issues.sort_by_key(|i| (!i.found, i.start_utf16.unwrap_or(usize::MAX)));
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

    // ===== M-04-01(LLM側)のパースと照合 =====

    #[test]
    fn parses_structured_response() {
        let raw = r#"{"issues":[{"quote":"昇降口","suggestion":"昇降口で","kind":"脱字","reason":"助詞が抜けています"}]}"#;
        let got = parse_ai_issues(raw);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].quote, "昇降口");
        assert_eq!(got[0].kind, "脱字");
    }

    #[test]
    fn parses_fenced_json() {
        let raw = "```json\n{\"issues\":[{\"quote\":\"雨だつた\",\"suggestion\":\"雨だった\",\"kind\":\"誤字\",\"reason\":\"促音\"}]}\n```";
        assert_eq!(parse_ai_issues(raw).len(), 1);
    }

    #[test]
    fn parses_bare_array_and_alternate_keys() {
        // スキーマ強制が効かないモデルの揺れを吸収する
        let raw = r#"[{"text":"つずく","fixed":"つづく"}]"#;
        let got = parse_ai_issues(raw);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].quote, "つずく");
        assert_eq!(got[0].suggestion, "つづく");
        assert_eq!(got[0].kind, "その他");
    }

    #[test]
    fn broken_response_yields_nothing_instead_of_panicking() {
        assert!(parse_ai_issues("すみません、見つかりませんでした").is_empty());
        assert!(parse_ai_issues("").is_empty());
        assert!(parse_ai_issues("{\"issues\":").is_empty());
    }

    #[test]
    fn prose_response_is_not_mistaken_for_no_typos() {
        // 形式を守らない応答を「誤りなし」と報告しないための区別
        assert!(!looks_structured("The writing can be improved by tightening the prose."));
        assert!(!looks_structured(""));
        assert!(!looks_structured(r#"{"summary":"問題ありません"}"#));
        // 空の一覧は「読み取れたうえで誤りなし」なので true
        assert!(looks_structured(r#"{"issues":[]}"#));
        assert!(looks_structured("```json\n{\"issues\":[]}\n```"));
    }

    #[test]
    fn empty_quote_is_dropped() {
        let raw = r#"{"issues":[{"quote":"  ","suggestion":"x","kind":"誤字","reason":""}]}"#;
        assert!(parse_ai_issues(raw).is_empty());
    }

    #[test]
    fn resolves_quote_position_in_body() {
        let body = "転校初日の朝は、雨だつた。";
        let issues = parse_ai_issues(
            r#"{"issues":[{"quote":"雨だつた","suggestion":"雨だった","kind":"誤字","reason":"促音"}]}"#,
        );
        let got = resolve_issues(body, issues);
        assert_eq!(got.len(), 1);
        assert!(got[0].found);
        assert_eq!(got[0].start_utf16, Some(8));
        assert_eq!(got[0].end_utf16, Some(12));
    }

    #[test]
    fn hallucinated_quote_is_marked_not_found_and_sorted_last() {
        let body = "転校初日の朝は、雨だつた。";
        let issues = parse_ai_issues(
            r#"{"issues":[
                {"quote":"存在しない文","suggestion":"直した文","kind":"誤字","reason":""},
                {"quote":"雨だつた","suggestion":"雨だった","kind":"誤字","reason":""}
            ]}"#,
        );
        let got = resolve_issues(body, issues);
        assert_eq!(got.len(), 2);
        // 本文に実在するものが先、幻覚は後ろ
        assert!(got[0].found);
        assert_eq!(got[0].quote, "雨だつた");
        assert!(!got[1].found);
    }

    #[test]
    fn no_op_suggestion_is_dropped() {
        // 直っていない提案は指摘として意味がない
        let body = "架純が来た。";
        let issues = parse_ai_issues(
            r#"{"issues":[{"quote":"架純","suggestion":"架純","kind":"誤字","reason":"?"}]}"#,
        );
        assert!(resolve_issues(body, issues).is_empty());
    }

    #[test]
    fn split_keeps_short_body_whole() {
        let body = "　転校初日の朝は、雨だった。\n　昇降口で靴を履き替える。\n";
        let chunks = split_for_check(body);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], body);
    }

    #[test]
    fn split_breaks_at_line_boundaries() {
        // 1行1200字 × 4行。3000字上限なので 2行ずつに割れる
        let line = format!("{}\n", "あ".repeat(1199));
        let body = line.repeat(4);
        let chunks = split_for_check(&body);
        assert_eq!(chunks.len(), 2);
        for c in &chunks {
            assert!(c.chars().count() <= CHUNK_CHARS, "上限超過: {}", c.chars().count());
            // 行の途中で切れていないこと
            assert!(c.ends_with('\n'));
        }
        assert_eq!(chunks.concat(), body, "分割で本文が変わってはいけない");
    }

    #[test]
    fn split_handles_single_long_line() {
        // 改行が無い長文でも落ちない(やむを得ず文字数で切る)
        let body = "あ".repeat(7_000);
        let chunks = split_for_check(&body);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks.concat(), body);
        assert!(chunks.iter().all(|c| c.chars().count() <= CHUNK_CHARS));
    }

    #[test]
    fn split_ignores_empty_body() {
        assert!(split_for_check("").is_empty());
        assert!(split_for_check("   \n  ").is_empty());
    }

    #[test]
    fn split_respects_custom_chunk_size() {
        // 設定で分割字数を変えられること(§7.2: 環境によって最適値が違う)
        let body = format!("{}\n", "あ".repeat(999)).repeat(6); // 約6000字
        assert_eq!(split_for_check_with(&body, 1_000).len(), 6);
        assert_eq!(split_for_check_with(&body, 3_000).len(), 2);
        assert_eq!(split_for_check_with(&body, 6_000).len(), 1);
        // どの分割でも本文は変わらない
        for size in [1_000, 3_000, 6_000] {
            assert_eq!(split_for_check_with(&body, size).concat(), body);
        }
    }

    #[test]
    fn dedupe_drops_the_same_issue_at_the_same_place() {
        let issues = parse_ai_issues(
            r#"{"issues":[
                {"quote":"雨だつた","suggestion":"雨だった","kind":"誤字","reason":"a"},
                {"quote":"雨だつた","suggestion":"雨だった","kind":"誤字","reason":"b"},
                {"quote":"つずく","suggestion":"つづく","kind":"誤字","reason":"c"}
            ]}"#,
        );
        let got = dedupe_issues(issues);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].reason, "a", "先に出たものを残す");
    }

    /// 同じ誤字でも**箇所が違えば**別の指摘として残す(潰すと2つ目を黙って見落とす)
    #[test]
    fn dedupe_keeps_the_same_typo_at_different_places() {
        let body = "雨だつた。\n翌日も雨だつた。\n";
        let one = || {
            parse_ai_issues(r#"{"issues":[{"quote":"雨だつた","suggestion":"雨だった","kind":"誤字","reason":"r"}]}"#)
        };
        let first_line = &body[..body.find('\n').unwrap() + 1];
        let mut all = resolve_issues_in(body, 0, first_line, one());
        all.extend(resolve_issues_in(body, first_line.len(), &body[first_line.len()..], one()));
        assert_eq!(dedupe_issues(all).len(), 2);
    }

    /// 塊を前からつなぐと本文に戻ること(長い1行を文字で割る場合も)。
    /// 校正の指摘の位置は、塊の始まりを前から数えて決めている(テスト計画 A4)
    #[test]
    fn chunks_are_consecutive_slices_of_the_body() {
        let long_line = "𠮷と葛\u{E0100}城の話。".repeat(150);
        let body = format!("短い行\n{long_line}\nまた短い行\n{}", "あ".repeat(250));
        for size in [100, 300, 1_000] {
            assert_eq!(split_for_check_with(&body, size).concat(), body, "{size}");
        }
    }

    #[test]
    fn prompt_includes_names_and_body() {
        let p = build_prompt("　雨だつた。", &names(&["佐藤架純"]));
        assert!(p.contains("佐藤架純"), "固有名詞を渡していない");
        assert!(p.contains("雨だつた"), "本文が入っていない");
        // 会話文や意図的な崩しを指摘させない指示が入っていること(M-04-04)
        assert!(p.contains("会話文"));
        assert!(p.contains("迷ったら指摘しないでください"));
        // ルビ記法を誤字として指摘させない(2026-08-04 未決-3の確定にともなう)
        assert!(p.contains("ルビ"), "ルビ記法の断りが無い");
        assert!(p.contains("｜漢字《かんじ》"));
    }

    #[test]
    fn schema_declares_required_fields() {
        let s = issue_schema();
        let req = &s["json_schema"]["schema"]["properties"]["issues"]["items"]["required"];
        assert!(req.to_string().contains("quote"));
        assert!(req.to_string().contains("suggestion"));
    }

    #[test]
    fn long_runs_are_bounded() {
        // 極端に長い連なりは名前ではないので候補にしない
        let long: String = "漢".repeat(30);
        let text = format!("{long}が続く。");
        assert!(check_notation(&text, &names(&["漢字表記"])).is_empty());
    }
}
