/**
 * プロジェクト全体の検索(左ペイン上部)。
 *
 * ファイル内検索(Ctrl+F、CodeMirror側)とは別物である。あちらは
 * 「いま開いている原稿の中を探す」、こちらは**「どのファイルにあるか」を探す**。
 * 混同しないよう、置き場所も見た目も分けてある。
 *
 * 索引の更新は**検索したときにRust側でまとめて行う**(変更されたファイルだけ)。
 * 常駐監視は持たない(06-decision-log §4 で撤回済み)。
 *
 * 3文字未満の語は trigram 索引では引けないので総当たりになる(PoC#2)。
 * 遅くなるわけではないが、**なぜ挙動が違うのか**が分かるように結果へ出す。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type SearchResult } from "../api";

type Props = {
  disabled: boolean;
  /** 結果をクリックしたとき。needle は開いた先で最初の一致へ飛ぶために渡す */
  onOpen: (path: string, needle: string) => void;
};

/** 入力が止まってから検索するまで。打つたびに索引を触らせない */
const DEBOUNCE_MS = 300;

export function ProjectSearch({ disabled, onOpen }: Props) {
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<SearchResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  /** 遅れて返ってきた古い検索で新しい結果を上書きしないための番号 */
  const seq = useRef(0);

  const run = useCallback(async (needle: string) => {
    const mine = ++seq.current;
    if (!needle.trim()) {
      setResult(null);
      setError(null);
      return;
    }
    setBusy(true);
    try {
      const r = await api.searchProject(needle);
      if (seq.current === mine) {
        setResult(r);
        setError(null);
      }
    } catch (e) {
      if (seq.current === mine) setError(String(e));
    } finally {
      if (seq.current === mine) setBusy(false);
    }
  }, []);

  // 入力が止まったら検索する
  useEffect(() => {
    if (disabled) return;
    const t = setTimeout(() => void run(query), DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [query, disabled, run]);

  // Ctrl+Shift+F でここへ来る(Ctrl+F は開いているファイル内の検索)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const clear = useCallback(() => {
    setQuery("");
    setResult(null);
    setError(null);
    inputRef.current?.focus();
  }, []);

  return (
    <div className="ps">
      <div className="ps-box">
        <input
          ref={inputRef}
          className="ps-input"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") clear();
            if (e.key === "Enter" && !e.nativeEvent.isComposing) void run(query);
          }}
          placeholder="プロジェクト内を検索(Ctrl+Shift+F)"
          disabled={disabled}
          spellCheck={false}
        />
        {query && (
          <button className="ps-clear" onClick={clear} title="検索を閉じる">
            ×
          </button>
        )}
      </div>

      {error && <p className="error ps-msg">{error}</p>}

      {result && (
        <div className="ps-results">
          <p className="ps-meta">
            {result.hits.length === 0
              ? "見つかりませんでした"
              : `${result.hits.length}件`}
            {busy && " / 検索中…"}
            {result.method === "like" && (
              <>
                <br />
                3文字未満なので全ファイルを走査しました
              </>
            )}
          </p>
          {result.hits.map((h) => (
            <button
              key={h.path}
              className="ps-hit"
              onClick={() => onOpen(h.path, query.trim())}
              title={h.path}
            >
              <span className="ps-hit-head">
                <span className="ps-hit-title">{h.title}</span>
                {h.count > 1 && <span className="ps-hit-count">{h.count}</span>}
              </span>
              <span className="ps-hit-snippet">{h.snippet}</span>
              <span className="ps-hit-path">{h.path}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
