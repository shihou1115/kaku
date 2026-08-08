//! AIへ渡すコンテキストの組み立て(docs/04-design.md §6.2)。
//!
//! **V1は3系統だけ**: 対象本文 / 名前一致した codex / ユーザーが手動追加した codex。
//! 優先順位アルゴリズム・トークン予算計算・段階的縮退・関係グラフ辿り・importance は
//! 撤回済み(docs/06-decision-log.md §4)。長すぎる場合は**単純な切り捨て**。

use serde::{Deserialize, Serialize};

use crate::project::CodexEntry;

/// AIへ実際に渡す素材の一覧。**送信前にユーザーへ提示する**(U-05の透明性)。
///
/// フロントで内容を確認してからそのまま送信コマンドへ返すため Deserialize も持つ。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPreview {
    /// 対象本文(必要なら末尾を切り捨てたもの)
    pub body: String,
    pub body_truncated: bool,
    /// 実際に含める codex エントリ
    pub entries: Vec<ContextEntry>,
    /// 上限で落としたエントリ数
    pub dropped_entries: usize,
    pub total_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEntry {
    pub path: String,
    pub title: String,
    /// "mention" = 本文に名前が出た / "manual" = ユーザーが手動で追加
    pub source: String,
    pub text: String,
}

/// 上限。トークン数ではなく文字数で単純に切る(V1の割り切り)。
pub const MAX_BODY_CHARS: usize = 12_000;
pub const MAX_ENTRIES: usize = 20;
pub const MAX_ENTRY_CHARS: usize = 1_200;

fn truncate(s: &str, max: usize) -> (String, bool) {
    if s.chars().count() <= max {
        return (s.to_string(), false);
    }
    let cut: String = s.chars().take(max).collect();
    (cut, true)
}

/// 対象本文と codex から、実際に送る内容を組み立てる。
///
/// `mentioned_paths` は言及検出で当たったエントリのパス、
/// `manual_paths` はユーザーがUIで選んだエントリのパス。
pub fn build(
    body: &str,
    codex: &[CodexEntry],
    entry_texts: &dyn Fn(&str) -> Option<String>,
    mentioned_paths: &[String],
    manual_paths: &[String],
) -> ContextPreview {
    let (body_text, body_truncated) = truncate(body, MAX_BODY_CHARS);

    let mut entries: Vec<ContextEntry> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();

    // 手動追加を先に入れる(ユーザーの明示的な意思を優先して切り捨てから守る)
    for (paths, source) in [(manual_paths, "manual"), (mentioned_paths, "mention")] {
        for p in paths {
            if seen.iter().any(|s| *s == p.as_str()) {
                continue;
            }
            let Some(c) = codex.iter().find(|c| &c.path == p) else {
                continue;
            };
            let Some(raw) = entry_texts(p) else { continue };
            let (text, _) = truncate(raw.trim(), MAX_ENTRY_CHARS);
            seen.push(p.as_str());
            entries.push(ContextEntry {
                path: c.path.clone(),
                title: c.title.clone(),
                source: source.to_string(),
                text,
            });
        }
    }

    let dropped_entries = entries.len().saturating_sub(MAX_ENTRIES);
    entries.truncate(MAX_ENTRIES);

    let total_chars = body_text.chars().count()
        + entries
            .iter()
            .map(|e| e.text.chars().count() + e.title.chars().count())
            .sum::<usize>();

    ContextPreview {
        body: body_text,
        body_truncated,
        entries,
        dropped_entries,
        total_chars,
    }
}

/// システムプロンプト。**AIは編集者であり、本文を書かない**(docs/02 §1.1の境界)。
pub const SYSTEM_PROMPT: &str = "\
あなたは小説の編集者兼アシスタントです。執筆者を助けますが、**本文の代筆はしません**。
- 求められた場合を除き、小説の地の文や会話文をそのまま書き下ろすことはしません。
- 代わりに、案の提示・整理・質問・指摘・要約を行います。
- 与えられた設定資料と矛盾しないようにし、資料に無いことは推測であると明示します。
- 日本語で、簡潔に答えます。";

/// 会話の1往復(§5.7 会話モード)。
///
/// **保持するのはメモリ上だけ**(A案)。アプリを閉じれば消え、残したいものは
/// 「この相談を残す」で `ideas/` へ書く。会話は原稿ではなく過程の副産物であり、
/// 正本(D-1)へ自動で混ぜない。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub question: String,
    pub answer: String,
}

/// 会話履歴に許す長さ(文字数)。超えた分は**古い往復から落とす**。
///
/// 要約による圧縮はしない — 情報が落ちるうえ、要約のためのAI呼び出しが増える
/// (§5.7 / 開発原則7。撤回済みの「予算計算・段階的縮退」を履歴管理の名目で
/// 復活させないこと=[06-decision-log.md] §4)。
pub const MAX_HISTORY_CHARS: usize = 8_000;

/// 履歴を上限に収める。**新しい往復を残す**(直前のやりとりが一番効くため)。
pub fn trim_history(turns: &[ChatTurn], max_chars: usize) -> Vec<ChatTurn> {
    let mut kept: Vec<ChatTurn> = Vec::new();
    let mut total = 0usize;
    for t in turns.iter().rev() {
        let len = t.question.chars().count() + t.answer.chars().count();
        if total + len > max_chars && !kept.is_empty() {
            break;
        }
        total += len;
        kept.push(t.clone());
    }
    kept.reverse();
    kept
}

/// 送るメッセージ列を組み立てる。
///
/// **設定資料と本文は最初のユーザーメッセージにだけ載せる。** 往復のたびに足すと
/// 数千字が積み上がり、ローカルLLMの上限にすぐ当たる(§5.7 のコンテキストの送り方)。
/// 載せるのは常に**最新の**本文なので、途中で書き足した分が見えなくなることもない。
pub fn build_chat_messages(
    ctx: &ContextPreview,
    question: &str,
    history: &[ChatTurn],
) -> Vec<crate::ai::ChatMessage> {
    let msg = |role: &str, content: String| crate::ai::ChatMessage {
        role: role.to_string(),
        content,
    };
    let mut messages = vec![msg("system", SYSTEM_PROMPT.to_string())];

    let history = trim_history(history, MAX_HISTORY_CHARS);
    for (i, turn) in history.iter().enumerate() {
        // 素材が載るのは先頭の1通だけ。落として先頭が入れ替わったら、そこへ載せ直す
        let content = if i == 0 {
            render_user_message(ctx, &turn.question)
        } else {
            turn.question.clone()
        };
        messages.push(msg("user", content));
        messages.push(msg("assistant", turn.answer.clone()));
    }

    let content = if history.is_empty() {
        render_user_message(ctx, question)
    } else {
        question.to_string()
    };
    messages.push(msg("user", content));
    messages
}

/// 組み立てた素材を1つのユーザーメッセージにする。
pub fn render_user_message(ctx: &ContextPreview, question: &str) -> String {
    let mut s = String::new();
    if !ctx.entries.is_empty() {
        s.push_str("# 設定資料\n\n");
        for e in &ctx.entries {
            s.push_str(&format!("## {}\n{}\n\n", e.title, e.text));
        }
    }
    if !ctx.body.trim().is_empty() {
        s.push_str("# 対象本文\n\n");
        s.push_str(&ctx.body);
        if ctx.body_truncated {
            s.push_str("\n\n(※本文はここで切り捨てられています)");
        }
        s.push_str("\n\n");
    }
    s.push_str("# 依頼\n\n");
    s.push_str(question);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, title: &str) -> CodexEntry {
        CodexEntry {
            path: path.to_string(),
            title: title.to_string(),
            aliases: vec![],
            type_: None,
            description: None,
        }
    }

    #[test]
    fn manual_entries_come_first_and_dedupe() {
        let codex = vec![
            entry("codex/characters/a.md", "架純"),
            entry("codex/characters/b.md", "悠二"),
        ];
        let texts = |p: &str| Some(format!("{p} の中身"));
        let ctx = build(
            "本文",
            &codex,
            &texts,
            &["codex/characters/a.md".into(), "codex/characters/b.md".into()],
            &["codex/characters/b.md".into()],
        );
        assert_eq!(ctx.entries.len(), 2);
        // 手動指定の b が先、かつ mention 側で重複しない
        assert_eq!(ctx.entries[0].title, "悠二");
        assert_eq!(ctx.entries[0].source, "manual");
        assert_eq!(ctx.entries[1].source, "mention");
    }

    #[test]
    fn truncates_long_body() {
        let long = "あ".repeat(MAX_BODY_CHARS + 100);
        let ctx = build(&long, &[], &|_| None, &[], &[]);
        assert!(ctx.body_truncated);
        assert_eq!(ctx.body.chars().count(), MAX_BODY_CHARS);
    }

    #[test]
    fn skips_entries_without_text() {
        let codex = vec![entry("codex/x.md", "X")];
        let ctx = build("本文", &codex, &|_| None, &["codex/x.md".into()], &[]);
        assert!(ctx.entries.is_empty());
    }

    #[test]
    fn rendered_message_contains_sections() {
        let codex = vec![entry("codex/x.md", "架純")];
        let ctx = build(
            "　転校初日。",
            &codex,
            &|_| Some("主人公".into()),
            &["codex/x.md".into()],
            &[],
        );
        let msg = render_user_message(&ctx, "この場面の問題点は?");
        assert!(msg.contains("# 設定資料"));
        assert!(msg.contains("## 架純"));
        assert!(msg.contains("# 対象本文"));
        assert!(msg.contains("# 依頼"));
    }

    // ===== 会話モード(§5.7) =====

    fn ctx_with_body(body: &str) -> ContextPreview {
        let codex = vec![entry("codex/x.md", "架純")];
        build(body, &codex, &|_| Some("主人公".into()), &["codex/x.md".into()], &[])
    }

    fn turn(q: &str, a: &str) -> ChatTurn {
        ChatTurn {
            question: q.to_string(),
            answer: a.to_string(),
        }
    }

    /// 履歴が無ければ、これまでと同じ2通(system + user)のまま
    #[test]
    fn without_history_the_shape_is_unchanged() {
        let ctx = ctx_with_body("　転校初日。");
        let msgs = build_chat_messages(&ctx, "問題点は?", &[]);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert!(msgs[1].content.contains("# 対象本文"));
    }

    /// **素材が載るのは先頭のユーザーメッセージだけ。**
    /// 往復のたびに本文を足すと、数千字が積み上がって上限にすぐ当たる
    #[test]
    fn materials_ride_only_on_the_first_user_message() {
        let ctx = ctx_with_body("　転校初日。");
        let history = vec![turn("1つ目", "答え1"), turn("2つ目", "答え2")];
        let msgs = build_chat_messages(&ctx, "3つ目", &history);

        let roles: Vec<&str> = msgs.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(
            roles,
            vec!["system", "user", "assistant", "user", "assistant", "user"]
        );
        assert!(msgs[1].content.contains("# 対象本文"), "先頭には素材が載る");
        assert_eq!(msgs[3].content, "2つ目", "2通目以降は依頼だけ");
        assert_eq!(msgs[5].content, "3つ目", "最後の依頼にも素材は載せない");
        assert_eq!(
            msgs.iter().filter(|m| m.content.contains("# 対象本文")).count(),
            1
        );
    }

    /// 上限を超えたら**古い往復から落とす**(新しい方を残す)
    #[test]
    fn old_turns_are_dropped_first() {
        let long = "あ".repeat(1_000);
        let turns: Vec<ChatTurn> = (0..10)
            .map(|i| turn(&format!("依頼{i}"), &long))
            .collect();
        let kept = trim_history(&turns, 3_000);
        assert!(kept.len() < turns.len(), "落ちていない");
        assert_eq!(
            kept.last().unwrap().question,
            "依頼9",
            "直前のやりとりが残っていない"
        );
        assert!(!kept.iter().any(|t| t.question == "依頼0"));
    }

    /// 1往復だけで上限を超える場合でも、直前の1件は残す(空にしない)
    #[test]
    fn a_single_huge_turn_is_still_kept() {
        let turns = vec![turn("依頼", &"あ".repeat(50_000))];
        assert_eq!(trim_history(&turns, 100).len(), 1);
    }

    /// 古い往復が落ちて先頭が入れ替わったら、**そこへ素材を載せ直す**
    #[test]
    fn materials_move_to_the_new_first_turn_after_trimming() {
        let ctx = ctx_with_body("　転校初日。");
        let long = "あ".repeat(MAX_HISTORY_CHARS);
        let history = vec![turn("古い依頼", "短い答え"), turn("新しい依頼", &long)];
        let msgs = build_chat_messages(&ctx, "次の依頼", &history);

        assert!(
            !msgs.iter().any(|m| m.content.contains("古い依頼")),
            "古い往復が落ちていない"
        );
        assert!(msgs[1].content.contains("# 対象本文"), "素材が消えた");
        assert!(msgs[1].content.contains("新しい依頼"));
    }
}
