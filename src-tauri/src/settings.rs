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
    /// この版が知らない項目(新しい版が足したもの・手で足したもの)。
    /// **書き直すときに消さない**(テスト計画 H1。以前は保存のたびに落としていた)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    parse(&text)
}

/// 設定ファイルの中身を読む。**項目ごとに読み**、型の合わない項目だけを既定に戻す。
///
/// 以前は1つでも型の合わない項目があると全体を読めない扱いにしていたため、
/// 手で直したときの書き間違い1つで、次の保存のときに接続先・モデル・前回のプロジェクトまで
/// 既定へ戻った(テスト計画 H1)。JSON として読めない・オブジェクトでなければ None
pub fn parse(text: &str) -> Option<StoredSettings> {
    let serde_json::Value::Object(mut obj) = serde_json::from_str(text).ok()? else {
        return None;
    };
    fn take(obj: &mut serde_json::Map<String, serde_json::Value>, key: &str) -> Option<serde_json::Value> {
        obj.remove(key)
    }
    let text_of = |v: Option<serde_json::Value>| v.and_then(|v| v.as_str().map(str::to_string));
    let base_url = text_of(take(&mut obj, "base_url"))
        .unwrap_or_else(|| crate::ai::DEFAULT_BASE_URL.to_string());
    let model = text_of(take(&mut obj, "model")).unwrap_or_default();
    let temperature = take(&mut obj, "temperature")
        .and_then(|v| v.as_f64())
        .map(|t| t as f32)
        .unwrap_or(0.7);
    let check_chunk_chars = take(&mut obj, "check_chunk_chars")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(crate::proofread::CHUNK_CHARS);
    let last_project = text_of(take(&mut obj, "last_project"));
    Some(StoredSettings {
        base_url,
        model,
        temperature,
        check_chunk_chars,
        last_project,
        // 知っている項目は取り除いた。残りは知らない項目としてそのまま持つ
        extra: obj,
    })
}

/// 保存する。**失敗は呼び出し元へ返す**(読み取り専用などで書けないとき、本人に知らせる)
pub fn save(s: &StoredSettings) -> std::io::Result<()> {
    save_to(&settings_path(), s)
}

/// 指定した場所へ保存する(テストは本物の設定ファイルに触れないよう、これを使う)
pub fn save_to(path: &std::path::Path, s: &StoredSettings) -> std::io::Result<()> {
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
    if let Some(s) = stored {
        next.last_project = last_project.or(s.last_project);
        // 知らない項目も引き継ぐ(新しい版の設定を古い版の保存で消さない)
        next.extra = s.extra;
    } else {
        next.last_project = last_project;
    }
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
            extra: Default::default(),
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
            extra: Default::default(),
        };
        let json = serde_json::to_string(&s).unwrap();
        // APIキーに相当するものが混ざっていないこと
        assert!(!json.contains("api_key"), "秘密情報が保存対象に入っている: {json}");
        let back: StoredSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.check_chunk_chars, 6_000);
        assert_eq!(back.model, "some-model");
    }

    /// テスト計画 H1: この版が知らない項目は、保存し直しても消さない
    /// (新しい版が足した設定を、古い版の保存で落とさない)
    #[test]
    fn unknown_fields_survive_a_save() {
        let text = r#"{"base_url":"http://localhost:1234/v1","model":"m","temperature":0.7,
            "check_chunk_chars":3000,"future_option":{"a":1},"note":"手で足した"}"#;
        let stored = parse(text).unwrap();
        let mut next = sample();
        next.model = "別のモデル".into();
        let merged = merge(Some(stored), next, None);
        let json = serde_json::to_string(&merged).unwrap();
        let back = parse(&json).unwrap();
        assert_eq!(back.model, "別のモデル");
        assert_eq!(back.extra.get("future_option"), Some(&serde_json::json!({"a": 1})));
        assert_eq!(back.extra.get("note"), Some(&serde_json::json!("手で足した")));
    }

    /// 型の合わない項目が1つあっても、ほかの項目は失わない(その項目だけ既定に戻す)
    #[test]
    fn one_bad_field_does_not_lose_the_others() {
        let text = r#"{"base_url":"http://192.168.0.5:1234/v1","model":"m","temperature":"熱い",
            "check_chunk_chars":3000,"last_project":"C:/novels/作品"}"#;
        let s = parse(text).unwrap();
        assert_eq!(s.base_url, "http://192.168.0.5:1234/v1");
        assert_eq!(s.model, "m");
        assert_eq!(s.temperature, 0.7, "型の合わない項目は既定");
        assert_eq!(s.check_chunk_chars, 3000);
        assert_eq!(s.last_project.as_deref(), Some("C:/novels/作品"));
        assert!(!s.extra.contains_key("temperature"), "知っている項目を二重に持たない");
    }

    /// JSON として読めない・オブジェクトでないものは読めない扱い(既定で起動する)
    #[test]
    fn unreadable_files_give_none() {
        for t in ["{壊れた", "[1,2]", "null", "123", ""] {
            assert!(parse(t).is_none(), "{t:?}");
        }
    }

    /// 書けなかったら失敗として返す(以前は握りつぶし、次の起動で黙って元に戻った)。
    /// **本物の設定ファイルには触れない**(使い捨ての場所で確かめる)
    #[test]
    fn save_failure_is_reported() {
        let dir = std::env::temp_dir().join(format!("kaku-settings-ro-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        save_to(&path, &sample()).unwrap();
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&path, perm.clone()).unwrap();
        assert!(save_to(&path, &sample()).is_err(), "読み取り専用なのに書けたことになった");
        #[allow(clippy::permissions_set_readonly_false)] // 後片付け(使い捨ての場所)
        perm.set_readonly(false);
        std::fs::set_permissions(&path, perm).unwrap();
        std::fs::remove_dir_all(dir).ok();
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
