//! プロジェクト全体の全文検索(M-01/M-02、PoC#2で方式を確定)。
//!
//! **索引は使い捨て**([03-data-format.md](../../docs/03-data-format.md) D-1)。
//! 正本はMarkdownファイルであり、`.app/index.sqlite` はいつ消しても再生成できる。
//! 壊れていたら黙って作り直す — 索引の破損でユーザーの検索が止まってはいけない。
//!
//! ## 方式(PoC#2の実測にもとづく)
//!
//! FTS5 の **trigram** トークナイザを使う。日本語の部分一致がそのまま効き、
//! 「県立青葉」のような語の途中や「白鏡の塔」のような助詞またぎも当たる。
//!
//! **ただし trigram は3文字未満を索引できない。** 「架純」「悠二」のような
//! 2文字の名前はFTS5では1件も当たらないので、**短い語は索引の全件を走査する**
//! (docs/04-design.md §7.3。以前は LIKE だったが、`%` `_` が「何でもよい」の印になった)。
//! 照合は大文字小文字を区別しない(`fold`。trigram と同じ扱い)。
//! 形態素解析(lindera)はPoC#2の結果として**不要と判断**した。
//!
//! ## しないこと
//!
//! - **ファイル監視による自動再索引はしない**(06-decision-log §4 で撤回済み)。
//!   検索するときに、変更されたファイルだけを索引し直す
//! - **文字オフセットは保存しない**(D-7)。索引はファイル粒度に留め、
//!   引用の前後は表示のたびに本文から切り出す

use std::path::Path;

use rusqlite::{Connection, params};
use serde::Serialize;

use crate::frontmatter;
use crate::project::{self, ProjectError};

/// trigram が索引できる最小の長さ。これ未満は全件を走査する
pub const MIN_TRIGRAM_CHARS: usize = 3;

/// 検索結果1件(ファイル粒度)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub path: String,
    pub title: String,
    /// 一致箇所の前後を切り出したもの。**保存はしない**(表示のたびに作る)
    pub snippet: String,
    /// そのファイル内での出現回数
    pub count: usize,
}

/// 検索の結果一式
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub hits: Vec<Hit>,
    /// "fts" = trigram索引で解決 / "like" = 短い語なので総当たり
    pub method: String,
    /// 索引し直したファイル数(0なら前回から変更なし)
    pub reindexed: usize,
    pub elapsed_ms: u64,
}

fn index_path(root: &Path) -> std::path::PathBuf {
    root.join(project::APP_DIR).join("index.sqlite")
}

/// 索引を開く。**壊れていたら作り直す**(使い捨て保証)
fn open(root: &Path) -> Result<Connection, ProjectError> {
    let path = index_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match Connection::open(&path).and_then(|c| {
        init_schema(&c)?;
        Ok(c)
    }) {
        Ok(c) => Ok(c),
        Err(_) => {
            // 壊れた索引で検索が止まるくらいなら、捨てて作り直す
            let _ = std::fs::remove_file(&path);
            let conn = Connection::open(&path)
                .map_err(|e| ProjectError::Index(format!("作れません: {e}")))?;
            init_schema(&conn)
                .map_err(|e| ProjectError::Index(format!("初期化できません: {e}")))?;
            Ok(conn)
        }
    }
}

fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS files(
             path TEXT PRIMARY KEY,
             mtime INTEGER NOT NULL,
             size INTEGER NOT NULL
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS docs USING fts5(
             path UNINDEXED, title, body, tokenize='trigram'
         );",
    )
}

/// 索引対象のファイルを集める(ツリーと同じ規則: `.app/` と隠しフォルダは除く)
fn collect(root: &Path) -> Result<Vec<String>, ProjectError> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), ProjectError> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let ft = entry.file_type()?;
            if ft.is_dir() {
                walk(root, &path, out)?;
            } else if ft.is_file() {
                let is_text = path
                    .extension()
                    .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("txt"))
                    .unwrap_or(false);
                if is_text {
                    if let Ok(rel) = path.strip_prefix(root) {
                        out.push(rel.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

/// 変更されたファイルだけを索引し直す(03-data-format §5-4)。
///
/// 全再構築はしない。判定は **mtime + サイズ** の比較だけで済ませる。
/// 戻り値は索引し直した件数。
pub fn reindex(root: &Path) -> Result<usize, ProjectError> {
    let conn = open(root)?;
    let files = collect(root)?;
    let mut changed = 0usize;

    for rel in &files {
        let full = root.join(rel);
        let Ok(meta) = std::fs::metadata(&full) else {
            continue;
        };
        let size = meta.len() as i64;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let known: Option<(i64, i64)> = conn
            .query_row(
                "SELECT mtime, size FROM files WHERE path = ?1",
                [rel],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        if known == Some((mtime, size)) {
            continue;
        }

        let Ok(source) = project::read_text(root, rel) else {
            continue; // 読めないファイルで索引更新全体を止めない
        };
        let fm = frontmatter::parse_source(&source);
        let (_, body) = frontmatter::split(&source);
        let title = fm.title.unwrap_or_else(|| {
            rel.rsplit('/')
                .next()
                .unwrap_or(rel)
                .trim_end_matches(".md")
                .to_string()
        });

        conn.execute("DELETE FROM docs WHERE path = ?1", [rel])
            .map_err(|e| ProjectError::Index(e.to_string()))?;
        conn.execute(
            "INSERT INTO docs(path, title, body) VALUES (?1, ?2, ?3)",
            params![rel, title, body],
        )
        .map_err(|e| ProjectError::Index(e.to_string()))?;
        conn.execute(
            "INSERT INTO files(path, mtime, size) VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET mtime = ?2, size = ?3",
            params![rel, mtime, size],
        )
        .map_err(|e| ProjectError::Index(e.to_string()))?;
        changed += 1;
    }

    // 消えたファイルを索引から落とす
    let mut stale: Vec<String> = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT path FROM files")
            .map_err(|e| ProjectError::Index(e.to_string()))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| ProjectError::Index(e.to_string()))?;
        for row in rows.flatten() {
            if !files.contains(&row) {
                stale.push(row);
            }
        }
    }
    for path in stale {
        let _ = conn.execute("DELETE FROM docs WHERE path = ?1", [&path]);
        let _ = conn.execute("DELETE FROM files WHERE path = ?1", [&path]);
        changed += 1;
    }

    Ok(changed)
}

/// FTS5 のクエリ文字列にする。
///
/// 検索語をそのまま渡すと `AND` `*` `-` などが**演算子として解釈される**。
/// 「そのままの文字列を探す」ためにフレーズとして囲む(内側の `"` は2つ重ねて逃がす)。
pub fn to_phrase(needle: &str) -> String {
    format!("\"{}\"", needle.replace('"', "\"\""))
}

/// 照合用に小文字へ寄せる。**1文字ずつ**寄せ、1文字に寄せられないものはそのまま残す。
///
/// FTS5 の trigram は大文字小文字を区別しない(全角の英字も)。以前は件数・抜き出し・
/// 開いた先での位置合わせが区別していたため、「hello」で「Hello」のファイルが当たっても
/// 件数が0で、抜き出しは関係のない先頭を出し、開いても一致箇所へ飛ばなかった。
/// 3文字未満の走査(LIKE)は ASCII しか寄せず、全角の英字で長い語と結果が食い違った
/// (テスト計画 D3)。すべてこの形で比べる。
///
/// 1文字ずつ寄せるので文字の位置は変わらない(抜き出しの位置を元の本文にそのまま当てられる)。
/// 画面側の同じ規則は `src/searchFold.ts`
pub fn fold(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut l = c.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) if x.len_utf16() == c.len_utf16() => x,
                _ => c,
            }
        })
        .collect()
}

/// 一致箇所の前後を切り出す。**位置は保存しない**ので毎回ここで作る(D-7)。
pub fn make_snippet(body: &str, needle: &str, radius: usize) -> String {
    let chars: Vec<char> = body.chars().collect();
    // 寄せた形で探す。1文字ずつ寄せているので、見つけた文字の位置は元の本文と同じ
    let folded = fold(body);
    let hit = folded
        .find(&fold(needle))
        .map(|byte_pos| folded[..byte_pos].chars().count());
    let (start, prefix) = match hit {
        Some(at) => (at.saturating_sub(radius), at > radius),
        None => (0, false),
    };
    let needle_len = needle.chars().count();
    let end = (start + radius * 2 + needle_len).min(chars.len());
    let body_slice: String = chars[start..end].iter().collect();
    let suffix = end < chars.len();
    format!(
        "{}{}{}",
        if prefix { "…" } else { "" },
        body_slice.replace('\n', " ").trim(),
        if suffix { "…" } else { "" }
    )
}

/// 本文中の出現回数(寄せた形で数える。当たったのに0回と出さない)
fn count_of(body: &str, needle: &str) -> usize {
    if needle.is_empty() {
        0
    } else {
        fold(body).matches(&fold(needle)).count()
    }
}

/// プロジェクトを検索する。**検索のたびに変更分だけ索引し直す**。
///
/// 3文字未満は trigram が索引できないので全件を走査する(PoC#2)。
pub fn search(root: &Path, needle: &str) -> Result<SearchResult, ProjectError> {
    let started = std::time::Instant::now();
    let needle = needle.trim();
    if needle.is_empty() {
        return Ok(SearchResult {
            hits: Vec::new(),
            method: "fts".into(),
            reindexed: 0,
            elapsed_ms: 0,
        });
    }
    let reindexed = reindex(root)?;
    let conn = open(root)?;

    let use_fts = needle.chars().count() >= MIN_TRIGRAM_CHARS;
    let rows: Vec<(String, String, String)> = if use_fts {
        let mut stmt = conn
            .prepare("SELECT path, title, body FROM docs WHERE docs MATCH ?1 ORDER BY rank")
            .map_err(|e| ProjectError::Index(e.to_string()))?;
        let mapped = stmt
            .query_map([to_phrase(needle)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| ProjectError::Index(e.to_string()))?;
        mapped.flatten().collect()
    } else {
        // 3文字未満は全件を走査する。以前は LIKE で探していたが、`%` と `_` が
        // 「何でもよい」の印になり、「_」で全ファイルが当たった(テスト計画 D1)。
        // 照合は下でまとめて行う
        let mut stmt = conn
            .prepare("SELECT path, title, body FROM docs")
            .map_err(|e| ProjectError::Index(e.to_string()))?;
        let mapped = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| ProjectError::Index(e.to_string()))?;
        mapped.flatten().collect()
    };

    // **本当に含むものだけ**を結果にする(寄せた形で比べる)。件数・抜き出し・開いた先での
    // 位置合わせと同じ基準にそろえ、当たったのに0回のファイルを出さない
    let folded_needle = fold(needle);
    let mut hits: Vec<Hit> = rows
        .into_iter()
        .filter(|(_, title, body)| {
            fold(body).contains(&folded_needle) || fold(title).contains(&folded_needle)
        })
        .map(|(path, title, body)| Hit {
            snippet: make_snippet(&body, needle, 20),
            count: count_of(&body, needle).max(count_of(&title, needle)),
            path,
            title,
        })
        .collect();
    // 出現の多い順。同数ならパス順で安定させる
    hits.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.path.cmp(&b.path)));

    Ok(SearchResult {
        hits,
        method: if use_fts { "fts" } else { "like" }.to_string(),
        reindexed,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_escapes_operators() {
        // 演算子として解釈されると「探した語と違うものが当たる」ことになる
        assert_eq!(to_phrase("青葉高校"), "\"青葉高校\"");
        assert_eq!(to_phrase("AND OR"), "\"AND OR\"");
        assert_eq!(to_phrase("引用\"つき"), "\"引用\"\"つき\"");
    }

    #[test]
    fn snippet_cuts_around_the_hit() {
        let body = "　転校初日の朝は、雨だった。佐藤架純は昇降口で靴を履き替えた。上履きはまだ新しく、踵が硬い。";
        let got = make_snippet(body, "昇降口", 6);
        assert!(got.contains("昇降口"));
        assert!(got.starts_with('…'), "前を切ったことを示す: {got}");
        assert!(got.ends_with('…'), "後ろを切ったことを示す: {got}");
    }

    #[test]
    fn snippet_without_ellipsis_when_whole_body_fits() {
        let got = make_snippet("短い本文です", "本文", 20);
        assert_eq!(got, "短い本文です");
    }

    #[test]
    fn snippet_handles_hit_at_the_head() {
        let got = make_snippet("昇降口で靴を履き替えた。", "昇降口", 4);
        assert!(!got.starts_with('…'), "先頭の一致で余計な印を付けない: {got}");
    }

    #[test]
    fn snippet_falls_back_when_needle_is_absent() {
        // FTS5が当てた行でも、表記の都合で単純検索が外れることはありうる。
        // そのときは先頭を出す(空を返して「中身が無い」ように見せない)
        let got = make_snippet("本文だけがある", "無い語", 5);
        assert!(!got.is_empty());
        assert!(got.starts_with('本'));
    }

    #[test]
    fn snippet_flattens_newlines() {
        let got = make_snippet("一行目\n二行目の昇降口\n三行目", "昇降口", 30);
        assert!(!got.contains('\n'), "改行が残ると一覧の高さが崩れる");
    }

    #[test]
    fn short_queries_are_routed_to_like() {
        // PoC#2の実測で決めた切り分け。ここが変わると2文字の名前が引けなくなる
        assert!("架純".chars().count() < MIN_TRIGRAM_CHARS);
        assert!("昇降口".chars().count() >= MIN_TRIGRAM_CHARS);
    }

    /// テスト計画 D3: 照合は1文字ずつ小文字に寄せる(全角の英字も)。長さの変わる文字は寄せない
    #[test]
    fn fold_lowers_one_char_at_a_time() {
        assert_eq!(fold("Hello ＡＢＣ ÀΣ"), "hello ａｂｃ àσ");
        assert_eq!(fold("İ"), "İ", "小文字が2文字になるものは寄せない(位置がずれる)");
        assert_eq!(fold("架純"), "架純");
    }

    /// 抜き出しは、大文字小文字の違う一致箇所の前後を出す(先頭を出して済ませない)
    #[test]
    fn snippet_finds_a_hit_that_differs_in_case() {
        let body = format!("{}そして Hello と言った。", "前置きの文章が長く続く。".repeat(6));
        // 語の側も寄せる(本文の「Hello」を「HELLO」で探す)
        let got = make_snippet(&body, "HELLO", 6);
        assert!(got.contains("Hello"), "一致箇所が抜き出されていない: {got}");
        assert!(got.starts_with('…'), "{got}");
        assert_eq!(count_of(&body, "HELLO"), 1);
    }

    /// 索引を使えないときは、索引の問題だと言う。以前は「文字コードを判別できませんでした」と
    /// 出ていた(索引の失敗を文字コードの失敗の型で返していた。テスト計画 B8)
    #[test]
    fn index_failure_is_reported_as_an_index_problem() {
        let root = std::env::temp_dir().join(format!("kaku-search-index-{}", std::process::id()));
        project::init(&root).unwrap();
        // 索引の場所にフォルダーがあると、開けず、消して作り直すこともできない
        std::fs::create_dir_all(index_path(&root)).unwrap();
        let err = search(&root, "本文").unwrap_err().to_string();
        assert!(err.contains("索引"), "{err}");
        assert!(!err.contains("UTF-8") && !err.contains("文字コード"), "{err}");
        std::fs::remove_dir_all(root).ok();
    }
}
