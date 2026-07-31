//! シーン/章のレビュー(M-05)。
//!
//! **校正(proofread.rs)と同じ骨格を写している**: プロンプト構築 / スキーマ /
//! 寛容パース / 引用の実在照合。新しい仕組みは足していない
//! (docs/05-roadmap.md §5.13「新しい設計は足さない」)。
//!
//! 校正と違うのは次の3点だけ:
//!
//! 1. **観点が5分類で固定**されている(docs/02-requirements.md M-05)。
//!    プロンプトはこの分類単位で管理し、勝手に増減させない
//! 2. 出力に**全体講評**が付く。引用付きコメントだけでは「編集者」の仕事にならない
//! 3. **失敗の形が違う**。校正で最悪なのは打ち切りを「指摘なし」と誤報告することだが、
//!    レビューではそれに加えて**道徳的な注釈・説教の混入**がある
//!    (docs/04-design.md §8.1)。拒否と違って動いてしまうので気づきにくく、
//!    「編集者として使えない出力」になる。システムプロンプトで明示的に禁じる
//!
//! V1の範囲(docs/05-roadmap.md §4): シーン/章(〜数章)まで。作品全体レビューはV1.5。

use serde::Serialize;

use crate::mentions::Utf16Map;

/// レビューの観点。**5分類で固定**(02-requirements.md M-05、06-decision-log.md §3)。
///
/// 増やさない。増やしたくなったら、それは既存のどれかの具体例であることが多い。
pub struct Aspect {
    /// 出力・設定で使う識別子
    pub key: &'static str,
    /// 画面に出す名前
    pub label: &'static str,
    /// プロンプトで「何を見るか」を伝える説明
    pub guide: &'static str,
}

pub const ASPECTS: &[Aspect] = &[
    Aspect {
        key: "style",
        label: "文章品質",
        guide: "文体の乱れ、冗長な言い回し、読みにくい構文、同じ語や語尾の繰り返し",
    },
    Aspect {
        key: "structure",
        label: "構成・テンポ",
        guide: "場面の要否、出来事の順序、説明と描写の配分、間延びしている箇所と急ぎすぎている箇所",
    },
    Aspect {
        key: "character",
        label: "キャラクター",
        guide: "言動の一貫性、動機の説得力、口調・一人称のぶれ、その人物である必然性",
    },
    Aspect {
        key: "consistency",
        label: "設定整合性",
        guide: "設定資料との食い違い(名前・外見・関係・時系列・固有名詞の表記)",
    },
    Aspect {
        key: "reader",
        label: "読者視点",
        guide: "初読で分かりにくい箇所、退屈しそうな箇所、次を読みたくなるかどうか",
    },
];

pub fn aspect_label(key: &str) -> &str {
    ASPECTS
        .iter()
        .find(|a| a.key == key)
        .map(|a| a.label)
        .unwrap_or(key)
}

/// 指定された観点を解決する。空・未知しか無い場合は全観点に落とす
/// (「観点を1つも選ばずに実行して何も返らない」より、既定で全部見る方が親切)。
pub fn selected_aspects(keys: &[String]) -> Vec<&'static Aspect> {
    let picked: Vec<&'static Aspect> = ASPECTS
        .iter()
        .filter(|a| keys.iter().any(|k| k == a.key))
        .collect();
    if picked.is_empty() {
        ASPECTS.iter().collect()
    } else {
        picked
    }
}

/// LLMが返した観点名を5分類へ丸める。
///
/// 分類できないものは捨てずに「文章品質」へ寄せる。**捨てると指摘そのものが
/// 消えてしまい、レビューで最も避けたい「取りこぼし」になる**。
/// 6つ目の分類は作らない(02 M-05: 勝手に増減させない)。
pub fn normalize_aspect(raw: &str) -> String {
    let k = raw.trim();
    for a in ASPECTS {
        if k == a.key || k == a.label {
            return a.key.to_string();
        }
    }
    match k {
        "文体" | "表現" | "文章" | "prose" | "writing" => "style",
        "構成" | "テンポ" | "展開" | "pacing" | "plot" | "structure・pacing" => "structure",
        "人物" | "キャラ" | "characters" | "character consistency" => "character",
        "設定" | "整合性" | "矛盾" | "setting" | "continuity" => "consistency",
        "読者" | "読者体験" | "readability" | "reader experience" => "reader",
        _ => "style",
    }
    .to_string()
}

/// 1件の指摘。**位置は保持せず**、引用文字列で本文を検索して解決する(D-7)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewComment {
    /// 5分類のいずれか(キー)
    pub aspect: String,
    /// 本文からの引用(そのままの表記)
    pub quote: String,
    /// 指摘の内容
    pub comment: String,
    /// 直す方向。**書き直した本文ではない**(AIは本文を書かない)
    pub suggestion: String,
    /// 引用が本文中に見つかったか。見つからないものは幻覚の疑いが強い
    pub found: bool,
    /// 見つかった場合の位置(UTF-16)。保存はしない
    pub start_utf16: Option<usize>,
    pub end_utf16: Option<usize>,
}

/// 応答から取り出したもの
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedReview {
    pub comments: Vec<ReviewComment>,
    /// 全体講評(引用に紐づかない講評)
    pub overall: String,
    /// 指定した形として読み取れたか。
    ///
    /// **`false` と「指摘なし」は違う**。前者は形式を守らない応答(散文で返された等)で、
    /// 中身はあるのに読めていない。これを「指摘なし」と報告するのがレビューで最悪の失敗
    /// なので、必ず区別して呼び出し側へ渡す。
    pub structured: bool,
}

/// 読み取れなかった応答をそのまま見せるときの上限(文字)。
/// 思考を垂れ流すモデルがあるため、際限なく画面へ出さない
pub const MAX_RAW_CHARS: usize = 2_000;

/// 形式を守らなかった応答を、そのまま見せられる長さに整える。
/// **捨てない**(捨てると「指摘なし」と区別が付かなくなる)が、切ったことは明示する。
pub fn clip_raw(raw: &str) -> String {
    let t = raw.trim();
    if t.chars().count() <= MAX_RAW_CHARS {
        return t.to_string();
    }
    let head: String = t.chars().take(MAX_RAW_CHARS).collect();
    format!("{head}\n\n(応答が長いため以下略)")
}

/// 引用の照合で許す最小の長さ(文字)。
/// これより短い断片で本文を探すと、無関係な箇所に当たって位置がでたらめになる。
const MIN_QUOTE_CHARS: usize = 8;

/// システムプロンプト。**説教を混ぜさせないことが最大の目的**(docs/04-design.md §8.1)。
///
/// 検閲ありモデルは「生成」を拒否するが、レビューのような分析用途では拒否せず、
/// 代わりに道徳的な注釈を足してくることがある。動いてしまうため気づきにくい。
pub const SYSTEM_PROMPT: &str = "\
あなたは小説の編集者です。原稿を良くするための指摘だけを書きます。

- **題材や登場人物の行いを道徳的に評価しません。** 犯罪・暴力・性の描写は物語の要素として扱い、
  善悪の判断、注意喚起、配慮の助言、作者への説教、免責の断り書きは一切書きません。
- 作者を励ますことや慰めることが目的ではありません。具体的で、直せる形の指摘をします。
- **本文の代筆はしません。** 直し方は方針として示し、地の文や会話文を書き下ろすことはしません。
- **日本語で書きます。** 対象は日本語の小説であり、指摘も日本語で返します。
- **指示された出力形式(JSON)だけを返します。** 前置き・箇条書き・見出しを付けません。";

/// 構造化出力(経路A)のスキーマ。失敗したら経路B(寛容パース)へ落とす(§6.3)
pub fn review_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "json_schema",
        "json_schema": {
            "name": "review_result",
            "strict": true,
            "schema": {
                "type": "object",
                "properties": {
                    "comments": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "aspect": { "type": "string" },
                                "quote": { "type": "string" },
                                "comment": { "type": "string" },
                                "suggestion": { "type": "string" }
                            },
                            "required": ["aspect", "quote", "comment", "suggestion"],
                            "additionalProperties": false
                        }
                    },
                    "overall": { "type": "string" }
                },
                "required": ["comments", "overall"],
                "additionalProperties": false
            }
        }
    })
}

/// レビュー依頼のプロンプト。
///
/// `materials` は設定資料(タイトル, 本文)。コンテキストは3系統だけで組み立てる
/// (docs/04-design.md §6.2)。優先順位・予算・縮退は撤回済み。
pub fn build_prompt(body: &str, aspects: &[&Aspect], materials: &[(String, String)]) -> String {
    let mut s = String::new();
    s.push_str("次の日本語の小説本文を、編集者としてレビューしてください。\n\n");

    s.push_str("見る観点(**これ以外の観点では書かないでください**):\n");
    for a in aspects {
        s.push_str(&format!("- {}({}): {}\n", a.label, a.key, a.guide));
    }
    s.push('\n');

    s.push_str(
        "守ること:\n\
         - quote には**本文に現れる文字列をそのまま書き写して**ください(15〜40字程度)。\n  \
           要約・言い換えをせず、前後も変えないでください。引用できない指摘は overall に書きます\n\
         - 指摘は具体的に書いてください。「もっと良くできる」だけの一般論は不要です\n\
         - 同じ内容を繰り返さないでください。**1つの観点につき多くても3件**まで\n\
         - suggestion には**直す方向**を書いてください。書き直した本文は書かないでください\n\
         - 題材の善悪や社会的な是非には触れないでください。注意喚起・配慮の助言も不要です\n\
         - overall には全体講評を3〜5文で書いてください。良い点と、\n  \
           次に直すと最も効く点を挙げます\n",
    );
    if materials.is_empty() {
        s.push_str(
            "- 設定資料は渡されていません。**設定との矛盾は判断できないため指摘しないでください**\n",
        );
    } else {
        s.push_str(
            "- 設定資料と食い違う点は「設定整合性」として挙げてください。\n  \
               ただし**資料に書かれていないことを矛盾扱いしないでください**(未設定と矛盾は別です)\n",
        );
    }
    s.push('\n');

    let keys = aspects
        .iter()
        .map(|a| a.key)
        .collect::<Vec<_>>()
        .join("|");
    s.push_str("出力は次のJSONだけを返してください(説明文は不要です):\n");
    s.push_str(&format!(
        "{{\"comments\":[{{\"aspect\":\"{keys}\",\"quote\":\"本文からの引用\",\"comment\":\"指摘\",\"suggestion\":\"直す方向\"}}],\"overall\":\"全体講評\"}}\n\n"
    ));

    if !materials.is_empty() {
        s.push_str("--- 設定資料ここから ---\n");
        for (title, text) in materials {
            s.push_str(&format!("## {title}\n{text}\n\n"));
        }
        s.push_str("--- 設定資料ここまで ---\n\n");
    }

    s.push_str("--- 本文ここから ---\n");
    s.push_str(body);
    s.push_str("\n--- 本文ここまで ---\n");
    s
}

/// 応答から指摘と全体講評を取り出す(経路A/B共通の寛容パース)。
///
/// スキーマ強制が効かないモデルでも拾えるよう、次を受け入れる:
///  - Markdownのコードフェンスで囲まれたもの
///  - {"comments":[...]} / {"items":[...]} / 素の配列
///  - overall / summary / 全体講評 のいずれか
///  - aspect や suggestion の欠落
pub fn parse(raw: &str) -> ParsedReview {
    let blob = crate::ai::extract_json_blob(raw);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(blob) else {
        return ParsedReview::default();
    };

    let overall = value
        .get("overall")
        .or_else(|| value.get("summary"))
        .or_else(|| value.get("全体講評"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let array = value
        .get("comments")
        .or_else(|| value.get("issues"))
        .or_else(|| value.get("items"))
        .or_else(|| value.get("results"))
        .and_then(|v| v.as_array())
        .or_else(|| value.as_array());
    let Some(array) = array else {
        return ParsedReview {
            comments: Vec::new(),
            // 講評だけ取れたなら読み取れている。JSONではあるが別物なら読み取れていない
            structured: !overall.is_empty(),
            overall,
        };
    };

    let comments = array
        .iter()
        .filter_map(|item| {
            let quote = item
                .get("quote")
                .or_else(|| item.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let comment = item
                .get("comment")
                .or_else(|| item.get("reason"))
                .or_else(|| item.get("issue"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            // 中身の無い指摘は落とす。引用だけ・空だけのものは情報がない
            if comment.is_empty() {
                return None;
            }
            let suggestion = item
                .get("suggestion")
                .or_else(|| item.get("fix"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let aspect = normalize_aspect(
                item.get("aspect")
                    .or_else(|| item.get("category"))
                    .or_else(|| item.get("kind"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            );
            Some(ReviewComment {
                aspect,
                quote,
                comment,
                suggestion,
                found: false,
                start_utf16: None,
                end_utf16: None,
            })
        })
        .collect();

    // 一覧が空でも、一覧そのものは読めている = 「指摘なし」として正しい
    ParsedReview {
        comments,
        overall,
        structured: true,
    }
}

/// 同じ指摘の重複を落とす(塊をまたいで同じ箇所が指摘されることがある)。
///
/// 引用が同じでも観点が違えば別の指摘なので、キーは(観点, 引用)にする。
/// 引用が無いものは指摘文で見る。
pub fn dedupe(comments: Vec<ReviewComment>) -> Vec<ReviewComment> {
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut out = Vec::new();
    for c in comments {
        let body = if c.quote.is_empty() {
            c.comment.clone()
        } else {
            c.quote.clone()
        };
        let key = (c.aspect.clone(), body);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(c);
    }
    out
}

/// 引用を本文の位置へ対応づける。
///
/// **完全一致 → 前後の約物を落として再検索 → 先頭からの部分一致**まで
/// (docs/04-design.md §6.4「完全一致→部分一致に留める」)。
/// あいまい一致・段落インデックス・失効管理は撤回済みなので作らない。
fn locate(body: &str, quote: &str) -> Option<(usize, usize)> {
    if quote.is_empty() {
        return None;
    }
    if let Some(p) = body.find(quote) {
        return Some((p, p + quote.len()));
    }

    // 引用の前後に鉤括弧や句読点を足す癖があるので、そこだけ剥がして探し直す。
    // これは剥がした後も**完全一致**なので、短い引用でも安全に使える
    let trimmed = quote.trim().trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, '「' | '」' | '『' | '』' | '"' | '\'' | '…' | '。' | '、' | '　')
    });
    if trimmed.chars().count() < 2 {
        return None;
    }
    if trimmed != quote {
        if let Some(p) = body.find(trimmed) {
            return Some((p, p + trimmed.len()));
        }
    }

    // ここから先は部分一致になる。短い断片では別の箇所に当たるので長さで足切りする
    if trimmed.chars().count() < MIN_QUOTE_CHARS {
        return None;
    }
    // 長い引用は末尾だけ言い換えられることがある。一致する先頭部分だけを採る
    let chars: Vec<char> = trimmed.chars().collect();
    let mut take = chars.len();
    while take > MIN_QUOTE_CHARS {
        take -= 1;
        let prefix: String = chars[..take].iter().collect();
        if let Some(p) = body.find(&prefix) {
            return Some((p, p + prefix.len()));
        }
    }
    None
}

/// 引用が本文に実在するかを照合し、位置を埋める(**幻覚を機械で落とす**)。
///
/// 見つからないものは消さずに `found=false` で後ろへ回す。
/// 消すとモデルの癖が見えなくなり、「レビューは当たっている」という誤解を生む。
///
/// 並び順は 本文に見つかったもの(出現順) → 引用なし → 見つからないもの。
pub fn resolve(body: &str, mut comments: Vec<ReviewComment>) -> Vec<ReviewComment> {
    let to_utf16 = Utf16Map::new(body);
    for c in comments.iter_mut() {
        if let Some((start, end)) = locate(body, &c.quote) {
            c.found = true;
            c.start_utf16 = Some(to_utf16.at(start));
            c.end_utf16 = Some(to_utf16.at(end));
        }
    }
    // 0 = 本文に見つかった / 1 = 引用なし(範囲全体への指摘) / 2 = 見つからない
    fn rank(c: &ReviewComment) -> u8 {
        if c.found {
            0
        } else if c.quote.is_empty() {
            1
        } else {
            2
        }
    }
    comments.sort_by_key(|c| (rank(c), c.start_utf16.unwrap_or(usize::MAX)));
    comments
}

/// 分割して実行したときの全体講評をまとめる。
///
/// 塊ごとに講評が返るので、**要約せずにそのまま並べる**(要約は情報を落とす。
/// 二段要約を採らないのと同じ理由 = docs/05-roadmap.md §5.2)。
pub fn merge_overall(parts: &[String]) -> String {
    let filled: Vec<&String> = parts.iter().filter(|p| !p.trim().is_empty()).collect();
    match filled.len() {
        0 => String::new(),
        1 => filled[0].clone(),
        n => filled
            .iter()
            .enumerate()
            .map(|(i, p)| format!("({}/{}) {}", i + 1, n, p.trim()))
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material(title: &str, text: &str) -> (String, String) {
        (title.to_string(), text.to_string())
    }

    // ===== 観点(5分類固定) =====

    #[test]
    fn taxonomy_is_fixed_at_five() {
        assert_eq!(ASPECTS.len(), 5, "観点は5分類で固定(02 M-05)。増やさない");
        let keys: Vec<&str> = ASPECTS.iter().map(|a| a.key).collect();
        assert_eq!(
            keys,
            vec!["style", "structure", "character", "consistency", "reader"]
        );
    }

    #[test]
    fn selects_requested_aspects() {
        let picked = selected_aspects(&["reader".into(), "style".into()]);
        assert_eq!(picked.len(), 2);
        // 並び順は定義順(画面の並びと一致させる)
        assert_eq!(picked[0].key, "style");
        assert_eq!(picked[1].key, "reader");
    }

    #[test]
    fn empty_or_unknown_selection_falls_back_to_all() {
        assert_eq!(selected_aspects(&[]).len(), 5);
        assert_eq!(selected_aspects(&["でたらめ".into()]).len(), 5);
    }

    #[test]
    fn normalizes_aspect_names() {
        assert_eq!(normalize_aspect("structure"), "structure");
        assert_eq!(normalize_aspect("構成・テンポ"), "structure");
        assert_eq!(normalize_aspect(" 読者 "), "reader");
        assert_eq!(normalize_aspect("矛盾"), "consistency");
        // 分類できないものは捨てずに寄せる(指摘そのものを消さない)
        assert_eq!(normalize_aspect("なんだこれ"), "style");
    }

    // ===== プロンプト =====

    #[test]
    fn prompt_lists_only_selected_aspects() {
        let aspects = selected_aspects(&["reader".into()]);
        let p = build_prompt("　本文。", &aspects, &[]);
        assert!(p.contains("読者視点"));
        assert!(!p.contains("構成・テンポ"), "選ばれていない観点を入れない");
        assert!(p.contains("これ以外の観点では書かないでください"));
        assert!(p.contains("　本文。"));
    }

    #[test]
    fn prompt_forbids_moralizing_and_ghostwriting() {
        // M2から持ち越した検証項目: 拒否ではなく「説教が混入する」失敗形(04 §8.1)
        assert!(SYSTEM_PROMPT.contains("道徳的に評価しません"));
        assert!(SYSTEM_PROMPT.contains("説教"));
        // プロダクトの境界: AIは本文を書かない
        assert!(SYSTEM_PROMPT.contains("代筆はしません"));
        let p = build_prompt("本文", ASPECTS.iter().collect::<Vec<_>>().as_slice(), &[]);
        assert!(p.contains("書き直した本文は書かないでください"));
        assert!(p.contains("善悪や社会的な是非には触れないでください"));
    }

    #[test]
    fn prompt_includes_materials_and_consistency_rule() {
        let mats = vec![material("架純", "主人公。一人称は「わたし」")];
        let p = build_prompt("本文", &selected_aspects(&["consistency".into()]), &mats);
        assert!(p.contains("## 架純"));
        assert!(p.contains("わたし"));
        assert!(p.contains("資料に書かれていないことを矛盾扱いしないでください"));
    }

    #[test]
    fn prompt_without_materials_suppresses_consistency_claims() {
        // 資料が無いのに「設定と矛盾」と言われるのは誤報告にしかならない
        let p = build_prompt("本文", &selected_aspects(&["consistency".into()]), &[]);
        assert!(p.contains("設定との矛盾は判断できないため指摘しないでください"));
        assert!(!p.contains("--- 設定資料ここから ---"));
    }

    #[test]
    fn schema_declares_comments_and_overall() {
        let s = review_schema();
        let req = s["json_schema"]["schema"]["required"].to_string();
        assert!(req.contains("comments"));
        assert!(req.contains("overall"), "全体講評を落とさない");
        let item_req =
            s["json_schema"]["schema"]["properties"]["comments"]["items"]["required"].to_string();
        assert!(item_req.contains("quote"));
        assert!(item_req.contains("aspect"));
    }

    // ===== パース =====

    #[test]
    fn parses_structured_response() {
        let raw = r#"{"comments":[
            {"aspect":"reader","quote":"雨だった","comment":"状況が伝わりにくい","suggestion":"視点人物の感覚を足す"}
        ],"overall":"全体としては読みやすい。"}"#;
        let got = parse(raw);
        assert_eq!(got.comments.len(), 1);
        assert_eq!(got.comments[0].aspect, "reader");
        assert_eq!(got.comments[0].quote, "雨だった");
        assert_eq!(got.overall, "全体としては読みやすい。");
    }

    #[test]
    fn parses_fenced_json_and_alternate_keys() {
        let raw = "```json\n{\"items\":[{\"category\":\"文体\",\"text\":\"雨だった\",\"reason\":\"冗長\"}],\"summary\":\"講評\"}\n```";
        let got = parse(raw);
        assert_eq!(got.comments.len(), 1);
        assert_eq!(got.comments[0].aspect, "style");
        assert_eq!(got.comments[0].quote, "雨だった");
        assert_eq!(got.comments[0].comment, "冗長");
        assert_eq!(got.overall, "講評");
    }

    #[test]
    fn parses_bare_array() {
        let raw = r#"[{"aspect":"structure","quote":"あ","comment":"間延びしている"}]"#;
        let got = parse(raw);
        assert_eq!(got.comments.len(), 1);
        assert!(got.overall.is_empty());
    }

    #[test]
    fn keeps_comment_without_quote() {
        // 引用に紐づかない指摘も捨てない(引用なしとして後ろに置く)
        let got = parse(r#"{"comments":[{"aspect":"reader","quote":"","comment":"引きが弱い"}]}"#);
        assert_eq!(got.comments.len(), 1);
        assert!(got.comments[0].quote.is_empty());
    }

    #[test]
    fn drops_empty_comment() {
        let got = parse(r#"{"comments":[{"aspect":"reader","quote":"雨","comment":"  "}]}"#);
        assert!(got.comments.is_empty(), "中身の無い指摘は情報がない");
    }

    #[test]
    fn broken_response_yields_nothing_instead_of_panicking() {
        assert_eq!(parse("すみません、判断できません"), ParsedReview::default());
        assert_eq!(parse(""), ParsedReview::default());
        assert_eq!(parse("{\"comments\":"), ParsedReview::default());
    }

    /// gpt-oss-20b が実際に返した応答(2026-07-31)。
    ///
    /// **英語・散文・箇条書き**で、本文を読んだ形跡もない(小説ではなく
    /// 「利用者の文章」を評している)。形式をまったく守っていない。
    const UNSTRUCTURED_REAL_RESPONSE: &str = "I read the user's text as best I could can be summarized as a long list of disjointed thoughts that are hard to follow. The writing is confusing and fragmented, making clear points unclear. The reader will have difficulty spotting a single idea or theme in it. - **The feedback** - We should give the user some constructive suggestions: - Try to focus on one main point at a time; - Use short sentences and keep a consistent tense; - Avoid excessive repetition of similar ideas. - Keep the overall tone clear. - The content is too long - - **Overall**: the user's writing style can be improved by tightening up the prose, making it more concise, avoiding repetition, and using a consistent tense.\n\n-> concise suggestions";

    #[test]
    fn prose_response_is_not_mistaken_for_no_findings() {
        // **これを「指摘なし」と報告するのがレビューで最悪の失敗**。
        // 中身はあるのに読めていない状態を、呼び出し側が区別できること
        let got = parse(UNSTRUCTURED_REAL_RESPONSE);
        assert!(got.comments.is_empty());
        assert!(
            !got.structured,
            "形式を守らない応答を『読み取れた』と扱ってはいけない"
        );
    }

    #[test]
    fn empty_comment_list_is_a_real_no_findings() {
        // 一覧が空でも、一覧そのものが読めていれば「指摘なし」で正しい
        let got = parse(r#"{"comments":[],"overall":""}"#);
        assert!(got.comments.is_empty());
        assert!(got.structured);
    }

    #[test]
    fn json_of_another_shape_is_not_structured() {
        // 妥当なJSONでも、こちらの形でなければ読み取れていない
        assert!(!parse(r#"{"foo":1}"#).structured);
    }

    #[test]
    fn clip_raw_keeps_short_text_and_marks_the_cut() {
        assert_eq!(clip_raw("  短い応答  "), "短い応答");
        let long = "あ".repeat(MAX_RAW_CHARS + 500);
        let clipped = clip_raw(&long);
        assert!(clipped.contains("以下略"), "切ったことを隠さない");
        assert!(clipped.chars().count() < long.chars().count());
    }

    #[test]
    fn overall_survives_when_comments_are_missing() {
        // 講評だけ返ってくることがある。指摘が無いからと捨てない
        let got = parse(r#"{"overall":"テンポは良い。"}"#);
        assert!(got.comments.is_empty());
        assert_eq!(got.overall, "テンポは良い。");
    }

    // ===== 引用の実在照合 =====

    #[test]
    fn resolves_quote_position_in_utf16() {
        let body = "転校初日の朝は、雨だった。昇降口で靴を履き替える。";
        let got = resolve(
            body,
            parse(r#"{"comments":[{"aspect":"style","quote":"昇降口で靴を履き替える","comment":"淡白"}]}"#)
                .comments,
        );
        assert!(got[0].found);
        assert_eq!(got[0].start_utf16, Some(13));
        assert_eq!(got[0].end_utf16, Some(24));
    }

    #[test]
    fn hallucinated_quote_is_marked_and_sorted_last() {
        let body = "転校初日の朝は、雨だった。";
        let got = resolve(
            body,
            parse(
                r#"{"comments":[
                    {"aspect":"style","quote":"まったく存在しない一文がここにあります","comment":"幻"},
                    {"aspect":"style","quote":"転校初日の朝は","comment":"実在"}
                ]}"#,
            )
            .comments,
        );
        assert_eq!(got.len(), 2);
        assert!(got[0].found);
        assert_eq!(got[0].comment, "実在");
        assert!(!got[1].found, "本文に無い引用は found=false のまま残す");
    }

    #[test]
    fn quote_with_added_brackets_still_resolves() {
        // 引用の前後に鉤括弧を足す癖がある。そこだけ剥がして探し直す
        let body = "架純は小さく息を吐いて、昇降口の扉を押した。";
        let got = resolve(
            body,
            parse(r#"{"comments":[{"aspect":"style","quote":"「架純は小さく息を吐いて」","comment":"a"}]}"#)
                .comments,
        );
        assert!(got[0].found);
        assert_eq!(got[0].start_utf16, Some(0));
    }

    #[test]
    fn partially_paraphrased_quote_resolves_to_matching_head() {
        // 末尾だけ言い換えられた引用。先頭の一致する範囲に寄せる
        let body = "架純は小さく息を吐いて、昇降口の扉を押した。";
        let got = resolve(
            body,
            parse(r#"{"comments":[{"aspect":"style","quote":"架純は小さく息を吐いて、扉を開けた","comment":"a"}]}"#)
                .comments,
        );
        assert!(got[0].found);
        assert_eq!(got[0].start_utf16, Some(0));
    }

    #[test]
    fn short_quote_in_brackets_still_resolves() {
        // 会話文の引用は短い。約物を剥がした先は完全一致なので長さで切らない
        let body = "男が低い声で、金の在処と言った。";
        let got = resolve(
            body,
            parse(r#"{"comments":[{"aspect":"character","quote":"「金の在処」","comment":"a"}]}"#)
                .comments,
        );
        assert!(got[0].found);
        assert_eq!(got[0].start_utf16, Some(7));
    }

    #[test]
    fn short_fragment_is_not_force_matched() {
        // 短い断片で無理に位置を作らない(無関係な箇所に当たる)
        let body = "架純は昇降口で立ち止まった。";
        let got = resolve(
            body,
            parse(r#"{"comments":[{"aspect":"style","quote":"架純は教室で","comment":"a"}]}"#).comments,
        );
        assert!(!got[0].found);
        assert_eq!(got[0].start_utf16, None);
    }

    #[test]
    fn quoteless_comment_sits_between_found_and_missing() {
        let body = "架純は昇降口で立ち止まった。";
        let got = resolve(
            body,
            parse(
                r#"{"comments":[
                    {"aspect":"reader","quote":"","comment":"引きが弱い"},
                    {"aspect":"style","quote":"本文には無い長めの引用文である","comment":"幻"},
                    {"aspect":"style","quote":"架純は昇降口で立ち止まった","comment":"実在"}
                ]}"#,
            )
            .comments,
        );
        assert_eq!(
            got.iter().map(|c| c.comment.as_str()).collect::<Vec<_>>(),
            vec!["実在", "引きが弱い", "幻"]
        );
    }

    // ===== 重複と講評 =====

    #[test]
    fn dedupe_keeps_same_quote_under_different_aspects() {
        let comments = parse(
            r#"{"comments":[
                {"aspect":"style","quote":"雨だった","comment":"a"},
                {"aspect":"style","quote":"雨だった","comment":"b"},
                {"aspect":"reader","quote":"雨だった","comment":"c"}
            ]}"#,
        )
        .comments;
        let got = dedupe(comments);
        assert_eq!(got.len(), 2, "観点が違えば別の指摘として残す");
        assert_eq!(got[0].comment, "a", "先に出たものを残す");
        assert_eq!(got[1].aspect, "reader");
    }

    #[test]
    fn dedupe_uses_comment_text_when_quote_is_empty() {
        let comments = parse(
            r#"{"comments":[
                {"aspect":"reader","quote":"","comment":"引きが弱い"},
                {"aspect":"reader","quote":"","comment":"引きが弱い"},
                {"aspect":"reader","quote":"","comment":"説明が多い"}
            ]}"#,
        )
        .comments;
        assert_eq!(dedupe(comments).len(), 2);
    }

    #[test]
    fn merge_overall_numbers_parts_and_skips_blanks() {
        assert_eq!(merge_overall(&[]), "");
        assert_eq!(merge_overall(&["ひとつ".into(), "  ".into()]), "ひとつ");
        let merged = merge_overall(&["前半".into(), "後半".into()]);
        assert!(merged.contains("(1/2) 前半"));
        assert!(merged.contains("(2/2) 後半"));
    }
}
