//! プロジェクトフォルダの読み書き(docs/03-data-format.md)。
//!
//! 原則: **ファイルが正、アプリは従**(D-1)。
//! アプリは通常のフォルダとMarkdownファイルを読み書きするだけで、
//! 独自形式のデータベースに取り込まない。

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::frontmatter;

/// プロジェクト直下に作るフォルダ(docs/03-data-format.md §3)
pub const TOP_DIRS: [&str; 5] = ["manuscript", "codex", "plot", "ideas", "reviews"];
/// codex の既定の種別フォルダ
pub const CODEX_DIRS: [&str; 5] = ["characters", "locations", "items", "terms", "notes"];
/// アプリ専用領域(人間の編集対象外)
pub const APP_DIR: &str = ".app";

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("パスがプロジェクト外を指しています: {0}")]
    OutsideProject(String),
    #[error("入出力エラー: {0}")]
    Io(#[from] io::Error),
    #[error("文字コードを判別できませんでした: {0}")]
    Encoding(String),
}

impl serde::Serialize for ProjectError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

/// ファイルツリーの1ノード
#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    /// プロジェクトルートからの相対パス(区切りは常に `/`)
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    /// 表示名: フロントマターの title があればそれ、無ければファイル名(§4.2)
    pub title: Option<String>,
    pub children: Vec<TreeNode>,
}

/// codex エントリ(言及検出に使う最小情報)
#[derive(Debug, Clone, Serialize)]
pub struct CodexEntry {
    pub path: String,
    /// 表示名。title が無ければファイル名(拡張子なし)
    pub title: String,
    pub aliases: Vec<String>,
    pub type_: Option<String>,
    pub description: Option<String>,
}

impl CodexEntry {
    /// 言及検出に渡すパターン(正式名+別名)
    pub fn patterns(&self) -> Vec<String> {
        let mut v = vec![self.title.clone()];
        v.extend(self.aliases.iter().cloned());
        v
    }
}

/// プロジェクト外への脱出(`..`・絶対パス)を防いで実パスへ解決する。
pub fn resolve(root: &Path, relative: &str) -> Result<PathBuf, ProjectError> {
    let rel = Path::new(relative);
    if rel.is_absolute() {
        return Err(ProjectError::OutsideProject(relative.to_string()));
    }
    for c in rel.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err(ProjectError::OutsideProject(relative.to_string())),
        }
    }
    Ok(root.join(rel))
}

fn rel_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// プロジェクトを新規作成する(既存フォルダにも追加できる)。
pub fn init(root: &Path) -> Result<(), ProjectError> {
    fs::create_dir_all(root)?;
    for d in TOP_DIRS {
        fs::create_dir_all(root.join(d))?;
    }
    for d in CODEX_DIRS {
        fs::create_dir_all(root.join("codex").join(d))?;
    }
    fs::create_dir_all(root.join(APP_DIR).join("backups"))?;
    fs::create_dir_all(root.join(APP_DIR).join("logs"))?;

    let project_md = root.join("project.md");
    if !project_md.exists() {
        let name = root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "無題".to_string());
        let content = format!("---\ntitle: {name}\n---\n\n(あらすじをここに書く)\n");
        write_new(&project_md, &content)?;
    }
    Ok(())
}

fn write_new(path: &Path, content: &str) -> Result<(), ProjectError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // 改行は LF、UTF-8 BOMなし(D-6)
    fs::write(path, content.replace("\r\n", "\n"))?;
    Ok(())
}

/// ゴミ箱の場所。削除した項目はここへ退避する
pub const TRASH_DIR: &str = ".app/trash";

/// ツリーを走査する。`.app/` と隠しフォルダは対象外(人間が触る領域だけを見せる)。
///
/// **ただしゴミ箱だけは末尾に出す。** 削除は消さずにここへ移す方式なので、
/// 見えないと「戻せる」ことが伝わらず、エクスプローラーを開くしか手段がなくなる。
/// 中身が無いときは出さない(空の箱を常に置いても場所を取るだけ)。
pub fn scan(root: &Path) -> Result<Vec<TreeNode>, ProjectError> {
    let mut tree = scan_dir(root, root)?;
    let trash = root.join(".app").join("trash");
    if trash.is_dir() {
        let children = scan_dir(root, &trash)?;
        if !children.is_empty() {
            tree.push(TreeNode {
                path: TRASH_DIR.to_string(),
                name: "trash".to_string(),
                is_dir: true,
                title: None,
                children,
            });
        }
    }
    Ok(tree)
}

fn scan_dir(root: &Path, dir: &Path) -> Result<Vec<TreeNode>, ProjectError> {
    let mut dirs: Vec<TreeNode> = Vec::new();
    let mut files: Vec<TreeNode> = Vec::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() {
            dirs.push(TreeNode {
                path: rel_string(root, &path),
                name,
                is_dir: true,
                title: None,
                children: scan_dir(root, &path)?,
            });
        } else if ft.is_file() {
            let is_md = path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("txt"))
                .unwrap_or(false);
            if !is_md {
                continue;
            }
            let title = fs::read_to_string(&path)
                .ok()
                .and_then(|s| frontmatter::parse_source(&s).title);
            files.push(TreeNode {
                path: rel_string(root, &path),
                name,
                is_dir: false,
                title,
                children: Vec::new(),
            });
        }
    }

    // ファイル名の連番プレフィックスで並ぶことを期待しているので単純な名前順(§3)
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| a.name.cmp(&b.name));
    dirs.extend(files);
    Ok(dirs)
}

/// codex/ 配下のエントリを読み込む。
pub fn load_codex(root: &Path) -> Result<Vec<CodexEntry>, ProjectError> {
    let codex_dir = root.join("codex");
    let mut out = Vec::new();
    if !codex_dir.exists() {
        return Ok(out);
    }
    collect_codex(root, &codex_dir, &mut out)?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn collect_codex(root: &Path, dir: &Path, out: &mut Vec<CodexEntry>) -> Result<(), ProjectError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_codex(root, &path, out)?;
        } else if path.extension().map(|e| e == "md").unwrap_or(false) {
            let source = match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => continue, // 壊れたファイルでも他を止めない
            };
            let fm = frontmatter::parse_source(&source);
            // title 未記入ならファイル名を表示名にする(§4.1: 手書き必須は title のみ)
            let fallback = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let title = fm.title.unwrap_or(fallback);
            if title.trim().is_empty() {
                continue;
            }
            // type 省略時はフォルダ位置から推定する(§3)
            let type_ = fm.type_.or_else(|| {
                path.parent()
                    .and_then(|p| p.file_name())
                    .map(|s| s.to_string_lossy().to_string())
            });
            out.push(CodexEntry {
                path: rel_string(root, &path),
                title,
                aliases: fm.aliases,
                type_,
                description: fm.description,
            });
        }
    }
    Ok(())
}

/// ファイルを読む。UTF-8(BOM有無)と Shift_JIS を受け入れる(§4.4)。
pub fn read_text(root: &Path, relative: &str) -> Result<String, ProjectError> {
    let path = resolve(root, relative)?;
    let bytes = fs::read(&path)?;
    decode(&bytes, relative)
}

fn decode(bytes: &[u8], label: &str) -> Result<String, ProjectError> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec())
            .map_err(|_| ProjectError::Encoding(label.to_string()));
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(s.to_string()),
        // UTF-8 として読めないものは既存原稿(Shift_JIS)の可能性がある。
        // V1では判別に踏み込まず、読めない旨を返して上書き事故を防ぐ(Q-4で扱う)
        Err(_) => Err(ProjectError::Encoding(label.to_string())),
    }
}

/// 保存する。**保存前に1世代のバックアップを取る**(M-01 / MVP要素5)。
///
/// 履歴管理はしない。同じファイルの前回内容だけを `.app/backups/` に残す。
/// アプリ専用領域(`.app/`)を指しているか。
///
/// ここはバックアップ・ゴミ箱・ログ・索引の置き場で、**アプリが書き込むのは
/// それぞれの専用処理からだけ**である。原稿の保存経路がここへ届いてはいけない。
/// 特にゴミ箱は「消さずに取っておいたもの」なので、上書きされると退避の意味が消える。
/// 相対パスを正規化する(`./` を落として区切りを `/` に揃える)。
///
/// **判定の前に必ずこれを通す。** `./.app/x` のような書き方で
/// `is_app_area` をすり抜けられると、退避しておいたものが上書きされる。
fn normalize_rel(relative: &str) -> String {
    relative
        .replace('\\', "/")
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != ".")
        .collect::<Vec<_>>()
        .join("/")
}

pub fn is_app_area(relative: &str) -> bool {
    let p = normalize_rel(relative);
    p == APP_DIR || p.starts_with(&format!("{APP_DIR}/"))
}

fn reject_app_area(relative: &str) -> Result<(), ProjectError> {
    if is_app_area(relative) {
        return Err(ProjectError::OutsideProject(format!(
            "{relative}(アプリ専用領域には書き込めません)"
        )));
    }
    Ok(())
}

pub fn write_text(root: &Path, relative: &str, content: &str) -> Result<(), ProjectError> {
    reject_app_area(relative)?;
    let path = resolve(root, relative)?;
    if path.exists() {
        backup(root, relative, &path)?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, content)?;
    Ok(())
}

fn backup(root: &Path, relative: &str, path: &Path) -> Result<(), ProjectError> {
    // 相対パスをそのまま backups/ 配下に写す(どのファイルの控えか目視で分かる)
    let dest = root.join(APP_DIR).join("backups").join(relative);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(path, dest)?;
    Ok(())
}

/// 新規ファイルを作る。既存なら何もしない(上書き事故の防止)。
pub fn create_file(root: &Path, relative: &str, content: &str) -> Result<bool, ProjectError> {
    reject_app_area(relative)?;
    let path = resolve(root, relative)?;
    if path.exists() {
        return Ok(false);
    }
    write_new(&path, content)?;
    Ok(true)
}

// ===== 削除・改名・複製 =====

/// 削除は**消さずにゴミ箱へ移す**(`.app/trash/<日時>/<元のパス>`)。
///
/// 原則(D-1/D-5)からの帰結: ユーザーの原稿を不可逆に失う操作をアプリが持たない。
/// 確認ダイアログを押し間違えても、エクスプローラで取り戻せる。
/// 戻り値は退避先の絶対パス(UIで案内するため)。
pub fn trash(root: &Path, relative: &str) -> Result<String, ProjectError> {
    // ゴミ箱の中身をさらにゴミ箱へ入れない(退避したものは動かさない)
    reject_app_area(relative)?;
    let path = resolve(root, relative)?;
    if !path.exists() {
        return Err(ProjectError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("見つかりません: {relative}"),
        )));
    }
    // 退避先のフォルダ名は秒まで(人が読めることを優先している)。
    // **同じ秒に同じ相対パスを2度退避すると先のものを踏む**ので、
    // 埋まっていたら連番を付けて空いている場所を取る。
    // `fs::rename` は Windows では黙って上書きするため、ここで避けないと退避の意味が消える
    let stamp = timestamp_dir(std::time::SystemTime::now());
    let base = root.join(APP_DIR).join("trash");
    let mut dest = base.join(&stamp).join(relative);
    for n in 2..1000 {
        if !dest.exists() {
            break;
        }
        dest = base.join(format!("{stamp}_{n}")).join(relative);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    // 同一ドライブ内なので rename で足りる。失敗したらコピーしてから消す
    if fs::rename(&path, &dest).is_err() {
        if path.is_dir() {
            copy_dir(&path, &dest)?;
            fs::remove_dir_all(&path)?;
        } else {
            fs::copy(&path, &dest)?;
            fs::remove_file(&path)?;
        }
    }
    Ok(dest.to_string_lossy().to_string())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), ProjectError> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// 中身の件数(削除確認で「フォルダごと消える」ことを見せるため)
pub fn count_files(root: &Path, relative: &str) -> Result<usize, ProjectError> {
    let path = resolve(root, relative)?;
    if path.is_file() {
        return Ok(1);
    }
    fn walk(dir: &Path) -> Result<usize, ProjectError> {
        let mut n = 0;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                n += walk(&entry.path())?;
            } else {
                n += 1;
            }
        }
        Ok(n)
    }
    walk(&path)
}

/// 改名・移動。プロジェクト内のMarkdownリンクも追随させる(§5-6)。
pub fn rename(root: &Path, from: &str, to: &str) -> Result<(), ProjectError> {
    // 退避したものを動かすのも、原稿をアプリ専用領域へ押し込むのも塞ぐ
    reject_app_area(from)?;
    reject_app_area(to)?;
    let src = resolve(root, from)?;
    let dst = resolve(root, to)?;
    if !src.exists() {
        return Err(ProjectError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("見つかりません: {from}"),
        )));
    }
    if dst.exists() {
        return Err(ProjectError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("同名のファイルが既にあります: {to}"),
        )));
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&src, &dst)?;
    rewrite_links(root, from, to)?;
    Ok(())
}

/// 複製。「〜のコピー」を付け、既にあれば連番にする。
pub fn duplicate(root: &Path, relative: &str) -> Result<String, ProjectError> {
    reject_app_area(relative)?;
    let src = resolve(root, relative)?;
    if !src.is_file() {
        return Err(ProjectError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ファイルのみ複製できます".to_string(),
        )));
    }
    let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = src
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    let dir = relative.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    for n in 1..100 {
        let name = if n == 1 {
            format!("{stem} のコピー{ext}")
        } else {
            format!("{stem} のコピー{n}{ext}")
        };
        let candidate = if dir.is_empty() {
            name
        } else {
            format!("{dir}/{name}")
        };
        if !resolve(root, &candidate)?.exists() {
            fs::copy(&src, resolve(root, &candidate)?)?;
            return Ok(candidate);
        }
    }
    Err(ProjectError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "複製名を決められませんでした".to_string(),
    )))
}

pub fn create_dir(root: &Path, relative: &str) -> Result<bool, ProjectError> {
    reject_app_area(relative)?;
    let path = resolve(root, relative)?;
    if path.exists() {
        return Ok(false);
    }
    fs::create_dir_all(path)?;
    Ok(true)
}

/// 改名に追随して、他ファイル内の相対リンクを書き換える。
///
/// `](相対パス)` 形式のみ扱う。リンク先を各ファイルの位置から解決し、
/// 改名対象と一致したものだけを差し替える(同名別ファイルを巻き込まない)。
fn rewrite_links(root: &Path, old_rel: &str, new_rel: &str) -> Result<(), ProjectError> {
    let mut targets = Vec::new();
    collect_md(root, root, &mut targets)?;
    for file in targets {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let dir = rel_string(root, file.parent().unwrap_or(root));
        let replaced = replace_links(&text, &dir, old_rel, new_rel);
        if replaced != text {
            fs::write(&file, replaced)?;
        }
    }
    Ok(())
}

fn collect_md(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), ProjectError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_md(root, &path, out)?;
        } else if path.extension().map(|e| e == "md").unwrap_or(false) {
            out.push(path);
        }
    }
    Ok(())
}

/// `base_dir` にあるファイルの本文中のリンクを書き換える。純関数(テスト用に分離)。
pub fn replace_links(text: &str, base_dir: &str, old_rel: &str, new_rel: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find("](") {
        let (head, tail) = rest.split_at(idx + 2);
        out.push_str(head);
        let Some(end) = tail.find(')') else {
            out.push_str(tail);
            return out;
        };
        let target = &tail[..end];
        // 完全一致(ファイルの改名)と、接頭辞一致(**フォルダーの改名**)の両方を見る。
        // 完全一致だけだと `codex/characters` を改名しても
        // `codex/characters/悠二.md` を指すリンクが切れたまま残る。
        // `old_rel` の直後に `/` を要求するので、兄弟の `codex/characters2` は巻き込まない
        let moved = normalize_join(base_dir, target).and_then(|r| {
            if r == old_rel {
                Some(new_rel.to_string())
            } else {
                r.strip_prefix(&format!("{old_rel}/"))
                    .map(|rest| format!("{new_rel}/{rest}"))
            }
        });
        match moved {
            Some(t) => out.push_str(&relative_from(base_dir, &t)),
            None => out.push_str(target),
        }
        out.push(')');
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// `base_dir` を起点に相対リンクを解決してプロジェクト相対パスにする。
/// 外部URLや絶対パスは対象外(None)。
fn normalize_join(base_dir: &str, target: &str) -> Option<String> {
    if target.is_empty()
        || target.contains("://")
        || target.starts_with('/')
        || target.starts_with('#')
    {
        return None;
    }
    let mut parts: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// `base_dir` から見た `target_rel` への相対パスを作る
fn relative_from(base_dir: &str, target_rel: &str) -> String {
    let base: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    let target: Vec<&str> = target_rel.split('/').collect();
    let common = base
        .iter()
        .zip(target.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let ups = base.len() - common;
    let mut parts: Vec<String> = std::iter::repeat("..".to_string()).take(ups).collect();
    parts.extend(target[common..].iter().map(|s| s.to_string()));
    parts.join("/")
}

/// `YYYY-MM-DD_HHMMSS`(UTC)。ゴミ箱フォルダ名を人が読めるようにするため。
fn timestamp_dir(t: std::time::SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}_{:02}{:02}{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// エポック日数 → 年月日(Howard Hinnant の civil_from_days)
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 外部編集の検知に使う更新時刻(エポックからのミリ秒)。
///
/// 常駐監視はしない。フロントがフォーカス復帰時に問い合わせる(§5-1)。
/// 読み込んだ後に外部で書き換えられたか(楽観ロック=T-08)。
///
/// `expected` が無ければ照合しない(新規作成直後など、まだ時刻を持たない経路)。
/// **0 は「時刻が読めなかった」の印**なので照合に使わない。ここを弾かないと、
/// 時刻を読めない環境で保存が毎回競合になり、一切書けなくなる。
pub fn is_stale(actual_ms: u64, expected_ms: Option<u64>) -> bool {
    matches!(expected_ms, Some(e) if e != 0 && e != actual_ms)
}

pub fn modified_ms(root: &Path, relative: &str) -> Result<u64, ProjectError> {
    let path = resolve(root, relative)?;
    let meta = fs::metadata(path)?;
    let ms = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Ok(ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// テスト用の作業フォルダ。
    ///
    /// **時刻だけでは一意にならない。** テストは並列に走るので、時計の分解能によっては
    /// 2つのテストが同じ名前を引いて互いのファイルを壊す(実際に踏んだ)。
    /// 連番を混ぜて確実に分ける。
    fn tmp() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "kaku-test-{}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn init_creates_expected_layout() {
        let root = tmp();
        init(&root).unwrap();
        for d in TOP_DIRS {
            assert!(root.join(d).is_dir(), "{d} が作られていない");
        }
        assert!(root.join("codex/characters").is_dir());
        assert!(root.join(".app/backups").is_dir());
        assert!(root.join("project.md").is_file());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn scan_skips_app_dir_and_reads_titles() {
        let root = tmp();
        init(&root).unwrap();
        create_file(
            &root,
            "manuscript/01-出会い.md",
            "---\ntitle: 出会い\n---\n本文\n",
        )
        .unwrap();
        let tree = scan(&root).unwrap();
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert!(!names.contains(&".app"), "アプリ専用領域が露出している");
        let manuscript = tree.iter().find(|n| n.name == "manuscript").unwrap();
        assert_eq!(manuscript.children.len(), 1);
        assert_eq!(manuscript.children[0].title.as_deref(), Some("出会い"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn load_codex_uses_filename_when_title_missing() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "codex/characters/佐藤架純.md", "外見の描写だけ\n").unwrap();
        create_file(
            &root,
            "codex/characters/五十嵐悠二.md",
            "---\ntitle: 五十嵐悠二\naliases: [悠二]\n---\n幼馴染\n",
        )
        .unwrap();
        let codex = load_codex(&root).unwrap();
        assert_eq!(codex.len(), 2);
        let kasumi = codex.iter().find(|c| c.title == "佐藤架純").unwrap();
        // title 未記入でもファイル名が使われ、type はフォルダから推定される
        assert_eq!(kasumi.type_.as_deref(), Some("characters"));
        let yuji = codex.iter().find(|c| c.title == "五十嵐悠二").unwrap();
        assert_eq!(yuji.patterns(), vec!["五十嵐悠二", "悠二"]);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn write_makes_one_generation_backup() {
        let root = tmp();
        init(&root).unwrap();
        write_text(&root, "manuscript/a.md", "1回目").unwrap();
        // 初回保存では控えはできない(元ファイルが無いため)
        assert!(!root.join(".app/backups/manuscript/a.md").exists());
        write_text(&root, "manuscript/a.md", "2回目").unwrap();
        assert_eq!(
            fs::read_to_string(root.join(".app/backups/manuscript/a.md")).unwrap(),
            "1回目"
        );
        write_text(&root, "manuscript/a.md", "3回目").unwrap();
        // 履歴は増やさない。常に「前回」だけ
        assert_eq!(
            fs::read_to_string(root.join(".app/backups/manuscript/a.md")).unwrap(),
            "2回目"
        );
        assert_eq!(
            fs::read_to_string(root.join("manuscript/a.md")).unwrap(),
            "3回目"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rejects_path_escape() {
        let root = tmp();
        assert!(resolve(&root, "../secret.md").is_err());
        assert!(resolve(&root, "manuscript/../../secret.md").is_err());
        assert!(resolve(&root, "manuscript/01.md").is_ok());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn create_file_does_not_overwrite() {
        let root = tmp();
        init(&root).unwrap();
        assert!(create_file(&root, "codex/terms/魔法.md", "初回").unwrap());
        assert!(!create_file(&root, "codex/terms/魔法.md", "二度目").unwrap());
        assert_eq!(
            fs::read_to_string(root.join("codex/terms/魔法.md")).unwrap(),
            "初回"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn trash_moves_instead_of_deleting() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "manuscript/01-出会い.md", "本文").unwrap();

        let dest = trash(&root, "manuscript/01-出会い.md").unwrap();

        // 元の場所からは消えるが、ゴミ箱に中身がそのまま残る
        assert!(!root.join("manuscript/01-出会い.md").exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "本文");
        assert!(dest.contains("trash"), "ゴミ箱配下にない: {dest}");
        assert!(dest.ends_with("manuscript\\01-出会い.md") || dest.ends_with("manuscript/01-出会い.md"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn trash_handles_directory_with_children() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "manuscript/第一章/01.md", "a").unwrap();
        create_file(&root, "manuscript/第一章/02.md", "b").unwrap();

        assert_eq!(count_files(&root, "manuscript/第一章").unwrap(), 2);
        let dest = trash(&root, "manuscript/第一章").unwrap();

        assert!(!root.join("manuscript/第一章").exists());
        assert_eq!(
            fs::read_to_string(PathBuf::from(&dest).join("02.md")).unwrap(),
            "b"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn trash_rejects_path_escape() {
        let root = tmp();
        init(&root).unwrap();
        assert!(trash(&root, "../外部ファイル.md").is_err());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rename_updates_links_in_other_files() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "codex/characters/悠二.md", "幼馴染\n").unwrap();
        create_file(
            &root,
            "codex/characters/架純.md",
            "[悠二](悠二.md)と[高校](../locations/青葉高校.md)\n",
        )
        .unwrap();
        create_file(
            &root,
            "manuscript/01.md",
            "登場: [悠二](../codex/characters/悠二.md)\n",
        )
        .unwrap();

        rename(
            &root,
            "codex/characters/悠二.md",
            "codex/characters/五十嵐悠二.md",
        )
        .unwrap();

        // 同じフォルダからのリンクも、階層をまたぐリンクも追随する
        assert_eq!(
            fs::read_to_string(root.join("codex/characters/架純.md")).unwrap(),
            "[悠二](五十嵐悠二.md)と[高校](../locations/青葉高校.md)\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("manuscript/01.md")).unwrap(),
            "登場: [悠二](../codex/characters/五十嵐悠二.md)\n"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rename_refuses_existing_target() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "manuscript/a.md", "a").unwrap();
        create_file(&root, "manuscript/b.md", "b").unwrap();
        assert!(rename(&root, "manuscript/a.md", "manuscript/b.md").is_err());
        // 上書きされていないこと
        assert_eq!(fs::read_to_string(root.join("manuscript/b.md")).unwrap(), "b");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn replace_links_leaves_unrelated_targets() {
        let text = "[a](悠二.md) [b](../locations/悠二.md) [c](https://example.com/悠二.md) [d](#見出し)";
        let got = replace_links(
            text,
            "codex/characters",
            "codex/characters/悠二.md",
            "codex/characters/五十嵐悠二.md",
        );
        // 同名でも別フォルダのファイル・外部URL・アンカーは触らない
        assert_eq!(
            got,
            "[a](五十嵐悠二.md) [b](../locations/悠二.md) [c](https://example.com/悠二.md) [d](#見出し)"
        );
    }

    #[test]
    fn app_area_is_never_written_through_the_normal_paths() {
        // ゴミ箱の中身が上書きされると、退避しておいた意味が消える。
        // **保存経路そのものを塞ぐ**(UIだけで防ぐと、別の呼び出しから抜ける)
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "manuscript/01.md", "元の中身").unwrap();
        let trashed = trash(&root, "manuscript/01.md").unwrap();

        // 退避先の相対パスを組み立て直して書き込みを試みる
        let rel = std::path::Path::new(&trashed)
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        assert!(is_app_area(&rel), "テストの前提が崩れている: {rel}");

        assert!(write_text(&root, &rel, "書き換え").is_err());
        assert!(create_file(&root, ".app/trash/新しい.md", "x").is_err());
        assert!(write_text(&root, ".app/settings.json", "{}").is_err());

        // 中身は無傷
        assert_eq!(fs::read_to_string(&trashed).unwrap(), "元の中身");
        // 通常の場所は今までどおり書ける
        assert!(write_text(&root, "manuscript/02.md", "新規").is_ok());
    }

    #[test]
    fn app_area_check_does_not_catch_lookalikes() {
        assert!(is_app_area(".app"));
        assert!(is_app_area(".app/trash/x.md"));
        assert!(!is_app_area("manuscript/.app.md"));
        assert!(!is_app_area(".application/x.md"));
        assert!(!is_app_area("codex/notes/.appendix.md"));
    }

    #[test]
    fn trash_appears_in_the_tree_only_when_it_has_something() {
        let root = tmp();
        init(&root).unwrap();

        // 空のうちは出さない(空の箱を常に置いても場所を取るだけ)
        assert!(
            scan(&root).unwrap().iter().all(|n| n.path != TRASH_DIR),
            "空のゴミ箱が出ている"
        );

        create_file(&root, "manuscript/01.md", "---\ntitle: 出会い\n---\n本文\n").unwrap();
        trash(&root, "manuscript/01.md").unwrap();

        let tree = scan(&root).unwrap();
        let node = tree.last().expect("ツリーが空");
        assert_eq!(node.path, TRASH_DIR, "ゴミ箱が末尾に無い");
        assert!(node.is_dir);
        assert!(!node.children.is_empty(), "中身が見えない");

        // 退避した中身までたどれること(戻す前に確認できる)
        fn find(nodes: &[TreeNode], name: &str) -> bool {
            nodes
                .iter()
                .any(|n| n.name == name || find(&n.children, name))
        }
        assert!(find(&node.children, "01.md"), "退避したファイルが見えない");
    }

    #[test]
    fn app_dir_other_than_trash_stays_hidden() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "manuscript/01.md", "本文").unwrap();
        // バックアップが作られる状況を作る
        write_text(&root, "manuscript/01.md", "書き換え").unwrap();
        trash(&root, "manuscript/01.md").unwrap();

        let tree = scan(&root).unwrap();
        // 出るのはゴミ箱だけ。backups や logs は人間の編集対象ではない
        assert!(tree.iter().all(|n| !n.path.starts_with(".app/backups")));
        assert!(tree.iter().all(|n| !n.path.starts_with(".app/logs")));
        assert_eq!(
            tree.iter().filter(|n| n.path.starts_with(".app")).count(),
            1
        );
    }

    #[test]
    fn duplicate_makes_numbered_copies() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "codex/characters/架純.md", "中身").unwrap();

        let first = duplicate(&root, "codex/characters/架純.md").unwrap();
        assert_eq!(first, "codex/characters/架純 のコピー.md");
        assert_eq!(fs::read_to_string(root.join(&first)).unwrap(), "中身");

        let second = duplicate(&root, "codex/characters/架純.md").unwrap();
        assert_eq!(second, "codex/characters/架純 のコピー2.md");
        fs::remove_dir_all(root).ok();
    }

    /// フォルダーを改名したら、配下を指すリンクも追随すること。
    /// 兄弟フォルダー(接頭辞が同じだけ)は巻き込まないこと
    #[test]
    fn folder_rename_follows_links_of_descendants() {
        let text = "[a](../codex/characters/悠二.md) [b](../codex/characters2/x.md) [c](../codex/characters)";
        let out = replace_links(text, "manuscript", "codex/characters", "codex/人物");
        assert!(out.contains("../codex/人物/悠二.md"), "配下が追随していない: {out}");
        assert!(out.contains("../codex/characters2/x.md"), "兄弟を巻き込んだ: {out}");
        assert!(out.contains("../codex/人物)"), "フォルダー自身が追随していない: {out}");
    }

    /// 同じ秒に同じ相対パスを2度捨てても、先に退避したものを踏まないこと。
    /// **Windows の `fs::rename` は黙って上書きする**ので、ここが抜けると退避の意味が消える
    #[test]
    fn trashing_the_same_path_twice_keeps_both() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "manuscript/01.md", "一度目").unwrap();
        let first = trash(&root, "manuscript/01.md").unwrap();
        create_file(&root, "manuscript/01.md", "二度目").unwrap();
        let second = trash(&root, "manuscript/01.md").unwrap();

        assert_ne!(first, second, "退避先が同じになっている");
        assert_eq!(fs::read_to_string(&first).unwrap(), "一度目");
        assert_eq!(fs::read_to_string(&second).unwrap(), "二度目");
    }

    /// `./` を挟んでもアプリ専用領域と判定できること。
    /// すり抜けると、退避したものを改名で踏める
    #[test]
    fn app_area_is_detected_through_dot_segments() {
        assert!(is_app_area(".app/trash/x.md"));
        assert!(is_app_area("./.app/trash/x.md"));
        assert!(is_app_area(".app"));
        assert!(!is_app_area("manuscript/.app風/x.md"));
    }

    /// 改名・複製・フォルダー作成もアプリ専用領域を触れないこと。
    /// **保存経路だけ塞いでも、別の呼び出しから抜けられては意味がない**
    #[test]
    fn app_area_is_closed_for_move_and_copy_too() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "manuscript/01.md", "原稿").unwrap();
        let trashed = trash(&root, "manuscript/01.md").unwrap();
        let rel = std::path::Path::new(&trashed)
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace(char::from(92), "/");

        create_file(&root, "manuscript/02.md", "べつの原稿").unwrap();
        // 原稿をアプリ専用領域へ押し込めない(`./` で回り込むのも塞ぐ)
        assert!(rename(&root, "manuscript/02.md", ".app/backups/02.md").is_err());
        assert!(rename(&root, "manuscript/02.md", "./.app/backups/02.md").is_err());
        // 退避したものを取り出す・複製する・その中にフォルダーを作る、も塞ぐ
        assert!(rename(&root, &rel, "manuscript/戻し.md").is_err());
        assert!(duplicate(&root, &rel).is_err());
        assert!(create_dir(&root, ".app/新しい").is_err());
        // 退避した中身は無傷
        assert_eq!(fs::read_to_string(&trashed).unwrap(), "原稿");
        assert!(resolve(&root, "manuscript/02.md").unwrap().exists());
    }

    #[test]
    fn stale_check_only_fires_on_a_real_mismatch() {
        // 時刻が一致していれば書いてよい
        assert!(!is_stale(1_000, Some(1_000)));
        // 外部で書き換えられた
        assert!(is_stale(2_000, Some(1_000)));
        // 期待値を持たない経路は素通し
        assert!(!is_stale(2_000, None));
        // **0 は「読めなかった」の印。** 照合すると保存が一切通らなくなる
        assert!(!is_stale(2_000, Some(0)));
    }

    #[test]
    fn timestamp_dir_is_readable() {
        // 2026-07-26 12:34:56 UTC
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_785_069_296);
        assert_eq!(timestamp_dir(t), "2026-07-26_123456");
        // エポック
        assert_eq!(timestamp_dir(std::time::UNIX_EPOCH), "1970-01-01_000000");
    }

    #[test]
    fn reads_utf8_with_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("本文".as_bytes());
        assert_eq!(decode(&bytes, "x").unwrap(), "本文");
    }
}
