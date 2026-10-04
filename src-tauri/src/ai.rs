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
    message: ResponseMessage,
    /// "stop" = 正常終了 / "length" = コンテキストや上限で打ち切られた
    #[serde(default)]
    finish_reason: Option<String>,
}

/// 応答側のメッセージ。**`content` は null で来ることがある。**
///
/// OpenAI の構造化出力ではモデルが拒否すると `content: null` と `refusal` が返る。
/// `String` で受けると解析エラーになり、拒否が「接続設定を確認してください」という
/// 的外れな失敗に化ける。null は空文字として受け、拒否(`refused()`)として扱う。
#[derive(Debug, Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,
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
    /// コンテキスト上限などで応答が打ち切られたか(`finish_reason == "length"`)。
    ///
    /// **`refused()` とは重ならない。** 以前は「空応答で終了理由あり」も打ち切りに
    /// 含めていたため、正常終了(`stop`)の空応答=検閲による拒否が両方に当たり、
    /// 先に `truncated()` を見た校正と抽出は拒否を「コンテキスト長が不足」と誤診していた
    /// (04-design §7.1「同じ0文字でも次にすべきことが逆なので混ぜて表示しない」に反する)。
    /// 推論で文脈を使い切った空応答は `length` で返るので、ここで拾える。
    pub fn truncated(&self) -> bool {
        self.finish_reason.as_deref() == Some("length")
    }

    /// 打ち切りではなく、正常終了したのに1文字も返らなかったか。
    ///
    /// **検閲による拒否はこの形で現れる**(エラーにならない。docs/04-design.md §8.1)。
    /// 打ち切り(コンテキスト不足)とは対処が違う — 前者はモデルを替える、
    /// 後者はコンテキスト長を増やす — ので、区別して伝える必要がある。
    pub fn refused(&self) -> bool {
        self.content.trim().is_empty() && !self.truncated()
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

/// エラー応答(HTTP の状態)を、原因と次にすべきことが分かる日本語にする(テスト計画 E7)。
///
/// 以前は「APIエラー: status=401 body=…」をそのまま出していたため、APIキーの入れ忘れも、
/// 接続先の `/v1` の抜けも、同じ形の文面で区別が付かなかった。本文は診断のために後ろへ残す
pub fn describe_status(status: reqwest::StatusCode, body: &str, url: &str) -> String {
    let code = status.as_u16();
    let detail = body.trim();
    let detail = if detail.is_empty() {
        String::new()
    } else {
        // 長い応答はそのまま出さない(1行の案内に収める)
        let clipped: String = detail.chars().take(200).collect();
        format!(" 応答: {clipped}")
    };
    let hint = match code {
        401 | 403 => "APIキーが無いか、正しくありません。設定の APIキー を確かめてください".to_string(),
        404 => format!(
            "接続先にAPIが見つかりません({url})。接続先の末尾に /v1 が要ることがあります。\
             モデル名が違う場合も、この応答になることがあります"
        ),
        400 | 422 if detail.to_lowercase().contains("model") => {
            "モデルを使えませんでした。LM Studio でモデルを読み込んでいるか、\
             モデル名が合っているか確かめてください"
                .to_string()
        }
        429 => "混み合っているか、利用の上限に達しました。少し待ってからやり直してください"
            .to_string(),
        500..=599 => "AIサーバーの中で失敗しました。LM Studio のログを確かめてください".to_string(),
        _ => "AIサーバーが要求を受け付けませんでした".to_string(),
    };
    format!("{hint}(status={code}){detail}")
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
        return Err(describe_status(status, &body, &url));
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
        return Err(describe_status(status, &text, &url));
    }
    let parsed: ChatResponse = resp
        .json()
        .await
        .map_err(|e| format!("応答の解析に失敗: {e}"))?;
    let first = parsed.choices.first();
    Ok(ChatOutcome {
        content: first
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default(),
        finish_reason: first.and_then(|c| c.finish_reason.clone()),
        usage: parsed.usage,
    })
}

/// 受信バッファから**完結した行だけ**を取り出す(残りは次のチャンクへ持ち越す)。
///
/// **チャンクの切れ目はUTF-8の文字境界と一致しない。** 受信するのは
/// ソケットの読み取り単位であって、SSEイベントでも文字でもない。
/// チャンクごとに `from_utf8_lossy` を通すと、2つに割れた日本語1文字が
/// その時点で U+FFFD に潰れ、あとから直せない。
///
/// バイト列のまま改行を探し、行が完結してから文字列にする。
/// 区切りの改行(0x0A)はASCIIなので、マルチバイト列の途中に現れることはない。
pub fn drain_sse_lines(buf: &mut Vec<u8>) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = buf.drain(..=pos).collect();
        lines.push(String::from_utf8_lossy(&line).into_owned());
    }
    lines
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

/// 応答が「指定した形の一覧」として読み取れたか。`keys` のどれかの下に配列がある、
/// または応答そのものが配列なら true。
///
/// **`false` と「0件」は違う。** 散文で返された応答は各機能の `parse` が空を返すので、
/// これを見ずに件数だけで判断すると「見つかりませんでした」と誤報告する。
/// 校正・抽出・分割の3箇所で同じ判定が要るため、ここに置く(キーは各 `parse` と揃える)。
pub fn has_list(raw: &str, keys: &[&str]) -> bool {
    let blob = extract_json_blob(raw);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(blob) else {
        return false;
    };
    keys.iter()
        .find_map(|k| value.get(*k))
        .and_then(|v| v.as_array())
        .or_else(|| value.as_array())
        .is_some()
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
mod tests_sse_lines {
    use super::*;

    /// **チャンクの切れ目が文字の途中に落ちても化けないこと。**
    /// ここが壊れると、日本語が U+FFFD に潰れたまま画面にも会話履歴にも残る
    #[test]
    fn a_character_split_across_chunks_survives() {
        let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"あい\"}}]}\n";
        let bytes = payload.as_bytes();
        // 「あ」の3バイトの途中で割る
        let head = payload.find('あ').unwrap() + 1;

        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&bytes[..head]);
        assert!(drain_sse_lines(&mut buf).is_empty(), "行が完結していないのに出した");
        buf.extend_from_slice(&bytes[head..]);
        let lines = drain_sse_lines(&mut buf);

        assert_eq!(lines.len(), 1);
        assert!(!lines[0].contains(char::REPLACEMENT_CHARACTER), "化けた: {}", lines[0]);
        assert!(matches!(parse_sse_line(&lines[0]), SseEvent::Delta(ref d) if d == "あい"));
        assert!(buf.is_empty());
    }

    /// 完結していない行は次のチャンクへ持ち越すこと
    #[test]
    fn incomplete_lines_are_carried_over() {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"data: 1\ndata: 2");
        let lines = drain_sse_lines(&mut buf);
        assert_eq!(lines, vec!["data: 1\n".to_string()]);
        assert_eq!(buf, b"data: 2");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト計画 E7: エラー応答の状態ごとに、原因と次にすべきことを言う
    #[test]
    fn describe_status_names_the_cause() {
        use reqwest::StatusCode as S;
        let url = "http://localhost:1234/models";
        assert!(describe_status(S::UNAUTHORIZED, "", url).contains("APIキー"));
        assert!(describe_status(S::FORBIDDEN, "", url).contains("APIキー"));
        let not_found = describe_status(S::NOT_FOUND, "", url);
        assert!(not_found.contains("/v1") && not_found.contains(url), "{not_found}");
        let no_model = describe_status(S::BAD_REQUEST, r#"{"error":"Model not loaded"}"#, url);
        assert!(no_model.contains("モデルを読み込んで"), "{no_model}");
        assert!(describe_status(S::INTERNAL_SERVER_ERROR, "", url).contains("ログ"));
        // 本文は診断のために残すが、長すぎるものは切る
        let long = describe_status(S::IM_A_TEAPOT, &"x".repeat(1000), url);
        assert!(long.contains("status=418") && long.chars().count() < 300, "{long}");
    }

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
        // 推論で文脈を使い切って1文字も出せなかった(§7.1)。これも打ち切り
        assert!(make("", Some("length")).truncated());
        assert!(!make("", Some("length")).refused());
        // 正常終了なのに1文字も返らない = 検閲の疑い(§8.1)。**打ち切りではない**
        assert!(make("", Some("stop")).refused());
        assert!(!make("", Some("stop")).truncated());
        // 終了理由を返さないサーバでも、空なら拒否として扱う
        assert!(make("", None).refused());
        assert!(!make("", None).truncated());
        // 普通の応答はどちらでもない
        assert!(!make("普通の応答", Some("stop")).refused());
        assert!(!make("普通の応答", Some("stop")).truncated());
    }

    /// **2つの判定が同時に真にならないこと。** 重なっていると、呼び出し側が
    /// どちらを先に見たかで診断が変わり、実際に校正と抽出が拒否を打ち切りと誤診していた
    #[test]
    fn truncation_and_refusal_never_overlap() {
        for content in ["", "  ", "本文"] {
            for reason in [None, Some("stop"), Some("length"), Some("content_filter")] {
                let o = ChatOutcome {
                    content: content.to_string(),
                    usage: None,
                    finish_reason: reason.map(|s| s.to_string()),
                };
                assert!(
                    !(o.truncated() && o.refused()),
                    "重なっている: {content:?} / {reason:?}"
                );
            }
        }
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
