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
            "type" => out.type_ = non_empty(unquote(value)),
            "title" => out.title = non_empty(unquote(value)),
            "description" => out.description = non_empty(unquote(value)),
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
                                if let Some(v) = non_empty(unquote(strip_comment(item.trim()))) {
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

fn parse_inline_list(value: &str) -> Vec<String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    inner
        .split(',')
        .filter_map(|p| non_empty(unquote(p.trim())))
        .collect()
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
            None if b == b'#' => {
                // ` #` の形だけをコメント開始と見なす(URL の # を誤爆させない)
                if i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t' {
                    return value[..i].trim_end();
                }
            }
            None => {}
        }
    }
    value
}

fn unquote(value: &str) -> &str {
    let v = value.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return &v[1..v.len() - 1];
        }
    }
    v
}

fn non_empty(value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn tolerates_broken_yaml() {
        // コロンが無い行・空行・入れ子があっても落ちない
        let fm = "title: 佐藤架純\nこれは壊れた行\n\nrelations:\n  - to: chr-yuji\n    rel: 幼馴染\n";
        let got = parse(fm);
        assert_eq!(got.title.as_deref(), Some("佐藤架純"));
        assert!(got.aliases.is_empty());
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
