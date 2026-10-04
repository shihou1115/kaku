//! 時間を測る(テスト計画 D4・G4)。**ふだんは走らない**(`#[ignore]`)。
//! 配布物と同じ最適化で測るため、リリースのプロファイルで走らせる:
//!
//! ```text
//! cargo test --release --test perf_probe -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `KAKU_PERF_DIR` を渡すと、そのフォルダー(Google ドライブ等)の下で測る。無ければ一時フォルダー。
//! 時間は同じ操作を3回(保存は10回)測った中央値。

mod common;

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use kaku_lib::{project, search};

fn base() -> PathBuf {
    let base = std::env::var_os("KAKU_PERF_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!("kaku-perf-{}", std::process::id()));
    project::init(&root).unwrap();
    root
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

/// 小説らしい文字の並び(ひらがな6割・漢字3割・句読点と改行)。同じ文の繰り返しにすると、
/// 索引に入る三文字組が少なすぎて、実際より速く測れてしまう
fn prose(rng: &mut common::Rng, chars: usize) -> String {
    let mut s = String::with_capacity(chars * 3);
    for i in 0..chars {
        let r = rng.below(100);
        let c = if i % 60 == 59 {
            '\n'
        } else if r < 60 {
            char::from_u32(0x3041 + rng.below(83) as u32).unwrap()
        } else if r < 90 {
            char::from_u32(0x4E00 + rng.below(3000) as u32).unwrap()
        } else if r < 96 {
            '、'
        } else {
            '。'
        };
        s.push(c);
    }
    s
}

/// D4: 1,000ファイル・合計300万字の原稿で、初回の索引づくりと差分の更新にかかる時間
#[test]
#[ignore]
fn search_index_time() {
    // 長編1冊(30万字)と、その10倍(300万字。計画の大きさ)
    for files in [100, 1000] {
        index_times(files, 3000);
    }
}

fn index_times(files: usize, per_file: usize) {
    let root = base();
    let mut rng = common::Rng(20261005);
    let paths: Vec<String> = (0..files)
        .map(|i| format!("manuscript/第{:02}章/{i:04}.md", i / 50))
        .collect();
    for p in &paths {
        let body = format!("---\ntitle: {p}\n---\n\n{}\n", prose(&mut rng, per_file));
        project::create_file(&root, p, &body).unwrap();
    }
    let index = root.join(project::APP_DIR).join("index.sqlite");

    let mut initial = Vec::new();
    for _ in 0..3 {
        let _ = fs::remove_file(&index);
        let t = Instant::now();
        let r = search::search(&root, "夜明けの鐘").unwrap();
        initial.push(t.elapsed());
        assert!(r.reindexed >= files, "{}", r.reindexed); // project.md なども入る
    }
    let mut unchanged = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        let r = search::search(&root, "夜明けの鐘").unwrap();
        unchanged.push(t.elapsed());
        assert_eq!(r.reindexed, 0);
    }
    let mut ten_changed = Vec::new();
    for round in 0..3 {
        for p in paths.iter().skip(round * 10).take(10) {
            let body = format!("---\ntitle: {p}\n---\n\n{}\n", prose(&mut rng, per_file));
            project::save_text(&root, p, &body).unwrap();
        }
        let t = Instant::now();
        let r = search::search(&root, "夜明けの鐘").unwrap();
        ten_changed.push(t.elapsed());
        assert_eq!(r.reindexed, 10);
    }
    let mut two_chars = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        search::search(&root, "鐘を").unwrap();
        two_chars.push(t.elapsed());
    }
    let size = fs::metadata(&index).map(|m| m.len()).unwrap_or(0);
    println!("D4: {files} files x {per_file} chars (root {})", root.display());
    println!("  initial index   {:?}", median(initial));
    println!("  unchanged       {:?}", median(unchanged));
    println!("  10 files edited {:?}", median(ten_changed));
    println!("  2-char search   {:?}", median(two_chars));
    println!("  index size      {} KB", size / 1024);
    fs::remove_dir_all(&root).ok();
}

/// G4: 1MB の原稿を保存する時間(保存の前に、前の内容をバックアップへ写す)。
/// 自動保存は入力が止まるたびに走るので、ここが長いと書き手を待たせる
#[test]
#[ignore]
fn save_time_for_a_large_manuscript() {
    let root = base();
    let mut rng = common::Rng(20261005);
    let rel = "manuscript/長い原稿.md";
    let mut body = prose(&mut rng, 340_000); // UTF-8 で約1MB
    project::create_file(&root, rel, &body).unwrap();
    let bytes = body.len();
    let mut times = Vec::new();
    for i in 0..10 {
        body.push_str(&format!("追記{i}。"));
        let expected = project::modified_ms(&root, rel).ok();
        let t = Instant::now();
        let r = project::save_checked(&root, rel, &body, expected).unwrap();
        times.push(t.elapsed());
        assert!(matches!(r, project::SaveCheck::Saved(_)), "{r:?}");
    }
    println!("G4: {} KB manuscript (root {})", bytes / 1024, root.display());
    println!("  save (with backup) median {:?}, max {:?}", median(times.clone()), times.iter().max().unwrap());
    fs::remove_dir_all(&root).ok();
}
