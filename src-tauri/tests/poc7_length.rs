//! PoC#7の追試: **本文の長さと誤字検知率の関係を実測する**。
//!
//! 「3000字を超えると検知率が有意に低下する」という観察が、
//!  (a) モデル固有の性質なのか
//!  (b) ロード中のコンテキスト長の制約なのか
//! を切り分ける。(b)なら、固定3000字ではなくコンテキスト長に応じた分割が正しい。
//!
//! 実行(LM Studio が起動しモデルがロードされていること):
//!   cargo test --test poc7_length -- --ignored --nocapture
//!
//! 接続先とモデルは環境変数で変えられる:
//!   KAKU_BASE_URL (既定 http://localhost:1234/v1)
//!   KAKU_MODEL    (既定 /v1/models の先頭)

use kaku_lib::{ai, proofread};

/// 埋め込む誤り。いずれも文脈に依存せず明確に誤りと分かるもの
const ERRORS: &[(&str, &str)] = &[
    ("そのとうり", "そのとおり"),
    ("言つた", "言った"),
    ("一つづつ", "一つずつ"),
    ("歩いいた", "歩いた"),
    ("以外に静か", "意外に静か"),
    ("感違い", "勘違い"),
    ("雰意気", "雰囲気"),
    ("完壁", "完璧"),
    ("一諸に", "一緒に"),
    ("original", "オリジナル"),
];

/// 誤りを含まない埋め草。1つおよそ100字
fn filler(i: usize) -> String {
    let bodies = [
        "　窓の外では雨が降り続いていた。傘を差した人々が足早に通り過ぎていく。図書室の机には古い本が積まれ、紙の匂いが静かに漂っている。",
        "　放課後の廊下は思いのほか静かで、遠くから吹奏楽部の音が届いてくる。夕日が窓枠の影を長く伸ばし、床に格子模様を描いていた。",
        "　彼女は鞄の中からノートを取り出し、ページをめくった。書きかけの文章が並んでいる。消しゴムの跡が、迷いのあとのように残っていた。",
        "　校庭では野球部が練習を続けている。金属バットの乾いた音が、規則正しく響いていた。空は次第に色を変え、雲の縁が金色に染まる。",
    ];
    format!("{}({})\n", bodies[i % bodies.len()], i)
}

/// 目標文字数の本文を作る。誤りは全体へ均等に散らす
fn build_text(target_chars: usize) -> String {
    let mut out = String::new();
    let mut error_idx = 0usize;
    let mut block = 0usize;
    // 誤りを入れる間隔(文字数)
    let interval = target_chars / ERRORS.len();

    while out.chars().count() < target_chars {
        if error_idx < ERRORS.len() && out.chars().count() >= interval * error_idx {
            out.push_str(&format!(
                "　男は{}と{}。\n",
                ERRORS[error_idx].0, "つぶやいた"
            ));
            error_idx += 1;
        }
        out.push_str(&filler(block));
        block += 1;
    }
    // 入りきらなかった誤りは末尾に足す(どの長さでも同じ10件を含める)
    while error_idx < ERRORS.len() {
        out.push_str(&format!("　男は{}とつぶやいた。\n", ERRORS[error_idx].0));
        error_idx += 1;
    }
    out
}

async fn resolve_model(base_url: &str) -> String {
    if let Ok(m) = std::env::var("KAKU_MODEL") {
        if !m.trim().is_empty() {
            return m;
        }
    }
    ai::list_models(base_url, &None)
        .await
        .ok()
        .and_then(|v| v.into_iter().next())
        .expect("モデル一覧を取得できませんでした")
}

#[tokio::test]
#[ignore = "LM Studio の実機が必要"]
async fn detection_rate_by_length() {
    let base_url =
        std::env::var("KAKU_BASE_URL").unwrap_or_else(|_| ai::DEFAULT_BASE_URL.to_string());
    let model = resolve_model(&base_url).await;
    eprintln!("接続先: {base_url}\nモデル: {model}\n");
    eprintln!("埋め込んだ誤り: {}件", ERRORS.len());
    eprintln!(
        "{:>7} | {:>6} | {:>8} | {:>6} | {:>7} | {}",
        "文字数", "検知", "検知率", "誤検出", "prompt", "所要"
    );
    eprintln!("{}", "-".repeat(64));

    let lengths: Vec<usize> = std::env::var("KAKU_LENGTHS")
        .ok()
        .map(|s| s.split(',').filter_map(|p| p.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![1_000, 3_000, 6_000, 9_000]);

    let repeat: usize = std::env::var("KAKU_REPEAT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    for target in lengths {
      for round in 1..=repeat {
        let _ = round;
        let text = build_text(target);
        let actual_chars = text.chars().count();
        let prompt = proofread::build_prompt(&text, &[]);
        let messages = vec![
            ai::ChatMessage {
                role: "system".into(),
                content:
                    "あなたは日本語の小説を校正する編集者です。指示に従い、JSONだけを返します。"
                        .into(),
            },
            ai::ChatMessage {
                role: "user".into(),
                content: prompt,
            },
        ];

        let started = std::time::Instant::now();
        let result = ai::chat(&base_url, &None, &model, &messages, 0.1, None).await;
        let elapsed = started.elapsed();

        match result {
            Ok(out) => {
                let (content, usage) = (out.content.clone(), out.usage);
                let issues = proofread::resolve_issues(&text, proofread::parse_ai_issues(&content));
                let detected = ERRORS
                    .iter()
                    .filter(|(wrong, _)| {
                        issues
                            .iter()
                            .any(|i| i.quote.contains(wrong) || i.suggestion.contains(wrong))
                    })
                    .count();
                // 埋め込んだ誤りに対応しない指摘の数(目安)
                let extra = issues
                    .iter()
                    .filter(|i| !ERRORS.iter().any(|(w, _)| i.quote.contains(w)))
                    .count();
                eprintln!(
                    "{:>7} | {:>4}/{:<2} | {:>7.0}% | {:>6} | {:>7} | {:.1}秒",
                    actual_chars,
                    detected,
                    ERRORS.len(),
                    detected as f64 / ERRORS.len() as f64 * 100.0,
                    extra,
                    usage.map(|u| u.prompt_tokens.to_string()).unwrap_or("-".into()),
                    elapsed.as_secs_f64()
                );
                // 0件のときは「見落とし」か「応答が壊れている」かを切り分ける
                if detected == 0 {
                    let parsed = proofread::parse_ai_issues(&content).len();
                    eprintln!(
                        "    診断: 応答{}文字 / パース結果{}件 / finish_reason={:?} / 打ち切り={} / completion={} / 末尾50字=<<{}>>",
                        content.chars().count(),
                        parsed,
                        out.finish_reason,
                        out.truncated(),
                        usage
                            .map(|u| u.completion_tokens.to_string())
                            .unwrap_or("-".into()),
                        content
                            .chars()
                            .rev()
                            .take(50)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect::<String>()
                    );
                }
            }
            Err(e) => {
                eprintln!(
                    "{:>7} | {:>7} | {:>8} | {:>6} | {:>7} | {:.1}秒  ← {}",
                    actual_chars, "失敗", "-", "-", "-", elapsed.as_secs_f64(), e
                );
            }
        }
      }
    }
}
