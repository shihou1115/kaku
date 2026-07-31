/**
 * 右ペイン: AI相談(MVP要素4 + M-03アイデア出し)。
 *
 * 原則:
 * - **送信内容を必ず事前に見せる**(U-05の透明性)。何を渡したか分からないまま送らない
 * - AIは編集者であり本文を書かない(システムプロンプトで明示)
 * - コンテキストは3系統だけ: 本文 / 名前一致codex / 手動追加codex
 *
 * M-03(2026-07-31 追加)のV1範囲は「方法論テンプレート起点 + 結果保存」:
 * - 依頼欄の上の**文例チップ**を押すと文面が入る。**送信はしない**(§5.6 案A)。
 *   押すたびに入れ替わる(選び直しても前の文面が残らない)
 * - 応答は「この相談を残す」で `ideas/` へMarkdown保存する。
 *   会話は原稿ではなく過程の副産物なので、**選んだものだけ**を正本へ置く(§5.7 A案)
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type AiSettings,
  type CodexEntry,
  type ContextPreview,
  type PromptTemplate,
} from "../api";
import { PromptDialog } from "./PromptDialog";
import { useMaterials } from "./useMaterials";

/** 常時見せる文例の数(カテゴリごと)。UI渋滞を避ける(§5.6) */
const VISIBLE_PER_CATEGORY = 2;

/** ファイル名に使えない文字を落とす */
const safeFileName = (s: string) =>
  s.replace(/[\\/:*?"<>|]/g, "_").slice(0, 40) || "相談";

/** `2026-07-31-1435` の形。並べたときに時系列になる */
function stamp(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}-${p(
    d.getHours(),
  )}${p(d.getMinutes())}`;
}

type Props = {
  body: string;
  mentionedPaths: string[];
  codex: CodexEntry[];
  disabled: boolean;
  /** AI設定はAppが一元管理する(タブ常駐で写しを持つと上書き事故が起きる) */
  settings: AiSettings | null;
  onPatchSettings: (p: Partial<AiSettings>) => void;
  /** 接続状態もAppが持つ(ヘッダーの状態表示と食い違わせないため) */
  models: string[];
  connDetail: string;
  onCheckConnection: () => Promise<string[]>;
  /** 実行中であることをヘッダーへ伝える。終わったら null */
  onBusy: (label: string | null) => void;
  /** 設定の中身を参照タブで開く */
  onShowReference: (path: string) => void;
  /** 相談を ideas/ へ残したあと。ツリーの読み直しと通知に使う */
  onSaved: (path: string | null, error?: string) => void;
};

export function AiPanel({
  body,
  mentionedPaths,
  codex,
  disabled,
  settings,
  onPatchSettings,
  models,
  connDetail,
  onCheckConnection,
  onBusy,
  onShowReference,
  onSaved,
}: Props) {
  const [showSettings, setShowSettings] = useState(false);
  /** 渡す資料の選択。自動で当たった分も**外せる**(U-05 / §6.2) */
  const materials = useMaterials(mentionedPaths);
  const [preview, setPreview] = useState<ContextPreview | null>(null);
  const [question, setQuestion] = useState("");
  const [answer, setAnswer] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** 応答が空のまま終わったか。検閲による拒否で起きうる(04-design §8.1) */
  const [emptyAnswer, setEmptyAnswer] = useState(false);
  const answerRef = useRef<HTMLDivElement | null>(null);
  const questionRef = useRef<HTMLTextAreaElement | null>(null);

  // --- M-03: 依頼の文例(設定フォルダのMarkdownが実体) ---
  const [prompts, setPrompts] = useState<PromptTemplate[]>([]);
  const [showAllPrompts, setShowAllPrompts] = useState(false);
  /** 保存する相談の件名を聞く */
  const [saving, setSaving] = useState<string | null>(null);
  /** 保存時の依頼と応答を固定する(保存中に書き換わっても取り違えない) */
  const saved = useRef<{ question: string; answer: string } | null>(null);

  useEffect(() => {
    api.listPrompts().then(setPrompts).catch(() => {
      /* 文例が無くても相談自体はできる */
    });
  }, []);

  /** カテゴリごとにまとめる。並び順はファイル名(数字の接頭辞)で決まる */
  const promptGroups = useMemo(() => {
    const groups: { category: string; items: PromptTemplate[] }[] = [];
    for (const p of prompts) {
      const g = groups.find((x) => x.category === p.category);
      if (g) g.items.push(p);
      else groups.push({ category: p.category, items: [p] });
    }
    return groups;
  }, [prompts]);

  /**
   * 文例を依頼欄へ入れる。**送信はしない**(§5.6 制約2)。
   *
   * 押すたびに**入れ替える**。足していく方式にすると、文例を選び直したときに
   * 前の文面が残ったまま次が積み重なる(2026-07-31 の指摘)。
   * 文例は「どれか1つを選ぶ」ものなので、置き換えが素直な挙動になる。
   */
  const applyPrompt = useCallback((body: string) => {
    setQuestion(body);
    questionRef.current?.focus();
  }, []);

  const patch = onPatchSettings;

  const connect = useCallback(async () => {
    const list = await onCheckConnection();
    // モデル未選択なら先頭を入れておく(P-8: 選ばせる手間を減らす)
    if (list.length > 0 && settings && !settings.model) {
      patch({ model: list[0] });
    }
  }, [settings, patch, onCheckConnection]);

  const { autoPaths, manualPaths } = materials;

  const makePreview = useCallback(async () => {
    setError(null);
    try {
      setPreview(await api.buildContext(body, autoPaths, manualPaths));
    } catch (e) {
      setError(String(e));
    }
  }, [body, autoPaths, manualPaths]);

  const send = useCallback(async () => {
    if (!question.trim()) return;
    let ctx = preview;
    if (!ctx) {
      try {
        ctx = await api.buildContext(body, autoPaths, manualPaths);
        setPreview(ctx);
      } catch (e) {
        setError(String(e));
        return;
      }
    }
    setBusy(true);
    onBusy("応答中");
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
      onBusy(null);
    }
  }, [question, preview, body, autoPaths, manualPaths, onBusy]);

  /**
   * この相談を `ideas/` へ残す(M-03の「結果保存」)。
   *
   * 会話は原稿ではなく過程の副産物なので**自動保存はしない**。残すと決めたものだけを
   * 正本へ置く(§5.7 A案 / 未決-5と同じ判断)。中身は普通のMarkdownなので、
   * あとから設定として使いたければそのまま codex へ移せる。
   */
  const saveIdea = useCallback(
    async (name: string) => {
      const src = saved.current;
      setSaving(null);
      if (!src) return;
      const path = `ideas/${stamp(new Date())}-${safeFileName(name)}.md`;
      const md = [
        "---",
        `title: ${name}`,
        `created: ${new Date().toISOString()}`,
        `model: ${settings?.model ?? ""}`,
        "---",
        "",
        "## 依頼",
        "",
        src.question,
        "",
        "## 応答",
        "",
        src.answer,
        "",
      ].join("\n");
      try {
        const created = await api.createFile(path, md);
        onSaved(created ? path : null, created ? undefined : "同名のファイルが既にあります");
      } catch (e) {
        onSaved(null, String(e));
      }
    },
    [settings, onSaved],
  );

  /** 本文で名前が当たったエントリ(外したものも一覧には残す) */
  const found = codex.filter((c) => mentionedPaths.includes(c.path));
  /** 手動で足せる候補。自動で当たっている分は上の一覧で扱うので出さない */
  const addable = codex.filter((c) => !mentionedPaths.includes(c.path));

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
              <span className="conn">{connDetail || "未確認"}</span>
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
        <div className="block-head">
          <h2>渡す設定資料</h2>
          {found.length > 1 && (
            <button
              className="mini"
              onClick={() => materials.setAllAuto(materials.excludedCount > 0)}
            >
              {materials.excludedCount > 0 ? "すべて戻す" : "すべて外す"}
            </button>
          )}
        </div>
        <p className="hint">
          本文に名前が出たエントリは自動で入る。
          <strong>相談の内容に要らないものは外せる</strong>。
        </p>
        {found.length === 0 && (
          <p className="empty">本文中にcodexの名前が見つかりません。</p>
        )}

        {/* チェック=渡す/渡さない、名前=中身を見る。
            1つのlabelで包むとクリックが競合するので要素を分ける */}
        {found.map((c) => (
          <div className="mat-row" key={c.path}>
            <label className="check" title="外すとAIに渡しません">
              <input
                type="checkbox"
                checked={materials.isOn(c.path)}
                onChange={() => materials.toggleAuto(c.path)}
              />
            </label>
            <button
              className={materials.isOn(c.path) ? "chip auto" : "chip auto off"}
              title={`${c.path}(クリックで中身を見る)`}
              onClick={() => onShowReference(c.path)}
            >
              {c.title}
            </button>
          </div>
        ))}

        <details>
          <summary>手動で追加({materials.manualPaths.length})</summary>
          {addable.length === 0 ? (
            <p className="empty">ほかに追加できる設定がありません。</p>
          ) : (
            <div className="manual-list">
              {addable.map((c) => (
                <label key={c.path} className="check">
                  <input
                    type="checkbox"
                    checked={materials.isManual(c.path)}
                    onChange={() => materials.toggleManual(c.path)}
                  />
                  {c.title}
                </label>
              ))}
            </div>
          )}
        </details>
      </div>

      <div className="block">
        <div className="block-head">
          <h2>依頼</h2>
          {prompts.length > 0 && (
            <button
              className="mini"
              onClick={() => void api.openPromptsDir().catch(() => {})}
              title="文例は普通のMarkdownです。自分の言い方に書き換えられます"
            >
              文例を編集
            </button>
          )}
        </div>

        {/* 押すと文面が入るだけ。**送信はしない**ので、そのまま直せる(§5.6 案A) */}
        {promptGroups.length > 0 && (
          <div className="pt-groups">
            {promptGroups.map((g) => {
              const items = showAllPrompts
                ? g.items
                : g.items.slice(0, VISIBLE_PER_CATEGORY);
              if (items.length === 0) return null;
              return (
                <div className="pt-group" key={g.category}>
                  <span className="pt-category">{g.category}</span>
                  {items.map((p) => (
                    <button
                      key={`${g.category}/${p.title}`}
                      className="pt-chip"
                      onClick={() => applyPrompt(p.body)}
                      title={p.body}
                    >
                      {p.title}
                    </button>
                  ))}
                </div>
              );
            })}
            {prompts.length >
              promptGroups.length * VISIBLE_PER_CATEGORY && (
              <button
                className="mini pt-more"
                onClick={() => setShowAllPrompts((v) => !v)}
              >
                {showAllPrompts ? "折りたたむ" : "もっと見る"}
              </button>
            )}
          </div>
        )}

        <textarea
          className="question"
          ref={questionRef}
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
          <div className="block-head">
            <h2>応答</h2>
            {/* チャットで完結させず資産化する(02 M-03)。
                ただし残すかどうかはユーザーが決める */}
            {!busy && answer.trim() && (
              <button
                className="mini"
                onClick={() => {
                  saved.current = { question, answer };
                  setSaving(question.trim().slice(0, 20) || "相談");
                }}
                title="この依頼と応答を ideas/ にMarkdownで残します"
              >
                この相談を残す
              </button>
            )}
          </div>
          <div className="answer" ref={answerRef}>
            {answer}
            {busy && <span className="caret">▍</span>}
          </div>
        </div>
      )}

      {saving !== null && (
        <PromptDialog
          title="この相談を残す"
          label="件名(ファイル名になります)"
          initial={saving}
          onSubmit={(v) => void saveIdea(v)}
          onCancel={() => setSaving(null)}
        />
      )}
    </div>
  );
}
