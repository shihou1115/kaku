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
 *
 * 会話モード(2026-08-06 追加、§5.7):
 * - **既定はオフ**(単発)。発想を広げる相談は往復が要るが、校正・レビューでは履歴は
 *   ノイズとコストにしかならない。一律に付けないのが§5.7の結論
 * - 履歴は**メモリ上だけ**(A案)。閉じれば消える。残すものは `ideas/` へ書く
 * - 素材(本文・設定資料)は**先頭の1通にだけ**載る。往復ごとに足すと上限にすぐ当たる
 * - 溢れたら**古い往復から落とす**。要約による圧縮はしない
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type AiSettings,
  type ChatTurn,
  type CodexEntry,
  type ContextPreview,
  type PromptTemplate,
} from "../api";
import { PromptDialog } from "./PromptDialog";
import { useMaterials } from "./useMaterials";
import { frontmatter, notePath } from "./saveNote";

/** 常時見せる文例の数(カテゴリごと)。UI渋滞を避ける(§5.6) */
const VISIBLE_PER_CATEGORY = 2;

/** 送信した時点の材料。**あとから画面を触られても記録がずれない**ように固める */
type LastRun = {
  question: string;
  /** どの文書について相談したか */
  path: string | null;
  context: ContextPreview;
  /** 送信した時点までの往復(会話モード)。記録にはやりとり全体を残す */
  history: ChatTurn[];
};

type Props = {
  body: string;
  /** いま開いている文書。相談の記録に「何について相談したか」を残す */
  currentPath: string | null;
  mentionedPaths: string[];
  codex: CodexEntry[];
  disabled: boolean;
  /**
   * AI設定はAppが一元管理する(タブ常駐で写しを持つと上書き事故が起きる)。
   *
   * **ここでは編集しない。** 設定の編集はヘッダーの「設定」へ集約した(§5.11)。
   * このタブが持つのは相談だけで、settings は相談の記録にモデル名を残すために読むだけ
   */
  settings: AiSettings | null;
  /** 実行中であることをヘッダーへ伝える。終わったら null */
  onBusy: (label: string | null) => void;
  /** 設定の中身を参照タブで開く */
  onShowReference: (path: string) => void;
  /** 相談を ideas/ へ残したあと。ツリーの読み直しと通知に使う */
  onSaved: (path: string | null, error?: string) => void;
};

export function AiPanel({
  body,
  currentPath,
  mentionedPaths,
  codex,
  disabled,
  settings,
  onBusy,
  onShowReference,
  onSaved,
}: Props) {
  /** 渡す資料の選択。自動で当たった分も**外せる**(U-05 / §6.2) */
  const materials = useMaterials(mentionedPaths);
  const [preview, setPreview] = useState<ContextPreview | null>(null);
  const [question, setQuestion] = useState("");
  const [answer, setAnswer] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** 会話モード。**既定はオフ**(§5.7: 一律に付けない) */
  const [conversation, setConversation] = useState(false);
  /** これまでの往復。**メモリ上だけ**に持つ(§5.7 A案) */
  const [turns, setTurns] = useState<ChatTurn[]>([]);
  /** 応答が空のまま終わったか。検閲による拒否で起きうる(04-design §8.1) */
  const [emptyAnswer, setEmptyAnswer] = useState(false);
  const answerRef = useRef<HTMLDivElement | null>(null);
  const questionRef = useRef<HTMLTextAreaElement | null>(null);

  // --- M-03: 依頼の文例(設定フォルダのMarkdownが実体) ---
  const [prompts, setPrompts] = useState<PromptTemplate[]>([]);
  const [showAllPrompts, setShowAllPrompts] = useState(false);
  /** 保存する相談の件名を聞く */
  const [saving, setSaving] = useState<string | null>(null);
  /**
   * 送信した時点の材料。
   *
   * 保存時に画面の値を読むと、送信後に依頼欄を直したり資料の選択を変えたりした場合に
   * **実際に送ったものと違う記録が残る**。それでは資産にならないので送信時に固める
   */
  const lastRun = useRef<LastRun | null>(null);

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

    // **送信のたびに組み立て直す**。
    //
    // 以前は preview があればそれを使い回していたが、preview は本文や資料の選択が
    // 変わっても無効化されない。そのため一度でも空の状態(ファイル未選択・codex未一致)で
    // 「送信内容を確認」を押すと、**以後ずっと空のコンテキストを送り続ける**状態になっていた。
    // 記録が「資料なし」になるだけでなく、AIにも設定資料が渡らない
    // (2026-07-31 の指摘で発覚。レビュータブは毎回組み立て直すので同じ問題が無かった)。
    //
    // preview は「送る前に見るためのもの」であって、送る実体を保持する場所ではない。
    let ctx: ContextPreview;
    try {
      ctx = await api.buildContext(body, autoPaths, manualPaths);
      setPreview(ctx);
    } catch (e) {
      setError(String(e));
      return;
    }
    // 会話モードのときだけ、これまでの往復を一緒に送る(既定は単発)
    const history = conversation ? turns : [];
    // 送信した内容をここで固める(以後、画面を触られても記録はずれない)
    lastRun.current = { question, path: currentPath, context: ctx, history };
    setBusy(true);
    onBusy("応答中");
    setAnswer("");
    setError(null);
    setEmptyAnswer(false);
    let received = 0;
    // 積むのは**確定した応答**。setAnswer は非同期なので、ここで別に持つ
    let full = "";
    try {
      await api.askAi(ctx, question, history, (ev) => {
        if (ev.kind === "Delta") {
          received += ev.value.length;
          full += ev.value;
          setAnswer((a) => a + ev.value);
          answerRef.current?.scrollTo(0, answerRef.current.scrollHeight);
        } else if (ev.kind === "Error") {
          setError(ev.value);
        }
      });
      // 応答が1文字も返らないことがある。多くは検閲による拒否(§8.1)。
      // 画面が無反応に見えて原因が分からないので、明示して次の手を示す
      if (received === 0) {
        setEmptyAnswer(true);
      } else if (conversation) {
        // 往復として積み、依頼欄を空ける(次の問いを書く場所にする)。
        // **空の応答は積まない** — 拒否された往復を履歴に入れても意味がない
        setTurns((t) => [...t, { question, answer: full }]);
        setQuestion("");
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      onBusy(null);
    }
  }, [
    question,
    body,
    currentPath,
    autoPaths,
    manualPaths,
    onBusy,
    conversation,
    turns,
  ]);

  /**
   * この相談を `ideas/` へ残す(M-03の「結果保存」)。
   *
   * 会話は原稿ではなく過程の副産物なので**自動保存はしない**。残すと決めたものだけを
   * 正本へ置く(§5.7 A案 / 未決-5と同じ判断)。中身は普通のMarkdownなので、
   * あとから設定として使いたければそのまま codex へ移せる。
   */
  const saveIdea = useCallback(
    async (name: string) => {
      const run = lastRun.current;
      setSaving(null);
      if (!run) return;
      const now = new Date();
      const path = notePath("ideas", name, now);

      // 何について相談したかが分からない記録は、あとから読んでも使えない。
      // **対象文書と渡した資料**を必ず残す(U-05の透明性を保存側にも通す)
      // 実際に渡ったものだけを書く(選んだつもりでも読めなければ渡っていない)。
      // 上限で落ちた分も黙って隠さない — 隠すと「全部渡したうえでの応答」として読める
      const entries = run.context.entries;
      const materialList = [
        ...(entries.length > 0
          ? entries.map(
              (e) => `- ${e.title}(${e.source === "manual" ? "手動" : "自動"}) — ${e.path}`,
            )
          : ["- (なし)"]),
        ...(run.context.dropped_entries > 0
          ? [`- ※ 上限を超えたため ${run.context.dropped_entries}件は渡していません`]
          : []),
      ].join("\n");

      const md = [
        frontmatter({
          title: name,
          created: now.toISOString(),
          model: settings?.model,
          source: run.path ?? undefined,
        }),
        "## 対象",
        "",
        `- 文書: ${run.path ?? "(ファイルを開いていない)"}`,
        `- 渡した本文: ${run.context.body.length}字${
          run.context.body_truncated ? "(長いため末尾を切り捨て)" : ""
        }`,
        "",
        "### 渡した設定資料",
        "",
        materialList,
        "",
        // 会話モードでは**やりとり全体**を残す。最後の1往復だけでは何の話か読めない
        ...run.history.flatMap((t, i) => [
          `## 依頼 ${i + 1}`,
          "",
          t.question,
          "",
          `## 応答 ${i + 1}`,
          "",
          t.answer,
          "",
        ]),
        run.history.length > 0 ? `## 依頼 ${run.history.length + 1}` : "## 依頼",
        "",
        run.question,
        "",
        run.history.length > 0 ? `## 応答 ${run.history.length + 1}` : "## 応答",
        "",
        answer,
        "",
      ].join("\n");

      try {
        const created = await api.createFile(path, md);
        onSaved(created ? path : null, created ? undefined : "同名のファイルが既にあります");
      } catch (e) {
        onSaved(null, String(e));
      }
    },
    [settings, answer, onSaved],
  );

  /** 本文で名前が当たったエントリ(外したものも一覧には残す) */
  const found = codex.filter((c) => mentionedPaths.includes(c.path));
  /** 手動で足せる候補。自動で当たっている分は上の一覧で扱うので出さない */
  const addable = codex.filter((c) => !mentionedPaths.includes(c.path));

  return (
    <div className="ai-panel">
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
        {/* 会話モード(§5.7)。**既定はオフ**で、続けたいときだけ入れる */}
        <div className="row conv-row">
          <label className="check" title="前のやりとりを踏まえて答えさせます">
            <input
              type="checkbox"
              checked={conversation}
              onChange={(e) => {
                setConversation(e.target.checked);
                if (!e.target.checked) setTurns([]);
              }}
              disabled={busy}
            />
            会話を続ける
          </label>
          {conversation && turns.length > 0 && (
            <>
              <span className="conv-count">{turns.length}往復</span>
              <button
                className="mini"
                onClick={() => {
                  setTurns([]);
                  setAnswer("");
                }}
                disabled={busy}
              >
                新しい相談を始める
              </button>
            </>
          )}
        </div>
        {conversation && (
          <p className="hint">
            やりとりは<strong>アプリを閉じると消えます</strong>。
            残すものは応答の「この相談を残す」で <code>ideas/</code> へ書いてください。
            長くなると古い往復から落とします。
          </p>
        )}
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

      {/* これまでの往復。**下(最新)へ向かって読む**ので、応答ブロックの上に置く */}
      {conversation && turns.length > 0 && (
        <div className="block">
          <div className="block-head">
            <h2>これまでのやりとり</h2>
          </div>
          <div className="conv-thread">
            {turns.map((t, i) => (
              <div className="conv-turn" key={i}>
                <p className="conv-q">{t.question}</p>
                <div className="conv-a">{t.answer}</div>
              </div>
            ))}
          </div>
        </div>
      )}

      {(answer || busy) && (
        <div className="block answer-block">
          <div className="block-head">
            <h2>応答</h2>
            {/* チャットで完結させず資産化する(02 M-03)。
                ただし残すかどうかはユーザーが決める */}
            {!busy && answer.trim() && lastRun.current && (
              <button
                className="mini"
                onClick={() =>
                  setSaving(
                    lastRun.current?.question.trim().slice(0, 20) || "相談",
                  )
                }
                title="依頼と応答に加えて、対象文書と渡した設定資料も ideas/ に残します"
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
