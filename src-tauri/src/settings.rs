//! アプリ設定の保存(接続先・モデル・校正の分割字数)。
//!
//! 置き場所は設定フォルダ(`<config_dir>/kaku/settings.json`)。
//! プロジェクトの中には置かない — 接続先やモデルは作品ごとではなく環境ごとの設定であり、
//! 小説データ(P-5の正本)に混ぜるものではないため。
//!
//! **APIキーは保存しない**(M-07: 認証情報を平文で持たない)。
//! キーはメモリ上にのみ置き、起動ごとに入力する。OS資格情報ストアへの保存は
//! uggg の secrets.rs を流用して別途対応する。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 保存するもの。APIキーは**意図的に含めない**
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSettings {
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    /// 校正で1回に送る本文の文字数(PoC#7 §7.2)
    pub check_chunk_chars: usize,
    /// 前回開いたプロジェクトの場所。**小説データではなく環境の設定**なので、
    /// プロジェクトの中ではなくここに置く。無くても起動は止まらない
    #[serde(default)]
    pub last_project: Option<String>,
}

pub fn settings_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("kaku")
        .join("settings.json")
}

/// 読み込む。無い・壊れている場合は None を返して既定値に任せる(起動を止めない)
pub fn load() -> Option<StoredSettings> {
    let text = std::fs::read_to_string(settings_path()).ok()?;
    serde_json::from_str(&text).ok()
}

/// 保存する。失敗しても致命的ではないので呼び出し側で握りつぶしてよい
pub fn save(s: &StoredSettings) -> std::io::Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// 保存する内容を組み立てる。
///
/// 設定ファイルは丸ごと書き直すため、**部分的な知識で上書きすると他の項目が消える**。
/// 接続設定を保存しただけで前回のプロジェクトを忘れる、といった事故を防ぐのがここ。
/// `last_project` に `None` を渡した場合は、保存済みの値をそのまま引き継ぐ。
pub fn merge(
    stored: Option<StoredSettings>,
    mut next: StoredSettings,
    last_project: Option<String>,
) -> StoredSettings {
    next.last_project = last_project.or_else(|| stored.and_then(|s| s.last_project));
    next
}

/// 設定値を安全な範囲に収める。
///
/// 分割字数は、小さすぎると実行回数が増えて遅くなり、大きすぎると
/// モデルのコンテキストを超えて応答が打ち切られる(§7.1)。
pub fn clamp_chunk_chars(v: usize) -> usize {
    v.clamp(500, 12_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_chunk_chars_into_safe_range() {
        assert_eq!(clamp_chunk_chars(3_000), 3_000);
        assert_eq!(clamp_chunk_chars(0), 500);
        assert_eq!(clamp_chunk_chars(100_000), 12_000);
    }

    fn sample() -> StoredSettings {
        StoredSettings {
            base_url: "http://localhost:1234/v1".into(),
            model: "some-model".into(),
            temperature: 0.7,
            check_chunk_chars: 6_000,
            last_project: None,
        }
    }

    #[test]
    fn roundtrips_without_api_key() {
        let s = StoredSettings {
            base_url: "http://localhost:1234/v1".into(),
            model: "some-model".into(),
            temperature: 0.7,
            check_chunk_chars: 6_000,
            last_project: Some(r"C:\novels\作品".into()),
        };
        let json = serde_json::to_string(&s).unwrap();
        // APIキーに相当するものが混ざっていないこと
        assert!(!json.contains("api_key"), "秘密情報が保存対象に入っている: {json}");
        let back: StoredSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.check_chunk_chars, 6_000);
        assert_eq!(back.model, "some-model");
    }

    #[test]
    fn broken_file_falls_back_to_default() {
        // load() は壊れたJSONでも None を返すだけで落ちない
        assert!(serde_json::from_str::<StoredSettings>("{壊れた").is_err());
    }

    /// 前回のプロジェクトを覚える前に書かれた settings.json も読めること。
    /// 読めないと、更新した瞬間に接続設定まで既定へ戻る
    #[test]
    fn reads_settings_written_before_last_project_existed() {
        let old = r#"{"base_url":"http://localhost:1234/v1","model":"m","temperature":0.7,"check_chunk_chars":3000}"#;
        let s: StoredSettings = serde_json::from_str(old).unwrap();
        assert_eq!(s.last_project, None);
        assert_eq!(s.model, "m");
    }

    /// **接続設定の保存で前回のプロジェクトを消さないこと。**
    /// 設定ファイルを丸ごと書き直す以上、ここを外すと「開き直すと忘れている」に戻る
    #[test]
    fn saving_ai_settings_keeps_the_last_project() {
        let stored = StoredSettings {
            last_project: Some(r"C:\novels\作品".into()),
            ..sample()
        };
        let mut next = sample();
        next.model = "別のモデル".into();

        let merged = merge(Some(stored), next, None);
        assert_eq!(merged.model, "別のモデル");
        assert_eq!(merged.last_project.as_deref(), Some(r"C:\novels\作品"));
    }

    /// 開き直したときは新しいプロジェクトで上書きされること
    #[test]
    fn opening_a_project_replaces_the_remembered_one() {
        let stored = StoredSettings {
            last_project: Some(r"C:\novels\古い".into()),
            ..sample()
        };
        let merged = merge(Some(stored), sample(), Some(r"C:\novels\新しい".into()));
        assert_eq!(merged.last_project.as_deref(), Some(r"C:\novels\新しい"));
    }
}
