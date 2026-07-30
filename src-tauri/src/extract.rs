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
    /// 同じ対象を指す別の呼び名(姓・名・肩書・愛称)。名寄せの成果
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Some の場合は**既存エントリへの別名追加**の提案。新規作成ではない
    #[serde(default)]
    pub existing_path: Option<String>,
}

/// LLMが返した1件(検証前)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEntity {
    pub name: String,
    pub kind: String,
    pub description: String,
    pub aliases: Vec<String>,
}

/// 検証の結果
#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    pub candidates: Vec<Candidate>,
    /// 複数の対象に結び付いたため、どちらにも付けなかった呼び名。
    /// 機械が勝手に決めずユーザーへ見せる(作中に教授が2人いる場合など)
    pub conflicts: Vec<String>,
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

/// 別名として使えるか。**正式名より緩い**判定にする。
///
/// あだ名はひらがなが普通(「りゅーちゃん」)なので、正式名の規則
/// (漢字・カタカナのみ)をそのまま当てると落ちてしまう。
/// 一方で1文字の別名は言及検出が誤爆するため却下する。
pub fn looks_like_alias(name: &str) -> bool {
    let n = name.trim();
    let len = n.chars().count();
    if !(2..=12).contains(&len) {
        return false;
    }
    if n.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    if STOPWORDS.contains(&n) {
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
         - 一度きりの通行人など、**名前が与えられていない存在**\n\n\
         **同じ対象を指す複数の呼び名は1件にまとめてください**(名寄せ)。\n\
         正式な名前を name に、それ以外の呼び名をすべて aliases に入れます。\n\
         例: 「黒木龍一」という教授が本文で「黒木」「龍一」「教授」「りゅーちゃん」と\n\
         呼ばれているなら、name=\"黒木龍一\"、aliases=[\"黒木\",\"龍一\",\"教授\",\"りゅーちゃん\"] とします。\n\
         - 姓だけ・名だけの呼び方、肩書、あだ名、敬称なしの呼び捨てを aliases に含めます\n\
         - **aliases はひらがなでも構いません**(あだ名)\n\
         - 別の人物を指す呼び名を混ぜないでください。**確実に同一だと本文から読み取れるものだけ**\n\
         - 同じ呼び名を2人以上に付けないでください\n\n\
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
        "{\"entities\":[{\"name\":\"正式な名前\",\"kind\":\"character|location|item|term\",\"description\":\"一行説明\",\"aliases\":[\"別の呼び名\"]}]}\n\n",
    );
    s.push_str("--- 本文ここから ---\n");
    s.push_str(body);
    s.push_str("\n--- 本文ここまで ---\n");
    s
}

/// 応答を解析する(校正と同じ寛容パース)
pub fn parse(raw: &str) -> Vec<RawEntity> {
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
            let aliases = item
                .get("aliases")
                .or_else(|| item.get("alias"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| a.as_str())
                        .map(|a| a.trim().to_string())
                        .filter(|a| !a.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some(RawEntity {
                name,
                kind: normalize_kind(kind),
                description,
                aliases,
            })
        })
        .collect()
}

/// LLMの出力を機械側で検証して候補に落とす(名寄せの検証を含む)。
///
/// 落とすもの:
///  - 固有名詞らしくないもの(§looks_like_proper_noun)
///  - **本文に実在しないもの**(幻覚)。別名も同じ扱い
///  - 重複
///
/// 名寄せの扱い:
///  - 別名が他の候補の正式名と衝突したら、**独立候補の方を落として別名に寄せる**
///  - 同じ別名が2つ以上の候補に付いたら、**どちらにも付けず** conflicts へ回す
///  - 既存codexに一致する名前があれば、新規作成ではなく**別名追加の提案**にする
pub fn verify(body: &str, raw: Vec<RawEntity>, codex: &[CodexEntry]) -> VerifyResult {
    // --- 1. 正式名の検証 ---
    let mut entities: Vec<RawEntity> = Vec::new();
    for e in raw {
        if !looks_like_proper_noun(&e.name) {
            continue;
        }
        if !body.contains(&e.name) {
            continue; // 幻覚
        }
        if entities.iter().any(|x| x.name == e.name) {
            continue;
        }
        entities.push(e);
    }

    // --- 2. 別名の検証 ---
    let canonical: Vec<String> = entities.iter().map(|e| e.name.clone()).collect();
    for e in entities.iter_mut() {
        let mut kept: Vec<String> = Vec::new();
        for a in std::mem::take(&mut e.aliases) {
            if a == e.name || !looks_like_alias(&a) || !body.contains(&a) {
                continue;
            }
            // 他の候補の正式名を別名にしてはいけない(別人を吸収してしまう)
            if canonical.iter().any(|c| *c == a && *c != e.name) {
                continue;
            }
            if !kept.contains(&a) {
                kept.push(a);
            }
        }
        e.aliases = kept;
    }

    // --- 3. 別名の衝突を解消(同じ呼び名が複数の対象に付いた場合) ---
    let mut conflicts: Vec<String> = Vec::new();
    let all_aliases: Vec<String> = entities.iter().flat_map(|e| e.aliases.clone()).collect();
    for a in &all_aliases {
        let n = all_aliases.iter().filter(|x| *x == a).count();
        if n > 1 && !conflicts.contains(a) {
            conflicts.push(a.clone());
        }
    }
    if !conflicts.is_empty() {
        for e in entities.iter_mut() {
            e.aliases.retain(|a| !conflicts.contains(a));
        }
    }

    // --- 4. 既存codexとの照合 ---
    let mut out: Vec<Candidate> = Vec::new();
    for e in entities {
        // 正式名・別名のどれかが既存エントリの名前と一致すれば「同じ対象」とみなす
        let existing = codex.iter().find(|c| {
            let names = c.patterns();
            names.iter().any(|n| *n == e.name || e.aliases.contains(n))
        });

        match existing {
            Some(c) => {
                // 既存に無い呼び名だけを追加提案にする
                let known = c.patterns();
                let additions: Vec<String> = std::iter::once(e.name.clone())
                    .chain(e.aliases.iter().cloned())
                    .filter(|n| !known.contains(n))
                    .collect();
                if additions.is_empty() {
                    continue; // 既に全部登録済み
                }
                out.push(Candidate {
                    count: body.matches(&e.name).count(),
                    name: c.title.clone(),
                    kind: c.type_.clone().unwrap_or_else(|| e.kind.clone()),
                    description: e.description,
                    aliases: additions,
                    existing_path: Some(c.path.clone()),
                });
            }
            None => out.push(Candidate {
                count: body.matches(&e.name).count(),
                name: e.name,
                kind: e.kind,
                description: e.description,
                aliases: e.aliases,
                existing_path: None,
            }),
        }
    }

    // 出現回数の多い順。作品にとって重要な語ほど上に来る
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    VerifyResult {
        candidates: out,
        conflicts,
    }
}

/// codexエントリのファイル本文を作る(テンプレートを使わない最小形)
pub fn entry_markdown(c: &Candidate) -> String {
    let aliases = c
        .aliases
        .iter()
        .map(|a| a.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "---\ntype: {}\ntitle: {}\naliases: [{}]\ndescription: {}\n---\n\n",
        c.kind, c.name, aliases, c.description
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

    fn raw(v: &[(&str, &str, &str)]) -> Vec<RawEntity> {
        v.iter()
            .map(|(a, b, c)| RawEntity {
                name: a.to_string(),
                kind: b.to_string(),
                description: c.to_string(),
                aliases: Vec::new(),
            })
            .collect()
    }

    fn one(name: &str, aliases: &[&str]) -> Vec<RawEntity> {
        vec![RawEntity {
            name: name.to_string(),
            kind: "character".into(),
            description: String::new(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
        }]
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
        assert_eq!(got.candidates.len(), 1);
        assert_eq!(got.candidates[0].name, "佐藤架純");
    }

    #[test]
    fn registered_name_yields_no_proposal_when_nothing_new() {
        let body = "架純と悠二が並んで歩く。";
        let codex = vec![entry("佐藤架純", &["架純"])];
        // 「架純」は登録済みで追加する呼び名も無いので提案は出ない。「悠二」だけ新規
        let got = verify(
            body,
            raw(&[("架純", "character", "主人公"), ("悠二", "character", "幼馴染")]),
            &codex,
        );
        assert_eq!(got.candidates.len(), 1);
        assert_eq!(got.candidates[0].name, "悠二");
        assert!(got.candidates[0].existing_path.is_none());
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
            got.candidates.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["架純", "青葉高校", "悠二"]
        );
        assert_eq!(got.candidates[0].count, 3);
    }

    #[test]
    fn deduplicates_candidates() {
        let body = "架純が来た。";
        let got = verify(
            body,
            raw(&[("架純", "character", "a"), ("架純", "character", "b")]),
            &[],
        );
        assert_eq!(got.candidates.len(), 1);
        assert_eq!(got.candidates[0].description, "a");
    }

    // ===== 名寄せ =====

    #[test]
    fn groups_aliases_into_one_candidate() {
        let body = "黒木龍一は教授だ。黒木は笑い、龍一は頷いた。りゅーちゃんと呼ぶ者もいる。";
        let got = verify(
            body,
            one("黒木龍一", &["黒木", "龍一", "教授", "りゅーちゃん"]),
            &[],
        );
        assert_eq!(got.candidates.len(), 1, "1件に集約されること");
        let c = &got.candidates[0];
        assert_eq!(c.name, "黒木龍一");
        // あだ名(ひらがな)も別名として通ること
        assert_eq!(c.aliases, vec!["黒木", "龍一", "教授", "りゅーちゃん"]);
    }

    #[test]
    fn drops_hallucinated_and_invalid_aliases() {
        let body = "黒木龍一は教授だ。";
        let got = verify(
            body,
            one("黒木龍一", &["教授", "本文にない呼び名", "黒", "彼女", "黒木龍一"]),
            &[],
        );
        // 本文に無い / 1文字 / 除外語 / 正式名と同じ、はすべて落ちる
        assert_eq!(got.candidates[0].aliases, vec!["教授"]);
    }

    #[test]
    fn alias_conflict_is_attached_to_nobody() {
        // 作中に教授が2人いる場合。機械が勝手に決めない
        let body = "黒木龍一は教授だ。白石渚も教授だ。";
        let raws = vec![
            RawEntity {
                name: "黒木龍一".into(),
                kind: "character".into(),
                description: String::new(),
                aliases: vec!["教授".into()],
            },
            RawEntity {
                name: "白石渚".into(),
                kind: "character".into(),
                description: String::new(),
                aliases: vec!["教授".into()],
            },
        ];
        let got = verify(body, raws, &[]);
        assert_eq!(got.candidates.len(), 2);
        assert!(got.candidates.iter().all(|c| c.aliases.is_empty()));
        assert_eq!(got.conflicts, vec!["教授"]);
    }

    #[test]
    fn other_canonical_name_is_never_absorbed_as_alias() {
        // 別人を別名として吸収してはいけない
        let body = "黒木龍一と白石渚が話す。";
        let raws = vec![
            RawEntity {
                name: "黒木龍一".into(),
                kind: "character".into(),
                description: String::new(),
                aliases: vec!["白石渚".into()],
            },
            RawEntity {
                name: "白石渚".into(),
                kind: "character".into(),
                description: String::new(),
                aliases: vec![],
            },
        ];
        let got = verify(body, raws, &[]);
        assert_eq!(got.candidates.len(), 2);
        let kuroki = got.candidates.iter().find(|c| c.name == "黒木龍一").unwrap();
        assert!(kuroki.aliases.is_empty());
    }

    #[test]
    fn proposes_alias_addition_for_existing_entry() {
        // 既存の「黒木龍一」に対しては新規作成ではなく別名追加を提案する
        let body = "黒木は教授だ。龍一は頷いた。";
        let codex = vec![entry("黒木龍一", &["黒木"])];
        let got = verify(body, one("黒木", &["龍一", "教授"]), &codex);
        assert_eq!(got.candidates.len(), 1);
        let c = &got.candidates[0];
        assert_eq!(c.name, "黒木龍一", "既存エントリの正式名で表示する");
        assert_eq!(c.existing_path.as_deref(), Some("codex/characters/黒木龍一.md"));
        // 既に登録済みの「黒木」は含めず、新しい呼び名だけを提案する
        assert_eq!(c.aliases, vec!["龍一", "教授"]);
    }

    #[test]
    fn alias_matching_existing_entry_links_to_it() {
        // 正式名が未登録でも、別名が既存エントリに一致すれば同じ対象とみなす
        let body = "龍一が来た。教授と呼ばれている。";
        let codex = vec![entry("黒木龍一", &["教授"])];
        let got = verify(body, one("龍一", &["教授"]), &codex);
        assert_eq!(got.candidates.len(), 1);
        assert_eq!(got.candidates[0].existing_path.is_some(), true);
        assert_eq!(got.candidates[0].aliases, vec!["龍一"]);
    }

    #[test]
    fn alias_validation_is_looser_than_canonical() {
        // 正式名はひらがなを弾くが、別名は通す
        assert!(!looks_like_proper_noun("りゅーちゃん"));
        assert!(looks_like_alias("りゅーちゃん"));
        // ただし1文字と除外語は別名でも弾く
        assert!(!looks_like_alias("黒"));
        assert!(!looks_like_alias("彼女"));
        assert!(!looks_like_alias("黒木 龍一"), "空白を含むものは弾く");
    }

    #[test]
    fn entry_markdown_includes_aliases() {
        let c = Candidate {
            name: "黒木龍一".into(),
            kind: "character".into(),
            description: "教授".into(),
            count: 3,
            aliases: vec!["黒木".into(), "教授".into()],
            existing_path: None,
        };
        assert!(entry_markdown(&c).contains("aliases: [黒木, 教授]"));
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
        assert_eq!(got[0].kind, "character");
        assert_eq!(got[1].kind, "location");
        assert_eq!(got[2].kind, "term", "未知の種別は term に寄せる");
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
            aliases: vec![],
            existing_path: None,
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
