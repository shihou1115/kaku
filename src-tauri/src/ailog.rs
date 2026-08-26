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
    let Ok(line) = serde_json::to_string(&entry) else {
        return;
    };

    let dir = root.join(crate::project::APP_DIR).join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(format!("{}.jsonl", crate::project::date_dir_utc(now)));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

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
