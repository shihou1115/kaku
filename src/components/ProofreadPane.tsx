/**
 * 右ペインの「校正」タブ(M-04)。2つの検査を並べる。
 *
 *  1. 表記ゆれ(M-04-02): codexとの機械照合。LLM不要・即座・無料・決定的
 *  2. 誤字脱字(M-04-01): LLM。接続とトークンを消費する
 *
 * 性質が違うので画面上でも明確に分ける。ユーザーが「どちらの結果を見ているか」
 * を取り違えると、決定的な指摘と確率的な指摘を同じ重みで扱ってしまう。
 *
 * 非破壊(M-04-03): 勝手に本文を書き換えない。1件ずつ移動して確認し、置換する。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type AiIssue, type NotationHit } from "../api";

type Props = {
  /** 現在の本文。結果の鮮度判定に使う */
  body: string;
  disabled: boolean;
  onJump: (from: number, to: number) => void;
  onReplace: (from: number, to: number, text: string) => void;
};

/** 引用文字列から位置を引き直す(D-7: 位置は保存せず表示時に解決する) */
function reresolve(issues: AiIssue[], body: string): AiIssue[] {
  return issues.map((i) => {
    const idx = body.indexOf(i.quote);
    return idx >= 0
      ? { ...i, found: true, start_utf16: idx, end_utf16: idx + i.quote.length }
      : { ...i, found: false, start_utf16: null, end_utf16: null };
  });
}

export function ProofreadPane({ body, disabled, onJump, onReplace }: Props) {
  // --- 表記ゆれ(機械照合) ---
  const [hits, setHits] = useState<NotationHit[] | null>(null);
  const [checkedBody, setCheckedBody] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rerun = useRef(false);

  // --- 誤字脱字(AI) ---
  const [issues, setIssues] = useState<AiIssue[] | null>(null);
  const [aiBody, setAiBody] = useState<string | null>(null);
  const [aiBusy, setAiBusy] = useState(false);
  const [aiError, setAiError] = useState<string | null>(null);
  const [aiMeta, setAiMeta] = useState<{
    path: string;
    model: string;
    unchecked: number;
  } | null>(null);

  const run = useCallback(async (target: string) => {
    setBusy(true);
    setError(null);
    try {
      const result = await api.checkNotation(target);
      setHits(result);
      setCheckedBody(target);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  // 置換で本文が変わったら測り直す(機械照合は即座に終わるので位置ずれを持ち越さない)
  useEffect(() => {
    if (!rerun.current) return;
    if (checkedBody === body) return;
    rerun.current = false;
    void run(body);
  }, [body, checkedBody, run]);

  const runAi = useCallback(async () => {
    setAiBusy(true);
    setAiError(null);
    try {
      const r = await api.proofreadAi(body);
      setIssues(r.issues);
      setAiBody(body);
      setAiMeta({ path: r.path, model: r.model, unchecked: r.unchecked_chars });
    } catch (e) {
      setAiError(String(e));
    } finally {
      setAiBusy(false);
    }
  }, [body]);

  const replaceNotation = useCallback(
    (from: number, to: number, text: string) => {
      rerun.current = true;
      onReplace(from, to, text);
    },
    [onReplace],
  );

  const replaceAllNotation = useCallback(
    (hit: NotationHit) => {
      // 後ろから適用する。先に前を書き換えると後続の位置がずれる
      const ordered = [...hit.occurrences].sort(
        (a, b) => b.start_utf16 - a.start_utf16,
      );
      rerun.current = true;
      for (const o of ordered) {
        onReplace(o.start_utf16, o.end_utf16, hit.suggestion);
      }
    },
    [onReplace],
  );

  /**
   * AIの指摘を1件適用する。
   * 再実行はしない(トークンを消費するため)。代わりに引用文字列から位置を引き直す。
   */
  const applyIssue = useCallback(
    (issue: AiIssue) => {
      if (issue.start_utf16 === null || issue.end_utf16 === null) return;
      const newBody =
        body.slice(0, issue.start_utf16) +
        issue.suggestion +
        body.slice(issue.end_utf16);
      onReplace(issue.start_utf16, issue.end_utf16, issue.suggestion);
      setIssues((prev) =>
        prev ? reresolve(prev.filter((i) => i !== issue), newBody) : prev,
      );
      setAiBody(newBody);
    },
    [body, onReplace],
  );

  const stale = checkedBody !== null && checkedBody !== body;
  const aiStale = aiBody !== null && aiBody !== body;

  return (
    <div className="proofread-pane">
      {/* ===== 表記ゆれ ===== */}
      <section className="pf-section">
        <h3 className="pf-section-title">
          表記ゆれ<span className="pf-tag local">設定と照合・即時</span>
        </h3>

        <div className="pf-head">
          <button
            className="primary"
            onClick={() => void run(body)}
            disabled={disabled || busy}
          >
            {busy ? "確認中…" : "確認する"}
          </button>
          {hits && !stale && (
            <span className="pf-count">
              {hits.length === 0 ? "指摘なし" : `${hits.length}件`}
            </span>
          )}
        </div>

        {stale && (
          <p className="pf-stale">
            本文が変わりました。もう一度確認してください。
          </p>
        )}
        {error && <p className="error">{error}</p>}

        {hits === null && !error && (
          <p className="hint pf-hint">
            設定(codex)の名前と<strong>1文字違い</strong>の語を探します。「架純」に対する
            「佳純」のような誤変換を、設定を知らない一般の校正ツールより確実に見つけます。
            ひらがなだけの語は誤検出が多いため対象外です。
          </p>
        )}
        {hits?.length === 0 && !stale && (
          <p className="hint pf-hint">紛らわしい語は見つかりませんでした。</p>
        )}

        {hits && hits.length > 0 && (
          <div className="pf-list">
            {hits.map((h) => (
              <div className="pf-item" key={`${h.candidate}-${h.suggestion}`}>
                <div className="pf-title">
                  <span className={`pf-badge ${h.confidence}`}>
                    {h.confidence === "high" ? "確度 高" : "確度 中"}
                  </span>
                  <span className="pf-candidate">{h.candidate}</span>
                  <span className="pf-arrow">→</span>
                  <span className="pf-suggestion">{h.suggestion}</span>
                </div>
                <p className="pf-reason">{h.reason}</p>
                <div className="pf-occurrences">
                  {h.occurrences.map((o, i) => (
                    <span className="pf-occ" key={o.start_utf16}>
                      <button
                        className="mini"
                        onClick={() => onJump(o.start_utf16, o.end_utf16)}
                        disabled={stale}
                        title="この箇所へ移動する"
                      >
                        {i + 1}箇所目
                      </button>
                      <button
                        className="mini"
                        disabled={stale}
                        onClick={() =>
                          replaceNotation(o.start_utf16, o.end_utf16, h.suggestion)
                        }
                        title={`「${h.suggestion}」に置き換える`}
                      >
                        置換
                      </button>
                    </span>
                  ))}
                  {h.occurrences.length > 1 && (
                    <button
                      className="mini pf-all"
                      disabled={stale}
                      onClick={() => replaceAllNotation(h)}
                      title={`${h.occurrences.length}箇所すべてを置き換える`}
                    >
                      すべて置換
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* ===== 誤字脱字(AI) ===== */}
      <section className="pf-section">
        <h3 className="pf-section-title">
          誤字脱字<span className="pf-tag ai">AI・トークンを使います</span>
        </h3>

        <div className="pf-head">
          <button onClick={() => void runAi()} disabled={disabled || aiBusy}>
            {aiBusy ? "確認中…" : "AIで確認する"}
          </button>
          {issues && !aiStale && (
            <span className="pf-count">
              {issues.length === 0 ? "指摘なし" : `${issues.length}件`}
            </span>
          )}
        </div>

        {aiMeta && (
          <p className="pf-meta">
            {aiMeta.model} / {aiMeta.path === "schema" ? "構造化出力" : "寛容パース"}
            {aiMeta.unchecked > 0 &&
              ` / 本文が長いため末尾${aiMeta.unchecked}字は未検査`}
          </p>
        )}
        {aiStale && (
          <p className="pf-stale">
            本文が変わりました。結果が古い可能性があります。
          </p>
        )}
        {aiError && <p className="error">{aiError}</p>}

        {issues === null && !aiError && (
          <p className="hint pf-hint">
            変換ミスや脱字を探します。会話文の崩しや意図的なひらがなは
            指摘しないよう指示しています。確率的な判定なので、
            <strong>採否は必ず自分で決めてください</strong>。
          </p>
        )}
        {issues?.length === 0 && !aiStale && (
          <p className="hint pf-hint">誤字脱字は見つかりませんでした。</p>
        )}

        {issues && issues.length > 0 && (
          <div className="pf-list">
            {issues.map((iss, idx) => (
              <div className="pf-item" key={`${iss.quote}-${idx}`}>
                <div className="pf-title">
                  <span className="pf-badge kind">{iss.kind}</span>
                  <span className="pf-candidate">{iss.quote}</span>
                  <span className="pf-arrow">→</span>
                  <span className="pf-suggestion">{iss.suggestion}</span>
                </div>
                {iss.reason && <p className="pf-reason">{iss.reason}</p>}
                <div className="pf-occurrences">
                  {iss.found ? (
                    <span className="pf-occ">
                      <button
                        className="mini"
                        disabled={aiStale}
                        onClick={() =>
                          onJump(iss.start_utf16!, iss.end_utf16!)
                        }
                      >
                        移動
                      </button>
                      <button
                        className="mini"
                        disabled={aiStale}
                        onClick={() => applyIssue(iss)}
                      >
                        置換
                      </button>
                    </span>
                  ) : (
                    <span className="pf-notfound">
                      本文に該当箇所が見つかりません(AIの取り違えの可能性)
                    </span>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
