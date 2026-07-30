/**
 * 右ペイン: AI相談(MVP要素4)。
 *
 * 原則:
 * - **送信内容を必ず事前に見せる**(U-05の透明性)。何を渡したか分からないまま送らない
 * - AIは編集者であり本文を書かない(システムプロンプトで明示)
 * - コンテキストは3系統だけ: 本文 / 名前一致codex / 手動追加codex
 */

import { useCallback, useRef, useState } from "react";
import {
  api,
  type AiSettings,
  type CodexEntry,
  type ContextPreview,
} from "../api";

type Props = {
  body: string;
  mentionedPaths: string[];
  codex: CodexEntry[];
  disabled: boolean;
  /** AI設定はAppが一元管理する(タブ常駐で写しを持つと上書き事故が起きる) */
  settings: AiSettings | null;
  onPatchSettings: (p: Partial<AiSettings>) => void;
  /** 設定の中身を参照タブで開く */
  onShowReference: (path: string) => void;
};

export function AiPanel({
  body,
  mentionedPaths,
  codex,
  disabled,
  settings,
  onPatchSettings,
  onShowReference,
}: Props) {
  const [models, setModels] = useState<string[]>([]);
  const [connState, setConnState] = useState("未接続");
  const [showSettings, setShowSettings] = useState(false);
  const [manual, setManual] = useState<string[]>([]);
  const [preview, setPreview] = useState<ContextPreview | null>(null);
  const [question, setQuestion] = useState("");
  const [answer, setAnswer] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** 応答が空のまま終わったか。検閲による拒否で起きうる(04-design §8.1) */
  const [emptyAnswer, setEmptyAnswer] = useState(false);
  const answerRef = useRef<HTMLDivElement | null>(null);

  const patch = onPatchSettings;

  const connect = useCallback(async () => {
    setConnState("接続中…");
    try {
      const list = await api.listModels();
      setModels(list);
      setConnState(`接続OK (${list.length}モデル)`);
      if (list.length > 0 && settings && !settings.model) {
        patch({ model: list[0] });
      }
    } catch (e) {
      setConnState(String(e));
    }
  }, [settings, patch]);

  const makePreview = useCallback(async () => {
    setError(null);
    try {
      setPreview(await api.buildContext(body, mentionedPaths, manual));
    } catch (e) {
      setError(String(e));
    }
  }, [body, mentionedPaths, manual]);

  const send = useCallback(async () => {
    if (!question.trim()) return;
    let ctx = preview;
    if (!ctx) {
      try {
        ctx = await api.buildContext(body, mentionedPaths, manual);
        setPreview(ctx);
      } catch (e) {
        setError(String(e));
        return;
      }
    }
    setBusy(true);
    setAnswer("");
    setError(null);
    setEmptyAnswer(false);
    let received = 0;
    try {
      await api.askAi(ctx, question, (ev) => {
        if (ev.kind === "Delta") {
          received += ev.value.length;
          setAnswer((a) => a + ev.value);
          answerRef.current?.scrollTo(0, answerRef.current.scrollHeight);
        } else if (ev.kind === "Error") {
          setError(ev.value);
        }
      });
      // 応答が1文字も返らないことがある。多くは検閲による拒否(§8.1)。
      // 画面が無反応に見えて原因が分からないので、明示して次の手を示す
      if (received === 0) setEmptyAnswer(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [question, preview, body, mentionedPaths, manual]);

  const mentioned = codex.filter((c) => mentionedPaths.includes(c.path));

  return (
    <div className="ai-panel">
      <div className="block">
        <div className="block-head">
          <h2>AI相談</h2>
          <button className="mini" onClick={() => setShowSettings((v) => !v)}>
            設定
          </button>
        </div>
        {showSettings && settings && (
          <div className="settings">
            <label>
              接続先
              <input
                value={settings.base_url}
                onChange={(e) => patch({ base_url: e.target.value })}
                spellCheck={false}
              />
            </label>
            <label>
              APIキー(ローカルLLMでは不要)
              <input
                type="password"
                value={settings.api_key ?? ""}
                onChange={(e) => patch({ api_key: e.target.value || null })}
              />
            </label>
            <div className="row">
              <button onClick={connect}>接続テスト</button>
              <span className="conn">{connState}</span>
            </div>
            <label>
              モデル
              {models.length > 0 ? (
                <select
                  value={settings.model}
                  onChange={(e) => patch({ model: e.target.value })}
                >
                  {models.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  value={settings.model}
                  onChange={(e) => patch({ model: e.target.value })}
                  placeholder="接続テストで一覧を取得"
                  spellCheck={false}
                />
              )}
            </label>
          </div>
        )}
      </div>

      <div className="block">
        <h2>渡す設定資料</h2>
        <p className="hint">
          本文に名前が出たエントリは自動で入る。足りなければ手動で足す。
        </p>
        {mentioned.length === 0 && (
          <p className="empty">本文中にcodexの名前が見つかりません。</p>
        )}
        {mentioned.map((c) => (
          <button
            key={c.path}
            className="chip auto"
            title={`${c.path}(クリックで中身を見る)`}
            onClick={() => onShowReference(c.path)}
          >
            {c.title}
          </button>
        ))}
        <details>
          <summary>手動で追加({manual.length})</summary>
          <div className="manual-list">
            {codex.map((c) => (
              <label key={c.path} className="check">
                <input
                  type="checkbox"
                  checked={manual.includes(c.path)}
                  onChange={(e) =>
                    setManual((m) =>
                      e.target.checked
                        ? [...m, c.path]
                        : m.filter((p) => p !== c.path),
                    )
                  }
                />
                {c.title}
              </label>
            ))}
          </div>
        </details>
      </div>

      <div className="block">
        <h2>依頼</h2>
        <textarea
          className="question"
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          placeholder="例: この場面で読者が混乱しそうな箇所を指摘して"
        />
        <div className="row">
          <button onClick={makePreview} disabled={disabled}>
            送信内容を確認
          </button>
          <button className="primary" onClick={send} disabled={disabled || busy}>
            {busy ? "応答中…" : "送信"}
          </button>
        </div>
        {preview && (
          <details className="preview" open>
            <summary>
              送信内容 {preview.total_chars}字 / 資料{preview.entries.length}件
              {preview.body_truncated && " (本文は末尾を切り捨て)"}
            </summary>
            <ul>
              {preview.entries.map((e) => (
                <li key={e.path}>
                  {e.source === "manual" ? "手動" : "自動"}: {e.title}(
                  {e.text.length}字)
                </li>
              ))}
            </ul>
          </details>
        )}
        {error && <p className="error">{error}</p>}
        {emptyAnswer && !error && (
          <p className="pf-stale">
            モデルが応答を返しませんでした。
            <strong>題材によっては検閲で拒否される</strong>ことがあります
            (犯罪・暴力・性描写など)。校正や設定抽出は通っても、
            <strong>展開案のような「書かせる」依頼は拒否されやすい</strong>傾向があります。
            非検閲モデルに切り替えてお試しください。
          </p>
        )}
      </div>

      {(answer || busy) && (
        <div className="block answer-block">
          <h2>応答</h2>
          <div className="answer" ref={answerRef}>
            {answer}
            {busy && <span className="caret">▍</span>}
          </div>
        </div>
      )}
    </div>
  );
}
