// user_store.rs - 用戶個人資料讀寫
//
// 對應 Python 的 src/user_store.py
// 每位用戶的資料存在 users/{chat_id}.json

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const USERS_DIR: &str = "users";

/// 用戶個人資料
///
/// # 對應 Python
/// ```python
/// DEFAULT_USER = {
///     "stocks": [],
///     "schedule": {"interval_enabled": True},
///     "alerts": {},
/// }
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserData {
    pub stocks: Vec<String>,                           // e.g. ["2330.TW", "6488.TWO"]
    pub schedule: UserSchedule,
    pub alerts: std::collections::HashMap<String, Vec<AlertCondition>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserSchedule {
    pub interval_enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlertCondition {
    #[serde(rename = "type")]
    pub kind: String,  // "price_below", "price_above", 等
    pub value: f64,
}

impl Default for UserData {
    fn default() -> Self {
        Self {
            stocks: vec![],
            schedule: UserSchedule { interval_enabled: true },
            alerts: std::collections::HashMap::new(),
        }
    }
}

fn user_path(chat_id: &str) -> PathBuf {
    PathBuf::from(USERS_DIR).join(format!("{}.json", chat_id))
}

/// 讀取用戶資料，不存在時回傳預設值
///
/// # 對應 Python
/// ```python
/// def get_user(chat_id: str) -> dict:
///     path = USERS_DIR / f"{chat_id}.json"
///     if not path.exists():
///         return copy.deepcopy(DEFAULT_USER)
///     with open(path) as f:
///         return json.load(f)
/// ```
pub fn get_user(chat_id: &str) -> UserData {
    let path = user_path(chat_id);
    if !path.exists() {
        return UserData::default();
    }
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return UserData::default(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// 寫入用戶資料，自動建立 users/ 目錄
///
/// # 對應 Python
/// ```python
/// def save_user(chat_id: str, data: dict):
///     USERS_DIR.mkdir(exist_ok=True)
///     with open(path, "w") as f:
///         json.dump(data, f, ...)
/// ```
pub fn save_user(chat_id: &str, data: &UserData) -> Result<()> {
    fs::create_dir_all(USERS_DIR)?;
    let content = serde_json::to_string_pretty(data)?;
    fs::write(user_path(chat_id), content)?;
    Ok(())
}
