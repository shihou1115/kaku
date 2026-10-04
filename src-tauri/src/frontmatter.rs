//! YAML フロントマターの寛容パース(docs/03-data-format.md §4.1・§5-3)。
//!
//! 方針:
//! - **V1で解釈するのは type / title / aliases / description の4つだけ**。
//!   それ以外(id, tags, timestamp, importance, relations, progression …)は
//!   解釈せず、生テキストのまま保持する。
//! - 本文の編集はエディタが**ファイル全体をテキストとして**扱うため、
//!   アプリがフロントマターを再シリアライズする経路を持たない。
//!   これにより「未知フィールドの無改変往復保存」が構造的に保証される
//!   (YAMLライブラリに依存しないので serde_yaml 非推奨問題も回避できる)。
//! - 壊れたYAMLでもエラーにしない。読めたものだけ返す。

/// 解釈できた既知フィールド。すべて任意。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontMatter {
    pub type_: Option<String>,
    pub title: Option<String>,
    pub aliases: Vec<String>,
    pub description: Option<String>,
}

/// フロントマターを開く区切り行か(`---`)。
///
/// **行末の空白と、ファイル先頭の BOM は区切りの一部として許す**(テスト計画 C3)。
/// 許さないと、`--- ` のファイルはフロントマター無しに見え、別名を足すときに新しい
/// フロントマターを頭に作って元の題名などを本文側へ押し出した。BOM の付いたファイル
/// (メモ帳などで保存したもの)には、別名を足しても黙って何も起きなかった。
/// `---text` は区切りではない(水平線や本文と区別する)
pub(crate) fn is_open_fence(line: &str) -> bool {
    line.trim_start_matches('\u{feff}').trim_end() == "---"
}

/// フロントマターを閉じる区切り行か(`---` か `...`。行末の空白は許す)
pub(crate) fn is_close_fence(line: &str) -> bool {
    let t = line.trim_end();
    t == "---" || t == "..."
}

/// ファイル全体を (フロントマター, 本文) に分ける。
///
/// フロントマターが無ければ `None` と全文を返す。
pub fn split(source: &str) -> (Option<&str>, &str) {
    // 先頭のBOMは許容する
    let trimmed = source.strip_prefix('\u{feff}').unwrap_or(source);
    // 1行目が区切りでなければフロントマター無し(区切りの後ろには改行が要る)
    let Some(first_end) = trimmed.find('\n').map(|i| i + 1) else {
        return (None, trimmed);
    };
    if !is_open_fence(&trimmed[..first_end]) {
        return (None, trimmed);
    }
    let rest = &trimmed[first_end..];

    // 閉じる区切りを行頭で探す
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        if is_close_fence(line) {
            let fm = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return (Some(fm), body);
        }
        offset += line.len();
    }
    // 閉じられていない場合は「フロントマター無し」と見なす(本文を失わない方を選ぶ)
    (None, trimmed)
}

/// フロントマター本体から既知フィールドだけを取り出す。
pub fn parse(fm: &str) -> FrontMatter {
    let mut out = FrontMatter::default();
    let mut lines = fm.lines().peekable();

    while let Some(line) = lines.next() {
        // インデントされた行はブロックの続き。キーとしては扱わない
        if line.starts_with(' ') || line.starts_with('\t') || line.trim().is_empty() {
            continue;
        }
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = strip_comment(value.trim());

        match key {
            "type" => out.type_ = non_empty(&unquote(value)),
            "title" => out.title = non_empty(&unquote(value)),
            "description" => out.description = non_empty(&unquote(value)),
            "aliases" => {
                if value.is_empty() {
                    // ブロック形式:
                    //   aliases:
                    //     - 架純
                    while let Some(next) = lines.peek() {
                        let t = next.trim();
                        // 途中のコメント行・空行は読み飛ばす。ここで止めると、
                        // 後ろの項目を別名として扱わなくなる(テスト計画 C1)
                        if t.is_empty() || t.starts_with('#') {
                            lines.next();
                            continue;
                        }
                        if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix('-')) {
                            if next.starts_with(' ') || next.starts_with('\t') || t.starts_with('-')
                            {
                                if let Some(v) = non_empty(&unquote(strip_comment(item.trim()))) {
                                    out.aliases.push(v);
                                }
                                lines.next();
                                continue;
                            }
                        }
                        break;
                    }
                } else {
                    // インライン形式: aliases: [架純, かすみん]
                    out.aliases = parse_inline_list(value);
                }
            }
            _ => {}
        }
    }
    out
}

/// 便利関数: ファイル全体から既知フィールドを取り出す
pub fn parse_source(source: &str) -> FrontMatter {
    match split(source).0 {
        Some(fm) => parse(fm),
        None => FrontMatter::default(),
    }
}

/// 既存ファイルに別名を足す(名寄せ用)。
///
/// **`aliases:` の行だけを書き換え、他の行はそのまま残す**。
/// フロントマター全体を作り直すと、V1が解釈しないフィールド(id/relations/progression 等)
/// が失われる([03-data-format.md](../../docs/03-data-format.md) §4.1)。
/// この関数はテキストとしての外科手術に徹する。
///
/// - 既に登録済みの別名、正式名と同じものは足さない
/// - 足すものが無ければ**元の文字列をそのまま返す**(無駄な書き込みを避ける)
/// - `aliases` 行が無ければ `title` の直後に挿入する
/// - フロントマターが無ければ先頭に作る
pub fn add_aliases(source: &str, additions: &[String]) -> String {
    let nl = if source.contains("\r\n") { "\r\n" } else { "\n" };
    let current = parse_source(source);

    let mut merged = current.aliases.clone();
    for a in additions {
        // 読み直したときの形にそろえてから比べる(改行は空白・前後の空白は落とす)
        let a = one_line(a);
        let a = a.trim();
        if a.is_empty() {
            continue;
        }
        if current.title.as_deref() == Some(a) {
            continue;
        }
        if merged.iter().any(|m| m == a) {
            continue;
        }
        merged.push(a.to_string());
    }
    if merged.len() == current.aliases.len() {
        return source.to_string();
    }

    let alias_line = format!(
        "aliases: [{}]",
        merged
            .iter()
            .map(|a| quote_if_needed(a))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // フロントマターが無ければ作る。
    // ただし `---` で始まるのに閉じられていない場合は**壊れたフロントマター**なので触らない
    // (先頭に足すと `---` が二重になり、さらに壊れる)
    if split(source).0.is_none() {
        let looks_broken = source.lines().next().is_some_and(is_open_fence);
        if looks_broken {
            return source.to_string();
        }
        return format!("---{nl}{alias_line}{nl}---{nl}{nl}{source}");
    }

    let mut out = String::with_capacity(source.len() + alias_line.len() + 8);
    let mut fence = 0u8;
    let mut wrote = false;
    let mut skipping_block = false;

    for line in source.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);

        // 1行目は開く区切り(BOM・行末の空白を許す。split と同じ判定)
        let is_fence = if fence == 0 {
            is_open_fence(line)
        } else {
            is_close_fence(line)
        };
        if is_fence {
            fence += 1;
            // フロントマターを閉じる直前で、まだ書けていなければここで入れる
            if fence == 2 && !wrote {
                out.push_str(&alias_line);
                out.push_str(nl);
                wrote = true;
            }
            skipping_block = false;
            out.push_str(line);
            continue;
        }

        let in_fm = fence == 1;
        if in_fm {
            // ブロック形式の続き( - 項目 )は読み飛ばす。**インデントの有無は問わない**
            // (`aliases:` の次の行が `- 黒木` でも YAML として正しい。読む側の parse も
            // そう扱う)。残すと `aliases: [...]` の下に項目行が並び、ほかのツールで
            // 読めないフロントマターになった(テスト計画 C1)
            if skipping_block {
                let t = bare.trim_start();
                if t.starts_with('-') {
                    continue;
                }
                // 途中のコメント行・空行は残して、項目行の読み飛ばしを続ける(parse と同じ扱い)
                if t.is_empty() || t.starts_with('#') {
                    out.push_str(line);
                    continue;
                }
                skipping_block = false;
            }
            if !wrote && bare.trim_start().starts_with("aliases:") {
                out.push_str(&alias_line);
                out.push_str(nl);
                wrote = true;
                // インライン形式なら後続は無い。ブロック形式なら項目行を捨てる
                skipping_block = bare.split_once(':').map(|(_, v)| v.trim().is_empty()).unwrap_or(false);
                continue;
            }
        }
        out.push_str(line);
    }

    // フロントマターが閉じていない等の異常時は末尾に足さない(壊さない方を選ぶ)
    if !wrote {
        return source.to_string();
    }
    out
}

/// `title:` `description:` などの値(1行の文字列)を、読み直して同じに戻る形で書く。
///
/// そのまま書くと、` #` 以降がコメントになって名前が切れる(「第1話 #序章」→「第1話」)。
/// 先頭の `[` `&` `!` `%` などや `: ` を含む値は、ほかのツールが読めないフロントマターになり、
/// 改行を含む値(抽出したAIの説明など)はフロントマターの形そのものを壊す(テスト計画 C2)。
///
/// 要るときだけ単一引用符で包む(中の `'` は `''`)。単一引用符はバックスラッシュを
/// 特別扱いしないので、どのツールでも同じに読める。改行などの制御文字は空白にする
pub fn yaml_scalar(value: &str) -> String {
    let flat = one_line(value);
    if needs_quotes(&flat) {
        single_quoted(&flat)
    } else {
        flat
    }
}

/// 1行の値にする(改行などの制御文字は空白に。改行のまま書くとフロントマターの形が壊れる)。
/// 行区切り(U+2028)と段落区切り(U+2029)も空白にする。制御文字ではないが、YAML 1.1 の
/// 読み手(libyaml・PyYAML)は改行として読み、フロントマター全体が読めなくなる(テスト計画 C5)
fn one_line(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// そのままでは YAML の値として読み直せないか。
/// 先頭が記号・`: ` や ` #` を含む・末尾が `:`・前後に空白・引用符を含む
fn needs_quotes(flat: &str) -> bool {
    flat.chars()
        .next()
        .is_some_and(|c| "-?:,[]{}#&*!|>'\"%@`".contains(c))
        || flat.contains(": ")
        || flat.contains(" #")
        || flat.ends_with(':')
        || flat.starts_with(char::is_whitespace)
        || flat.ends_with(char::is_whitespace)
        || flat.contains(['"', '\''])
}

/// 単一引用符で包む(中の `'` は `''`。バックスラッシュは特別扱いしない)
fn single_quoted(flat: &str) -> String {
    format!("'{}'", flat.replace('\'', "''"))
}

/// YAML で意味を持つ文字を含む名前(ファイル名に使えるもの)。テスト用。
/// そのまま `title: ` の後ろに置くと、` #` 以降がコメントになって名前が切れたり、
/// 先頭の `[` `&` `!` などでほかのツールが読めないフロントマターになる
#[cfg(test)]
pub(crate) const YAML_TRICKY: &[&str] = &[
    "第1話 #序章",
    "[序] 始まり",
    "&印",
    "!注意",
    "'単'の話",
    "%率",
    "@名",
    "- 前置き",
    "? 問い",
    "{波}",
    "`記号`",
    "場面: 再会",
    "終わり:",
    "He said \"hi\"",
    "  前後の空白  ",
];

/// YAMLのインライン配列(`[a, b]`)の要素として書く。壊れる文字があれば引用符で包む。
///
/// 配列の中では `,` `[` `]` `{` `}` も区切りになる。以前は二重引用符で包み `"` を `\"` に
/// していたが、読み手は `\"` の `"` で引用を閉じてしまうので、`"` の後ろにカンマがある
/// 別名(`",「?` など)が2つに割れた。改行を含む別名はフロントマターの形を壊した
/// (テスト計画 C4)。題名と同じく単一引用符で包む
pub(crate) fn quote_if_needed(value: &str) -> String {
    let flat = one_line(value);
    if needs_quotes(&flat) || flat.contains([',', '[', ']', '{', '}', '#', ':']) {
        single_quoted(&flat)
    } else {
        flat
    }
}

fn parse_inline_list(value: &str) -> Vec<String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    split_top_level(inner)
        .iter()
        .filter_map(|p| non_empty(&unquote(p.trim())))
        .collect()
}

/// 引用符の外にあるカンマだけで割る。
///
/// 素朴に `split(',')` すると `["黒木, 龍一", "教授"]` のような別名が途中で切れる。
/// 書き手(quote_if_needed)がカンマを含む値を引用符で包む以上、
/// 読み手も引用符を見なければ往復が壊れる。
fn split_top_level(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in inner.chars() {
        match quote {
            Some(q) => {
                cur.push(ch);
                if ch == q {
                    quote = None;
                }
            }
            None => match ch {
                '"' | '\'' => {
                    quote = Some(ch);
                    cur.push(ch);
                }
                ',' => out.push(std::mem::take(&mut cur)),
                _ => cur.push(ch),
            },
        }
    }
    out.push(cur);
    out
}

/// 行末コメントを落とす。ただし引用符の中の `#` は残す
fn strip_comment(value: &str) -> &str {
    let bytes = value.as_bytes();
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'#'
                // ` #` の形だけをコメント開始と見なす(URL の # を誤爆させない)
                && (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') => {
                    return value[..i].trim_end();
                }
            None => {}
        }
    }
    value
}

/// 引用符を外す。**書き手が付けたエスケープもここで戻す。**
///
/// 外すだけだと往復で壊れる:
///  - 二重引用符: quote_if_needed が `"` を `\"` にして書く
///  - 単一引用符: saveNote.ts が YAML の規則どおり `'` を `''` にして書く
///
/// 戻さないと、件名や別名に `\"` や `''` が見えたまま残る。
fn unquote(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        return v[1..v.len() - 1].replace("\\\"", "\"");
    }
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        return v[1..v.len() - 1].replace("''", "'");
    }
    v.to_string()
}

fn non_empty(value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// フロントマターの `title:` 行だけを外科的に書き換える。
///
/// **再シリアライズしない**(03-data-format §4.1)。他の行はバイト単位で温存し、
/// 未知フィールドを壊さない。`title` が無ければ開始の `---` 直後へ挿入する。
///
/// フロントマターごと無い場合は、本文の前に付ける。
pub fn set_title(source: &str, title: &str) -> String {
    // 見出しはAIが付けることもある(シーンの分割)。YAML として読み直せる形にして書く
    let title = yaml_scalar(title);
    let (fm, body) = split(source);
    let Some(fm) = fm else {
        return format!("---\ntitle: {title}\n---\n\n{source}");
    };
    let mut out = String::from("---\n");
    let mut done = false;
    for line in fm.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if !done && bare.trim_start().starts_with("title:") {
            out.push_str(&format!("title: {title}\n"));
            done = true;
            continue;
        }
        out.push_str(line);
    }
    if !done {
        out = format!("---\ntitle: {title}\n{}", &out["---\n".len()..]);
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("---\n");
    out.push_str(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 引用符の中のカンマで別名が切れないこと。
    /// 書き手がカンマを含む値を包む以上、読み手も引用符を見なければ往復が壊れる
    #[test]
    fn aliases_keep_commas_inside_quotes() {
        let src = "---\naliases: [\"黒木, 龍一\", 教授, '佐藤, 架純']\n---\n本文\n";
        let fm = parse(src);
        assert_eq!(fm.aliases, vec!["黒木, 龍一", "教授", "佐藤, 架純"]);
    }

    /// 引用符のエスケープが往復で戻ること。
    /// 戻さないと、件名に `''` や `\"` が見えたまま残る
    #[test]
    fn quoted_escapes_survive_a_round_trip() {
        let src = "---\ntitle: '架純''s: 展開案'\n---\n";
        assert_eq!(parse(src).title.as_deref(), Some("架純's: 展開案"));

        let src = "---\ntitle: \"引用の\\\"中\\\"\"\n---\n";
        assert_eq!(parse(src).title.as_deref(), Some("引用の\"中\""));
    }

    #[test]
    fn splits_frontmatter_and_body() {
        let src = "---\ntitle: 出会い\n---\n本文です\n";
        let (fm, body) = split(src);
        assert_eq!(fm, Some("title: 出会い\n"));
        assert_eq!(body, "本文です\n");
    }

    #[test]
    fn no_frontmatter_returns_whole_body() {
        let src = "　転校初日の朝は、雨だった。\n";
        let (fm, body) = split(src);
        assert!(fm.is_none());
        assert_eq!(body, src);
    }

    #[test]
    fn unclosed_frontmatter_keeps_body() {
        // 閉じ忘れで本文が消えるのが最悪なので、フロントマター無し扱いにする
        let src = "---\ntitle: 出会い\n本文\n";
        let (fm, body) = split(src);
        assert!(fm.is_none());
        assert_eq!(body, src);
    }

    #[test]
    fn horizontal_rule_is_not_frontmatter() {
        let src = "---本文ではない\n";
        assert!(split(src).0.is_none());
    }

    #[test]
    fn parses_known_fields_only() {
        let fm = "type: character\ntitle: 佐藤架純\naliases: [架純, かすみん]\ndescription: 主人公。\nid: chr-kasumi\nimportance: main\n";
        let got = parse(fm);
        assert_eq!(got.type_.as_deref(), Some("character"));
        assert_eq!(got.title.as_deref(), Some("佐藤架純"));
        assert_eq!(got.aliases, vec!["架純", "かすみん"]);
        assert_eq!(got.description.as_deref(), Some("主人公。"));
    }

    #[test]
    fn parses_block_style_aliases() {
        let fm = "title: 佐藤架純\naliases:\n  - 架純\n  - かすみん\ntype: character\n";
        let got = parse(fm);
        assert_eq!(got.aliases, vec!["架純", "かすみん"]);
        assert_eq!(got.type_.as_deref(), Some("character"));
    }

    #[test]
    fn strips_quotes_and_comments() {
        let fm = "title: \"佐藤架純\"  # 主人公\naliases: ['架純']\n";
        let got = parse(fm);
        assert_eq!(got.title.as_deref(), Some("佐藤架純"));
        assert_eq!(got.aliases, vec!["架純"]);
    }

    #[test]
    fn set_title_replaces_only_that_line() {
        // シーン分割で使う。**未知フィールドを壊さないこと**が要点
        let src = "---\ntype: scene\ntitle: 元の見出し\npov: 架純\nprogression:\n  - at: 三章\n---\n本文\n";
        let got = set_title(src, "新しい見出し");
        assert!(got.contains("title: 新しい見出し"));
        assert!(!got.contains("元の見出し"));
        assert!(got.contains("pov: 架純"), "他の行が消えた: {got}");
        assert!(got.contains("  - at: 三章"), "入れ子が消えた: {got}");
        assert!(got.ends_with("本文\n"));
    }

    #[test]
    fn set_title_inserts_when_absent() {
        let got = set_title("---\ntype: scene\n---\n本文\n", "見出し");
        assert!(got.starts_with("---\ntitle: 見出し\ntype: scene\n---\n"));
        assert!(got.ends_with("本文\n"));
    }

    #[test]
    fn set_title_handles_missing_frontmatter() {
        let got = set_title("　本文だけがある。\n", "見出し");
        assert_eq!(parse_source(&got).title.as_deref(), Some("見出し"));
        assert!(got.contains("　本文だけがある。"));
    }

    #[test]
    fn reads_titles_written_by_the_app() {
        // AIの出力を ideas/ と reviews/ へ残すとき、件名は人が入力するので
        // コロンや記号が混ざりうる(src/components/saveNote.ts)。
        // **書く側は単一引用符で囲む**。ここが読めないと、残した資産の表示名が壊れる
        let got = parse("title: '架純: 展開案'\n");
        assert_eq!(got.title.as_deref(), Some("架純: 展開案"));

        // 「#」から始まる件名。囲んでいればコメントとして切られない
        assert_eq!(
            parse("title: '#タグ風'\n").title.as_deref(),
            Some("#タグ風")
        );

        // 二重引用符を含む件名。単一引用符で囲むので、囲みを外すだけで元に戻る
        assert_eq!(
            parse("title: '「\"引用\"」: メモ'\n").title.as_deref(),
            Some("「\"引用\"」: メモ")
        );
    }

    #[test]
    fn tolerates_broken_yaml() {
        // コロンが無い行・空行・入れ子があっても落ちない
        let fm = "title: 佐藤架純\nこれは壊れた行\n\nrelations:\n  - to: chr-yuji\n    rel: 幼馴染\n";
        let got = parse(fm);
        assert_eq!(got.title.as_deref(), Some("佐藤架純"));
        assert!(got.aliases.is_empty());
    }

    // ===== 名寄せ: 別名の追加 =====

    #[test]
    fn adds_aliases_to_inline_form() {
        let src = "---\ntype: character\ntitle: 黒木龍一\naliases: [黒木]\n---\n\n本文\n";
        let got = add_aliases(src, &["龍一".into(), "教授".into()]);
        assert!(got.contains("aliases: [黒木, 龍一, 教授]"), "{got}");
        // 他の行は温存されること
        assert!(got.contains("type: character"));
        assert!(got.contains("title: 黒木龍一"));
        assert!(got.ends_with("本文\n"));
    }

    #[test]
    fn adds_aliases_to_block_form() {
        let src = "---\ntitle: 黒木龍一\naliases:\n  - 黒木\n  - 龍一\ntype: character\n---\n本文\n";
        let got = add_aliases(src, &["教授".into()]);
        assert!(got.contains("aliases: [黒木, 龍一, 教授]"), "{got}");
        // ブロックの項目行が残っていないこと
        assert!(!got.contains("  - 黒木"), "{got}");
        // 後続のキーは温存
        assert!(got.contains("type: character"));
        assert!(got.ends_with("本文\n"));
    }

    /// テスト計画 C1: **インデントの無い**ブロック形式も YAML として正しい。
    /// 項目行を残すと `aliases: [...]` の下に `- 黒木` が並び、ほかのツールで読めなくなる
    #[test]
    fn adds_aliases_to_an_unindented_block_form() {
        let src = "---\ntitle: 黒木龍一\naliases:\n- 黒木\n- 龍一\ntype: character\n---\n本文\n";
        assert_eq!(parse_source(src).aliases, vec!["黒木", "龍一"]);
        let got = add_aliases(src, &["教授".into()]);
        assert_eq!(
            got,
            "---\ntitle: 黒木龍一\naliases: [黒木, 龍一, 教授]\ntype: character\n---\n本文\n"
        );
    }

    /// 別名のブロックの途中にコメント行があっても、後ろの項目を落とさない
    #[test]
    fn block_aliases_survive_a_comment_line() {
        let src = "---\ntitle: 黒木龍一\naliases:\n  - 黒木  # 苗字\n  # 呼び名\n  - 龍一\n---\n本文\n";
        assert_eq!(parse_source(src).aliases, vec!["黒木", "龍一"]);
        let got = add_aliases(src, &["教授".into()]);
        assert_eq!(parse_source(&got).aliases, vec!["黒木", "龍一", "教授"], "{got}");
        assert!(!got.contains("- 龍一"), "項目行が残っている: {got}");
    }

    /// 空・スカラー・引用符付きの形にも足せる
    #[test]
    fn adds_aliases_to_other_inline_shapes() {
        for (src, want) in [
            ("---\ntitle: 黒木\naliases: []\n---\n", vec!["教授"]),
            ("---\ntitle: 黒木\naliases: 龍一\n---\n", vec!["龍一", "教授"]),
            ("---\ntitle: 黒木\naliases: [\"龍一\", '黒木さん']\n---\n", vec!["龍一", "黒木さん", "教授"]),
        ] {
            let got = add_aliases(src, &["教授".into()]);
            assert_eq!(parse_source(&got).aliases, want, "{src:?} -> {got:?}");
        }
    }

    /// CRLF のファイルでも同じ(項目行を残さない・改行コードを変えない)
    #[test]
    fn adds_aliases_to_an_unindented_block_form_with_crlf() {
        let src = "---\r\ntitle: 黒木龍一\r\naliases:\r\n- 黒木\r\n- 龍一\r\n---\r\n本文\r\n";
        let got = add_aliases(src, &["教授".into()]);
        assert_eq!(
            got,
            "---\r\ntitle: 黒木龍一\r\naliases: [黒木, 龍一, 教授]\r\n---\r\n本文\r\n"
        );
    }

    #[test]
    fn inserts_aliases_when_absent() {
        let src = "---\ntype: character\ntitle: 黒木龍一\n---\n\n本文\n";
        let got = add_aliases(src, &["教授".into()]);
        assert!(got.contains("aliases: [教授]"), "{got}");
        assert!(got.contains("title: 黒木龍一"));
        assert!(got.ends_with("本文\n"));
    }

    #[test]
    fn preserves_unknown_fields() {
        // V1が解釈しないフィールドを失わないこと(再シリアライズしない原則)
        let src = "---\nid: chr-kuroki\ntitle: 黒木龍一\nrelations:\n  - to: chr-x\n    rel: 師弟\nprogression:\n  - at: 第三章\n    note: 失踪\n---\n本文\n";
        let got = add_aliases(src, &["教授".into()]);
        assert!(got.contains("id: chr-kuroki"), "{got}");
        assert!(got.contains("  - to: chr-x"), "{got}");
        assert!(got.contains("    note: 失踪"), "{got}");
        assert!(got.contains("aliases: [教授]"), "{got}");
    }

    #[test]
    fn skips_duplicates_and_title() {
        let src = "---\ntitle: 黒木龍一\naliases: [黒木]\n---\n";
        // 登録済み・正式名と同じものは足さない
        let got = add_aliases(src, &["黒木".into(), "黒木龍一".into()]);
        assert_eq!(got, src, "足すものが無ければ原文のまま返す");
    }

    #[test]
    fn quotes_values_that_would_break_inline_array() {
        let src = "---\ntitle: X\n---\n";
        let got = add_aliases(src, &["a, b".into()]);
        // 単一引用符で包む(二重引用符と \" では、" の後ろのカンマで割れた。テスト計画 C4)
        assert!(got.contains("aliases: ['a, b']"), "{got}");
        assert_eq!(parse_source(&got).aliases, vec!["a, b"]);
    }

    #[test]
    fn creates_frontmatter_when_missing() {
        let src = "本文だけのファイル\n";
        let got = add_aliases(src, &["教授".into()]);
        assert!(got.starts_with("---\naliases: [教授]\n---\n"), "{got}");
        assert!(got.ends_with("本文だけのファイル\n"));
    }

    #[test]
    fn keeps_crlf_line_endings() {
        let src = "---\r\ntitle: X\r\n---\r\n本文\r\n";
        let got = add_aliases(src, &["別名".into()]);
        assert!(got.contains("aliases: [別名]\r\n"), "{got:?}");
        assert!(got.contains("title: X\r\n"));
    }

    #[test]
    fn unclosed_frontmatter_is_left_untouched() {
        // 壊れたファイルは触らない(壊さない方を選ぶ)
        let src = "---\ntitle: X\n本文\n";
        assert_eq!(add_aliases(src, &["別名".into()]), src);
    }

    #[test]
    fn crlf_is_supported() {
        let src = "---\r\ntitle: 出会い\r\n---\r\n本文\r\n";
        let (fm, body) = split(src);
        assert!(fm.is_some());
        assert_eq!(body, "本文\r\n");
        assert_eq!(parse(fm.unwrap()).title.as_deref(), Some("出会い"));
    }

    /// テスト計画 C2: 見出しの書き換え(シーンの分割で AI が付けた見出し)も、
    /// YAML の記号や改行を含んだまま読み直して同じに戻る
    #[test]
    fn set_title_survives_tricky_titles() {
        for title in YAML_TRICKY {
            for src in ["---\ntitle: 元\n---\n本文\n", "---\ntype: scene\n---\n本文\n", "本文だけ\n"] {
                let got = set_title(src, title);
                assert_eq!(parse_source(&got).title.as_deref(), Some(title.trim()), "{got:?}");
            }
        }
        // 改行は空白にする(見出しは1行。改行のまま書くとフロントマターが壊れる)
        let got = set_title("本文\n", "一行目\n二行目");
        assert_eq!(parse_source(&got).title.as_deref(), Some("一行目 二行目"));
        assert_eq!(split(&got).1, "\n本文\n");
    }

    /// テスト計画 C3: BOM の付いたファイル(メモ帳などで保存したもの)にも別名を足せる。
    /// 以前は1行目の「BOM+---」を区切りと見なさず、何もせずに元のまま返していた(黙って効かない)
    #[test]
    fn aliases_are_added_to_a_file_with_bom() {
        let src = "\u{feff}---\ntitle: 黒木\naliases: [龍一]\n---\n本文\n";
        let got = add_aliases(src, &["教授".into()]);
        assert_eq!(got, "\u{feff}---\ntitle: 黒木\naliases: [龍一, 教授]\n---\n本文\n");
    }

    /// 区切りの末尾に空白がある(`--- `)。区切りとして読み、別名を足すときに
    /// 新しいフロントマターを頭に作らない(作ると元の題名などが本文側へ押し出される)
    #[test]
    fn fences_with_trailing_spaces_are_still_fences() {
        let src = "--- \ntitle: 黒木\n---\t\n本文\n";
        assert_eq!(parse_source(src).title.as_deref(), Some("黒木"));
        assert_eq!(split(src).1, "本文\n");
        let got = add_aliases(src, &["教授".into()]);
        assert_eq!(got, "--- \ntitle: 黒木\naliases: [教授]\n---\t\n本文\n");
    }

    /// 壊れたフロントマター(閉じ忘れ)には、別名を足さない(足すと壊れた側が広がる)
    #[test]
    fn broken_frontmatter_is_never_widened() {
        for src in [
            "---\ntitle: 黒木\n本文\n",
            "\u{feff}---\ntitle: 黒木\n本文\n",
            "--- \ntitle: 黒木\n本文\n",
        ] {
            assert_eq!(add_aliases(src, &["教授".into()]), src, "{src:?}");
        }
    }

    /// テスト計画 C4: 別名を足して読み直すと、同じ別名の並びに戻る(往復)
    #[test]
    fn tricky_aliases_survive_the_round_trip() {
        let tricky = [
            "黒木, 龍一",
            "He said \"hi\"",
            "ゴロウ's",
            "'教授'",
            "C#",
            "a #b",
            "時刻: 夜",
            "[仮]",
            "{波}",
            "&印",
            "*星",
            "!注意",
            "- 前",
            "黒木、龍一",
            "「教授」",
            "＃全角",
            "a\\b",
            "\"a\", 'b'",
            "改\n行",
            // 行区切り・段落区切り。YAML 1.1 の読み手は改行として読む(テスト計画 C5)
            "行\u{2028}区\u{2029}切",
        ];
        let src = "---\ntitle: 黒木\naliases: [龍一]\n---\n本文\n";
        for alias in tricky {
            let got = add_aliases(src, &[alias.to_string()]);
            assert!(!got.contains(['\u{2028}', '\u{2029}']), "{alias:?} → {got:?}");
            let expected_alias = alias.replace(['\n', '\u{2028}', '\u{2029}'], " ");
            assert_eq!(
                parse_source(&got).aliases,
                vec!["龍一".to_string(), expected_alias],
                "{alias:?} → {got:?}"
            );
            assert_eq!(split(&got).1, "本文\n", "フロントマターが壊れた: {got:?}");
        }
    }

    /// 読み直したときの形でそろえて比べる。改行を含む同じ別名を、もう1つ足さない
    #[test]
    fn alias_with_a_newline_is_not_added_twice() {
        let src = "---\ntitle: 黒木\naliases: [改 行]\n---\n本文\n";
        assert_eq!(add_aliases(src, &["改\n行".into()]), src);
    }

    /// 配列の区切り(`,` `[` `]` `{` `}`)が途中にある別名も包む。アプリの読み手は包まなくても
    /// 読めるが、ほかのツール(標準の YAML の読み手)では配列が壊れる
    #[test]
    fn flow_indicators_inside_an_alias_are_quoted() {
        for v in ["a,b", "a[b", "a]b", "a{b", "a}b"] {
            assert_eq!(quote_if_needed(v), format!("'{v}'"));
        }
        assert_eq!(quote_if_needed("黒木"), "黒木", "要らないときは包まない");
    }

    /// 乱数で作った別名(固定シード)でも往復する
    #[test]
    fn random_aliases_survive_the_round_trip() {
        const PIECES: &[&str] = &[
            ",", "\"", "'", "#", ":", " ", "[", "]", "{", "}", "&", "*", "!", "\\", "-", "?", "|",
            "、", "「", "」", "黒", "木", "a", "\n", "\t",
        ];
        let mut seed = 20261004u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _ in 0..2000 {
            let alias: String = (0..1 + next() % 6).map(|_| PIECES[next() % PIECES.len()]).collect();
            // 足す側は前後の空白を落とし、改行などの制御文字は空白にして読む形になる
            let expected = alias.replace(['\n', '\t'], " ").trim().to_string();
            if expected.is_empty() || expected == "黒木" || expected == "龍一" {
                continue;
            }
            let src = "---\ntitle: 黒木\naliases: [龍一]\n---\n本文\n";
            let got = add_aliases(src, std::slice::from_ref(&alias));
            assert_eq!(
                parse_source(&got).aliases,
                vec!["龍一".to_string(), expected.clone()],
                "{alias:?} → {got:?}"
            );
            assert_eq!(split(&got).1, "本文\n", "フロントマターが壊れた: {got:?}");
        }
    }

    /// 引用の要らない値はそのまま書く(既存のファイルの見た目を変えない)
    #[test]
    fn plain_values_stay_plain() {
        for v in ["佐藤架純", "第1話", "a-b", "見出し#1", "C#", "ゴロウ!", "時刻 12:30"] {
            assert_eq!(yaml_scalar(v), v);
        }
    }
}
