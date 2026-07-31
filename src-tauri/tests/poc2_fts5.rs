//! PoC#2: **FTS5 trigram による日本語検索**([docs/04-design.md](../../docs/04-design.md) §7)。
//!
//! 検証すること:
//!  1. rusqlite(bundled)で **FTS5 が有効か**
//!  2. **trigram トークナイザ**が使えるか(SQLite 3.34+ が必要)
//!  3. trigram + LIKE フォールバックの品質
//!
//! 判定基準(§7): **キャラ名・2文字語・かな交じり検索が期待通り**であること。
//! 不合格なら lindera-sqlite の導入を検討する。
//!
//! 素材は**サンプルプロジェクトの本文をそのまま使う**(`sample.rs`)。
//! 作り物のテストデータではなく、実際にアプリが扱う文章で測る。
//!
//! 実行:
//!   cargo test --test poc2_fts5 -- --nocapture

use rusqlite::{Connection, params};

/// 索引に入れる素材(パス, タイトル, 本文)
fn corpus() -> Vec<(String, String, String)> {
    kaku_lib::sample::FILES
        .iter()
        .map(|(path, body)| {
            let fm = kaku_lib::frontmatter::parse_source(body);
            let (_, text) = kaku_lib::frontmatter::split(body);
            (
                path.to_string(),
                fm.title.unwrap_or_else(|| path.to_string()),
                text.to_string(),
            )
        })
        .collect()
}

fn open_indexed(tokenizer: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE docs USING fts5(
             path UNINDEXED, title, body, tokenize='{tokenizer}'
         );"
    ))?;
    {
        let mut stmt = conn.prepare("INSERT INTO docs(path, title, body) VALUES (?1, ?2, ?3)")?;
        for (path, title, body) in corpus() {
            stmt.execute(params![path, title, body])?;
        }
    }
    Ok(conn)
}

/// FTS5 で検索してヒットしたパスを返す
fn fts_search(conn: &Connection, query: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM docs WHERE docs MATCH ?1 ORDER BY rank")?;
    let rows = stmt.query_map([query], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// LIKE によるフォールバック(trigram が扱えない短い語の受け皿)
fn like_search(conn: &Connection, needle: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT path FROM docs WHERE body LIKE '%' || ?1 || '%' OR title LIKE '%' || ?1 || '%'",
    )?;
    let rows = stmt.query_map([needle], |r| r.get::<_, String>(0))?;
    rows.collect()
}

#[test]
fn fts5_and_trigram_are_available() {
    let conn = Connection::open_in_memory().expect("SQLiteを開けない");

    let version: String = conn
        .query_row("SELECT sqlite_version()", [], |r| r.get(0))
        .expect("バージョンを取れない");
    eprintln!("SQLite {version}");

    // FTS5 が組み込まれているか
    let has_fts5: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_compile_options WHERE compile_options LIKE 'ENABLE_FTS5%')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    eprintln!("FTS5: {}", if has_fts5 { "有効" } else { "無効" });
    assert!(has_fts5, "FTS5が無効。bundled featureの設定を見直すこと");

    // trigram トークナイザ(SQLite 3.34+)
    let trigram_ok = conn
        .execute_batch("CREATE VIRTUAL TABLE t USING fts5(x, tokenize='trigram');")
        .is_ok();
    eprintln!("trigram: {}", if trigram_ok { "使える" } else { "使えない" });
    assert!(
        trigram_ok,
        "trigramトークナイザが無い。SQLite 3.34+ が必要(代替: lindera-sqlite)"
    );
}

#[test]
fn japanese_search_meets_the_gate() {
    let conn = open_indexed("trigram").expect("索引を作れない");

    // (検索語, 少なくとも1件は当たってほしいか, 説明)
    let cases: &[(&str, bool, &str)] = &[
        ("昇降口", true, "3文字の一般語"),
        ("五十嵐悠二", true, "フルネーム"),
        ("青葉高校", true, "4文字の固有名詞"),
        ("県立青葉", true, "語の途中で切った部分一致"),
        ("雨だった", true, "かな交じり"),
        ("白鏡の塔", true, "助詞をまたぐ"),
        ("架純", false, "**2文字**。trigramは3文字未満を索引できない"),
        ("悠二", false, "2文字"),
        ("存在しない語句", false, "当たってはいけない"),
    ];

    eprintln!("\n{:<14} | {:>6} | {:>6} | 説明", "検索語", "FTS5", "LIKE");
    eprintln!("{}", "-".repeat(64));

    let mut fts_ok = 0;
    let mut like_rescued = 0;
    for (needle, want_hit, note) in cases {
        let fts = fts_search(&conn, needle).unwrap_or_default();
        let like = like_search(&conn, needle).unwrap_or_default();
        eprintln!(
            "{:<14} | {:>6} | {:>6} | {}",
            needle,
            fts.len(),
            like.len(),
            note
        );

        let is_negative = *needle == "存在しない語句";
        if is_negative {
            assert!(fts.is_empty(), "当たってはいけない語がFTS5で当たった");
            assert!(like.is_empty(), "当たってはいけない語がLIKEで当たった");
            continue;
        }
        if *want_hit {
            assert!(!fts.is_empty(), "「{needle}」がFTS5で当たらない({note})");
            fts_ok += 1;
        } else {
            // 2文字語は trigram では当たらない想定。**LIKEが受け止めること**を確認する
            if fts.is_empty() {
                assert!(
                    !like.is_empty(),
                    "「{needle}」がFTS5でもLIKEでも当たらない。フォールバックが機能していない"
                );
                like_rescued += 1;
            } else {
                fts_ok += 1;
            }
        }
    }

    eprintln!(
        "\nFTS5で解決 {fts_ok}件 / LIKEで救済 {like_rescued}件 → **trigram+LIKEで全件到達**"
    );
}

#[test]
fn like_fallback_is_needed_only_for_short_queries() {
    // フォールバックの発動条件を決めるための確認。
    // trigram は3文字未満を索引しないので、**2文字以下はLIKEへ回す**のが正しい切り分けになる
    let conn = open_indexed("trigram").expect("索引を作れない");
    for short in ["架純", "悠二", "雨"] {
        assert!(
            fts_search(&conn, short).unwrap_or_default().is_empty(),
            "{short}: 3文字未満がFTS5で当たった。切り分けの前提が変わるので見直すこと"
        );
        assert!(
            !like_search(&conn, short).unwrap_or_default().is_empty(),
            "{short}: LIKEで当たらない"
        );
    }
    for long in ["昇降口", "青葉高校"] {
        assert!(
            !fts_search(&conn, long).unwrap_or_default().is_empty(),
            "{long}: 3文字以上がFTS5で当たらない"
        );
    }
}

#[test]
fn snippet_can_be_produced_for_display() {
    // 検索結果に前後の文脈を出せること(位置は保存しない=D-7。表示時に作る)
    let conn = open_indexed("trigram").expect("索引を作れない");
    let mut stmt = conn
        .prepare(
            "SELECT path, snippet(docs, 2, '[', ']', '…', 12) FROM docs WHERE docs MATCH ?1 ORDER BY rank",
        )
        .expect("snippetを使えない");
    let rows: Vec<(String, String)> = stmt
        .query_map(["昇降口"], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert!(!rows.is_empty(), "snippetが取れない");
    for (path, snip) in &rows {
        eprintln!("{path}: {snip}");
        assert!(snip.contains('['), "強調の印が入っていない: {snip}");
    }
}
