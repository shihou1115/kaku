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

/// ファイル全体を (フロントマター, 本文) に分ける。
///
/// フロントマターが無ければ `None` と全文を返す。
pub fn split(source: &str) -> (Option<&str>, &str) {
    // 先頭のBOM・空行は許容する
    let trimmed = source.strip_prefix('\u{feff}').unwrap_or(source);
    let rest = match trimmed.strip_prefix("---") {
        Some(r) => r,
        None => return (None, trimmed),
    };
    // "---" の直後は改行でなければならない(水平線 "---text" と区別する)
    let rest = match rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n')) {
        Some(r) => r,
        None => return (None, trimmed),
    };

    // 終端の "---" を行頭で探す
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if bare == "---" || bare == "..." {
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
        let head = source.strip_prefix('\u{feff}').unwrap_or(source);
        let looks_broken = head
            .strip_prefix("---")
            .map(|r| r.starts_with('\n') || r.starts_with("\r\n"))
            .unwrap_or(false);
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

        if bare == "---" || bare == "..." {
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
            // ブロック形式の続き( - 項目 )は読み飛ばす
            if skipping_block {
                let t = bare.trim_start();
                if t.starts_with('-') && (bare.starts_with(' ') || bare.starts_with('\t')) {
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

/// YAMLのインライン配列に入れて壊れる文字があれば引用符で包む
fn quote_if_needed(value: &str) -> String {
    if value.contains([',', '[', ']', '"', '\'', ':', '#']) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
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
        assert!(got.contains("aliases: [\"a, b\"]"), "{got}");
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
}
