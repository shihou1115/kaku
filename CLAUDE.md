# AI小説執筆支援ツール — 開発ガイド

## このプロジェクトは何か

AIを「執筆者」ではなく **「編集者」+「執筆者のためのスマートな外部記憶装置」** として使う小説執筆支援デスクトップアプリ。
AIの役割は ①アイデア出しの補助 ②設定管理 ③誤字脱字チェック ④レビューと改善提案 の4つ。
**AIは本文を書かない**(将来対応=C-01。それまでAI出力を原稿ファイルへ直接挿入する機能は作らない)。

## 文書(docs/)

仕様策定は**完了済み**。以降の更新は「実装で判明した事実の反映」とゲート判定の記録に限る。**紙上での設計追加はしない。**

| 文書 | 役割 |
|---|---|
| [docs/01-survey.md](docs/01-survey.md) | 市場調査(凍結・参考資料) |
| [docs/02-requirements.md](docs/02-requirements.md) | 要求仕様。**何を作るか** |
| [docs/03-data-format.md](docs/03-data-format.md) | データ形式。プロジェクトフォルダ構成とファイル規約 |
| [docs/04-design.md](docs/04-design.md) | 設計。技術選定・uggg流用・AI連携のV1実装範囲・PoC項目 |
| [docs/05-roadmap.md](docs/05-roadmap.md) | **実装順。作業はここを見る**。MVP定義とゲート判定記録 |
| [docs/06-decision-log.md](docs/06-decision-log.md) | 決定記録。**「なぜそれを作らないと決めたか」** |

**実装前に必ず [docs/06-decision-log.md](docs/06-decision-log.md) §4 を読むこと。** 一度採用して撤回した設計(汎用AI実行エンジン、入力ハッシュキャッシュ、importance、コンテキスト優先順位アルゴリズム、指摘の永続再アンカー、要約のバックグラウンド生成、リアルタイムファイル監視 等)が列挙されている。これらは「良い設計に見えるが意図的に外した」ものであり、再提案しない。

## 守る原則

1. **ファイルが正、アプリは従**。ユーザーの小説データは人間が直接編集できるMarkdown+YAMLフロントマター。索引は再生成可能なものだけ
2. **文字オフセットを永続化しない**。位置は表示時に算出する
3. **AIは非破壊**。指摘・提案は必ず採否をユーザーに委ねる。勝手に書き換えない
4. **機械照合が主、LLMは解釈補助**(言及検出・表記ゆれはaho-corasickでやる)
5. **共通化は同じコードが2回以上必要になってから**。先に抽象化の枠を作らない
6. **単純な切り捨てから始めて、必要になってから高度化する**(truncation → selection)

## 技術構成

- Tauri 2 + Rust(ドメインロジックはRust側)
- React 19 + TypeScript + Vite(表示と編集に徹する)
- CodeMirror 6(**PoC#1のゲート通過が前提**。失敗時の対応ラダーは docs/04-design.md §3.2)
- AI接続: OpenAI互換API1本。第一検証環境は LM Studio(`http://localhost:1234/v1`)
- 参考・流用元: `C:\claude\uggg`(OpenAI互換クライアント / keyring / コスト集計)

## コマンド

```bash
npm run tauri dev
```

```bash
npm run build
```

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

## 現在地

- **M0 完了**(2026-07-26): PoC#1合格(8/8)。CodeMirror 6を確定。対応ラダーは発動せず
- **M1 実装完了・ドッグフーディング待ち**: MVP 6要素を実装済み。次は**1万字書いて体験メモを残す**こと。その結果でM2の優先順位を決める

コード構成:

| 場所 | 役割 |
|---|---|
| `src-tauri/src/project.rs` | プロジェクトの読み書き・保存前バックアップ・パス検証 |
| `src-tauri/src/frontmatter.rs` | 寛容パース。**再シリアライズしない**ので未知フィールドは壊れない |
| `src-tauri/src/mentions.rs` | aho-corasickの言及検出。UTF-16位置換算あり |
| `src-tauri/src/ai.rs` | OpenAI互換の薄いクライアント(send/stream/models) |
| `src-tauri/src/context.rs` | AIへ渡す3系統の組み立て+システムプロンプト |
| `src/editor/` | CodeMirror 6本体とハイライト |
| `src/components/` | ツリー・AIパネル |
