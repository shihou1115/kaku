//! codex・原稿の新規作成テンプレート。
//!
//! 方針(P-5と同じ思想):
//! - テンプレートは**普通のMarkdownファイル**として設定フォルダに置く。
//!   ユーザーはエディタで自由に書き換えられるし、ジャンルを増やすのも
//!   フォルダを1つ作るだけでよい。
//! - 初回起動時に既定を書き出すが、**既にあるファイルは絶対に上書きしない**
//!   (ユーザーの編集を壊さない)。
//! - 構造化した項目は**フロントマターではなく本文の見出し・箇条書き**に置く。
//!   V1が解釈するフロントマターは type/title/aliases/description の4つだけ
//!   という規約(docs/03-data-format.md §4.1)を、テンプレート側から崩さないため。
//!
//! 配置: `<config_dir>/kaku/templates/<ジャンル>/<種別>.md`

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

/// 既定テンプレート: (ジャンル, 種別, 本文)
const BUILTIN: &[(&str, &str, &str)] = &[
    // ---------------- 汎用 ----------------
    (
        "汎用",
        "character",
        r#"---
type: character
title:
aliases: []
description:
---

## 基本
- 性別:
- 年齢:
- 職業・立場:

## 外見

## 性格
- 長所:
- 短所:
- 恐れているもの:

## 話し方
- 一人称:
- 二人称:
- 口癖:

## 背景

## この物語での役割
- 目的(何が欲しいか):
- 障害(何が邪魔をするか):
- 変化(物語の終わりにどうなるか):
"#,
    ),
    (
        "汎用",
        "location",
        r#"---
type: location
title:
aliases: []
description:
---

## どんな場所か

## 見え方・音・匂い

## ここで起きること

## 関係する人物
"#,
    ),
    (
        "汎用",
        "item",
        r#"---
type: item
title:
aliases: []
description:
---

## どんな物か

## 由来

## 物語での役割
"#,
    ),
    (
        "汎用",
        "term",
        r#"---
type: term
title:
aliases: []
description:
---

## 定義

## 補足・注意
"#,
    ),
    (
        "汎用",
        "note",
        r#"---
type: note
title:
aliases: []
description:
---

"#,
    ),
    (
        "汎用",
        "scene",
        r#"---
title:
status: draft
pov:
synopsis:
---

"#,
    ),
    // ---------------- ファンタジー ----------------
    (
        "ファンタジー",
        "character",
        r#"---
type: character
title:
aliases: []
description:
---

## 基本
- 性別:
- 年齢:
- 種族:
- 身分・所属:

## 外見

## 能力
- 使える魔法・技:
- 代償・制約:

## 性格
- 長所:
- 短所:
- 恐れているもの:

## 話し方
- 一人称:
- 口癖:

## 背景

## この物語での役割
- 目的:
- 障害:
"#,
    ),
    (
        "ファンタジー",
        "location",
        r#"---
type: location
title:
aliases: []
description:
---

## どんな場所か
- 地方・国:
- 気候・地形:

## 住む者・治める者

## 風習・禁忌

## ここで起きること
"#,
    ),
    (
        "ファンタジー",
        "term",
        r#"---
type: term
title:
aliases: []
description:
---

## 定義

## 仕組み
- 何ができるか:
- 何ができないか(制約):
- 代償:

## 世間の扱い
"#,
    ),
    // ---------------- SF ----------------
    (
        "SF",
        "character",
        r#"---
type: character
title:
aliases: []
description:
---

## 基本
- 性別:
- 年齢:
- 出身(惑星・区画・年代):
- 所属組織:

## 外見・身体
- 義体・改造の有無:

## 性格
- 長所:
- 短所:
- 恐れているもの:

## 話し方
- 一人称:
- 口癖:

## 背景

## この物語での役割
- 目的:
- 障害:
"#,
    ),
    (
        "SF",
        "location",
        r#"---
type: location
title:
aliases: []
description:
---

## どんな場所か
- 位置・規模:
- 重力・大気・環境:

## 統治・経済

## 技術水準

## ここで起きること
"#,
    ),
    (
        "SF",
        "term",
        r#"---
type: term
title:
aliases: []
description:
---

## 定義

## 原理・仕組み
- できること:
- 限界・制約:
- 副作用・コスト:

## 社会への影響
"#,
    ),
    // ---------------- ミステリ ----------------
    (
        "ミステリ",
        "character",
        r#"---
type: character
title:
aliases: []
description:
---

## 基本
- 性別:
- 年齢:
- 職業:
- 事件での立場(探偵/被害者/容疑者/関係者):

## 外見

## 性格
- 長所:
- 短所:

## 話し方
- 一人称:
- 口癖:

## 事件との関係
- 動機になりうるもの:
- アリバイ:
- 隠していること:

## 背景
"#,
    ),
    (
        "ミステリ",
        "location",
        r#"---
type: location
title:
aliases: []
description:
---

## どんな場所か

## 見取り図・出入口

## 事件当日の状況
- 施錠・人の出入り:
- 目撃者:

## 現場に残るもの
"#,
    ),
    (
        "ミステリ",
        "term",
        r#"---
type: term
title:
aliases: []
description:
---

## 定義

## 事件での意味

## 読者に伏せること
"#,
    ),
    // ---------------- 恋愛 ----------------
    (
        "恋愛",
        "character",
        r#"---
type: character
title:
aliases: []
description:
---

## 基本
- 性別:
- 年齢:
- 職業・学年:

## 外見

## 性格
- 長所:
- 短所:
- 恋愛での癖:

## 話し方
- 一人称:
- 相手の呼び方:
- 口癖:

## 背景
- 過去の恋愛:
- 家族:

## この物語での役割
- 求めているもの:
- 素直になれない理由:
"#,
    ),
    (
        "恋愛",
        "location",
        r#"---
type: location
title:
aliases: []
description:
---

## どんな場所か

## ふたりにとっての意味

## ここで起きること
"#,
    ),
];

#[derive(Debug, Clone, Serialize)]
pub struct TemplateInfo {
    pub genre: String,
    /// character / location / item / term / note / scene
    pub kind: String,
}

/// テンプレートの置き場所。設定フォルダが取れない環境では一時フォルダに退避する。
pub fn templates_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("kaku")
        .join("templates")
}

/// 既定テンプレートを書き出す。**既存ファイルは上書きしない**。
pub fn ensure_defaults() -> std::io::Result<()> {
    let base = templates_dir();
    for (genre, kind, body) in BUILTIN {
        let dir = base.join(genre);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{kind}.md"));
        if !path.exists() {
            fs::write(path, body.replace("\r\n", "\n"))?;
        }
    }
    Ok(())
}

/// 利用可能なテンプレートを列挙する(ユーザーが足したジャンル・種別も拾う)。
pub fn list() -> std::io::Result<Vec<TemplateInfo>> {
    let base = templates_dir();
    let mut out = Vec::new();
    if !base.exists() {
        return Ok(out);
    }
    for genre_entry in fs::read_dir(&base)? {
        let genre_entry = genre_entry?;
        if !genre_entry.file_type()?.is_dir() {
            continue;
        }
        let genre = genre_entry.file_name().to_string_lossy().to_string();
        if genre.starts_with('.') {
            continue;
        }
        for file in fs::read_dir(genre_entry.path())? {
            let file = file?;
            let path = file.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Some(kind) = path.file_stem() {
                    out.push(TemplateInfo {
                        genre: genre.clone(),
                        kind: kind.to_string_lossy().to_string(),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| (&a.genre, &a.kind).cmp(&(&b.genre, &b.kind)));
    Ok(out)
}

/// テンプレート本文を読む。`title` 行には作成時の名前を差し込む。
pub fn render(genre: &str, kind: &str, title: &str) -> std::io::Result<String> {
    let path = templates_dir().join(genre).join(format!("{kind}.md"));
    let body = fs::read_to_string(path)?;
    Ok(fill_title(&body, title))
}

/// フロントマター内の空の `title:` に名前を入れる。
/// 既に値が入っているテンプレートには触らない。
pub fn fill_title(body: &str, title: &str) -> String {
    if title.trim().is_empty() {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len() + title.len());
    let mut done = false;
    let mut in_fm = false;
    let mut fence_seen = 0;
    for line in body.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        // 区切りの判定はフロントマターの読み手と同じ(BOM・行末の空白を許す。テスト計画 C3)。
        // 文例は本人が書き換えるファイルなので、メモ帳で保存した形でも題名が入るように
        let is_fence = if fence_seen == 0 {
            crate::frontmatter::is_open_fence(line)
        } else {
            crate::frontmatter::is_close_fence(line)
        };
        if is_fence {
            fence_seen += 1;
            in_fm = fence_seen == 1;
            out.push_str(line);
            continue;
        }
        if !done && in_fm && bare.trim_start().starts_with("title:") {
            let rest = bare.split_once(':').map(|(_, v)| v.trim()).unwrap_or("");
            if rest.is_empty() {
                let nl = if line.ends_with("\r\n") {
                    "\r\n"
                } else if line.ends_with('\n') {
                    "\n"
                } else {
                    ""
                };
                // YAML として読み直して同じ名前に戻る形で書く(テスト計画 C2)
                out.push_str(&format!(
                    "title: {}{nl}",
                    crate::frontmatter::yaml_scalar(title)
                ));
                done = true;
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_empty_title() {
        let src = "---\ntype: character\ntitle:\naliases: []\n---\n\n## 基本\n";
        let got = fill_title(src, "佐藤架純");
        assert!(got.contains("title: 佐藤架純"));
        assert!(got.contains("## 基本"));
    }

    #[test]
    fn keeps_existing_title() {
        let src = "---\ntitle: 既定名\n---\n本文\n";
        assert_eq!(fill_title(src, "新しい名前"), src);
    }

    #[test]
    fn ignores_title_outside_frontmatter() {
        // 本文中の "title:" には触らない
        let src = "---\ntype: note\n---\n\ntitle: これは本文\n";
        let got = fill_title(src, "X");
        assert!(got.contains("title: これは本文"));
        assert!(!got.contains("title: X"));
    }

    #[test]
    fn empty_title_is_noop() {
        let src = "---\ntitle:\n---\n";
        assert_eq!(fill_title(src, "   "), src);
    }

    /// テスト計画 C3: 文例を本人がメモ帳などで保存して、BOM や区切りの行末の空白が付いても題名が入る
    #[test]
    fn fills_title_in_a_template_saved_with_bom_or_trailing_spaces() {
        for src in ["\u{feff}---\ntitle:\n---\n本文\n", "--- \r\ntitle:\r\n---\r\n本文\r\n"] {
            let got = fill_title(src, "佐藤架純");
            assert_eq!(
                crate::frontmatter::parse_source(&got).title.as_deref(),
                Some("佐藤架純"),
                "{got:?}"
            );
        }
    }

    /// テスト計画 C2: どんな名前でも、読み直すと同じ名前に戻る
    /// (js-yaml でも同じ名前に読めることを確かめた。引用しないと15通り中14通りが壊れた)
    #[test]
    fn tricky_names_survive_the_round_trip() {
        let src = "---\ntype: scene\ntitle:\n---\n本文\n";
        for name in crate::frontmatter::YAML_TRICKY {
            let got = fill_title(src, name);
            let read = crate::frontmatter::parse_source(&got).title;
            assert_eq!(read.as_deref(), Some(name.trim()), "{name:?} → {got:?}");
        }
        // 引用の要らない名前は今までどおり素のまま書く
        assert!(fill_title(src, "佐藤架純").contains("title: 佐藤架純\n"));
    }

    #[test]
    fn builtin_templates_are_wellformed() {
        for (genre, kind, body) in BUILTIN {
            assert!(
                body.starts_with("---\n"),
                "{genre}/{kind} がフロントマターで始まっていない"
            );
            // scene 以外は type を持つ(codex のグルーピングに使う)
            if *kind != "scene" {
                assert!(
                    body.contains(&format!("type: {kind}")),
                    "{genre}/{kind} の type が種別と一致しない"
                );
            }
            // 名前を差し込める空の title があること
            assert!(
                body.contains("\ntitle:\n"),
                "{genre}/{kind} に空の title 行がない"
            );
        }
    }
}
