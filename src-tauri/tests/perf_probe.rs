//! 時間を測る(テスト計画 D4・G4)。**ふだんは走らない**(`#[ignore]`)。
//! 配布物と同じ最適化で測るため、リリースのプロファイルで走らせる:
//!
//! ```text
//! cargo test --release --test perf_probe -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `KAKU_PERF_DIR` を渡すと、そのフォルダー(Google ドライブ等)の下で測る。無ければ一時フォルダー。
//! `KAKU_PERF_FILES=100` のように渡すと、索引はその数のファイルだけで測る(既定は 100 と 1000)。
//! 時間は同じ操作を3回(保存は10回)測った中央値。
//!
//! 作業フォルダーは測るものごとに分け、途中で止まっても消す。以前は2つの計測が同じフォルダーを
//! 使い、先に止まった保存の計測が残した1MB の原稿を、索引の計測が一緒に索引していた(G: で起きた)

mod common;

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kaku_lib::{project, search};

/// 測るものごとの作業フォルダー。落ちても消える(同期フォルダーに残さない)
struct Workdir(PathBuf);

impl Workdir {
    fn new(tag: &str) -> Workdir {
        let base = std::env::var_os("KAKU_PERF_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let root = base.join(format!("kaku-perf-{tag}-{}", std::process::id()));
        project::init(&root).unwrap();
        Workdir(root)
    }
}

impl Drop for Workdir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn now_ms() -> i128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i128
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
    let counts: Vec<usize> = std::env::var("KAKU_PERF_FILES")
        .ok()
        .map(|s| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![100, 1000]);
    for files in counts {
        index_times(files, 3000);
    }
}

fn index_times(files: usize, per_file: usize) {
    let dir = Workdir::new(&format!("index{files}"));
    let root = &dir.0;
    let mut rng = common::Rng(20261005);
    let paths: Vec<String> = (0..files)
        .map(|i| format!("manuscript/第{:02}章/{i:04}.md", i / 50))
        .collect();
    for p in &paths {
        let body = format!("---\ntitle: {p}\n---\n\n{}\n", prose(&mut rng, per_file));
        project::create_file(root, p, &body).unwrap();
    }
    let index = root.join(project::APP_DIR).join("index.sqlite");

    let mut initial = Vec::new();
    for _ in 0..3 {
        let _ = fs::remove_file(&index);
        let t = Instant::now();
        let r = search::search(root, "夜明けの鐘").unwrap();
        initial.push(t.elapsed());
        assert!(r.reindexed >= files, "{}", r.reindexed); // project.md なども入る
    }
    let mut unchanged = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        let r = search::search(root, "夜明けの鐘").unwrap();
        unchanged.push(t.elapsed());
        assert_eq!(r.reindexed, 0);
    }
    let mut one_changed = Vec::new();
    let mut ten_changed = Vec::new();
    for round in 0..3 {
        // 1つ書いて検索(ふだんの使い方)、10個書いて検索
        let p = &paths[round];
        let body = format!("---\ntitle: {p}\n---\n\n{}\n", prose(&mut rng, per_file));
        project::save_text(root, p, &body).unwrap();
        let t = Instant::now();
        let r = search::search(root, "夜明けの鐘").unwrap();
        one_changed.push(t.elapsed());
        assert_eq!(r.reindexed, 1);
        for p in paths.iter().skip(10 + round * 10).take(10) {
            let body = format!("---\ntitle: {p}\n---\n\n{}\n", prose(&mut rng, per_file));
            project::save_text(root, p, &body).unwrap();
        }
        let t = Instant::now();
        let r = search::search(root, "夜明けの鐘").unwrap();
        ten_changed.push(t.elapsed());
        assert_eq!(r.reindexed, 10);
    }
    let mut two_chars = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        search::search(root, "鐘を").unwrap();
        two_chars.push(t.elapsed());
    }
    let size = fs::metadata(&index).map(|m| m.len()).unwrap_or(0);
    println!("D4: {files} files x {per_file} chars");
    println!("  initial index   {:?}", median(initial));
    println!("  unchanged       {:?}", median(unchanged));
    println!("  1 file edited   {:?}", median(one_changed));
    println!("  10 files edited {:?}", median(ten_changed));
    println!("  2-char search   {:?}", median(two_chars));
    println!("  index size      {} KB", size / 1024);
}

/// G4: 1MB の原稿を保存する時間(保存の前に、前の内容をバックアップへ写す)。
/// 自動保存は入力が止まるたびに走るので、ここが長いと書き手を待たせる。
///
/// アプリと同じく、照合で**時刻だけ**が変わっていた(中身は前に書いたまま)ときは、読んだ時刻で
/// 照合し直す(saveFlow の「時刻だけの競合」)。同期フォルダーでは、書いた後で同期ソフトが
/// 時刻を動かすことがあるので、その回数と、時刻のずれも出す
#[test]
#[ignore]
fn save_time_for_a_large_manuscript() {
    let dir = Workdir::new("save");
    let root = &dir.0;
    let mut rng = common::Rng(20261005);
    let rel = "manuscript/長い原稿.md";
    let mut body = prose(&mut rng, 340_000); // UTF-8 で約1MB
    project::create_file(root, rel, &body).unwrap();
    let bytes = body.len();
    let mut saved = body.clone();
    let mut times = Vec::new();
    let mut time_only = 0;
    let mut skew_ms: i128 = 0;
    for i in 0..10 {
        // 打つ間(同期ソフトが動く時間)を置く
        std::thread::sleep(Duration::from_millis(300));
        body.push_str(&format!("追記{i}。"));
        let expected = project::modified_ms(root, rel).ok();
        let t = Instant::now();
        let mut r = project::save_checked(root, rel, &body, expected).unwrap();
        if let project::SaveCheck::Conflict(actual) = r {
            assert_eq!(project::read_text(root, rel).unwrap(), saved, "時刻だけでなく中身も変わっていた");
            time_only += 1;
            r = project::save_checked(root, rel, &body, Some(actual)).unwrap();
        }
        times.push(t.elapsed());
        let project::SaveCheck::Saved(ms) = r else {
            panic!("照合し直しても書けなかった: {r:?}");
        };
        skew_ms = skew_ms.max((ms as i128 - now_ms()).abs());
        saved = body.clone();
    }
    println!("G4: {} KB manuscript", bytes / 1024);
    println!(
        "  save (with backup) median {:?}, max {:?}",
        median(times.clone()),
        times.iter().max().unwrap()
    );
    println!("  time-only conflicts {time_only}/10, max |mtime - now| {skew_ms} ms");
}
