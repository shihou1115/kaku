//! OpenAI互換APIの薄いクライアント(docs/04-design.md §6.1)。
//!
//! **意図的に薄く保つ**: send / stream / cancel とログだけ。
//! プロンプト解決・モデル自動解決・キャッシュ・レート制御・複数モデル多重化は
//! 第3回レビューで撤回済み(docs/06-decision-log.md §4)。再追加しないこと。
//!
//! uggg (`C:\claude\uggg` の src-tauri/src/dialogue/llm.rs) を参考にしている:
//! プロバイダ抽象を持たず base_url とモデル名の差し替えだけで
//! LM Studio / OpenAI / Ollama / OpenRouter を吸収する。

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// LM Studio の既定
pub const DEFAULT_BASE_URL: &str = "http://localhost:1234/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    temperature: f32,
    stream: bool,
    /// 構造化出力(経路A)。対応しない接続先には送らない
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChatMessage,
    /// "stop" = 正常終了 / "length" = コンテキストや上限で打ち切られた
    #[serde(default)]
    finish_reason: Option<String>,
}

/// 応答一式。**打ち切りを見逃さない**ために finish_reason を必ず持ち回る。
///
/// 校正のように「指摘なし」が意味を持つ機能では、打ち切りを
/// 「誤りが無かった」と取り違えると致命的な誤報告になる。
#[derive(Debug, Clone)]
pub struct ChatOutcome {
    pub content: String,
    pub usage: Option<Usage>,
    pub finish_reason: Option<String>,
}

impl ChatOutcome {
    /// コンテキスト上限などで応答が打ち切られたか
    pub fn truncated(&self) -> bool {
        self.finish_reason.as_deref() == Some("length")
            || (self.content.trim().is_empty() && self.finish_reason.is_some())
    }

    /// 打ち切りではなく、正常終了したのに1文字も返らなかったか。
    ///
    /// **検閲による拒否はこの形で現れる**(エラーにならない。docs/04-design.md §8.1)。
    /// 打ち切り(コンテキスト不足)とは対処が違う — 前者はモデルを替える、
    /// 後者はコンテキスト長を増やす — ので、区別して伝える必要がある。
    pub fn refused(&self) -> bool {
        self.content.trim().is_empty() && self.finish_reason.as_deref() != Some("length")
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct ModelList {
    data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    id: String,
}

/// 応答を待つ上限(秒)。
///
/// ローカルLLMは初回のモデルロードに時間がかかる(uggg 由来の初期値は 180 秒だった)。
/// 校正やレビューのように出力トークン数が多い用途では、遅いモデルだとこれでも足りない。
/// 待ち続けるよりは「設定を見直せ」と伝える方が親切なので、上限は必ず設ける
/// (docs/04-design.md §7.2)。
///
/// **2026-07-31 に 180 → 240 秒へ延長**(M-05のドッグフーディング)。
/// レビューの質を上げようとすると推論能力の高い=遅いモデルを選ぶことになり、
/// 180秒では打ち切られることが多かった。校正と違い、レビューは1回の結果が成果物なので
/// 「待てば得られるものを時間切れで捨てる」損失が大きい。
pub const TIMEOUT_SECS: u64 = 240;

pub fn client() -> reqwest::Client {
    // 接続自体は localhost なら即時なので connect は短くてよい。
    reqwest::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .expect("reqwest client build failed")
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), path)
}

fn auth(req: reqwest::RequestBuilder, api_key: &Option<String>) -> reqwest::RequestBuilder {
    match api_key {
        Some(k) if !k.trim().is_empty() => req.bearer_auth(k),
        _ => req,
    }
}

/// 接続失敗の理由を、ユーザーが次に何をすべきか分かる日本語にする。
///
/// タイムアウトを「接続できません」と出すと、設定ミスだと誤解して
/// 接続先やキーを疑うことになる。実際にはモデルが遅いだけのことがある
/// (LM Studio の並列数やコンテキスト長の設定で速度は桁違いに変わる)。
pub fn describe_error(e: &reqwest::Error, url: &str) -> String {
    if e.is_timeout() {
        format!(
            "応答が {} 秒以内に返りませんでした。モデルが遅すぎる可能性があります。\
             LM Studio の設定(コンテキスト長・並列数)を見直すか、軽いモデルに替えてください",
            TIMEOUT_SECS
        )
    } else if e.is_connect() {
        format!("接続できませんでした({url})。LM Studio が起動しているか確認してください")
    } else {
        format!("通信に失敗しました({url}): {e}")
    }
}

/// 接続テスト兼モデル一覧取得(M-07)。LM Studio では現在ロード中のモデルが返る。
pub async fn list_models(base_url: &str, api_key: &Option<String>) -> Result<Vec<String>, String> {
    let url = endpoint(base_url, "models");
    let resp = auth(client().get(&url), api_key)
        .send()
        .await
        .map_err(|e| describe_error(&e, &url))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("APIエラー: status={status} body={body}"));
    }
    let list: ModelList = resp
        .json()
        .await
        .map_err(|e| format!("モデル一覧の解析に失敗: {e}"))?;
    Ok(list.data.into_iter().map(|m| m.id).collect())
}

/// 単発の chat completion。
///
/// `schema` を渡すと構造化出力(経路A)を要求する。対応しないモデルでは
/// エラーになるため、呼び出し側で経路B(スキーマ無し+寛容パース)へ落とすこと。
pub async fn chat(
    base_url: &str,
    api_key: &Option<String>,
    model: &str,
    messages: &[ChatMessage],
    temperature: f32,
    schema: Option<serde_json::Value>,
) -> Result<ChatOutcome, String> {
    let url = endpoint(base_url, "chat/completions");
    let body = ChatRequest {
        model,
        messages,
        temperature,
        stream: false,
        response_format: schema,
    };
    let resp = auth(client().post(&url), api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| describe_error(&e, &url))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("APIエラー: status={status} body={text}"));
    }
    let parsed: ChatResponse = resp
        .json()
        .await
        .map_err(|e| format!("応答の解析に失敗: {e}"))?;
    let first = parsed.choices.first();
    Ok(ChatOutcome {
        content: first.map(|c| c.message.content.clone()).unwrap_or_default(),
        finish_reason: first.and_then(|c| c.finish_reason.clone()),
        usage: parsed.usage,
    })
}

/// SSE の1行から本文の増分を取り出す。
///
/// `data: {...}` 形式。`[DONE]` は終端。解析できない行は無視する(寛容に扱う)。
pub fn parse_sse_line(line: &str) -> SseEvent {
    let line = line.trim();
    if line.is_empty() {
        return SseEvent::Ignore;
    }
    let Some(payload) = line.strip_prefix("data:") else {
        return SseEvent::Ignore;
    };
    let payload = payload.trim();
    if payload == "[DONE]" {
        return SseEvent::Done;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
        return SseEvent::Ignore;
    };
    let delta = v["choices"][0]["delta"]["content"].as_str().unwrap_or("");
    if delta.is_empty() {
        SseEvent::Ignore
    } else {
        SseEvent::Delta(delta.to_string())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SseEvent {
    Delta(String),
    Done,
    Ignore,
}

/// マークダウンのコードフェンスを剥がして JSON 本体を取り出す(uggg 由来)。
///
/// 構造化出力(json_schema)が使えないモデル向けのフォールバック経路
/// (docs/04-design.md §6.3 B経路)で使う。
pub fn extract_json_blob(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        let after_lang = rest.split_once('\n').map(|(_, body)| body).unwrap_or(rest);
        if let Some(end) = after_lang.rfind("```") {
            return after_lang[..end].trim();
        }
        return after_lang.trim();
    }
    trimmed
}

pub fn stream_request(
    base_url: &str,
    api_key: &Option<String>,
    model: &str,
    messages: &[ChatMessage],
    temperature: f32,
) -> reqwest::RequestBuilder {
    let url = endpoint(base_url, "chat/completions");
    let body = ChatRequest {
        model,
        messages,
        temperature,
        stream: true,
        response_format: None,
    };
    auth(client().post(&url), api_key).json(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_handles_trailing_slash() {
        assert_eq!(
            endpoint("http://localhost:1234/v1/", "models"),
            "http://localhost:1234/v1/models"
        );
        assert_eq!(
            endpoint("http://localhost:1234/v1", "chat/completions"),
            "http://localhost:1234/v1/chat/completions"
        );
    }

    #[test]
    fn parses_sse_delta() {
        let line = r#"data: {"choices":[{"delta":{"content":"こん"}}]}"#;
        assert_eq!(parse_sse_line(line), SseEvent::Delta("こん".to_string()));
    }

    #[test]
    fn detects_done_and_ignores_noise() {
        assert_eq!(parse_sse_line("data: [DONE]"), SseEvent::Done);
        assert_eq!(parse_sse_line(""), SseEvent::Ignore);
        assert_eq!(parse_sse_line(": ping"), SseEvent::Ignore);
        assert_eq!(parse_sse_line("data: {壊れたJSON"), SseEvent::Ignore);
        // role だけ入る最初のチャンクは本文が空なので無視される
        assert_eq!(
            parse_sse_line(r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#),
            SseEvent::Ignore
        );
    }

    #[test]
    fn distinguishes_truncation_from_refusal() {
        let make = |content: &str, reason: Option<&str>| ChatOutcome {
            content: content.to_string(),
            usage: None,
            finish_reason: reason.map(|s| s.to_string()),
        };
        // コンテキスト不足で切れた
        assert!(make("途中まで", Some("length")).truncated());
        assert!(!make("途中まで", Some("length")).refused());
        // 正常終了なのに1文字も返らない = 検閲の疑い(§8.1)
        assert!(make("", Some("stop")).refused());
        // 空応答は打ち切り判定にも当たるが、対処が違うので呼び出し側で先に length を見る
        assert!(!make("普通の応答", Some("stop")).refused());
        assert!(!make("普通の応答", Some("stop")).truncated());
    }

    #[test]
    fn extracts_fenced_json() {
        assert_eq!(
            extract_json_blob("```json\n{\"a\":1}\n```"),
            "{\"a\":1}"
        );
        assert_eq!(extract_json_blob("{\"a\":1}"), "{\"a\":1}");
    }
}
