/**
 * 「講評に残す」で `reviews/` へ書く中身(M-05 / 03 §4.3)。
 *
 * 組み立ては画面から切り出して、純関数としてテストする(テスト計画 E4)。
 * **記録は実際と食い違ってはいけない**: 何を渡し、どこまで見て、何が起きたかを
 * そのまま書く。食い違うと「全部渡し、全部見たうえでの講評」として読めてしまう。
 */

import { REVIEW_ASPECTS, type ReviewAspect } from "../api";
import { frontmatter } from "./saveNote";

export type ReviewNoteComment = {
  aspect: ReviewAspect;
  comment: string;
  quote: string;
  found: boolean;
  suggestion: string;
  /** 採否の印。付けていなければ undefined */
  verdict?: "done" | "dropped";
};

export type ReviewNoteInput = {
  name: string;
  now: Date;
  model?: string;
  /** レビューした文書 */
  path: string | null;
  aspects: ReviewAspect[];
  /** **実際に渡した**資料(Rust の material_paths の順) */
  materials: { path: string; title: string; kind: "自動" | "手動" }[];
  /** 頼んだのに渡らなかった資料の数(上限を超えた・読めなかった) */
  notPassed: number;
  warning: string | null;
  /** 本文が長くて見ていない末尾の字数(0なら全文を見た) */
  unchecked: number;
  overall: string;
  comments: ReviewNoteComment[];
};

const VERDICT_LABEL = { done: " 【対応済み】", dropped: " 【棄却】" } as const;

function aspectLabel(k: ReviewAspect): string {
  return REVIEW_ASPECTS.find((a) => a.key === k)?.label ?? k;
}

export function reviewNote(i: ReviewNoteInput): string {
  const lines: string[] = [];
  for (const a of REVIEW_ASPECTS) {
    const items = i.comments.filter((c) => c.aspect === a.key);
    if (items.length === 0) continue;
    lines.push(`### ${a.label}`, "");
    for (const c of items) {
      lines.push(`- ${c.comment}${c.verdict ? VERDICT_LABEL[c.verdict] : ""}`);
      if (c.quote) {
        lines.push(`  - 引用: 「${c.quote}」${c.found ? "" : " ※本文に見つかりません"}`);
      }
      if (c.suggestion) lines.push(`  - 方向: ${c.suggestion}`);
    }
    lines.push("");
  }

  const aspects = i.aspects.map(aspectLabel).join("、");
  const materials = [
    ...(i.materials.length > 0
      ? i.materials.map((m) => `- ${m.title}(${m.kind}) — ${m.path}`)
      : ["- (なし)"]),
    // 頼んだのに渡らなかったものを黙って隠さない
    ...(i.notPassed > 0
      ? [`- ※ 上限を超えた・読めなかったため ${i.notPassed}件は渡していません`]
      : []),
  ];

  return [
    frontmatter({
      title: i.name,
      created: i.now.toISOString(),
      model: i.model,
      source: i.path ?? undefined,
      aspects,
    }),
    "## 対象",
    "",
    `- 文書: ${i.path ?? "(ファイルを開いていない)"}`,
    `- 観点: ${aspects}`,
    // 見ていない部分があるなら必ず書く(全文を見た講評として読めないように)
    ...(i.unchecked > 0
      ? [`- ※ 本文が長いため、末尾 ${i.unchecked}字は見ていません`]
      : []),
    "",
    "### 渡した設定資料",
    "",
    ...materials,
    "",
    // 打ち切り・拒否・形式違反があったなら必ず書く。
    // 警告を落とすと「全部見たうえでの講評」として読めてしまう
    ...(i.warning ? ["> ⚠ " + i.warning, ""] : []),
    "## 全体講評",
    "",
    i.overall || "(なし)",
    "",
    `## 指摘(${i.comments.length}件)`,
    "",
    ...(lines.length > 0 ? lines : ["(なし)", ""]),
  ].join("\n");
}
