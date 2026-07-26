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
}
