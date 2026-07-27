//! ゴールデンセット(最小版)。
//!
//! docs/05-roadmap.md M2 の「ゴールデンセット最小版(誤字10件程度)」の実体。
//! 検出率(recall)と誤検出率を**機械的に測る**ための固定データで、
//! プロンプトやアルゴリズムを変えたときの回帰判定に使う。
//!
//! ここで測るのは **機械照合による表記ゆれ検出(M-04-02)だけ**。
//! LLMによる誤字脱字検出(M-04-01)は接続先とモデルに依存するため、
//! PoC#7 で同じ本文を使って別途測る(手順は docs/04-design.md §7)。

use kaku_lib::proofread;

/// 設定に登録されている名前(codex の正式名+別名を想定)
fn codex_names() -> Vec<String> {
    [
        "佐藤架純",
        "架純",
        "五十嵐悠二",
        "悠二",
        "県立青葉高校",
        "ウルスラ",
        "白鏡の塔",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// 意図的に表記ゆれを埋め込んだ本文。
///
/// 埋め込んだ誤り(10件):
///  1. 佳純      ← 架純(同音異字)
///  2. 佐藤佳純  ← 佐藤架純
///  3. 五十嵐悠仁 ← 五十嵐悠二
///  4. 悠仁      ← 悠二
///  5. 県立青葉高枚 ← 県立青葉高校
///  6. ウルスナ  ← ウルスラ(カタカナ1文字違い)
///  7. 白鏡の搭  ← 白鏡の塔 ※「の」を挟むため候補語は「白鏡」「搭」に割れる → **検出できない既知の限界**
///  8. 五十悠二  ← 五十嵐悠二(脱字)
///  9. 架澄      ← 架純
/// 10. 青葉高校  ← 県立青葉高校 ※「県立」が無い。距離2のため **検出できない既知の限界**
const MANUSCRIPT: &str = "\
　転校初日の朝は、雨だった。佐藤架純は昇降口で靴を履き替える。
　佳純は前の学校のことを思い出していた。佐藤佳純と呼ばれるのは久しぶりだ。
　県立青葉高校の廊下は薄暗い。県立青葉高枚の噂は聞いていた。
　五十嵐悠二がこちらを見た。五十嵐悠仁は何も言わない。悠二は歩き去る。
　悠仁の背中を、架純は目で追った。五十悠二という名を、彼女は知らない。
　架澄という字を書く者もいる、と誰かが言った。
　ウルスラの物語を思い出す。ウルスナは塔にいた。
　白鏡の塔と、白鏡の搭。青葉高校の制服は紺色だ。
";

/// 検出できるべき誤り(このアルゴリズムの守備範囲)
const EXPECTED: &[(&str, &str)] = &[
    ("佳純", "架純"),
    ("佐藤佳純", "佐藤架純"),
    ("五十嵐悠仁", "五十嵐悠二"),
    ("悠仁", "悠二"),
    ("県立青葉高枚", "県立青葉高校"),
    ("五十悠二", "五十嵐悠二"),
    ("架澄", "架純"),
    ("ウルスナ", "ウルスラ"),
];

/// 誤りではないので指摘してはいけない語
const MUST_NOT_FLAG: &[&str] = &[
    "転校初日", // 普通の語
    "昇降口",
    "紺色",
    "青葉高校", // 「県立」が無いだけの短縮形。距離2で拾わない設計
    "白鏡",
];

#[test]
fn golden_recall_and_precision() {
    let hits = proofread::check_notation(MANUSCRIPT, &codex_names());

    // --- 検出率 ---
    let mut missed = Vec::new();
    for (candidate, suggestion) in EXPECTED {
        let found = hits
            .iter()
            .any(|h| h.candidate == *candidate && h.suggestion == *suggestion);
        if !found {
            missed.push(format!("{candidate} -> {suggestion}"));
        }
    }
    let recall = (EXPECTED.len() - missed.len()) as f64 / EXPECTED.len() as f64;

    // --- 誤検出 ---
    let false_positives: Vec<&str> = hits
        .iter()
        .map(|h| h.candidate.as_str())
        .filter(|c| MUST_NOT_FLAG.contains(c))
        .collect();

    // 想定外の指摘(EXPECTEDにもMUST_NOT_FLAGにも無いもの)も記録して見えるようにする
    let unexpected: Vec<&str> = hits
        .iter()
        .map(|h| h.candidate.as_str())
        .filter(|c| !EXPECTED.iter().any(|(e, _)| e == c) && !MUST_NOT_FLAG.contains(c))
        .collect();

    eprintln!(
        "表記ゆれ検出: recall={:.0}% ({}/{}) / 誤検出={} / 想定外={:?}",
        recall * 100.0,
        EXPECTED.len() - missed.len(),
        EXPECTED.len(),
        false_positives.len(),
        unexpected
    );
    if !missed.is_empty() {
        eprintln!("未検出: {missed:?}");
    }

    // 誤検出は1件も許さない(M-04-04: 会話文や普通の語を指摘しない)
    assert!(
        false_positives.is_empty(),
        "指摘してはいけない語を検出した: {false_positives:?}"
    );
    // 守備範囲の誤りは全部拾えること
    assert!(
        missed.is_empty(),
        "検出できるはずの表記ゆれを取りこぼした: {missed:?}"
    );
}

#[test]
fn known_limitations_are_documented() {
    let hits = proofread::check_notation(MANUSCRIPT, &codex_names());
    let flagged: Vec<&str> = hits.iter().map(|h| h.candidate.as_str()).collect();

    // 既知の限界1: 「白鏡の搭」は助詞を挟むため語が割れて拾えない
    assert!(
        !flagged.contains(&"白鏡の搭"),
        "この限界が解消されたなら、テストとロードマップを更新すること"
    );
    // 既知の限界2: 「青葉高校」は「県立青葉高校」と距離2のため拾えない
    assert!(!flagged.contains(&"青葉高校"));
}

#[test]
fn confidence_is_high_when_correct_name_coexists() {
    let hits = proofread::check_notation(MANUSCRIPT, &codex_names());
    // 本文には正しい名前も出ているので、主要な指摘は確度「高」で出る
    let kasumi = hits.iter().find(|h| h.candidate == "佳純").unwrap();
    assert_eq!(kasumi.confidence, "high");
    assert!(kasumi.suggestion_count > 0);
    // 確度の高いものが先頭に並ぶ
    assert_eq!(hits.first().map(|h| h.confidence.as_str()), Some("high"));
}
