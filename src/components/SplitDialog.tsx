/**
 * シーンの自動分割(P-8 / 05-roadmap §5.9-D)。
 *
 * **分割は破壊的操作なので提案型にする。** ここでできるのは
 * 「切れ目の候補を見て、採るものを選ぶ」ことだけで、押すまで何も起きない。
 *
 * 見せ方の要点:
 * - 切れ目の**前後を並べて**出す。前の場面の終わりと次の場面の始まりが
 *   並んでいないと、そこで切ってよいか判断できない
 * - 見出しは**その場で直せる**。AIの付けた名前をそのまま使わせない
 * - 元のファイルがどうなるかを**押す前に**書く(ゴミ箱へ移る)
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type SplitPoint } from "../api";

type Props = {
  path: string;
  /** 分割前のファイルの表示名。最初の場面の見出しの初期値になる */
  title: string;
  onBusy: (label: string | null) => void;
  onDone: (created: string[] | null, error?: string) => void;
  onClose: () => void;
};

export function SplitDialog({ path, title, onBusy, onDone, onClose }: Props) {
  const [points, setPoints] = useState<SplitPoint[] | null>(null);
  const [meta, setMeta] = useState<{
    totalChars: number;
    model: string;
    elapsedMs: number;
    warning: string | null;
  } | null>(null);
  /** 採用する切れ目(既定は全部オン。外すほうが例外) */
  const [chosen, setChosen] = useState<Set<number>>(new Set());
  /** 見出しはその場で直せる */
  const [titles, setTitles] = useState<Record<number, string>>({});
  const [firstTitle, setFirstTitle] = useState(title);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /**
   * 親から来るコールバックは**呼ぶだけで依存に入れない**。
   *
   * `onBusy` は App がインラインで渡すので再描画のたびに別の関数になる。
   * これを `suggest` の依存に入れると、
   * 「suggest が onBusy を呼ぶ → App が再描画 → onBusy が別物になる →
   *  suggest が作り直される → useEffect が再発火 → また suggest」
   * という**無限ループ**になり、LM Studio へリクエストを撃ち続ける
   * (2026-08-03 に実際に踏んだ)。ref に逃がして identity を固定する。
   */
  const cb = useRef({ onBusy, onDone });
  cb.current = { onBusy, onDone };
  /** 実行中の多重発火よけ。押しっぱなしでも1本しか投げない */
  const inflight = useRef(false);

  const suggest = useCallback(async () => {
    if (inflight.current) return;
    inflight.current = true;
    setBusy(true);
    cb.current.onBusy("分割の検討中");
    setError(null);
    try {
      const r = await api.suggestSceneSplit(path);
      setPoints(r.points);
      setChosen(new Set(r.points.map((p, i) => (p.found ? i : -1)).filter((i) => i >= 0)));
      setTitles(Object.fromEntries(r.points.map((p, i) => [i, p.title])));
      setMeta({
        totalChars: r.total_chars,
        model: r.model,
        elapsedMs: r.elapsed_ms,
        warning: r.warning,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      inflight.current = false;
      setBusy(false);
      cb.current.onBusy(null);
    }
    // 依存は path だけ。親のコールバックは ref 経由なので identity が揺れない
  }, [path]);

  // 開いたときに1回だけ提案する(path が変われば作り直される)
  useEffect(() => {
    void suggest();
  }, [suggest]);

  const accepted = (points ?? [])
    .map((p, i) => ({ p, i }))
    .filter(({ p, i }) => p.found && chosen.has(i));

  const apply = useCallback(async () => {
    if (inflight.current) return;
    inflight.current = true;
    setBusy(true);
    setError(null);
    try {
      const created = await api.applySceneSplit(
        path,
        firstTitle.trim() || title,
        accepted.map(({ p, i }) => ({
          quote: p.quote,
          title: (titles[i] ?? p.title).trim() || `場面${i + 2}`,
        })),
      );
      inflight.current = false;
      cb.current.onDone(created);
      onClose();
    } catch (e) {
      // 本文が変わっていた等。**ずれた位置で切らずに失敗する**のが正しい
      inflight.current = false;
      setError(String(e));
      setBusy(false);
    }
  }, [path, firstTitle, title, accepted, titles, onClose]);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal split-modal" onClick={(e) => e.stopPropagation()}>
        <div className="help-head">
          <h2>シーンに分割</h2>
          <button onClick={onClose}>閉じる</button>
        </div>

        <div className="split-body">
          <p className="hint">
            {path}
            {meta && ` / ${meta.totalChars.toLocaleString()}字`}
          </p>

          {busy && !points && <p className="hint">切れ目を探しています…</p>}
          {error && <p className="error">{error}</p>}
          {meta?.warning && <p className="pf-stale">{meta.warning}</p>}

          {points && points.length === 0 && !error && (
            <p className="hint pf-hint">
              切れ目は見つかりませんでした。1つの場面として書かれているか、
              モデルが切れ目を読み取れなかった可能性があります。
            </p>
          )}

          {points && points.length > 0 && (
            <>
              <label className="field">
                最初の場面の見出し
                <input
                  value={firstTitle}
                  onChange={(e) => setFirstTitle(e.target.value)}
                />
              </label>

              <div className="split-list">
                {points.map((p, i) =>
                  p.found ? (
                    <div
                      className={chosen.has(i) ? "split-item" : "split-item off"}
                      key={i}
                    >
                      <label className="check">
                        <input
                          type="checkbox"
                          checked={chosen.has(i)}
                          onChange={() =>
                            setChosen((prev) => {
                              const next = new Set(prev);
                              if (next.has(i)) next.delete(i);
                              else next.add(i);
                              return next;
                            })
                          }
                        />
                        ここで切る
                      </label>
                      {/* 前後を並べる。並んでいないと切ってよいか判断できない */}
                      <p className="split-before">…{p.before}</p>
                      <p className="split-cut">
                        <span className="split-scissors">✂</span>
                        <input
                          value={titles[i] ?? p.title}
                          onChange={(e) =>
                            setTitles((t) => ({ ...t, [i]: e.target.value }))
                          }
                          placeholder="場面の見出し"
                          disabled={!chosen.has(i)}
                        />
                        <span className="split-chars">{p.chars}字</span>
                      </p>
                      <p className="split-after">{p.after}…</p>
                    </div>
                  ) : (
                    <div className="split-item missing" key={i}>
                      <span className="pf-notfound">
                        引用が本文に見つかりません(AIの取り違えの可能性):
                        「{p.quote.slice(0, 24)}」
                      </span>
                    </div>
                  ),
                )}
              </div>

              <p className="hint">
                <strong>本文は書き換えません。</strong>切る位置を決めるだけです。
                {accepted.length > 0 && (
                  <>
                    <br />
                    {accepted.length + 1}個のファイルに分かれ、
                    <strong>元のファイルはゴミ箱へ移ります</strong>
                    (<code>.app/trash/</code> から戻せます)。
                  </>
                )}
              </p>
            </>
          )}
        </div>

        <div className="modal-actions">
          <button onClick={() => void suggest()} disabled={busy}>
            もう一度探す
          </button>
          <span className="spacer" />
          <button onClick={onClose}>キャンセル</button>
          <button
            className="primary"
            onClick={() => void apply()}
            disabled={busy || accepted.length === 0}
          >
            {accepted.length + 1}個に分ける
          </button>
        </div>
      </div>
    </div>
  );
}
