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

/// ツリーを走査する。`.app/` と隠しフォルダは対象外(人間が触る領域だけを見せる)。
pub fn scan(root: &Path) -> Result<Vec<TreeNode>, ProjectError> {
    scan_dir(root, root)
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
pub fn write_text(root: &Path, relative: &str, content: &str) -> Result<(), ProjectError> {
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
    let path = resolve(root, relative)?;
    if path.exists() {
        return Ok(false);
    }
    write_new(&path, content)?;
    Ok(true)
}

/// 外部編集の検知に使う更新時刻(エポックからのミリ秒)。
///
/// 常駐監視はしない。フロントがフォーカス復帰時に問い合わせる(§5-1)。
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

    fn tmp() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "kaku-test-{}-{}",
            std::process::id(),
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
    fn reads_utf8_with_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("本文".as_bytes());
        assert_eq!(decode(&bytes, "x").unwrap(), "本文");
    }
}
