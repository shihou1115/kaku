/**
 * 設定のモデル欄に並べる選択肢。
 *
 * **保存してあるモデルが接続先の一覧に無くても、それを選ばれた状態で見せる。**
 * 一覧だけを並べると `<select>` は一覧の先頭を表示するが、実際に送るのは保存してある方で、
 * 画面と実際が食い違っていた(2026-10-04 の実機確認で見つけた)。
 * モデルが未設定のときも同じで、先頭が選ばれているように見えて実際は未設定だった。
 */

export type ModelChoice = {
  value: string;
  label: string;
};

export function modelChoices(models: string[], current: string): ModelChoice[] {
  const listed = models.map((m) => ({ value: m, label: m }));
  if (!current) {
    return [{ value: "", label: "(モデルを選んでください)" }, ...listed];
  }
  if (!models.includes(current)) {
    return [
      { value: current, label: `${current}(接続先の一覧にありません)` },
      ...listed,
    ];
  }
  return listed;
}

/** 保存してあるモデルが接続先の一覧に無いか。一覧を取れていないときは判断しない */
export function modelMissing(models: string[], current: string): boolean {
  return models.length > 0 && current !== "" && !models.includes(current);
}
