//! サンプルプロジェクト(M-08)。
//!
//! 「機能を触りながら学べる」ためのもの(02-requirements.md M-08)。
//! 空のプロジェクトを渡されても、何ができるアプリなのかは分からない。
//!
//! **本文にはわざと不備を入れてある**:
//!  - 登録名と1文字違いの誤変換 → 表記ゆれ検出(M-04-02)が当たる
//!  - 本文に出るのに未登録の固有名詞 → 設定抽出(M-09)が当たる
//!  - 単調な語尾・説明過多 → レビュー(M-05)に言うことがある
//!
//! これは「うまく書けた原稿」ではなく**道具の効きを見せるための素材**である。
//! 不備を消すとサンプルの意味が無くなるので、下のテストで見張っている。

use std::path::Path;

use crate::project::{self, ProjectError};

/// サンプルの中身。(プロジェクトからの相対パス, 本文)
pub const FILES: &[(&str, &str)] = &[
    (
        "はじめに.md",
        r#"---
title: はじめに(サンプル)
---

　これは機能を試すためのサンプルです。**消してしまって構いません**。

## 試す順番

1. **原稿を開く** — 左の「原稿」から「転校初日」を開きます。
   本文の中で色が付いている語(架純・悠二・青葉高校)は、設定に登録されている名前です。
   **Ctrl+クリック**すると、右側にその設定の中身が出ます。

2. **校正してみる** — 右ペインの「校正」タブで「確認する」を押します。
   この本文には**わざと1文字違いの誤変換**を混ぜてあります。
   設定に正しい名前が登録されているので、まぎらわしい表記を見つけられます。
   ここはAIを使わないので、接続していなくてもすぐ終わります。

3. **設定を育てる** — 「抽出」タブで「この本文から抽出」。
   本文に出てくるのに未登録の名前が、追加候補として並びます。
   登録されるのは**選んだものだけ**です。

4. **相談する** — 「AI相談」タブ。依頼欄の上の文例を押すと文面が入ります。
   送る前に**何を渡すか**が一覧で見え、要らない資料は外せます。

5. **レビューしてもらう** — 「レビュー」タブ。観点を選んで実行します。
   引用つきの指摘と全体講評が返ります。

3〜5 は**AIの接続が要ります**。ヘッダーの「?」からヘルプを開いて、
接続の手順を確認してください。1と2 は接続なしで動きます。

## 覚えておくこと

- **AIは本文を書きません。** 指摘や案を出すだけで、採るかどうかはあなたが決めます
- 原稿も設定も**ただのMarkdownファイル**です。他のエディタからも開けます
- 保存は自動です。上書きする前に1世代のバックアップが `.app/backups/` に残ります
"#,
    ),
    (
        "manuscript/01-転校初日.md",
        r#"---
title: 転校初日
status: draft
pov: 佐藤架純
synopsis: 転校してきた架純が、昇降口で悠二と出会う。
---

　転校初日の朝は、雨だった。
　佐藤架純は昇降口で靴を履き替えた。上履きはまだ新しく、踵が硬い。廊下の奥から声がした。
「新しい人?」
　振り返ると、背の高い男子が立っていた。五十嵐悠二と名乗った。架純は頷いた。悠二は少し笑って、教室の方を指した。
　架純は歩き出した。窓の外では雨が降っていた。廊下は薄暗かった。蛍光灯が一本切れていた。
　県立青葉高校。校門に掲げられた名前を、架純は今朝はじめて読んだ。
　佳純はこの学校のことを何も知らない。前の学校のことも、もう思い出さないようにしていた。
　白鏡の塔の話を聞いたのは、その日の昼だった。
"#,
    ),
    (
        "manuscript/02-図書室.md",
        r#"---
title: 図書室
status: draft
pov: 佐藤架純
synopsis: 昼休み、架純は悠二から白鏡の塔の噂を聞く。
---

　昼休みの図書室は静かだった。架純は窓際の席に座った。
　悠二が向かいに来た。彼は本を開かずに話し始めた。
「白鏡の塔って知ってる?」
　架純は首を横に振った。悠二は続けた。校舎の裏に古い給水塔があって、日が落ちる頃に鏡のように光るのだと言った。
　夜にそれを見ると、映るのは自分ではないという噂があるらしい。
　架純は窓の外を見た。雨は上がっていた。
"#,
    ),
    (
        "codex/characters/佐藤架純.md",
        r#"---
type: character
title: 佐藤架純
aliases: [架純]
description: 主人公。二年生の途中で青葉高校へ転校してきた。
---

## 基本
- 性別: 女
- 年齢: 16
- 職業・立場: 県立青葉高校 二年(転校生)

## 性格
- 長所: 落ち着いている
- 短所: 人に頼るのが下手
- 恐れているもの: また置いていかれること

## 話し方
- 一人称: わたし

## この物語での役割
- 目的(何が欲しいか): 新しい場所に居場所を作る
- 障害(何が邪魔をするか): 前の学校での出来事を誰にも話せない
"#,
    ),
    (
        "codex/characters/五十嵐悠二.md",
        r#"---
type: character
title: 五十嵐悠二
aliases: [悠二]
description: 架純のクラスメイト。校内の噂に詳しい。
---

## 基本
- 性別: 男
- 年齢: 16
- 職業・立場: 県立青葉高校 二年

## 性格
- 長所: 物おじしない
- 短所: 口が軽い

## 話し方
- 一人称: おれ

## この物語での役割
- 目的: 白鏡の塔の噂の出どころを確かめたい
"#,
    ),
    (
        "codex/locations/県立青葉高校.md",
        r#"---
type: location
title: 県立青葉高校
aliases: [青葉高校]
description: 架純が転校してきた高校。古い校舎が残っている。
---

## どんな場所か
- 創立五十年。校舎の一部は建て替え前のまま残っている

## 見え方・音・匂い
- 廊下は昼でも薄暗い。雨の日は特に

## ここで起きること
- 架純と悠二の出会い(転校初日)
"#,
    ),
    (
        "plot/outline.md",
        r#"---
title: 構成
---

## 起
- 架純が転校してくる → [転校初日](../manuscript/01-転校初日.md)

## 承
- 白鏡の塔の噂を聞く → [図書室](../manuscript/02-図書室.md)

## 転
- (未定)

## 結
- (未定)
"#,
    ),
];

/// サンプルをプロジェクトへ書き出す。
///
/// **既にあるファイルは上書きしない**(テンプレートと同じ原則)。
/// 戻り値は実際に作ったファイルのパス。
pub fn create(root: &Path) -> Result<Vec<String>, ProjectError> {
    project::init(root)?;
    let mut created = Vec::new();
    for (path, body) in FILES {
        if project::create_file(root, path, body)? {
            created.push(path.to_string());
        }
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{extract, frontmatter, proofread};

    fn file(path: &str) -> &'static str {
        FILES
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, b)| *b)
            .unwrap_or_else(|| panic!("サンプルに {path} がない"))
    }

    /// サンプルのcodexに登録されている名前(正式名+別名)
    fn codex_names() -> Vec<String> {
        FILES
            .iter()
            .filter(|(p, _)| p.starts_with("codex/"))
            .flat_map(|(_, body)| {
                let fm = frontmatter::parse_source(body);
                let mut v: Vec<String> = fm.title.into_iter().collect();
                v.extend(fm.aliases);
                v
            })
            .collect()
    }

    #[test]
    fn every_file_has_frontmatter() {
        for (path, body) in FILES {
            assert!(
                body.starts_with("---\n"),
                "{path} がフロントマターで始まっていない"
            );
            assert!(
                frontmatter::parse_source(body).title.is_some(),
                "{path} に title が無い(ツリーの表示名になる)"
            );
        }
    }

    #[test]
    fn manuscript_contains_a_detectable_notation_error() {
        // **サンプルの肝**。表記ゆれ検出が当たる誤変換が入っていること。
        // 本文を直すときにここを消すと、サンプルの意味が無くなる
        let body = file("manuscript/01-転校初日.md");
        let hits = proofread::check_notation(body, &codex_names());
        assert!(
            hits.iter().any(|h| h.candidate == "佳純" && h.suggestion == "架純"),
            "わざと入れた誤変換が検出されない: {hits:?}"
        );
        // 正しい名前も本文にあるので確度は「高」で出る
        let hit = hits.iter().find(|h| h.candidate == "佳純").unwrap();
        assert_eq!(hit.confidence, "high");
    }

    #[test]
    fn manuscript_has_no_unintended_notation_hits() {
        // サンプルで誤検出が出ると「この道具は当てにならない」という第一印象になる
        for (path, body) in FILES.iter().filter(|(p, _)| p.starts_with("manuscript/")) {
            let hits = proofread::check_notation(body, &codex_names());
            let unexpected: Vec<&str> = hits
                .iter()
                .map(|h| h.candidate.as_str())
                .filter(|c| *c != "佳純")
                .collect();
            assert!(unexpected.is_empty(), "{path} で誤検出: {unexpected:?}");
        }
    }

    #[test]
    fn manuscript_contains_an_unregistered_proper_noun() {
        // 設定抽出(M-09)に拾わせる余地を残してあること
        let body = file("manuscript/01-転校初日.md");
        assert!(body.contains("白鏡"), "未登録の固有名詞が本文から消えている");
        assert!(extract::looks_like_proper_noun("白鏡"));
        assert!(
            !codex_names().iter().any(|n| n == "白鏡" || n == "白鏡の塔"),
            "抽出の題材にするため、この名前は登録しないでおく"
        );
    }

    #[test]
    fn registered_names_actually_appear_in_the_manuscript() {
        // 言及ハイライトが効くこと。登録だけして本文に出ないと何も起きない
        let body = file("manuscript/01-転校初日.md");
        for name in ["佐藤架純", "架純", "五十嵐悠二", "悠二", "県立青葉高校"] {
            assert!(body.contains(name), "{name} が本文に出てこない");
        }
    }

    #[test]
    fn guide_explains_what_works_without_ai() {
        // AI未接続でも試せることを書いておく(U-01: ゼロ設定で始められる)
        let guide = file("はじめに.md");
        assert!(guide.contains("接続なしで動きます"));
        assert!(guide.contains("AIは本文を書きません"));
    }
}
