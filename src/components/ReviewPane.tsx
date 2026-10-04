/**
 * 右ペインの「レビュー」タブ(M-05)。**AIの4役割で最後の1つ**。
 *
 * 校正(ProofreadPane)と同じ作りにしてある。違うのは次の3点:
 *
 *  1. **観点を選んでから実行する**。観点は5分類で固定(02-requirements.md M-05)
 *  2. 引用付きコメントに加えて**全体講評**が出る
 *  3. **置換ボタンを持たない**。レビューの指摘は文字列の差し替えでは直らないし、
 *     AIは本文を書かない(06-decision-log.md §2-1)。ここでできるのは
 *     「その箇所へ移動する」ことと「採否を記録する」ことだけ
 *
 * 非破壊(U-06): 指摘は消さずに畳む。棄却しても本文には一切触れない。
 */

import { useCallback, useMemo, useState } from "react";
import {
  api,
  REVIEW_ASPECTS,
  type CodexEntry,
  type ReviewAspect,
  type ReviewComment,
} from "../api";
import { useMaterials } from "./useMaterials";
import { PromptDialog } from "./PromptDialog";
import { notePath } from "./saveNote";
import { reviewNote } from "./reviewNote";
import { countLabel } from "./resultLabel";

type Props = {
  /** 現在の本文。結果の鮮度判定に使う */
  body: string;
  /** レビュー対象の文書。講評を残すときに「何を見たか」を書く */
  currentPath: string | null;
  disabled: boolean;
  /** 設定資料の手動追加に使う(コンテキストの3系統目) */
  codex: CodexEntry[];
  /** 本文に名前が出たエントリ(2系統目) */
  mentionedPaths: string[];
  /** 実行中であることをヘッダーへ伝える。終わったら null */
  onBusy: (label: string | null) => void;
  onJump: (from: number, to: number) => void;
  /** 講評を reviews/ へ残したあと。ツリーの読み直しと通知に使う */
  onSaved: (path: string | null, error?: string) => void;
};

/** 指摘ごとの採否。本文には触れず、見え方だけを変える */
type Verdict = "done" | "dropped";

const ALL_ASPECTS = REVIEW_ASPECTS.map((a) => a.key);

export function ReviewPane({
  body,
  currentPath,
  disabled,
  codex,
  mentionedPaths,
  onBusy,
  onJump,
  onSaved,
}: Props) {
  const [aspects, setAspects] = useState<ReviewAspect[]>(ALL_ASPECTS);
  /** 渡す資料の選択。自動で当たった分も**外せる**(U-05 / §6.2) */
  const materials = useMaterials(mentionedPaths);
  const [comments, setComments] = useState<ReviewComment[] | null>(null);
  const [overall, setOverall] = useState("");
  /** 実行時の本文。変わったら「古い結果」として扱う(04-design §6.4) */
  const [reviewedBody, setReviewedBody] = useState<string | null>(null);
  const [verdicts, setVerdicts] = useState<Record<number, Verdict>>({});
  /** 講評として残すときの件名を聞く */
  const [saving, setSaving] = useState<string | null>(null);
  /** レビューした時点の対象。あとで別の文書を開かれても記録がずれない */
  const [reviewedPath, setReviewedPath] = useState<string | null>(null);
  /**
   * レビューした時点の観点と資料。
   *
   * 記録に書くのは**実際に送ったもの**でなければ意味がない。画面の現在値を読むと、
   * 実行後にチップや資料のチェックを触っただけで記録が変わる(AI相談で同じ穴を
   * 2026-07-31 に塞いだ。こちらが取りこぼしになっていた)。
   */
  const [reviewedAspects, setReviewedAspects] = useState<ReviewAspect[]>([]);
  const [reviewedMaterials, setReviewedMaterials] = useState<{
    auto: string[];
    manual: string[];
  }>({ auto: [], manual: [] });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [meta, setMeta] = useState<{
    model: string;
    path: string;
    chunks: number;
    elapsedMs: number;
    tokensPerSec: number | null;
    unchecked: number;
    materials: string[];
    /** 実際に渡した資料のパス(記録に書くのはこちら) */
    materialPaths: string[];
    refused: boolean;
    unparsed: boolean;
    warning: string | null;
  } | null>(null);

  const toggleAspect = useCallback((key: ReviewAspect) => {
    setAspects((prev) =>
      prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key],
    );
  }, []);

  const { autoPaths, manualPaths } = materials;

  const run = useCallback(async () => {
    setBusy(true);
    onBusy("レビュー中");
    setError(null);
    try {
      const r = await api.reviewAi(body, aspects, autoPaths, manualPaths);
      setComments(r.comments);
      setOverall(r.overall);
      setReviewedBody(body);
      setReviewedPath(currentPath);
      // 観点はバックエンドが解決した「実際に見た観点」を使う
      setReviewedAspects(r.aspects);
      setReviewedMaterials({ auto: autoPaths, manual: manualPaths });
      setVerdicts({});
      setMeta({
        model: r.model,
        path: r.path,
        chunks: r.chunks,
        elapsedMs: r.elapsed_ms,
        tokensPerSec: r.tokens_per_sec,
        unchecked: r.unchecked_chars,
        materials: r.materials,
        materialPaths: r.material_paths,
        refused: r.refused,
        unparsed: r.unparsed,
        warning: r.warning,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      onBusy(null);
    }
  }, [body, currentPath, aspects, autoPaths, manualPaths, onBusy]);

  /**
   * 講評を `reviews/` へ残す(03-data-format §4.3「AIの出力も人間可読な資産として残す」)。
   *
   * **自動保存はしない**。全レビューを自動で置くとゴミが溜まる(未決-5)。
   * 採否の印(対応済み/棄却)もそのまま書く。指摘は棄却したものも消さずに残す —
   * 「何を採らなかったか」は後から見返す価値がある記録である。
   */
  const saveReview = useCallback(
    async (name: string) => {
      setSaving(null);
      if (!comments && !overall) return;
      const now = new Date();
      const path = notePath("reviews", name, now);

      // 記録には**実際に渡した資料**を書く(頼んだ資料ではない。上限を超えたもの・
      // 読めなかったものは渡っていない。テスト計画 E4)
      const passed = meta?.materialPaths ?? [];
      const requested = new Set([...reviewedMaterials.auto, ...reviewedMaterials.manual]);
      const md = reviewNote({
        name,
        now,
        model: meta?.model,
        path: reviewedPath,
        aspects: reviewedAspects,
        materials: passed.map((p) => ({
          path: p,
          title: codex.find((x) => x.path === p)?.title ?? p,
          kind: reviewedMaterials.manual.includes(p) ? "手動" : "自動",
        })),
        notPassed: [...requested].filter((p) => !passed.includes(p)).length,
        warning: meta?.warning ?? null,
        unchecked: meta?.unchecked ?? 0,
        overall,
        comments: (comments ?? []).map((c, index) => ({ ...c, verdict: verdicts[index] })),
      });

      try {
        const created = await api.createFile(path, md);
        onSaved(created ? path : null, created ? undefined : "同名のファイルが既にあります");
      } catch (e) {
        onSaved(null, String(e));
      }
    },
    [
      comments,
      overall,
      verdicts,
      meta,
      reviewedPath,
      reviewedAspects,
      reviewedMaterials,
      codex,
      onSaved,
    ],
  );

  const setVerdict = useCallback((index: number, v: Verdict) => {
    setVerdicts((prev) => {
      const next = { ...prev };
      if (next[index] === v) delete next[index];
      else next[index] = v;
      return next;
    });
  }, []);

  const stale = reviewedBody !== null && reviewedBody !== body;

  /** 観点ごとにまとめる。並びは定義順(選択チップと同じ順に見える) */
  const grouped = useMemo(() => {
    if (!comments) return [];
    return REVIEW_ASPECTS.map((a) => ({
      ...a,
      items: comments
        .map((c, index) => ({ c, index }))
        .filter((x) => x.c.aspect === a.key),
    })).filter((g) => g.items.length > 0);
  }, [comments]);

  const droppedCount = Object.values(verdicts).filter(
    (v) => v === "dropped",
  ).length;
  const openCount = comments ? comments.length - droppedCount : 0;

  return (
    <div className="review-pane">
      <section className="pf-section">
        <h3 className="pf-section-title">
          観点<span className="pf-tag ai">AI・トークンを使います</span>
        </h3>
        <div className="rv-aspects">
          {REVIEW_ASPECTS.map((a) => (
            <button
              key={a.key}
              className={aspects.includes(a.key) ? "rv-aspect on" : "rv-aspect"}
              onClick={() => toggleAspect(a.key)}
              disabled={busy}
              title={a.hint}
            >
              {a.label}
            </button>
          ))}
        </div>

        <details className="rv-materials">
          <summary>
            渡す設定資料(自動{autoPaths.length}件
            {materials.excludedCount > 0 && `・${materials.excludedCount}件を外した`}
            {manualPaths.length > 0 && ` + 手動${manualPaths.length}件`})
          </summary>
          <p className="hint">
            本文に名前が出たエントリは自動で入ります。
            <strong>要らないものは外せます</strong>。名前が出ない人物や、
            前の場面から引き継いだ設定は手動で足してください。
            <strong>資料が無いと「設定整合性」は判断できません</strong>。
          </p>
          {codex.length === 0 ? (
            <p className="empty">設定がまだありません。</p>
          ) : (
            <div className="manual-list">
              {codex.map((c) => {
                const isFound = mentionedPaths.includes(c.path);
                return (
                  <label key={c.path} className="check">
                    <input
                      type="checkbox"
                      checked={
                        isFound ? materials.isOn(c.path) : materials.isManual(c.path)
                      }
                      onChange={() =>
                        isFound
                          ? materials.toggleAuto(c.path)
                          : materials.toggleManual(c.path)
                      }
                    />
                    {c.title}
                    {isFound && <span className="rv-auto">自動</span>}
                  </label>
                );
              })}
            </div>
          )}
        </details>

        <div className="pf-head">
          <button
            className="primary"
            onClick={() => void run()}
            disabled={disabled || busy || aspects.length === 0}
          >
            {busy ? "レビュー中…" : "この本文をレビュー"}
          </button>
          {comments && !stale && (
            <span className="pf-count">
              {droppedCount > 0
                ? `${openCount}/${comments.length}件`
                : countLabel(
                    comments.length,
                    {
                      warning: meta?.warning ?? null,
                      unchecked: meta?.unchecked ?? 0,
                    },
                    "指摘なし",
                  )}
            </span>
          )}
          {/* 講評を資産として残す(03 §4.3)。**自動保存はしない**(未決-5) */}
          {!busy && (comments?.length || overall) ? (
            <button
              className="mini"
              onClick={() =>
                setSaving(
                  reviewedPath
                    ? (reviewedPath.split("/").pop() ?? "").replace(/\.md$/, "")
                    : "レビュー",
                )
              }
              title="全体講評と指摘を reviews/ にMarkdownで残します"
            >
              講評に残す
            </button>
          ) : null}
        </div>

        {meta && (
          <p className="pf-meta">
            {meta.model} /{" "}
            {meta.path === "schema" ? "構造化出力" : "寛容パース"}
            {meta.chunks > 1 && ` / ${meta.chunks}分割`}
            {` / ${(meta.elapsedMs / 1000).toFixed(1)}秒`}
            {meta.tokensPerSec !== null &&
              ` (${meta.tokensPerSec.toFixed(0)} tok/s)`}
            {meta.materials.length > 0 && (
              <>
                <br />
                渡した資料: {meta.materials.join("、")}
              </>
            )}
            {meta.unchecked > 0 && (
              <>
                <br />
                本文が長いため末尾{meta.unchecked}字は見ていません。分けて実行してください。
              </>
            )}
          </p>
        )}
        {meta?.warning && <p className="pf-stale">{meta.warning}</p>}
        {stale && (
          <p className="pf-stale">
            本文が変わりました。結果が古い可能性があります。
          </p>
        )}
        {error && <p className="error">{error}</p>}

        {comments === null && !error && (
          <p className="hint pf-hint">
            編集者として読んでもらい、<strong>引用つきの指摘と全体講評</strong>を受け取ります。
            観点は上の5つで固定です(増やしません)。絞るほど速く終わります。
            <br />
            <br />
            <strong>本文は書き換えません</strong>。できるのは指摘箇所へ移動することと、
            採否を記録することだけです。どう直すかは執筆者が決めます。
            <br />
            <br />
            レビューは校正よりモデルの能力を要求します。指摘が的外れなら、
            推論能力の高いモデルに替えてください。検閲の有無はレビューにはあまり効きません
            (拒否されるのは「書かせる」依頼の方です)。
            <br />
            <br />
            応答も校正より長くなります。うまくいかないときは、
            <strong>症状から当たってください</strong>。必要な設定は本文の長さとモデルで
            変わるので、決まった数値はありません。
            <br />
            <br />
            <strong>打ち切りの警告が出る・応答が返らない</strong>なら、
            コンテキスト長を増やすか、1回に送る字数を減らしてみてください。
            <br />
            <strong>応答が遅い・時間切れになる</strong>なら、LM Studioの並列数を下げるか、
            軽いモデルに替えてみてください(コンテキスト長は並列数の分だけVRAMを使います)。
            賢いモデルほど遅いので、応答は240秒まで待ちます。
          </p>
        )}
      </section>

      {overall && (
        <section className="pf-section">
          <h3 className="pf-section-title">
            {meta?.unparsed ? "モデルの生の応答" : "全体講評"}
            {meta?.unparsed && (
              <span className="pf-tag raw">形式を読み取れませんでした</span>
            )}
          </h3>
          {/* 読み取れなかった応答は「講評」として見せない。
              引用の照合を通っていないものを、通ったものと同じ顔で並べないため */}
          <p className={meta?.unparsed ? "rv-overall raw" : "rv-overall"}>
            {overall}
          </p>
        </section>
      )}

      {comments && comments.length > 0 && (
        <section className="pf-section rv-comments">
          <div className="ex-actions">
            {droppedCount > 0 && (
              <>
                <span className="pf-count">{droppedCount}件を棄却中</span>
                {/* 棄却は取り消せること。消していないので戻せる(U-06) */}
                <button className="mini" onClick={() => setVerdicts({})}>
                  棄却を戻す
                </button>
              </>
            )}
            <button
              className="mini danger"
              onClick={() =>
                setVerdicts(
                  Object.fromEntries(
                    comments.map((_, i) => [i, "dropped" as Verdict]),
                  ),
                )
              }
              title="すべての指摘を棄却する(本文には触れません)"
            >
              すべて棄却
            </button>
          </div>

          {grouped.map((g) => (
            <div className="rv-group" key={g.key}>
              <h4 className="rv-group-title">
                {g.label}
                <span className="rv-group-count">{g.items.length}</span>
              </h4>
              {g.items.map(({ c, index }) => {
                const verdict = verdicts[index];
                return (
                  <div
                    className={`pf-item rv-item${verdict ? ` ${verdict}` : ""}`}
                    key={index}
                  >
                    {c.quote && (
                      <blockquote className="rv-quote">{c.quote}</blockquote>
                    )}
                    <p className="rv-comment">{c.comment}</p>
                    {c.suggestion && (
                      <p className="rv-suggestion">→ {c.suggestion}</p>
                    )}
                    <div className="pf-occurrences">
                      {c.found ? (
                        <button
                          className="mini"
                          disabled={stale}
                          onClick={() => onJump(c.start_utf16!, c.end_utf16!)}
                        >
                          移動
                        </button>
                      ) : c.quote ? (
                        <span className="pf-notfound">
                          引用が本文に見つかりません(AIの取り違えの可能性)
                        </span>
                      ) : (
                        <span className="rv-noquote">範囲全体への指摘</span>
                      )}
                      <button
                        className={verdict === "done" ? "mini toggled" : "mini"}
                        onClick={() => setVerdict(index, "done")}
                        title="対応した(記録するだけで本文は変わりません)"
                      >
                        対応済み
                      </button>
                      <button
                        className={
                          verdict === "dropped" ? "mini toggled" : "mini"
                        }
                        onClick={() => setVerdict(index, "dropped")}
                        title="この指摘は採らない"
                      >
                        棄却
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          ))}
        </section>
      )}

      {comments?.length === 0 && !stale && (
        <p className="hint pf-hint">
          {meta?.refused ? (
            <>
              モデルが応答を返しませんでした。
              <strong>「指摘なし」ではありません</strong>。題材によっては検閲で
              拒否されることがあります。非検閲モデルに切り替えてお試しください。
            </>
          ) : meta?.unparsed ? (
            <>
              応答は返りましたが、指定した形式で読み取れませんでした。
              <strong>「指摘なし」ではありません</strong>。上に出しているのは生の応答です。
            </>
          ) : meta?.warning ? (
            <>
              指摘は挙がりませんでしたが、
              <strong>上の警告のとおり見落としの可能性があります</strong>。
            </>
          ) : meta && meta.unchecked > 0 ? (
            <>
              見た範囲では指摘は挙がりませんでした。
              <strong>末尾は見ていない</strong>ので、分けて実行してください。
            </>
          ) : (
            "指摘は挙がりませんでした。観点を絞りすぎていないか、本文が短すぎないか確かめてください。"
          )}
        </p>
      )}

      {saving !== null && (
        <PromptDialog
          title="講評に残す"
          label="件名(ファイル名になります)"
          initial={saving}
          onSubmit={(v) => void saveReview(v)}
          onCancel={() => setSaving(null)}
        />
      )}
    </div>
  );
}
