/**
 * 右ペインの「設定抽出」タブ(M-09)。
 *
 * 本文に出てくる固有名詞をcodexへの追加候補として並べる。
 * **提案するだけで勝手に登録しない**(U-06)。ユーザーが選んだものだけを作る。
 *
 * 「書けば書くほど外部記憶が育つ」導線であり、U-01(ゼロ設定で書き始める)と
 * 設定DB前提の機能(校正・レビュー)を橋渡しする唯一の経路
 * (docs/06-decision-log.md §2-8)。
 *
 * V1は手動実行のみ。保存のたびに走らせると候補ノイズが執筆を阻害するため。
 */

import { useCallback, useState } from "react";
import { api, type Candidate } from "../api";

type Props = {
  body: string;
  disabled: boolean;
  /** 実行中であることをヘッダーへ伝える。終わったら null */
  onBusy: (label: string | null) => void;
  /** 登録後にツリーとcodexを読み直す */
  onCreated: (paths: string[]) => void;
};

const KIND_LABEL: Record<string, string> = {
  character: "人物",
  location: "場所",
  item: "物品",
  term: "用語",
};

export function ExtractPane({ body, disabled, onBusy, onCreated }: Props) {
  const [candidates, setCandidates] = useState<Candidate[] | null>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [meta, setMeta] = useState<{
    rejected: number;
    chunks: number;
    elapsedMs: number;
    warning: string | null;
  } | null>(null);

  const run = useCallback(async () => {
    setBusy(true);
    onBusy("抽出中");
    setError(null);
    try {
      const r = await api.extractEntities(body);
      setCandidates(r.candidates);
      // 既定では何も選ばない。採否はユーザーが決める
      setChosen(new Set());
      setMeta({
        rejected: r.rejected,
        chunks: r.chunks,
        elapsedMs: r.elapsed_ms,
        warning: r.warning,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      onBusy(null);
    }
  }, [body, onBusy]);

  const toggle = useCallback((name: string) => {
    setChosen((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const register = useCallback(async () => {
    if (!candidates) return;
    const picked = candidates.filter((c) => chosen.has(c.name));
    if (picked.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      const created = await api.createCodexEntries(picked);
      // 登録したものは一覧から消す
      setCandidates((prev) => prev?.filter((c) => !chosen.has(c.name)) ?? null);
      setChosen(new Set());
      onCreated(created);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [candidates, chosen, onCreated]);

  return (
    <div className="extract-pane">
      <div className="pf-head">
        <button
          className="primary"
          onClick={() => void run()}
          disabled={disabled || busy}
        >
          {busy ? "処理中…" : "この本文から抽出"}
        </button>
        {candidates && (
          <span className="pf-count">
            {candidates.length === 0 ? "候補なし" : `${candidates.length}件`}
          </span>
        )}
      </div>

      {meta && (
        <p className="pf-meta">
          {(meta.elapsedMs / 1000).toFixed(1)}秒
          {meta.chunks > 1 && ` / ${meta.chunks}分割`}
          {meta.rejected > 0 && ` / ${meta.rejected}件は機械側で除外`}
        </p>
      )}
      {meta?.warning && <p className="pf-stale">{meta.warning}</p>}
      {error && <p className="error">{error}</p>}

      {candidates === null && !error && (
        <p className="hint pf-hint">
          本文に出てくる人物・場所・用語を探して、設定への追加候補として並べます。
          <strong>登録は選んだものだけ</strong>で、勝手には作りません。
          <br />
          <br />
          一般名詞や、本文に実在しない語(AIの取り違え)、すでに登録済みの名前は
          機械側で除いてから並べます。
        </p>
      )}

      {candidates?.length === 0 && (
        <p className="hint pf-hint">
          新しい固有名詞は見つかりませんでした。
        </p>
      )}

      {candidates && candidates.length > 0 && (
        <>
          <div className="ex-actions">
            <button
              className="mini"
              onClick={() => setChosen(new Set(candidates.map((c) => c.name)))}
            >
              すべて選ぶ
            </button>
            <button className="mini" onClick={() => setChosen(new Set())}>
              選択を解除
            </button>
            <button
              className="mini danger"
              onClick={() => {
                setCandidates(null);
                setChosen(new Set());
                setMeta(null);
              }}
              title="候補をすべて捨てる"
            >
              一括棄却
            </button>
          </div>

          <div className="ex-list">
            {candidates.map((c) => (
              <label className="ex-item" key={c.name}>
                <input
                  type="checkbox"
                  checked={chosen.has(c.name)}
                  onChange={() => toggle(c.name)}
                />
                <span className="ex-body">
                  <span className="ex-title">
                    <span className={`ex-kind ${c.kind}`}>
                      {KIND_LABEL[c.kind] ?? c.kind}
                    </span>
                    <strong>{c.name}</strong>
                    <span className="ex-count">{c.count}回</span>
                  </span>
                  {c.description && (
                    <span className="ex-desc">{c.description}</span>
                  )}
                </span>
              </label>
            ))}
          </div>

          <button
            className="primary"
            onClick={() => void register()}
            disabled={busy || chosen.size === 0}
          >
            選んだ{chosen.size}件を設定に追加
          </button>
        </>
      )}
    </div>
  );
}
