//! 本文からの設定自動抽出(M-09)。
//!
//! **提案型**: 見つけた固有名詞をcodexへ勝手に登録しない。候補として並べ、
//! ユーザーが採否を決める(U-06の非破壊原則)。
//!
//! 位置づけ(docs/06-decision-log.md §2-8): U-01「ゼロ設定で書き始められる」と、
//! 設定DBを前提とする差別化機能(M-04-02/M-05)を両立させる唯一の経路。
//! これが無いと初期ユーザーはコールドスタート問題で価値を体験できない。
//!
//! V1の割り切り(02 M-09):
//!  - **手動実行のみ**。保存のたびに走らせない(ローカルLLMのNER精度では
//!    候補ノイズが執筆を阻害する)
//!  - 抽出は「名前 + 種別 + 一行説明」まで。詳細プロフィールの自動生成はしない
//!  - **固有名詞らしさの判定は機械側**で行い、LLMの出力を鵜呑みにしない

use serde::{Deserialize, Serialize};

use crate::project::CodexEntry;

/// codexへの追加候補。
///
/// ユーザーが画面で選んだものをそのまま登録コマンドへ返すため Deserialize も持つ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub name: String,
    /// character / location / item / term のいずれか。不明なら term
    pub kind: String,
    /// 一行説明(本文から読み取れる範囲)
    pub description: String,
    /// 本文での出現回数(機械側で数える。LLMの自己申告は使わない)
    pub count: usize,
}

/// 一般名詞・普通の語を弾くための除外リスト。
///
/// 形態素解析辞書を持たないV1では、LLMが挙げた候補を機械側で完全に検証できない。
/// そこで「小説の地の文に頻出し、固有名詞ではまずない語」だけを最小限で弾く。
/// 増やしすぎると正当な固有名詞まで落とすので、**確実なものに限る**。
const STOPWORDS: &[&str] = &[
    "彼女", "自分", "今日", "昨日", "明日", "今朝", "夕方", "午前", "午後",
    "本当", "最初", "最後", "一人", "二人", "全部", "普通", "少女", "少年",
    "男性", "女性", "人間", "場所", "時間", "世界", "気持", "表情", "言葉",
    "何度", "一度", "瞬間", "空気", "視線", "様子", "感じ", "程度", "以上",
    "以下", "場合", "理由", "問題", "必要", "結果", "状態", "関係", "内容",
];

/// 名前に使われうる文字か(proofread と同じ考え方。ひらがなは含めない)
fn is_name_char(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{30A1}'..='\u{30FA}'
        | '\u{30FC}'
        | '\u{3005}'
        | '\u{3006}'
    )
}

/// 固有名詞として見込みがあるか(機械側のふるい)
pub fn looks_like_proper_noun(name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    // 2〜12文字。1文字は一般名詞が多すぎ、長すぎるものは文の切り出し失敗
    if !(2..=12).contains(&chars.len()) {
        return false;
    }
    // 漢字・カタカナのみで構成されていること
    if !chars.iter().all(|c| is_name_char(*c)) {
        return false;
    }
    if STOPWORDS.contains(&name) {
        return false;
    }
    true
}

/// 種別を推奨語彙に丸める。未知の値は term に寄せる
fn normalize_kind(raw: &str) -> String {
    let k = raw.trim();
    match k {
        "character" | "人物" | "キャラクター" => "character",
        "location" | "場所" | "地名" => "location",
        "item" | "物品" | "アイテム" => "item",
        _ => "term",
    }
    .to_string()
}

/// LLMへ渡すプロンプト。**既存の登録名を渡して重複提案を防ぐ**
pub fn build_prompt(body: &str, known: &[String]) -> String {
    let mut s = String::new();
    s.push_str(
        "次の小説本文から、**この作品固有の名前**を抜き出してください。\n\n\
         抜き出すもの:\n\
         - 登場人物の名前(character)\n\
         - 場所・建物・地名(location)\n\
         - 作品に固有の物・道具(item)\n\
         - 作品に固有の用語・組織・技術・概念(term)\n\n\
         抜き出さないもの:\n\
         - 「彼女」「少年」「男」のような**役割や属性を指すだけの語**\n\
         - 「昇降口」「figure」のような一般名詞\n\
         - 一度きりの通行人など、**名前が与えられていない存在**\n\
         - ひらがなだけの語\n\n\
         description は本文から読み取れる範囲で一行にまとめてください。\
         本文に書かれていないことを推測して書かないでください。\n\n",
    );
    if !known.is_empty() {
        s.push_str("次の名前は**登録済みなので挙げないでください**:\n");
        for n in known.iter().take(200) {
            s.push_str("- ");
            s.push_str(n);
            s.push('\n');
        }
        s.push('\n');
    }
    s.push_str("出力は次のJSONだけを返してください(説明文は不要です):\n");
    s.push_str(
        "{\"entities\":[{\"name\":\"名前\",\"kind\":\"character|location|item|term\",\"description\":\"一行説明\"}]}\n\n",
    );
    s.push_str("--- 本文ここから ---\n");
    s.push_str(body);
    s.push_str("\n--- 本文ここまで ---\n");
    s
}

/// 応答を解析する(校正と同じ寛容パース)
pub fn parse(raw: &str) -> Vec<(String, String, String)> {
    let blob = crate::ai::extract_json_blob(raw);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(blob) else {
        return Vec::new();
    };
    let array = value
        .get("entities")
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
            let name = item
                .get("name")
                .or_else(|| item.get("title"))
                .and_then(|v| v.as_str())?
                .trim()
                .to_string();
            if name.is_empty() {
                return None;
            }
            let kind = item.get("kind").and_then(|v| v.as_str()).unwrap_or("term");
            let description = item
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            Some((name, normalize_kind(kind), description))
        })
        .collect()
}

/// LLMの出力を機械側で検証して候補に落とす。
///
/// 落とすもの:
///  - 固有名詞らしくないもの(§looks_like_proper_noun)
///  - **本文に実在しないもの**(幻覚)
///  - 既にcodexにあるもの(正式名・別名のどちらとも照合)
///  - 重複
pub fn verify(
    body: &str,
    raw_candidates: Vec<(String, String, String)>,
    codex: &[CodexEntry],
) -> Vec<Candidate> {
    let known: Vec<String> = codex.iter().flat_map(|c| c.patterns()).collect();
    let mut out: Vec<Candidate> = Vec::new();

    for (name, kind, description) in raw_candidates {
        if !looks_like_proper_noun(&name) {
            continue;
        }
        if known.iter().any(|k| k == &name) {
            continue;
        }
        let count = body.matches(&name).count();
        if count == 0 {
            continue; // 本文に無いものは幻覚
        }
        if out.iter().any(|c| c.name == name) {
            continue;
        }
        out.push(Candidate {
            name,
            kind,
            description,
            count,
        });
    }

    // 出現回数の多い順。作品にとって重要な語ほど上に来る
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    out
}

/// codexエントリのファイル本文を作る(テンプレートを使わない最小形)
pub fn entry_markdown(c: &Candidate) -> String {
    format!(
        "---\ntype: {}\ntitle: {}\naliases: []\ndescription: {}\n---\n\n",
        c.kind, c.name, c.description
    )
}

/// 種別からcodexの保存先フォルダを決める
pub fn folder_for(kind: &str) -> &'static str {
    match kind {
        "character" => "codex/characters",
        "location" => "codex/locations",
        "item" => "codex/items",
        _ => "codex/terms",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, aliases: &[&str]) -> CodexEntry {
        CodexEntry {
            path: format!("codex/characters/{title}.md"),
            title: title.to_string(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            type_: Some("character".into()),
            description: None,
        }
    }

    fn raw(v: &[(&str, &str, &str)]) -> Vec<(String, String, String)> {
        v.iter()
            .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
            .collect()
    }

    #[test]
    fn accepts_proper_nouns_only() {
        assert!(looks_like_proper_noun("佐藤架純"));
        assert!(looks_like_proper_noun("ウルスラ"));
        assert!(looks_like_proper_noun("青葉高校"));
        // 1文字・ひらがな混じり・長すぎ・一般名詞は落とす
        assert!(!looks_like_proper_noun("森"));
        assert!(!looks_like_proper_noun("かすみん"));
        assert!(!looks_like_proper_noun("架純の家"));
        assert!(!looks_like_proper_noun(&"漢".repeat(20)));
        assert!(!looks_like_proper_noun("彼女"));
        assert!(!looks_like_proper_noun("少年"));
    }

    #[test]
    fn drops_hallucinated_names() {
        let body = "佐藤架純は青葉高校へ向かった。";
        let got = verify(
            body,
            raw(&[
                ("佐藤架純", "character", "主人公"),
                ("存在しない人", "character", "幻"),
            ]),
            &[],
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "佐藤架純");
    }

    #[test]
    fn drops_already_registered_names_and_aliases() {
        let body = "架純と悠二が並んで歩く。";
        let codex = vec![entry("佐藤架純", &["架純"])];
        // 正式名でも別名でも、登録済みなら提案しない
        let got = verify(
            body,
            raw(&[("架純", "character", "主人公"), ("悠二", "character", "幼馴染")]),
            &codex,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "悠二");
    }

    #[test]
    fn counts_occurrences_and_sorts_by_frequency() {
        let body = "架純。架純。架純。悠二。青葉高校。青葉高校。";
        let got = verify(
            body,
            raw(&[
                ("悠二", "character", ""),
                ("架純", "character", ""),
                ("青葉高校", "location", ""),
            ]),
            &[],
        );
        assert_eq!(
            got.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["架純", "青葉高校", "悠二"]
        );
        assert_eq!(got[0].count, 3);
    }

    #[test]
    fn deduplicates_candidates() {
        let body = "架純が来た。";
        let got = verify(
            body,
            raw(&[("架純", "character", "a"), ("架純", "character", "b")]),
            &[],
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].description, "a");
    }

    #[test]
    fn parses_response_and_normalizes_kind() {
        let got = parse(
            r#"{"entities":[
                {"name":"佐藤架純","kind":"人物","description":"主人公"},
                {"name":"青葉高校","kind":"location","description":"通う高校"},
                {"name":"白鏡","kind":"よく分からない","description":""}
            ]}"#,
        );
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].1, "character");
        assert_eq!(got[1].1, "location");
        assert_eq!(got[2].1, "term", "未知の種別は term に寄せる");
    }

    #[test]
    fn broken_response_is_safe() {
        assert!(parse("見つかりませんでした").is_empty());
        assert!(parse("").is_empty());
        assert!(parse(r#"{"entities":[{"kind":"character"}]}"#).is_empty());
    }

    #[test]
    fn prompt_lists_known_names_and_excludes_rules() {
        let p = build_prompt("本文", &["佐藤架純".to_string()]);
        assert!(p.contains("登録済みなので挙げないでください"));
        assert!(p.contains("佐藤架純"));
        assert!(p.contains("役割や属性を指すだけの語"));
    }

    #[test]
    fn entry_markdown_matches_v1_frontmatter() {
        let c = Candidate {
            name: "五十嵐悠二".into(),
            kind: "character".into(),
            description: "幼馴染".into(),
            count: 3,
        };
        let md = entry_markdown(&c);
        // V1が解釈する4フィールドだけを書く(03 §4.1)
        assert!(md.starts_with("---\ntype: character\ntitle: 五十嵐悠二\n"));
        assert!(md.contains("aliases: []"));
        assert!(md.contains("description: 幼馴染"));
        assert!(!md.contains("id:"), "V1で解釈しないフィールドを書かない");
    }

    #[test]
    fn folder_matches_kind() {
        assert_eq!(folder_for("character"), "codex/characters");
        assert_eq!(folder_for("location"), "codex/locations");
        assert_eq!(folder_for("item"), "codex/items");
        assert_eq!(folder_for("term"), "codex/terms");
        assert_eq!(folder_for("なんだこれ"), "codex/terms");
    }
}
