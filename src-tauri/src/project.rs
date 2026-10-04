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
    /// Windows で名前として使えない(理由は文に含める)
    #[error("{0}")]
    BadName(String),
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

/// ファイルを読む。**UTF-8(BOM有無)だけを受け入れる。**
///
/// 03-data-format.md §4.4 は Shift_JIS の受容も挙げているが、実装はしていない。
/// 判別を誤ると本文が静かに壊れるので、読めないものは読めないと言う方を採る
/// (既存原稿の取り込みは Q-4 で決まってから)。
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

/// アプリ専用領域(`.app/`)を指しているか。
///
/// ここはバックアップ・ゴミ箱・ログ・索引の置き場で、**アプリが書き込むのは
/// それぞれの専用処理からだけ**である。原稿の保存経路がここへ届いてはいけない。
/// 特にゴミ箱は「消さずに取っておいたもの」なので、上書きされると退避の意味が消える。
///
/// **名前は Windows が解決するとおりに比べる。** 大文字小文字を区別せず(`.APP` も同じ
/// フォルダー)、名前の末尾の点と空白を無視する(`.app.` も同じ)。文字列のまま比べて
/// いた間は、改名で `.APP/backups/x.md` と打てば原稿を専用領域へ押し込めた。
/// 8.3形式の短い名前(`APP~1` 等)までは見ない。人が打ち間違えて届く形ではない。
pub fn is_app_area(relative: &str) -> bool {
    let p = normalize_rel(relative);
    let first = p.split('/').next().unwrap_or("");
    first
        .trim_end_matches(['.', ' '])
        .eq_ignore_ascii_case(APP_DIR)
}

/// Windows の予約名(拡張子が付いていても使えない)
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// ファイル名・フォルダー名として使ってよいか。**作る・改名する前に必ず通す。**
///
/// 検証せずに書くと、Windows では次が起きた(2026-10-04 テスト計画 B1 でこの機械で確認):
/// - `第1話: 出会い.md` は「第1話」という空のファイルと**代替データストリーム**になり、
///   本文はツリーから見えない場所に書かれる
/// - 末尾の点・空白は、書き方によって残ったり黙って落とされたりする。残ると
///   エクスプローラーなど多くのツールで開けない
/// - `con.md`・`aux` などの予約名も、作れてしまうが多くのツールで扱えない
///
/// 日本語の原稿では全角の記号(:?)を使う人が多いので、断るときに代わりを示す。
pub fn check_name(relative: &str) -> Result<(), ProjectError> {
    for seg in relative.split(['/', '\\']).filter(|s| !s.is_empty() && *s != ".") {
        if let Some(c) = seg
            .chars()
            .find(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control())
        {
            let shown = if c.is_control() {
                "制御文字".to_string()
            } else {
                format!("「{c}」")
            };
            return Err(ProjectError::BadName(format!(
                "「{seg}」に、名前に使えない文字{shown}が含まれています。\
                 \\ / : * ? \" < > | は使えません(全角の :?などなら使えます)"
            )));
        }
        if seg.ends_with('.') || seg.ends_with(' ') {
            return Err(ProjectError::BadName(format!(
                "「{seg}」は末尾が点か空白です。Windows では末尾の点・空白を名前に使えません"
            )));
        }
        let stem = seg.split('.').next().unwrap_or("").trim_end();
        if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
            return Err(ProjectError::BadName(format!(
                "「{seg}」は Windows の予約名({stem})なので使えません"
            )));
        }
        if seg.encode_utf16().count() > 255 {
            return Err(ProjectError::BadName(format!(
                "名前が長すぎます({}字)。255字以内にしてください",
                seg.chars().count()
            )));
        }
    }
    Ok(())
}

fn reject_app_area(relative: &str) -> Result<(), ProjectError> {
    if is_app_area(relative) {
        return Err(ProjectError::OutsideProject(format!(
            "{relative}(アプリ専用領域には書き込めません)"
        )));
    }
    Ok(())
}

/// 改行を LF に揃える。**画面へ渡す本文は必ずこれを通す。**
///
/// エディタ(CodeMirror)は改行を LF に揃えて持つ。CRLF のまま渡すと、本文の文字位置
/// (校正・レビューの指摘位置)がエディタ上の位置と1行につき1字ずつずれ、
/// 「置換」が別の文字を書き換える。
pub fn to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// 画面から来た本文(LF)を保存する。**既存のファイルが CRLF なら CRLF で書く**
/// (03 §4.4「ファイル単位で既存の改行コードを保持」)。新しいファイルは LF。
///
/// 以前は画面の LF のまま書いていたため、ほかのエディタで作った CRLF の原稿は
/// 一度編集しただけで全行の改行が変わり、git の差分が全行になった。
pub fn save_text(root: &Path, relative: &str, text: &str) -> Result<(), ProjectError> {
    let lf = to_lf(text);
    let crlf = resolve(root, relative)
        .ok()
        .and_then(|p| fs::read(p).ok())
        .is_some_and(|b| b.windows(2).any(|w| w == b"\r\n"));
    let content = if crlf { lf.replace('\n', "\r\n") } else { lf };
    write_text(root, relative, &content)
}

/// 保存する。**保存前に1世代のバックアップを取る**(M-01 / MVP要素5)。
///
/// 履歴管理はしない。同じファイルの前回内容だけを `.app/backups/` に残す。
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
    check_name(relative)?;
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
    check_name(to)?;
    let src = resolve(root, from)?;
    let dst = resolve(root, to)?;
    if !src.exists() {
        return Err(ProjectError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("見つかりません: {from}"),
        )));
    }
    // 大文字小文字だけの改名(`a.md` → `A.md`)は、Windows では改名先が「ある」と見える。
    // 同じものを指しているなら衝突ではない。別のものなら上書きしない
    if dst.exists() && !same_entry(&src, &dst) {
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

/// 2つのパスがディスク上の同じものを指しているか。
///
/// 大文字小文字を区別しないファイルシステムで、`a.md` と `A.md` を同じと見分けるためだけに使う
/// (パスの検証には使わない=§9)。解決できなければ「別のもの」として扱い、上書きしない側に倒す
fn same_entry(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
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
    check_name(relative)?;
    let path = resolve(root, relative)?;
    if path.exists() {
        return Ok(false);
    }
    fs::create_dir_all(path)?;
    Ok(true)
}

/// 改名・移動に追随して、プロジェクト内の相対リンクを書き換える(03 §5-6)。
///
/// `](相対パス)` 形式のみ扱う。直すのは2種類:
///  - **リンク先が動いた**もの(他のファイルから、改名したファイル・フォルダーを指すリンク)
///  - **リンク元が動いた**もの(動かしたファイル・フォルダーの中から外を指すリンク)。
///    深い階層へ移すと `../` の数が変わる
///
/// 書き換える前に、保存と同じく1世代のバックアップを取る。
/// 一度に多くのファイルへ書くので、取り違えたときに戻せるようにしておく。
fn rewrite_links(root: &Path, old_rel: &str, new_rel: &str) -> Result<(), ProjectError> {
    let mut targets = Vec::new();
    collect_md(root, &mut targets)?;
    for file in targets {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let rel = rel_string(root, &file);
        // 動いたファイルなら、動く前の場所を復元する(リンクはそこから書かれている)
        let before = if rel == new_rel {
            old_rel.to_string()
        } else if let Some(rest) = rel.strip_prefix(&format!("{new_rel}/")) {
            format!("{old_rel}/{rest}")
        } else {
            rel.clone()
        };
        let replaced = replace_links(&text, parent_dir(&before), parent_dir(&rel), old_rel, new_rel);
        if replaced != text {
            backup(root, &rel, &file)?;
            fs::write(&file, replaced)?;
        }
    }
    Ok(())
}

/// プロジェクト相対パスの親フォルダー(直下なら空)
fn parent_dir(rel: &str) -> &str {
    rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), ProjectError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_md(&path, out)?;
        } else if path.extension().map(|e| e == "md").unwrap_or(false) {
            out.push(path);
        }
    }
    Ok(())
}

/// 本文中のリンクを、改名・移動に合わせて書き換える。純関数(テスト用に分離)。
///
/// このファイルは `old_dir` にあり、いまは `new_dir` にある(動いていなければ同じ)。
/// リンクは**動く前の場所から**解決する。いまの場所から解決すると、
/// 動いたファイルの `../x.md` が別のファイルを指して見え、取り違えて書き換える。
///
/// 書き換えるのは、元の書き方のままでは**同じ先を指さなくなる**リンクだけ。
/// 指し続けるものは書き方(`./` の有無など)も含めて残す。
pub fn replace_links(
    text: &str,
    old_dir: &str,
    new_dir: &str,
    old_rel: &str,
    new_rel: &str,
) -> String {
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
        match normalize_join(old_dir, target) {
            Some(was) => {
                // 完全一致(ファイルの改名)と、接頭辞一致(**フォルダーの改名**)の両方を見る。
                // 完全一致だけだと `codex/characters` を改名しても
                // `codex/characters/悠二.md` を指すリンクが切れたまま残る。
                // `old_rel` の直後に `/` を要求するので、兄弟の `codex/characters2` は巻き込まない
                let dest = if was == old_rel {
                    new_rel.to_string()
                } else if let Some(rest) = was.strip_prefix(&format!("{old_rel}/")) {
                    format!("{new_rel}/{rest}")
                } else {
                    was
                };
                if normalize_join(new_dir, target).as_deref() == Some(dest.as_str()) {
                    out.push_str(target);
                } else {
                    out.push_str(&relative_from(new_dir, &dest));
                }
            }
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
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), ups).collect();
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

/// UNIXミリ秒 → `YYYY-MM-DD`(UTC)。AI呼び出しログの日別ファイル名に使う
pub fn date_dir_utc(at_ms: u128) -> String {
    let days = (at_ms / 86_400_000) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
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

/// 読み込んだ後に外部で書き換えられたか(楽観ロック=T-08)。
///
/// `expected` が無ければ照合しない(新規作成直後など、まだ時刻を持たない経路)。
/// **0 は「時刻が読めなかった」の印**なので照合に使わない。ここを弾かないと、
/// 時刻を読めない環境で保存が毎回競合になり、一切書けなくなる。
pub fn is_stale(actual_ms: u64, expected_ms: Option<u64>) -> bool {
    matches!(expected_ms, Some(e) if e != 0 && e != actual_ms)
}

/// 外部編集の検知に使う更新時刻(エポックからのミリ秒)。
///
/// 常駐監視はしない。読み込み時に控え、保存時に照合する(`is_stale`)。
/// フォーカス復帰時の確認は、フロントが読み直した中身と見比べる(03 §5-1)。
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

    /// 大文字小文字だけの改名ができること。Windows では改名先が「既にある」と見えるため、
    /// 以前は「同名のファイルが既にあります」で断られていた
    #[test]
    fn renaming_only_the_letter_case_works() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "manuscript/scene.md", "本文").unwrap();
        create_file(&root, "manuscript/index.md", "[場面](scene.md)\n").unwrap();

        rename(&root, "manuscript/scene.md", "manuscript/Scene.md").unwrap();

        let names: Vec<String> = fs::read_dir(root.join("manuscript"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"Scene.md".to_string()), "{names:?}");
        assert!(!names.contains(&"scene.md".to_string()), "{names:?}");
        assert_eq!(
            fs::read_to_string(root.join("manuscript/Scene.md")).unwrap(),
            "本文"
        );
        assert_eq!(
            fs::read_to_string(root.join("manuscript/index.md")).unwrap(),
            "[場面](Scene.md)\n"
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
        let out = replace_links(
            text,
            "manuscript",
            "manuscript",
            "codex/characters",
            "codex/人物",
        );
        assert!(out.contains("../codex/人物/悠二.md"), "配下が追随していない: {out}");
        assert!(out.contains("../codex/characters2/x.md"), "兄弟を巻き込んだ: {out}");
        assert!(out.contains("../codex/人物)"), "フォルダー自身が追随していない: {out}");
    }

    /// 深い階層へ移したファイルの中のリンクは `../` の数を直す(03 §5-6)。
    /// 自分自身へのリンクのように、そのままで同じ先を指すものは書き方ごと残す
    #[test]
    fn moving_a_file_deeper_keeps_its_own_links() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "codex/characters/悠二.md", "幼馴染\n").unwrap();
        create_file(
            &root,
            "manuscript/01.md",
            "[悠二](../codex/characters/悠二.md) [ここ](01.md) [外](https://example.com)\n",
        )
        .unwrap();

        rename(&root, "manuscript/01.md", "manuscript/第一章/01.md").unwrap();

        assert_eq!(
            fs::read_to_string(root.join("manuscript/第一章/01.md")).unwrap(),
            "[悠二](../../codex/characters/悠二.md) [ここ](01.md) [外](https://example.com)\n"
        );
        // 書き換えた分は控えが残る(保存と同じ1世代)
        assert!(root.join(".app/backups/manuscript/第一章/01.md").exists());
        fs::remove_dir_all(root).ok();
    }

    /// フォルダーごと深い階層へ移したら、中から外へのリンクだけ直し、中どうしのリンクは残す
    #[test]
    fn moving_a_folder_deeper_keeps_links_inside_and_out() {
        let root = tmp();
        init(&root).unwrap();
        create_file(&root, "codex/locations/青葉高校.md", "舞台\n").unwrap();
        create_file(&root, "codex/characters/悠二.md", "幼馴染\n").unwrap();
        create_file(
            &root,
            "codex/characters/架純.md",
            "[悠二](悠二.md) [悠二](./悠二.md) [高校](../locations/青葉高校.md)\n",
        )
        .unwrap();
        create_file(
            &root,
            "manuscript/01.md",
            "[架純](../codex/characters/架純.md)\n",
        )
        .unwrap();

        rename(&root, "codex/characters", "codex/人物/主要").unwrap();

        assert_eq!(
            fs::read_to_string(root.join("codex/人物/主要/架純.md")).unwrap(),
            "[悠二](悠二.md) [悠二](./悠二.md) [高校](../../locations/青葉高校.md)\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("manuscript/01.md")).unwrap(),
            "[架純](../codex/人物/主要/架純.md)\n"
        );
        fs::remove_dir_all(root).ok();
    }

    /// 同じ深さでのフォルダー改名では、中のファイルは1文字も変えない
    #[test]
    fn same_depth_folder_rename_leaves_inner_links_alone() {
        let root = tmp();
        init(&root).unwrap();
        let inner = "[悠二](悠二.md) [悠二](./悠二.md) [高校](../locations/青葉高校.md)\n";
        create_file(&root, "codex/characters/悠二.md", "幼馴染\n").unwrap();
        create_file(&root, "codex/characters/架純.md", inner).unwrap();

        rename(&root, "codex/characters", "codex/人物").unwrap();

        assert_eq!(
            fs::read_to_string(root.join("codex/人物/架純.md")).unwrap(),
            inner
        );
        // 書き換えていないので控えも作らない
        assert!(!root.join(".app/backups/codex/人物/架純.md").exists());
        fs::remove_dir_all(root).ok();
    }

    /// 動いたファイルのリンクは、**動く前の場所から**解決する。
    /// いまの場所から解決すると、別のファイルを指していた `../01.md` を
    /// 改名したファイル自身へのリンクと取り違える
    #[test]
    fn links_of_a_moved_file_are_read_from_where_it_was() {
        // manuscript/01.md を manuscript/a/01.md へ移した。中の `../01.md` は
        // もともと直下の 01.md を指していた(manuscript/01.md ではない)
        let got = replace_links(
            "[直下](../01.md)",
            "manuscript",
            "manuscript/a",
            "manuscript/01.md",
            "manuscript/a/01.md",
        );
        assert_eq!(got, "[直下](../../01.md)");
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

    /// Windows は名前の大文字小文字を区別せず、末尾の点と空白を無視する。
    /// 文字列のまま比べると `.APP/...` や `.app./...` が同じフォルダーへ届く
    #[test]
    fn app_area_is_detected_the_way_windows_resolves_names() {
        assert!(is_app_area(".APP/trash/x.md"));
        assert!(is_app_area(".App/backups/x.md"));
        assert!(is_app_area(".app./trash/x.md"));
        assert!(is_app_area(".app /trash/x.md"));
        assert!(is_app_area("./.APP"));
        assert!(is_app_area(".APP\\backups\\x.md"));
        // 似ているだけの名前と、下の階層にある同名は巻き込まない
        assert!(!is_app_area("..app/x.md"));
        assert!(!is_app_area(".apps/x.md"));
        assert!(!is_app_area("manuscript/.APP/x.md"));
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
        // 大文字や末尾の点で書いても、Windows では同じ場所を指す
        assert!(rename(&root, "manuscript/02.md", ".APP/backups/02.md").is_err());
        assert!(rename(&root, "manuscript/02.md", ".app./backups/02.md").is_err());
        assert!(create_file(&root, ".APP/trash/新しい.md", "x").is_err());
        assert!(write_text(&root, ".App/settings.json", "{}").is_err());
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

    /// 画面へ渡す本文は改行を LF に揃える。CRLF のままだと、指摘の位置がエディタ上の
    /// 位置と1行につき1字ずれ、「置換」が別の文字を書き換える
    #[test]
    fn text_for_the_editor_uses_lf_only() {
        assert_eq!(to_lf("一\r\n二\r\n"), "一\n二\n");
        assert_eq!(to_lf("一\r二"), "一\n二");
        assert_eq!(to_lf("一\n二"), "一\n二");
    }

    /// 保存はファイルごとに元の改行コードを保つ(03 §4.4)。新しいファイルは LF
    #[test]
    fn saving_keeps_each_files_line_endings() {
        let root = tmp();
        init(&root).unwrap();
        fs::write(root.join("manuscript/crlf.md"), "一\r\n二\r\n").unwrap();
        create_file(&root, "manuscript/lf.md", "一\n二\n").unwrap();

        save_text(&root, "manuscript/crlf.md", "一\n二\n三\n").unwrap();
        save_text(&root, "manuscript/lf.md", "一\n二\n三\n").unwrap();
        save_text(&root, "manuscript/new.md", "一\n二\n").unwrap();

        assert_eq!(
            fs::read(root.join("manuscript/crlf.md")).unwrap(),
            "一\r\n二\r\n三\r\n".as_bytes()
        );
        assert_eq!(
            fs::read(root.join("manuscript/lf.md")).unwrap(),
            "一\n二\n三\n".as_bytes()
        );
        assert_eq!(
            fs::read(root.join("manuscript/new.md")).unwrap(),
            "一\n二\n".as_bytes()
        );
        // 読み直した本文は LF に揃うので、画面が持っている本文と一致する(競合と誤判定しない)
        assert_eq!(
            to_lf(&read_text(&root, "manuscript/crlf.md").unwrap()),
            "一\n二\n三\n"
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
