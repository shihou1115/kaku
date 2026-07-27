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

pub fn client() -> reqwest::Client {
    // ローカルLLMは初回のモデルロードに時間がかかる(uggg の実測にならい 180 秒)。
    // 接続自体は localhost なら即時なので connect は短くてよい。
    reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
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

/// 接続テスト兼モデル一覧取得(M-07)。LM Studio では現在ロード中のモデルが返る。
pub async fn list_models(base_url: &str, api_key: &Option<String>) -> Result<Vec<String>, String> {
    let url = endpoint(base_url, "models");
    let resp = auth(client().get(&url), api_key)
        .send()
        .await
        .map_err(|e| format!("接続できませんでした ({url}): {e}"))?;
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
) -> Result<(String, Option<Usage>), String> {
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
        .map_err(|e| format!("接続できませんでした ({url}): {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("APIエラー: status={status} body={text}"));
    }
    let parsed: ChatResponse = resp
        .json()
        .await
        .map_err(|e| format!("応答の解析に失敗: {e}"))?;
    let content = parsed
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();
    Ok((content, parsed.usage))
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
    fn extracts_fenced_json() {
        assert_eq!(
            extract_json_blob("```json\n{\"a\":1}\n```"),
            "{\"a\":1}"
        );
        assert_eq!(extract_json_blob("{\"a\":1}"), "{\"a\":1}");
    }
}
