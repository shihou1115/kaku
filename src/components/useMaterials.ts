/**
 * AIへ渡す設定資料の選択(U-05 / docs/04-design.md §6.2)。
 *
 * §6.2 はこう要求している:
 *   「実行前に『何を渡すか』の一覧を表示し、**その場で外せること**」
 *
 * 当初の実装は手動追加分しか外せず、**本文で名前が当たった分を外せなかった**。
 * 自動で入るのが便利かどうかは相談内容によるため、既定ONのまま外せる形にする。
 *
 * ここで共通化するのは**状態のロジックだけ**。見た目はAI相談(常時表示)と
 * レビュー(details内)で求められる形が違うので、各ペインが持つ。
 *
 * ## 「選択リスト」ではなく「除外リスト」を持つ理由
 *
 * 既定ONを「選択済みリスト」で表すと、本文が変わって当たるエントリが増減するたびに
 * リストを追随させる同期処理が要る。外した直後に本文を1文字打って復活する、といった
 * 事故が起きやすい。**除外リストなら「入っていなければON」**で済み、新しく当たった
 * エントリは自動でONになる(既定ONの意味が保たれる)。
 *
 * 除外は**セッション中ずっと保つ**(ファイル切替でリセットしない)。パスで持つので、
 * 当たらなくなったエントリの除外は無害である。
 */

import { useCallback, useMemo, useState } from "react";

export type Materials = {
  /** 本文で名前が当たったエントリ(外したものも含む。一覧の表示用) */
  found: string[];
  /** 実際に渡す「自動」分 */
  autoPaths: string[];
  /** 実際に渡す「手動追加」分。自動と重ならない */
  manualPaths: string[];
  isOn: (path: string) => boolean;
  toggleAuto: (path: string) => void;
  /** 一括で外す / 戻す */
  setAllAuto: (on: boolean) => void;
  isManual: (path: string) => boolean;
  toggleManual: (path: string) => void;
  /** 外している自動エントリの数 */
  excludedCount: number;
};

/**
 * 実際に渡すパスを決める(純粋関数。ここだけテストできるようにしてある)。
 *
 * 要点は**自動と手動を重ねない**こと。重なりを残すと
 * 「自動側で外したのに手動側で入っているので結局渡る」という矛盾が起きる。
 * **自動で当たっている間は自動側の判断が優先**とし、名前が本文から消えたら
 * 手動追加として戻ってくる。
 */
export function resolveMaterials(
  mentionedPaths: string[],
  excluded: string[],
  manual: string[],
): { autoPaths: string[]; manualPaths: string[] } {
  return {
    autoPaths: mentionedPaths.filter((p) => !excluded.includes(p)),
    manualPaths: manual.filter((p) => !mentionedPaths.includes(p)),
  };
}

export function useMaterials(mentionedPaths: string[]): Materials {
  /** ユーザーが外した自動エントリ。**選択ではなく除外を持つ**(上記の理由) */
  const [excluded, setExcluded] = useState<string[]>([]);
  const [manual, setManual] = useState<string[]>([]);

  const { autoPaths, manualPaths } = useMemo(
    () => resolveMaterials(mentionedPaths, excluded, manual),
    [mentionedPaths, excluded, manual],
  );

  const toggleAuto = useCallback((path: string) => {
    setExcluded((prev) =>
      prev.includes(path) ? prev.filter((p) => p !== path) : [...prev, path],
    );
  }, []);

  const setAllAuto = useCallback(
    (on: boolean) => setExcluded(on ? [] : [...mentionedPaths]),
    [mentionedPaths],
  );

  const toggleManual = useCallback((path: string) => {
    setManual((prev) =>
      prev.includes(path) ? prev.filter((p) => p !== path) : [...prev, path],
    );
  }, []);

  return {
    found: mentionedPaths,
    autoPaths,
    manualPaths,
    isOn: (path) => !excluded.includes(path),
    toggleAuto,
    setAllAuto,
    isManual: (path) => manual.includes(path),
    toggleManual,
    excludedCount: mentionedPaths.filter((p) => excluded.includes(p)).length,
  };
}
