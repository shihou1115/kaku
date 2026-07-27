/**
 * 新規ファイル作成ダイアログ(テンプレート選択つき)。
 *
 * テンプレートは設定フォルダ内の普通のMarkdownファイル。
 * 「テンプレートを編集」でフォルダを開けば、ユーザーが自由に書き換え・追加できる
 * (ジャンルを増やすのはフォルダを1つ作るだけ)。
 */

import { useEffect, useMemo, useState } from "react";
import { api, type TemplateInfo } from "../api";
import { folderDisplayPath } from "./folderLabels";

type Props = {
  dirPath: string;
  onCancel: () => void;
  onCreate: (fileName: string, genre: string | null, kind: string | null) => void;
};

const GENRE_KEY = "kaku.lastGenre";

/** codex/characters → character のように、フォルダ名から既定の種別を推測する */
function kindFromDir(dirPath: string): string {
  const seg = dirPath.split("/").pop() ?? "";
  const map: Record<string, string> = {
    characters: "character",
    locations: "location",
    items: "item",
    terms: "term",
    notes: "note",
  };
  if (map[seg]) return map[seg];
  if (dirPath.startsWith("manuscript")) return "scene";
  if (dirPath.startsWith("codex")) return "note";
  return "note";
}

export function NewFileDialog({ dirPath, onCancel, onCreate }: Props) {
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [genre, setGenre] = useState<string>(
    () => localStorage.getItem(GENRE_KEY) ?? "汎用",
  );
  const [name, setName] = useState("");
  const [useTemplate, setUseTemplate] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const defaultKind = useMemo(() => kindFromDir(dirPath), [dirPath]);
  const [kind, setKind] = useState(defaultKind);

  useEffect(() => {
    api
      .listTemplates()
      .then(setTemplates)
      .catch((e) => setError(String(e)));
  }, []);

  const genres = useMemo(
    () => [...new Set(templates.map((t) => t.genre))],
    [templates],
  );
  const kindsInGenre = useMemo(
    () => templates.filter((t) => t.genre === genre).map((t) => t.kind),
    [templates, genre],
  );

  // 選んだジャンルに目的の種別が無ければ汎用へ落とす
  const effectiveGenre = kindsInGenre.includes(kind)
    ? genre
    : templates.some((t) => t.genre === "汎用" && t.kind === kind)
      ? "汎用"
      : null;

  const submit = () => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError("名前を入力してください");
      return;
    }
    localStorage.setItem(GENRE_KEY, genre);
    onCreate(
      trimmed.endsWith(".md") ? trimmed : `${trimmed}.md`,
      useTemplate ? effectiveGenre : null,
      useTemplate ? kind : null,
    );
  };

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>新規作成</h2>
        <p className="hint" title={dirPath}>
          {folderDisplayPath(dirPath)} に作ります
        </p>

        <label className="field">
          名前
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.nativeEvent.isComposing) submit();
              if (e.key === "Escape") onCancel();
            }}
            placeholder={
              dirPath.startsWith("manuscript") ? "01-出会い" : "佐藤架純"
            }
          />
        </label>

        <label className="check">
          <input
            type="checkbox"
            checked={useTemplate}
            onChange={(e) => setUseTemplate(e.target.checked)}
          />
          テンプレートを使う
        </label>

        {useTemplate && (
          <div className="template-row">
            <label className="field">
              ジャンル
              <select value={genre} onChange={(e) => setGenre(e.target.value)}>
                {genres.map((g) => (
                  <option key={g} value={g}>
                    {g}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              種別
              <select value={kind} onChange={(e) => setKind(e.target.value)}>
                {[...new Set(templates.map((t) => t.kind))].map((k) => (
                  <option key={k} value={k}>
                    {k}
                  </option>
                ))}
              </select>
            </label>
          </div>
        )}

        {useTemplate && effectiveGenre === null && (
          <p className="hint">
            この種別のテンプレートがありません。空のファイルを作ります。
          </p>
        )}
        {useTemplate && effectiveGenre !== null && effectiveGenre !== genre && (
          <p className="hint">
            「{genre}」に {kind} が無いので「{effectiveGenre}」を使います。
          </p>
        )}

        {error && <p className="error">{error}</p>}

        <div className="modal-actions">
          <button
            className="link"
            onClick={() =>
              api
                .openTemplatesDir()
                .then((p) => setError(`テンプレートの場所: ${p}`))
                .catch((e) => setError(String(e)))
            }
          >
            テンプレートを編集
          </button>
          <span className="spacer" />
          <button onClick={onCancel}>キャンセル</button>
          <button className="primary" onClick={submit}>
            作成
          </button>
        </div>
      </div>
    </div>
  );
}
