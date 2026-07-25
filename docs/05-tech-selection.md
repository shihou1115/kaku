# 技術選定書(Q-2) v0.4

- 作成日: 2026-07-25 / 最終更新: 2026-07-25(**v0.4 第3回外部レビュー反映: 装飾要件の緩和、対応ラダーに「校正モードのread-only化」を追加**、[10-review-disposition-3.md](10-review-disposition-3.md))
- v0.3: 第2回外部レビュー反映(代替戦略を「装飾の外部化」優先へ変更、[09-review-disposition-2.md](09-review-disposition-2.md))
- v0.2: 外部レビュー反映([07-review-disposition.md](07-review-disposition.md) #3/#18)
- 状態: ドラフト(レビュー待ち)
- 前提: Q-1決定(Tauri製デスクトップアプリ、Windows先行)、[03-data-format.md](03-data-format.md)(ファイル正本+SQLite索引)、[04-uggg-reuse-notes.md](04-uggg-reuse-notes.md)(AI接続はuggg流用)
- 目的: フロントエンドフレームワーク、エディタ部品、Rust側ライブラリを選定しQ-2をクローズする。

## 1. 判断軸

| 軸 | 内容 |
|---|---|
| A. 日本語処理 | 日本語IME(MS-IME/Google日本語入力)での長文入力の安定性が最優先。縦書きはプレビューのみ(エディタ内縦書きは要求しない) |
| B. 要件適合 | M-04校正指摘・M-05レビューコメント・M-02言及検出を本文上にオーバーレイ表示する**装飾(デコレーション)レイヤ**が必須 |
| C. 長文性能 | シーン単位ファイル(通常数千〜1万字)が編集単位のため極端な長文は稀だが、数万字でも劣化しないこと(N-01) |
| D. 開発体制との相性 | 開発はClaude Code等のAI支援を前提とするため、情報量・事例の多さ(=AIの精度)とエコシステムの部品充実を重視 |
| E. 保守性 | メンテナンスが活発で、ライセンスが緩い(MIT/Apache)こと |

## 2. 全体アーキテクチャ

```
┌─ WebView2 (フロントエンド: React + TypeScript) ─────────┐
│  執筆画面(CodeMirror 6) / codexブラウザ / レビューパネル │
│  状態管理: Zustand / UI: Tailwind CSS + shadcn/ui       │
└────────────┬────────────────────────────────────────────┘
       Tauri IPC(コマンド+イベント/チャンネル: SSE中継含む)
┌────────────┴────────────────────────────────────────────┐
│  Rustコア                                                │
│  ・file store(正本の読み書き、フロントマター解析)         │
│  ・indexer(SQLite FTS5全文検索、言及検出 aho-corasick)   │
│  ・watcher(notify: 外部編集の監視→再インデックス T-08)   │
│  ・snapshot(M-06 世代スナップショット+差分 similar)      │
│  ・llm client(uggg流用+SSE/json_schema追加)              │
│  ・secrets(keyring: APIキー)                             │
└──────┬──────────────────────────┬───────────────────────┘
       ▼                          ▼
  プロジェクトフォルダ        LM Studio (localhost:1234/v1)
  (Markdown+YAML=P-5)         ※クラウドはbase URL差し替え
```

方針: **ドメインロジック(解析・索引・検出・版管理)はRust側に置き、フロントは表示と編集に徹する**。ugggのコマンド分割スタイルを踏襲する。

## 3. エディタ部品の選定

### 3.1 比較

| 候補 | 型 | 装飾レイヤ(軸B) | 日本語IME(軸A) | 長文(軸C) | 評価 |
|---|---|---|---|---|---|
| **CodeMirror 6** | プレーンテキスト/コードエディタ | ◎ Decoration APIが強力(下線・ウィジェット・ガター)。lint/検索UIも標準拡張 | ○ v6.5(2023)でIME処理改善。ただし日本語IMEの不具合報告が現在も散発(iOS行頭、カーソル位置等)→**PoC必須** | ◎ 仮想描画で大文書に強い | **本命** |
| ProseMirror | 構造化リッチテキスト | ○ Decoration相当あり。スキーマ設計が必要 | ◎ CJK/Androidで最も堅牢との評価(実戦済みのMutationObserver処理) | ○ 長期安定性に定評 | **IME問題発生時の代替** |
| Lexical (Meta) | リッチテキスト | ○ | △ CJK系の不具合履歴。負荷継続時に性能劣化の測定報告 | △ | 見送り |
| Monaco | コードエディタ | ○ | △ | ◎ | 重量級・小説用途に過剰。見送り |
| 素のtextarea | ネイティブ | × 装飾不可(要件を満たせない) | ◎ ネイティブIME | ○ | 要件Bで脱落 |

### 3.2 決定

- **CodeMirror 6 を第一候補として採用**。原稿(プレーンテキスト+装飾)にも、codexのMarkdown編集にも同一部品を使う
- 採用理由: 要件B(校正・レビュー・言及のオーバーレイ表示)への適合が最良。MIT、活発、フレームワーク非依存
- **条件: PoC#1(§7-1)を実装フェーズの最初(Week 1)に実施する**([08-roadmap.md](08-roadmap.md) M0)。長文入力・変換確定・装飾共存・undo履歴を確認する
- **装飾要件の緩和(v0.4)**: MVPで必要な装飾は**codex名の単純ハイライトのみ**。校正・レビューの指摘は本文へ装飾せずサイドパネルに引用+ジャンプボタンで示せば成立する。これによりIMEリスクとアンカー処理の複雑性が大きく下がる(外部レビューR1-9)
- **失敗時の対応ラダー(v0.4改訂)**: ①IME変換中のみ装飾を一時非表示にする → ②**校正・レビューモードではエディタを読み取り専用にして装飾を載せ、執筆中は装飾を出さない**(最も実装コストが低い逃げ道) → ③装飾をエディタ外(サイドパネル)へ完全に逃がす(※本文行とのスクロール同期はインライン装飾より面倒なレイアウト計算になるため②より後ろに置く) → ④ProseMirror/Tiptapへの切替は**最終手段**。PM/Tiptapはドキュメントツリー型のため「Markdown⇔内部モデル」の常時相互変換が必要になり、03のプレーンテキスト正本と不整合を起こす。**開発中盤での切替は致命的な手戻りになるため、判断は必ずWeek 1のPoCで行う**(外部レビューR1-3/R2-摩擦A)
- 留意: 三点リーダ・ダッシュの入力補助、字下げ、将来のルビ表示などの小説記法まわりはCM6拡張の自作前提であり、相応の実装コストを見込む(v0.2)
- 縦書き(S-09)はエディタでは行わず、**プレビューペインにCSS `writing-mode: vertical-rl` で表示**する(横書きで書き、縦書きで確認する分業)

## 4. フロントエンドフレームワークの選定

### 4.1 比較

| 候補 | 軸D(AI支援・事例) | エコシステム(ツリー/DnD/パネル等) | 性能・サイズ | 評価 |
|---|---|---|---|---|
| **React 19 + TypeScript** | ◎ 情報量最大。Claude支援の精度が最も出る | ◎ dnd-kit(シーン並べ替え)、react-resizable-panels(3ペイン)、react-arborist(ツリー)、shadcn/ui(UI部品)が全て揃う | ○ 十分(エディタ本体はCM6が担う) | **採用** |
| Svelte 5 | ○ 情報量は増加中だがReactに劣る | △ 部品を自作する場面が増える | ◎ 軽量 | 次点 |
| Vue 3 | ○ | ○ | ○ | 積極採用理由なし |
| Vanilla TS(uggg方式) | △ 規模に対して素手 | × 全自作 | ◎ | マスコット規模向け。本アプリでは見送り |

### 4.2 決定

- **React 19 + TypeScript + Vite** を採用
- 決め手は軸D: 本アプリは3ペインの執筆UI・ツリー・パネル群などUI部品が多く、部品の既製品が揃い、AI支援開発で最も事例が厚い構成が総開発コスト最小と判断
- 付随選定:
  - 状態管理: **Zustand**(小さく、ボイラープレート最小)
  - スタイル/UI: **Tailwind CSS + shadcn/ui**(P-4のUI/UX品質を短期間で確保。アクセシブルなRadix基盤)
  - アイコン: lucide-react
  - Markdownプレビュー: remark系(react-markdown)+wiki-link対応プラグイン(必要時)
  - ドラッグ&ドロップ: dnd-kit / 分割ペイン: react-resizable-panels
- 注: 開発者に別フレームワークの強い習熟がある場合はSvelte 5への差し替え余地あり(この場合もCM6・Rust側選定は不変)

## 5. Rust側ライブラリの選定

| 用途 | 採用 | 備考 |
|---|---|---|
| YAML(フロントマター) | **serde-saphyr**(候補1)/ yaml-rust2(候補2) | **注意: ugggが使うserde_yamlは2024年にアーカイブ済み・非推奨**。fork版serde_ymlは健全性問題(RUSTSEC-2025-0068)で非推奨。新規採用は保守されている代替から選ぶ(§7-5でAPI確認) |
| フロントマター分離 | 自前実装(`---`区切りの先頭ブロック切り出し) | 依存を増やすほどの処理ではない。寛容パース(T-08)を自前で制御 |
| Markdown解析 | pulldown-cmark | 高速・軽量。見出し抽出(T-05の部分供給)・リンク抽出に使用。拡張記法が必要になればcomrak再検討 |
| ファイル監視 | notify + notify-debouncer-full | T-08。デバウンス必須(エディタ保存やクラウド同期の連続イベント対策) |
| 言及検出 | **aho-corasick** | M-02/M-04。codex全エントリの名前+別名を一括マルチパターン照合。数百パターン×数十万字でも高速 |
| 全文検索 | rusqlite(bundled)+ **FTS5 trigram** | 日本語の部分一致に対応(SQLite 3.34+)。**制約: 2文字以下の語はtrigram索引不可→LIKEフォールバックで補完**。将来の精度向上はlindera-sqlite(形態素解析トークナイザ)を検討 |
| 差分・スナップショット | similar | M-06世代スナップショットの差分表示 |
| LLM接続 | uggg `llm.rs` 流用 + SSE追加(eventsource-stream等)+ `response_format.json_schema` 対応 | [04-uggg-reuse-notes.md](04-uggg-reuse-notes.md) §3 |
| APIキー | keyring 3(uggg流用) | service名のみ変更 |
| ID生成 | ulid | 時系列ソート可能なID。フロントマターの`id`に使用(接頭辞`chr-`等は種別で付与) |

## 6. 開発環境・その他

| 項目 | 決定 |
|---|---|
| 雛形 | create-tauri-app(react-tsテンプレート)から生成し、ugggのプロジェクト構成(commands分割・テスト流儀)を移植 |
| パッケージ管理 | npm(uggg踏襲。差し替え自由) |
| テスト | Rust: cargo test(ドメインロジックはRust側に集約するためここを厚くする)/ フロント: Vitest(ロジックのみ。UIはドッグフーディングで補完) |
| 対象OS | Windows 10/11(WebView2)。コードはクロスプラットフォームを壊さない範囲で |
| リポジトリ | 本プロジェクトをgit管理下に置く(現状未初期化)。docs/も含めてコミット |

## 7. 実装前PoC項目(リスクの先出し)

| # | 検証内容 | 判定基準 | 失敗時の代替 |
|---|---|---|---|
| 1 | **CM6 × Windows日本語IME**: MS-IME/Google日本語入力で数千字入力、変換・確定・再変換、undo。**判定はMVPに必要な範囲で行う(v0.4): ①素の状態での日本語長文入力が安定すること(必須) ②codex名ハイライト(単純装飾)が載った文字列上でのIME変換が崩れないこと(必須)**。AI指摘の装飾との共存は§3.2の緩和策で回避できるため合格条件に含めない | ①②が実用上問題ないこと | §3.2の対応ラダー(①変換中の装飾非表示→②校正モードのread-only化→③サイドパネルへ externalize→④最終手段としてPM/Tiptap) |
| 2 | **FTS5 trigram日本語検索**: rusqlite(bundled)でFTS5が有効か、trigram+LIKEフォールバックの検索品質 | キャラ名・2文字語・かな交じり検索が期待通り | lindera-sqlite導入前倒し |
| 3 | **LM Studio `json_schema`**: 校正・レビュー想定のスキーマで構造化出力が安定するか(モデル2〜3種)。**スキーマ強制による日本語の不自然化・生成速度低下の有無も計測**(v0.2、外部レビューR2-1の検証) | パース成功率が実用水準、かつ日本語品質・速度の劣化が許容範囲 | プレーンテキスト+規約プロンプト+寛容パースの軽量経路へ切替([06-ai-pipeline.md](06-ai-pipeline.md) §3 B経路) |
| 4 | **SSE→Tauriイベント中継**: Rustで受けたストリームをフロントへ逐次流す(チャンネルAPI) | 体感遅延なくストリーミング表示できる | ポーリング(妥協) |
| 5 | **YAML代替クレートのAPI確認**: serde-saphyr(or yaml-rust2)でフロントマター構造体のserde往復 | serde deriveで素直に読めること | 候補間で切替 |
| 6 | **縦書きプレビュー**: WebView2でCSS `writing-mode: vertical-rl`+禁則・約物の見え方 | 通読確認に耐える表示 | S-09の優先度を下げQ-3再判断 |
| 7 | **ローカルLLMの実力見極め(v0.2追加)**: 本文2,000字+設定エントリ5件を渡し、表記ゆれ・簡単な設定矛盾を検出できるか(Qwen/Gemma系 7B〜14B級で計測) | ゴールデンセット最小版([06-ai-pipeline.md](06-ai-pipeline.md) §8)での検出率・誤検知率が実用水準 | 当該機能はクラウド推奨に倒す(06 §7の参考値を実測で更新) |

## 8. 決定まとめ(Q-2クローズ)

- シェル: **Tauri 2 + Rust**(Q-1決定を踏襲)
- フロントエンド: **React 19 + TypeScript + Vite + Zustand + Tailwind CSS + shadcn/ui**
- エディタ: **CodeMirror 6**(PoC#1をWeek 1のゲートとし、失敗時は§3.2の対応ラダー: ①変換中の装飾非表示 → ②校正モードのread-only化 → ③サイドパネルへ外部化 → ④最終手段としてProseMirror/Tiptap)
- Rustコア: rusqlite(FTS5 trigram)+ notify + aho-corasick + pulldown-cmark + similar + ulid + serde-saphyr系YAML + uggg流用(llm.rs/secrets.rs/cost.rs)
- 残未決: 相関図描画ライブラリ(S-05実装時に選定)、ルビ表示方式(未決-3と同時)

## 9. 参考リンク

- [CodeMirror公式](https://codemirror.net/)(v6.5でのIME処理改善)
- [CM6 日本語IMEカーソル位置の報告例(discuss.codemirror.net)](https://discuss.codemirror.net/t/issue-with-google-japanese-ime-cursor-position-in-v6/8810)
- [iOS日本語入力の挙動(codemirror/dev #430)](https://github.com/codemirror/codemirror.next/issues/430)
- [ProseMirror vs Lexical 比較・CJKはProseMirror優位の議論(discuss.prosemirror.net)](https://discuss.prosemirror.net/t/differences-between-prosemirror-and-lexical/4557)
- [Lexical/ProseMirror 負荷比較(Emergence Engineering)](https://emergence-engineering.com/blog/lexical-prosemirror-comparison)
- [SQLite FTS5 trigramによるCJK全文検索とハイブリッド戦略(Zenn)](https://zenn.dev/kanseilink/articles/kanseilink-fts5-trigram-cjk-20260507?locale=en)
- [FTS5日本語検索の精度改善(Zenn)](https://zenn.dev/mtk0/articles/sui-memory-fts5-search-tuning?locale=en)
- [lindera-sqlite(FTS5用日本語トークナイザ)](https://github.com/lindera/lindera-sqlite)
- [serde_yaml非推奨の議論(users.rust-lang.org)](https://users.rust-lang.org/t/serde-yaml-deprecation-alternatives/108868)
- [serde_yml の健全性勧告(RUSTSEC-2025-0068)](https://osv.dev/vulnerability/RUSTSEC-2025-0068)
- [serde_yml移行ガイド(serde-saphyr/yaml-rust2への言及)](https://github.com/sebastienrousseau/serde_yml/blob/master/MIGRATION.md)
