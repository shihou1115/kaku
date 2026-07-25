# uggg 流用評価メモ v0.1

- 作成日: 2026-07-25
- 対象: `C:\claude\uggg`(Tauri 2製デスクトップコンパニオンアプリ、開発中 v0.3.0)
- 目的: P-2(OpenAI互換API先行・LM Studio利用)の実装にあたり、既存実装の参考・流用範囲を特定する([02-requirements.md](02-requirements.md))。

## 1. ugggの技術構成(確認結果)

| 層 | 構成 |
|---|---|
| シェル | Tauri 2.1(tray-icon)、Windows対応 |
| バックエンド | Rust。rusqlite(bundled SQLite)、keyring 3、reqwest 0.12(rustls / json / **stream有効**)、serde/serde_yaml、tokio、chrono |
| フロントエンド | Vanilla TypeScript + Vite 5(フレームワークなし)、@tauri-apps/api |
| テスト | Rust側ユニットテストあり(cargo test) |

## 2. 流用可能な資産

| ファイル | 内容 | 本アプリでの用途 | 流用度 |
|---|---|---|---|
| `src-tauri/src/dialogue/llm.rs` | OpenAI互換 chat completions クライアント。プロバイダ抽象を持たず base_url 差し替えで OpenAI/LM Studio/Ollama を吸収。ローカルLLMの初回モデルロードを見込んだタイムアウト(全体180秒/接続15秒)。usageトークン取得。モデル別コスト概算(未知・ローカルは$0)。` ```json `フェンス剥がしの寛容JSONパース `extract_json_blob`(テスト付き) | M-07 AI接続基盤、T-01/T-02、T-04のフォールバック側 | ◎ ほぼそのまま |
| `src-tauri/src/system/secrets.rs` | OS資格情報ストア(keyring)によるAPIキーの保存・取得・削除。1プロバイダ1キー | M-07(APIキーを平文で持たない) | ◎ service名変更のみ |
| `src-tauri/src/system/cost.rs` + `db.rs`の`api_usage`テーブル | 使用トークン・概算コストのSQLite記録、当月集計、上限に対する80%/100%閾値判定(テスト付き) | S-08 コスト可視化 | ○ 集計ロジック流用(保存先は本アプリでは`.app/index.sqlite`ではなく専用の利用ログへ) |
| プロジェクト全体 | Tauri 2のコマンド分割(`commands/`)、状態管理、日本語コメントの流儀、リリースプロファイル設定 | プロジェクト雛形 | ○ 参考 |

## 3. 本アプリで追加実装が必要なもの(ugggに無い)

| 項目 | 対応要件 | 備考 |
|---|---|---|
| SSEストリーミング受信 | T-03 | reqwestの`stream` featureは既に有効。消費側の実装のみ追加 |
| `response_format.json_schema`(構造化出力) | T-04 | LM Studio対応確認済み。失敗時は`extract_json_blob`+再試行にフォールバック |
| `/v1/models` によるモデル一覧取得・接続テスト | M-07 | 設定画面のUX(LM Studioで今ロードされているモデルの表示)に必須 |
| リトライ・レート制御 | T-05周辺 | ugggは単発会話用途のため未実装 |
| 機能別プロンプトプロファイル | M-03/04/05 | ブレスト/校正/レビューで温度・システムプロンプトを分ける |
| コスト単価表の外部化 | S-08 | ugggはコード内ハードコード。本アプリは設定ファイル化を検討(単価改定に追従) |

## 4. 設計上の注意(そのまま持ち込まないこと)

1. **SQLiteの役割が逆**: ugggはSQLiteが正データ(会話ログ・設定)。本アプリは**ファイルが正・SQLiteは再生成可能な索引**(P-5 / [03-data-format.md](03-data-format.md) D-1)。db.rsを流用する際は役割を索引・利用ログに限定する。
2. **プロバイダ抽象を持たない方針は維持してよい**: base_url+モデル名で吸収する設計はT-01と一致。Anthropic等のOpenAI非互換APIはOpenRouter等のゲートウェイ経由とする割り切りも引き継ぐ。
3. **フロントエンドは同構成にしない**: ugggはマスコット用途のVanilla TS。本アプリはエディタ中心UIのため、エディタ部品(CodeMirror 6等)とフレームワークの選定が別途必要(Q-2の残課題)。
