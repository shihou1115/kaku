//! AI相談の依頼テンプレート(M-03「方法論テンプレート起点」)。
//!
//! 課題(docs/05-roadmap.md §5.6): 依頼欄が空欄からの自由入力しかなく、
//! 毎回「何をどう頼むか」を考える負荷がかかって機能が使われなくなる。
//!
//! 採る形は**案A: チップを押すと依頼欄へ文面が入る**。
//! 押しても**送信はしない**。文面が見えるのでユーザーが「どう頼めばいいか」を学べる。
//!
//! 実体は `<config_dir>/kaku/prompts/<n-カテゴリ>.md` に置く普通のMarkdown
//! (codexテンプレート=templates.rs と同じ思想)。自分の口癖に書き換えられ、
//! ファイルを1つ足せばカテゴリが増える。
//!
//! **守ること(§5.6の設計上の制約)**:
//!  1. **本文を書かせる文面を入れない**。「続きを書いて」ではなく「展開案を挙げて」。
//!     プロダクトの境界(AIは本文を書かない)を定型文の側から崩さない
//!  2. 挿入であって即送信ではない(採否をユーザーに委ねる原則と同じ姿勢)
//!  3. 常時表示は絞る。UI渋滞を避ける
//!
//! **入れないもの**: 使用頻度による自動並べ替え、選択範囲に応じた文面の自動切替
//! (どちらも§5.6で見送りと判断済み)。レビュー系の文面も入れない —
//! M-05のレビュータブが観点つき・引用照合つきで担うため、同じ依頼を質の違う
//! 2経路で提供すると使い分けが分からなくなる。

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

/// 既定の定型文: (ファイル名, 中身)。
///
/// ファイル名の数字は**並び順**のためだけにある(表示では落とす)。
/// ユーザーが `5-自分用.md` を足せば末尾に並ぶ。
const BUILTIN: &[(&str, &str)] = &[
    (
        "1-プロット",
        r#"# プロット

三幕構成・起承転結などの方法論を起点にした依頼をここに置く。
「書いて」ではなく「挙げて/整理して」と頼むこと。

## 続きの展開案を3つ
いまの場面の続きとして考えられる展開を3つ、それぞれ一行で挙げてください。
ありがちなもの・意外なものを混ぜてください。本文は書かないでください。

## 起こりうる障害・波乱
この場面のあと、登場人物の目的を邪魔しうる出来事を挙げてください。
外からの障害と、内面の迷いの両方から考えてください。

## この人物なら次にどう動くか
設定資料の性格・目的に沿って、この人物が次に取りそうな行動を挙げてください。
資料から読み取れない部分は、推測であると明示してください。

## 別の切り口(What if)
「もし〜だったら」という形で、いまの前提を1つだけ変えた場合の展開を挙げてください。

## 起承転結のどこにいるか
ここまでの流れを起承転結に当てはめると、いまどの位置にあたるか整理してください。
不足している要素があれば指摘してください。

## 三幕構成で見て次に要るもの
三幕構成で見たとき、次に起きる必要がある出来事は何か挙げてください。
転換点(プロットポイント)が足りているかも見てください。
"#,
    ),
    (
        "2-キャラクター",
        r#"# キャラクター

人物を深掘りする依頼。**AIに質問させる**形が記入の負担を下げる。

## 未定の項目を質問して
この人物について、まだ決まっていない項目を質問の形で挙げてください。
物語に効きそうな順に並べてください。

## 動機・欠点・恐れを掘り下げる
この人物の動機・欠点・恐れているものを掘り下げる質問をしてください。
答えは書かず、質問だけを挙げてください。

## 役割が被っていないか
既存の登場人物と役割・機能が重なっていないか見てください。
重なっている場合は、どう差別化できるかの方向を挙げてください。
"#,
    ),
    (
        "3-世界観",
        r#"# 世界観・設定

設定の穴を見つける依頼。埋めるのは執筆者の仕事。

## 設定の穴・未定義の箇所
この設定について、決まっていない箇所・矛盾しそうな箇所を指摘してください。
"これが決まっていないと後で困る"ものを優先してください。

## 代償・制約は何か
この設定に代償・制約・限界があるかを問うてください。
制約が無い設定は物語で扱いにくいので、候補があれば挙げてください。

## 既存設定との整合を質問して
既に登録されている設定と食い違いそうな点を質問の形で挙げてください。
断定せず、確認すべき点として挙げてください。
"#,
    ),
    (
        "4-整理",
        r#"# 整理

書く前・書いた後に流れを掴み直すための依頼。

## この場面を3行で要約
この場面で起きたことを3行で要約してください。解釈や評価は加えないでください。

## ここまでの流れを整理
ここまでの出来事を時系列で整理してください。
未回収のまま残っている事柄があれば最後に挙げてください。
"#,
    ),
];

/// 1件の定型文
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptTemplate {
    /// 表示用のカテゴリ名(ファイル名から並び順の数字を落としたもの)
    pub category: String,
    /// チップに出す短い名前(`##` 見出し)
    pub title: String,
    /// 依頼欄へ挿入する文面
    pub body: String,
}

pub fn prompts_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("kaku")
        .join("prompts")
}

/// 既定の定型文を書き出す。**既存ファイルは上書きしない**(ユーザーの編集を壊さない)
pub fn ensure_defaults() -> std::io::Result<()> {
    let dir = prompts_dir();
    fs::create_dir_all(&dir)?;
    for (name, body) in BUILTIN {
        let path = dir.join(format!("{name}.md"));
        if !path.exists() {
            fs::write(path, body.replace("\r\n", "\n"))?;
        }
    }
    Ok(())
}

/// 表示用のカテゴリ名。先頭の `数字-` は並び順のためだけなので落とす
pub fn display_category(file_stem: &str) -> String {
    match file_stem.split_once('-') {
        Some((head, rest)) if !head.is_empty() && head.chars().all(|c| c.is_ascii_digit()) => {
            rest.to_string()
        }
        _ => file_stem.to_string(),
    }
}

/// Markdownを定型文の一覧に分解する。
///
/// `## 見出し` が1件の区切り。見出しの後から次の `##` までが挿入される文面になる。
/// `#`(1段)はカテゴリの説明なので読み飛ばす。
pub fn parse(category: &str, markdown: &str) -> Vec<PromptTemplate> {
    let mut out: Vec<PromptTemplate> = Vec::new();
    let mut title: Option<String> = None;
    let mut body = String::new();

    let mut flush = |title: &mut Option<String>, body: &mut String, out: &mut Vec<PromptTemplate>| {
        if let Some(t) = title.take() {
            let text = body.trim().to_string();
            if !text.is_empty() {
                out.push(PromptTemplate {
                    category: category.to_string(),
                    title: t,
                    body: text,
                });
            }
        }
        body.clear();
    };

    for line in markdown.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            flush(&mut title, &mut body, &mut out);
            title = Some(rest.trim().to_string());
            continue;
        }
        // `#` 1段はカテゴリ見出し。ここで区切りはしないが、見出し前の説明は捨てる
        if trimmed.starts_with("# ") {
            flush(&mut title, &mut body, &mut out);
            continue;
        }
        if title.is_some() {
            body.push_str(trimmed);
            body.push('\n');
        }
    }
    flush(&mut title, &mut body, &mut out);
    out
}

/// 定型文を読み込む。ファイル名順に並ぶ(数字の接頭辞で制御する)
pub fn list() -> std::io::Result<Vec<PromptTemplate>> {
    let dir = prompts_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
        .collect();
    files.sort();

    let mut out = Vec::new();
    for path in files {
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        if stem.starts_with('.') {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue; // 読めないファイルは黙って飛ばす(1つの壊れで全部止めない)
        };
        out.extend(parse(&display_category(&stem), &text));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sections_by_heading() {
        let md = "# プロット\n\n説明文はチップにしない。\n\n## 展開案を3つ\n一行目。\n二行目。\n\n## What if\nもし〜だったら。\n";
        let got = parse("プロット", md);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].title, "展開案を3つ");
        assert_eq!(got[0].body, "一行目。\n二行目。");
        assert_eq!(got[0].category, "プロット");
        assert_eq!(got[1].title, "What if");
    }

    #[test]
    fn category_description_is_not_a_template() {
        // `#` の下に書いた説明は挿入対象にしない
        let got = parse("x", "# 見出し\nここは説明。\n");
        assert!(got.is_empty());
    }

    #[test]
    fn empty_section_is_dropped() {
        let got = parse("x", "## 見出しだけ\n\n## 中身あり\n本文\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "中身あり");
    }

    #[test]
    fn handles_empty_and_broken_input() {
        assert!(parse("x", "").is_empty());
        assert!(parse("x", "見出しの無いただの文章").is_empty());
    }

    #[test]
    fn strips_order_prefix_from_category() {
        assert_eq!(display_category("1-プロット"), "プロット");
        assert_eq!(display_category("12-世界観"), "世界観");
        // 数字で始まらないものはそのまま(ユーザーが自由に名付けられる)
        assert_eq!(display_category("自分用"), "自分用");
        assert_eq!(display_category("A-メモ"), "A-メモ");
        assert_eq!(display_category("-先頭"), "-先頭");
    }

    #[test]
    fn builtin_templates_parse_and_are_nonempty() {
        for (name, md) in BUILTIN {
            let items = parse(&display_category(name), md);
            assert!(!items.is_empty(), "{name} から定型文が取れない");
            for t in items {
                assert!(!t.title.is_empty());
                assert!(!t.body.is_empty());
            }
        }
    }

    #[test]
    fn builtin_templates_never_ask_the_ai_to_write_prose() {
        // §5.6の制約1: プロダクトの境界(AIは本文を書かない)を定型文から崩さない。
        // 「書いて」と頼む文面が混ざっていないことを機械で見張る
        const FORBIDDEN: &[&str] = &[
            "続きを書い",
            "本文を書い",
            "執筆して",
            "書き下ろ",
            "小説にして",
            "セリフを書い",
            "描写して",
        ];
        for (name, md) in BUILTIN {
            for t in parse(&display_category(name), md) {
                for word in FORBIDDEN {
                    assert!(
                        !t.body.contains(word) && !t.title.contains(word),
                        "{name} / {} に本文を書かせる文面がある: {word}",
                        t.title
                    );
                }
            }
        }
    }

    #[test]
    fn builtin_covers_the_three_systems() {
        // M-03の3系統(プロット / キャラクター / 世界観設定)が揃っていること
        let names: Vec<String> = BUILTIN
            .iter()
            .map(|(n, _)| display_category(n))
            .collect();
        assert!(names.contains(&"プロット".to_string()));
        assert!(names.contains(&"キャラクター".to_string()));
        assert!(names.contains(&"世界観".to_string()));
    }
}
