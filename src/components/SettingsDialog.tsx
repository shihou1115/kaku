/**
 * 設定(§5.11 案A: ヘッダーに入口を1つ置いて集約する)。
 *
 * それまでAI設定は「AI相談タブの設定ボタン」に埋まっていた。
 * **AIを使うのは相談だけではない**(校正・レビュー・抽出も使う)のに設定はそこにしかなく、
 * 発見性が低かった。ここへ集約し、AI相談タブは相談だけを持つ。
 *
 * 分割字数は**ここと校正タブの両方**にある。校正の結果を見ながら下げたくなる値なので、
 * 一元化のために取り上げると使いにくくなる(§5.11 論点1が許容した二重配置)。
 * 実体は同じ `AiSettings` なので、どちらで変えても同じ場所に効く。
 *
 * **APIキーはここでも保存しない**(M-07)。メモリ上にだけ置き、起動のたびに入力する。
 */

import { useEffect } from "react";
import { api, type AiSettings } from "../api";

/** 校正で1回に送る字数の候補。上限はコンテキスト長との兼ね合いで決まる(§7.1) */
const CHUNK_CHOICES = [1000, 2000, 3000, 4000, 6000, 8000, 12000];

type Props = {
  settings: AiSettings | null;
  onPatch: (p: Partial<AiSettings>) => void;
  models: string[];
  connDetail: string;
  onCheckConnection: () => Promise<string[]>;
  /** 本文中のcodex名を強調するか */
  highlightEnabled: boolean;
  onHighlightChange: (v: boolean) => void;
  onClose: () => void;
};

export function SettingsDialog({
  settings,
  onPatch,
  models,
  connDetail,
  onCheckConnection,
  highlightEnabled,
  onHighlightChange,
  onClose,
}: Props) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  /** 接続テスト。モデル未選択なら先頭を入れておく(P-8: 選ばせる手間を減らす) */
  const connect = async () => {
    const list = await onCheckConnection();
    if (list.length > 0 && settings && !settings.model) {
      onPatch({ model: list[0] });
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal settings-modal"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label="設定"
      >
        <div className="help-head">
          <h2>設定</h2>
          <button onClick={onClose}>閉じる</button>
        </div>

        <div className="help-body">
          <section className="set-section">
            <h3>AI接続</h3>
            <p className="hint">
              OpenAI互換API(<code>/v1/chat/completions</code>)を1本だけ使います。
              ここで設定した接続先を<strong>相談・校正・レビュー・抽出のすべてで使います</strong>。
            </p>
            {settings ? (
              <div className="settings">
                <label>
                  接続先
                  <input
                    value={settings.base_url}
                    onChange={(e) => onPatch({ base_url: e.target.value })}
                    spellCheck={false}
                  />
                </label>
                <label>
                  APIキー(ローカルLLMでは不要)
                  <input
                    type="password"
                    value={settings.api_key ?? ""}
                    onChange={(e) => onPatch({ api_key: e.target.value || null })}
                  />
                </label>
                <p className="hint">
                  <strong>APIキーは保存しません。</strong>起動のたびに入力してください。
                </p>
                <div className="row">
                  <button onClick={() => void connect()}>接続テスト</button>
                  <span className="conn">{connDetail || "未確認"}</span>
                </div>
                <label>
                  モデル
                  {models.length > 0 ? (
                    <select
                      value={settings.model}
                      onChange={(e) => onPatch({ model: e.target.value })}
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
                      onChange={(e) => onPatch({ model: e.target.value })}
                      placeholder="接続テストで一覧を取得"
                      spellCheck={false}
                    />
                  )}
                </label>
              </div>
            ) : (
              <p className="empty">設定を読み込めませんでした。</p>
            )}
          </section>

          <section className="set-section">
            <h3>校正・抽出</h3>
            {settings && (
              <div className="settings">
                <label>
                  1回に送る字数
                  <select
                    value={settings.check_chunk_chars}
                    onChange={(e) =>
                      onPatch({ check_chunk_chars: Number(e.target.value) })
                    }
                  >
                    {CHUNK_CHOICES.map((n) => (
                      <option key={n} value={n}>
                        {n.toLocaleString()}字
                      </option>
                    ))}
                  </select>
                </label>
              </div>
            )}
            <p className="hint">
              長い本文はこの字数で分割して検査します。
              <strong>応答が打ち切られるなら、モデルのコンテキスト長を増やすか、この字数を下げてください。</strong>
              校正タブからも同じ値を変えられます。
            </p>
          </section>

          <section className="set-section">
            <h3>本文の表示</h3>
            <label className="check">
              <input
                type="checkbox"
                checked={highlightEnabled}
                onChange={(e) => onHighlightChange(e.target.checked)}
              />
              設定名を強調する
            </label>
            <p className="hint">
              本文中に出てくるcodexの名前・別名に色を付けます。Ctrl+クリックで中身を開けます。
              行番号・ルーラー・折り返し字数・配色はヘッダーの「表示」にあります
              (書きながら変えるものなので、そちらに置いています)。
            </p>
          </section>

          <section className="set-section">
            <h3>雛形と文例</h3>
            <p className="hint">
              codexの作成テンプレートと、AI相談の依頼の文例は
              <strong>設定フォルダの普通のMarkdown</strong>です。書き換えれば、そのまま反映されます。
            </p>
            <div className="row">
              <button onClick={() => void api.openTemplatesDir().catch(() => {})}>
                テンプレートを開く
              </button>
              <button onClick={() => void api.openPromptsDir().catch(() => {})}>
                文例を開く
              </button>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
