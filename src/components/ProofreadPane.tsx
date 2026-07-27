/**
 * 右ペインの「校正」タブ(M-04-02 設定DB連携の固有名詞チェック)。
 *
 * ここはLLMを使わない。codexの正式名・別名との機械照合だけで表記ゆれを出す
 * (docs/06-decision-log.md §2-5 機械照合が主、LLMは解釈補助)。
 *
 * 非破壊(M-04-03): 勝手に本文を書き換えない。指摘を並べ、
 * ユーザーが1件ずつ「移動」して確認し、必要なら「置換」する。
 *
 * 置換すると後続の出現位置がずれるため、置換後は**自動で再検査**する。
 * 位置の差分を自前で管理するとずれの温床になるうえ、この検査は
 * ローカルの機械照合で即座に終わるので、素直に測り直す方が確実。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type NotationHit } from "../api";

type Props = {
  /** 現在の本文。結果の鮮度判定に使う */
  body: string;
  disabled: boolean;
  onJump: (from: number, to: number) => void;
  onReplace: (from: number, to: number, text: string) => void;
};

export function ProofreadPane({ body, disabled, onJump, onReplace }: Props) {
  const [hits, setHits] = useState<NotationHit[] | null>(null);
  /** 検査した時点の本文。変わったら結果は「古い」 */
  const [checkedBody, setCheckedBody] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** 置換直後に本文が届いたら自動で測り直すための印 */
  const rerun = useRef(false);

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

  // 置換で本文が変わったら測り直す(位置ずれを持ち越さない)
  useEffect(() => {
    if (!rerun.current) return;
    if (checkedBody === body) return;
    rerun.current = false;
    void run(body);
  }, [body, checkedBody, run]);

  const replaceAt = useCallback(
    (from: number, to: number, text: string) => {
      rerun.current = true;
      onReplace(from, to, text);
    },
    [onReplace],
  );

  const replaceAll = useCallback(
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

  const stale = checkedBody !== null && checkedBody !== body;

  return (
    <div className="proofread-pane">
      <div className="pf-head">
        <button
          className="primary"
          onClick={() => void run(body)}
          disabled={disabled || busy}
        >
          {busy ? "確認中…" : "表記ゆれを確認"}
        </button>
        {hits && !stale && (
          <span className="pf-count">
            {hits.length === 0 ? "指摘なし" : `${hits.length}件`}
          </span>
        )}
      </div>

      {stale && (
        <p className="pf-stale">
          本文が変わったため、この結果は古くなっています。もう一度確認してください。
        </p>
      )}

      {error && <p className="error">{error}</p>}

      {hits === null && !error && (
        <p className="hint pf-hint">
          設定(codex)に登録された名前と<strong>1文字違い</strong>の語を探します。
          「架純」に対する「佳純」のような誤変換を、設定を知らない一般の校正ツールより
          確実に見つけられます。
          <br />
          <br />
          ひらがなだけの語は誤検出が多いため対象外です。
        </p>
      )}

      {hits?.length === 0 && !stale && (
        <p className="hint pf-hint">登録名と紛らわしい語は見つかりませんでした。</p>
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
                        replaceAt(o.start_utf16, o.end_utf16, h.suggestion)
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
                    onClick={() => replaceAll(h)}
                    title={`${h.occurrences.length}箇所すべてを「${h.suggestion}」に置き換える`}
                  >
                    すべて置換
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
