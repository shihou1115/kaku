//! AI呼び出しのログ(T-06)。
//!
//! **プロンプトの実体を残す**ことで、結果が変わった原因をあとから追える。
//! これは単なる便利機能ではない — [06-decision-log.md] §4 で
//! 「プロンプトのバージョン管理機構」を撤回したときの根拠が
//! 「ログに実体を残せば後から比較できる」だった。ログが無いと撤回の前提が崩れる。
//!
//! 形式は1行1件のJSONL(`.app/logs/YYYY-MM-DD.jsonl` へ追記)。
//! ローテーションも自動削除もしない。`.app/` は消しても再生成される領域なので、
//! 溜まって困ったらフォルダごと消せばよい。
//!
//! **書き込みに失敗しても呼び出し元は止めない。** ログのために執筆を止める理由はない。

use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::ai::ChatMessage;

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct Entry<'a> {
    /// 記録した時刻(UTC, ミリ秒)
    at_ms: u128,
    /// どの機能から呼んだか(chat / proofread / review / extract / split)
    feature: &'a str,
    model: &'a str,
    base_url: &'a str,
    /// 送ったメッセージ**全文**。要約しない(要約すると比較の役に立たない)
    messages: Vec<Message<'a>>,
    /// 受け取った応答全文
    response: &'a str,
    elapsed_ms: u64,
    /// 打ち切り・拒否など、結果を読むときに要る但し書き
    note: Option<&'a str>,
}

/// 1件追記する。失敗は握りつぶす(呼び出し元の処理を止めない)。
///
/// 引数は多いが、構造体にまとめると呼び出し側が冗長になるだけで読みやすくならない
#[allow(clippy::too_many_arguments)]
pub fn write(
    root: &Path,
    feature: &str,
    model: &str,
    base_url: &str,
    messages: &[ChatMessage],
    response: &str,
    elapsed_ms: u64,
    note: Option<&str>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let entry = Entry {
        at_ms: now,
        feature,
        model,
        base_url,
        messages: messages
            .iter()
            .map(|m| Message {
                role: &m.role,
                content: &m.content,
            })
            .collect(),
        response,
        elapsed_ms,
        note,
    };
    let Ok(mut line) = serde_json::to_string(&entry) else {
        return;
    };
    line.push('\n');

    let dir = root.join(crate::project::APP_DIR).join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(format!("{}.jsonl", crate::project::date_dir_utc(now)));
    // **1行を1回の書き込みで出し、同時には書かない。** 校正・レビュー・抽出は同時に走れる。
    // 以前は `writeln!`(本体と改行の2回)を排他なしで書いていたため、行どうしが混ざって
    // JSON として読めない行ができた(テスト計画 E6。200行中70〜100行が壊れた)
    let _turn = WRITE_TURN.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// ログを書く順番(同じプロセスの中で1本ずつにする)
static WRITE_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        // 時刻だけでは並列実行で衝突する(過去に踏んだ)
        let base = std::env::temp_dir().join(format!(
            "kaku-ailog-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    /// テスト計画 E6: 校正・レビュー・抽出は同時に走れるので、ログも同時に書かれる。
    /// **1行が丸ごと1つの JSON として残ること**(行どうしが混ざらないこと)
    #[test]
    fn concurrent_writes_keep_each_line_whole() {
        let root = tmp();
        let big = "あ".repeat(20_000); // 1行が大きいほど、書き込みの合間に割り込まれやすい
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let root = root.clone();
                let big = big.clone();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        let m = vec![msg("user", &format!("{t}-{i}{big}"))];
                        write(&root, "proofread", "m", "http://localhost:1234/v1", &m, &big, 1, None);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let dir = root.join(".app").join("logs");
        let file = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        let text = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let broken = lines
            .iter()
            .filter(|l| serde_json::from_str::<serde_json::Value>(l).is_err())
            .count();
        assert_eq!(broken, 0, "JSON として読めない行が {broken} 行ある(行が混ざった)");
        assert_eq!(lines.len(), 200, "行が足りない・余る");
    }

    /// **プロンプト全文と応答全文が残ること。** 要約して残すと比較の役に立たない
    #[test]
    fn writes_one_line_per_call_with_full_text() {
        let root = tmp();
        let messages = vec![msg("system", "編集者です"), msg("user", "本文\nと依頼")];
        write(&root, "chat", "some-model", "http://localhost:1234/v1", &messages, "応答\n全文", 1234, Some("打ち切られた"));
        write(&root, "review", "some-model", "http://localhost:1234/v1", &messages, "2件目", 10, None);

        let dir = root.join(".app").join("logs");
        let file = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        let text = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "1回の呼び出しにつき1行のはず");

        let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(v["feature"], "chat");
        assert_eq!(v["model"], "some-model");
        assert_eq!(v["messages"][1]["content"], "本文\nと依頼");
        assert_eq!(v["response"], "応答\n全文");
        assert_eq!(v["elapsed_ms"], 1234);
        assert_eq!(v["note"], "打ち切られた");
        // APIキーは記録しない(保存しないものを別経路で残さない)
        assert!(!text.contains("api_key"));
    }

    /// プロジェクトが無い場所でも落ちないこと(ログのために処理を止めない)
    #[test]
    fn never_panics_when_the_place_is_unusable() {
        let missing = std::env::temp_dir().join("kaku-ailog-なにもない").join("さらに下");
        write(&missing, "chat", "m", "u", &[], "r", 0, None);
    }
}
