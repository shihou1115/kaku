//! 実機のファイルシステムで、アプリが前提にしている挙動を確かめる(テスト計画 B5)。
//! **ふだんは走らない**(`#[ignore]`)。確かめたいフォルダーを環境変数で渡して実行する:
//!
//! ```text
//! KAKU_FS_PROBE_DIR=G:\マイドライブ\kaku-検証用 cargo test --test fs_probe -- --ignored --nocapture
//! ```
//!
//! Google ドライブのような同期フォルダーでは、更新時刻の分解能・改名・連続保存の
//! 振る舞いがローカルのディスクと違うことがある。楽観ロック(更新時刻の照合)と
//! 自動保存は、その前提の上に立っている。

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use kaku_lib::project;

fn probe_root() -> Option<PathBuf> {
    let base = PathBuf::from(std::env::var_os("KAKU_FS_PROBE_DIR")?);
    let root = base.join(format!("probe-{}", std::process::id()));
    project::init(&root).expect("検証用のフォルダーを作れない");
    Some(root)
}

fn names_in(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// 書き込んでから、更新時刻が変わって見えるまでの最短の間隔(ミリ秒)
fn mtime_step(root: &std::path::Path, rel: &str) -> Option<u64> {
    project::create_file(root, rel, "0").unwrap();
    let first = project::modified_ms(root, rel).unwrap();
    for i in 1..=200u64 {
        std::thread::sleep(Duration::from_millis(5));
        project::save_text(root, rel, &format!("{i}")).unwrap();
        if project::modified_ms(root, rel).unwrap() != first {
            return Some(i * 5);
        }
    }
    None
}

#[test]
#[ignore]
fn filesystem_behaves_as_the_app_assumes() {
    let Some(root) = probe_root() else {
        eprintln!("KAKU_FS_PROBE_DIR が無いので何もしない");
        return;
    };
    println!("PROBE ROOT: {}", root.display());

    // 1. 更新時刻の分解能。楽観ロックは「時刻が変わったか」で外部の変更を見つける。
    //    この間隔より短い間に外部で書かれると、時刻では気づけない(中身の照合が頼り)
    let steps: Vec<Option<u64>> = (0..5)
        .map(|i| mtime_step(&root, &format!("manuscript/時刻{i}.md")))
        .collect();
    println!("MTIME STEP (ms until a write shows a new time): {steps:?}");
    let sample = project::modified_ms(&root, "manuscript/時刻0.md").unwrap();
    println!("MTIME SAMPLE: {sample} (ms part {})", sample % 1000);

    // 2. 大文字小文字だけの改名
    project::create_file(&root, "manuscript/scene.md", "場面").unwrap();
    project::create_file(&root, "manuscript/index.md", "[場面](scene.md)\n").unwrap();
    let r = project::rename(&root, "manuscript/scene.md", "manuscript/Scene.md");
    println!("CASE RENAME: {r:?}");
    assert!(r.is_ok(), "大文字小文字だけの改名ができない: {r:?}");
    assert!(names_in(&root.join("manuscript")).contains(&"Scene.md".to_string()));
    assert_eq!(
        project::read_text(&root, "manuscript/index.md").unwrap(),
        "[場面](Scene.md)\n"
    );

    // 3. 自動保存より速い連続保存。同期ソフトが開いている間に書けずに失敗しないか
    let mut errors = Vec::new();
    for i in 0..20 {
        if let Err(e) = project::save_text(&root, "manuscript/Scene.md", &format!("連続{i}")) {
            errors.push(format!("{i}: {e}"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("RAPID SAVES: {} errors {errors:?}", errors.len());
    assert!(errors.is_empty(), "連続保存で失敗した: {errors:?}");
    assert_eq!(project::read_text(&root, "manuscript/Scene.md").unwrap(), "連続19");

    // 4. ゴミ箱への退避と、深い階層への移動(リンク追随)
    let trashed = project::trash(&root, "manuscript/Scene.md");
    println!("TRASH: {trashed:?}");
    assert!(trashed.is_ok());
    let moved = project::rename(&root, "manuscript/index.md", "manuscript/第一章/index.md");
    println!("MOVE: {moved:?}");
    assert!(moved.is_ok());

    fs::remove_dir_all(&root).ok();
    println!("PROBE DONE");
}
