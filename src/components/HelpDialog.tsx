/**
 * アプリ内ヘルプ / FAQ(M-08 最小オンボーディング)。
 *
 * ここに書く項目は思いつきで選んでいない。02-requirements.md M-08 と
 * 05-roadmap.md §5.10 に「実装時に漏らしてはいけない項目」として列挙されている、
 * **知らないと結果を誤解する**性質のものである:
 *
 *  1. 題材に応じたモデル選定(検閲の有無)。用途で影響が違うことまで書く
 *  2. 検閲による失敗は**エラーではなく応答0文字**で現れる
 *  3. LLMの動作設定(速度・コンテキスト長・並列数・分割字数)
 *  4. レビューは校正より長い応答になる。必要な設定は本文とモデルで変わる
 *
 * 段階的開示(U-02)のため、節は折りたたみにして必要なものだけ開く。
 */

import { api } from "../api";

type Props = {
  onClose: () => void;
};

export function HelpDialog({ onClose }: Props) {
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal help-modal"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label="ヘルプ"
      >
        <div className="help-head">
          <h2>ヘルプ</h2>
          <button onClick={onClose}>閉じる</button>
        </div>

        <div className="help-body">
          <p className="help-lead">
            このアプリのAIは<strong>編集者</strong>であって執筆者ではありません。
            <strong>本文は書きません。</strong>
            指摘・案・質問・要約を出すだけで、採るかどうかは常にあなたが決めます。
            原稿と設定は普通のMarkdownファイルなので、アプリを使わなくなっても
            データは手元にテキストのまま残ります。
          </p>

          <details open>
            <summary>最初の一歩</summary>
            <ol className="help-list">
              <li>
                <strong>プロジェクトを開く</strong> — 空のフォルダーを選ぶと、
                原稿 / 設定 / プロット などの構成を作ります。
                既にある小説フォルダーを選んでも構いません。
              </li>
              <li>
                <strong>まず触ってみる</strong> — 何も無い状態の画面で
                「サンプルを試す」を選ぶと、短い原稿と設定が入ったプロジェクトを作ります。
                <strong>AIに接続していなくても</strong>、ハイライトと表記ゆれの検出は動きます。
              </li>
              <li>
                <strong>設定(codex)を作る</strong> — 左ペインの「設定」で新規作成。
                ジャンル別のテンプレートが選べます。名前を登録すると、
                本文中のその名前が色づき、AIに渡す資料の候補にもなります。
              </li>
              <li>
                <strong>書く</strong> — 保存は自動です(入力が止まった時・切替時・
                ウィンドウを閉じる時)。上書き前に1世代のバックアップが
                <code>.app/backups/</code> に残ります。
              </li>
            </ol>
          </details>

          <details>
            <summary>AIをつなぐ(ローカルLLMを推奨)</summary>
            <p className="hint">
              推奨は<strong>ローカルLLM</strong>です。理由は費用ではなく、
              <strong>原稿が外部へ出ない</strong>ことと、
              <strong>題材によって拒否されない</strong>ことの2つです。
            </p>
            <p className="hint">
              よく使われるものに <strong>LM Studio</strong> と{" "}
              <strong>Ollama</strong> があります。
              <strong>はじめてなら LM Studio</strong> を勧めます。
              モデルを探して入れるところからサーバーの起動まで画面上で完結し、
              コマンド操作が要りません。Ollama は普段から使っている方向けです
              (こちらもOpenAI互換なので、接続先を合わせれば同じように動きます)。
            </p>

            <h4>LM Studio の手順</h4>
            <ol className="help-list">
              <li>
                <a href="https://lmstudio.ai/" target="_blank" rel="noreferrer">
                  LM Studio
                </a>
                をインストールします。
              </li>
              <li>
                モデルを1つ入手します。日本語を扱えるものを選んでください。
                速度は<strong>30 tok/s 程度</strong>あると実用的です。
              </li>
              <li>
                LM Studio の<strong>サーバー画面</strong>(バージョンにより
                「Developer」「Local Server」などの名前)でモデルを読み込み、
                サーバーを起動します。
              </li>
              <li>
                接続先は既定で <code>http://localhost:1234/v1</code> です。
                このアプリの「AI相談」タブ →「設定」に入れて
                <strong>接続テスト</strong>を押し、モデルを選びます。
              </li>
            </ol>
            <p className="hint">
              クラウド(OpenAI・OpenRouter 等)を使う場合も、OpenAI互換であれば
              接続先URLとモデル名の差し替えだけで動きます。
              <strong>APIキーは保存しません</strong>ので、起動のたびに入力してください。
              クラウドを選んだ場合、<strong>原稿は外部へ送信されます</strong>。
            </p>
          </details>

          <details>
            <summary>⚠ 知らないと結果を誤解すること</summary>

            <h4>題材によってはモデルを選ぶ必要がある</h4>
            <p className="hint">
              暴力・犯罪・性描写を扱う場合、モデルによっては応答が返りません。
              ただし<strong>影響は用途によって違います</strong>(実測):
            </p>
            <ul className="help-list">
              <li>
                <strong>校正・設定抽出・レビュー</strong>(与えられた文章を分析する用途)は、
                <strong>検閲ありのモデルでも通ります</strong>。検閲を理由に避ける必要はありません
              </li>
              <li>
                <strong>展開案などの「書かせる」依頼</strong>は、検閲ありのモデルでは拒否されます。
                ここが非検閲モデルを選ぶ理由になります
              </li>
              <li>モデル差もあります。校正すら拒否したモデルもありました</li>
            </ul>

            <h4>失敗の形が分かりにくい</h4>
            <p className="pf-stale">
              検閲による拒否は<strong>エラーではなく「応答が1文字も返らない」</strong>という形で
              現れます。アプリ側でも検出して伝えますが、
              <strong>無反応に見えたら検閲を疑って</strong>ください。
            </p>

            <h4>設定を誤ると「問題なし」と誤報告されうる</h4>
            <p className="hint">
              モデルの応答が途中で打ち切られると、指摘が0件で返ってきます。
              これを「誤りなし」と表示するのは最悪の誤報告なので、アプリは打ち切りを検出して
              警告を出します。<strong>警告が出たときの結果は信用しないでください。</strong>
              形式を守らない応答が返った場合も同様に警告します。
            </p>

            <h4>うまく動かないときは症状から当たる</h4>
            <table className="help-table">
              <tbody>
                <tr>
                  <td>打ち切りの警告が出る・応答が返らない</td>
                  <td>コンテキスト長を増やす / 1回に送る字数を減らす</td>
                </tr>
                <tr>
                  <td>応答が遅い・時間切れになる</td>
                  <td>並列数を下げる / 軽いモデルに替える</td>
                </tr>
                <tr>
                  <td>指摘が的外れ</td>
                  <td>推論能力の高いモデルに替える(レビューは特に影響が大きい)</td>
                </tr>
              </tbody>
            </table>
            <p className="hint">
              必要な値は<strong>本文の長さとモデルで変わる</strong>ので、決まった推奨値はありません。
              なお LM Studio では、コンテキスト長を増やすと
              <strong>並列数の分だけVRAMを余分に使います</strong>。上げても遅くなる場合は
              並列数を下げてみてください。
            </p>
          </details>

          <details>
            <summary>AIに何が渡っているか</summary>
            <p className="hint">
              AIへ渡すのは<strong>3つだけ</strong>です。開いている本文、
              本文に名前が出た設定、あなたが手動で足した設定。
              それ以外は渡りません。
            </p>
            <ul className="help-list">
              <li>
                渡す資料は実行前に一覧で見えます。
                <strong>要らないものはチェックを外せます</strong>
              </li>
              <li>
                AI相談では「送信内容を確認」で実際の中身を確認できます
              </li>
              <li>
                本文が長い場合は分割して送ります。字数は校正タブで変えられます
              </li>
            </ul>
          </details>

          <details>
            <summary>ファイルはどこにあるか</summary>
            <pre className="help-tree">{`作品フォルダ/
├─ manuscript/   原稿
├─ codex/        設定(人物・場所・物品・用語・メモ)
├─ plot/         プロット
├─ ideas/        AI相談の記録(残すと決めたものだけ)
├─ reviews/      AIレビューの講評(残すと決めたものだけ)
└─ .app/         アプリ専用(バックアップ・ゴミ箱・ログ)`}</pre>
            <p className="hint">
              削除した項目は消えず <code>.app/trash/</code> へ移ります。
              エクスプローラーから元に戻せます。
              <br />
              テンプレートと依頼の文例は設定フォルダ内のMarkdownです。
              自由に書き換えられます。
            </p>
            <div className="help-actions">
              <button
                className="mini"
                onClick={() => void api.openTemplatesDir().catch(() => {})}
              >
                テンプレートのフォルダを開く
              </button>
              <button
                className="mini"
                onClick={() => void api.openPromptsDir().catch(() => {})}
              >
                文例のフォルダを開く
              </button>
            </div>
          </details>

          <details>
            <summary>キーボード操作</summary>
            <table className="help-table">
              <tbody>
                <tr>
                  <td>
                    <kbd>Ctrl</kbd>+<kbd>S</kbd>
                  </td>
                  <td>保存(自動保存もされます)</td>
                </tr>
                <tr>
                  <td>
                    <kbd>Ctrl</kbd>+<kbd>F</kbd>
                  </td>
                  <td>ファイル内の検索</td>
                </tr>
                <tr>
                  <td>
                    <kbd>Ctrl</kbd>+<kbd>\</kbd>
                  </td>
                  <td>右ペインの開閉</td>
                </tr>
                <tr>
                  <td>
                    <kbd>Ctrl</kbd>+クリック
                  </td>
                  <td>本文中の設定名から、その設定を開く</td>
                </tr>
              </tbody>
            </table>
          </details>
        </div>
      </div>
    </div>
  );
}
