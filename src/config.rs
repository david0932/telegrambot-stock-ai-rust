// config.rs - 設定檔載入與驗證
//
// 對應 Python 的 load_config() / save_config()
// 使用 serde 自動將 JSON 反序列化為 Rust struct

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

/// 對應 config.json 的結構
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub telegram_token: String,
    pub gemini_api_key: String,
    pub gemini_model: String,
    pub admin_ids: Vec<String>,
    pub allowed_users: Vec<String>,
    pub schedule: ScheduleConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScheduleConfig {
    pub interval_minutes: u32,
    pub key_times: Vec<String>, // e.g. ["09:30", "13:25"]
}

/// 從檔案載入並驗證設定
///
/// # 對應 Python
/// ```python
/// def load_config() -> dict:
///     with open(CONFIG_PATH) as f:
///         config = json.load(f)
///     ...
/// ```
pub fn load_config(path: &str) -> Result<Config> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("無法讀取設定檔：{path}"))?;

    let config: Config = serde_json::from_str(&content)
        .with_context(|| "config.json 格式錯誤")?;

    Ok(config)
}

/// 將設定寫回檔案（例如 /allow 新增用戶後更新 allowed_users）
///
/// # 對應 Python
/// ```python
/// def save_config(config: dict):
///     with open(CONFIG_PATH, "w") as f:
///         json.dump(config, f, ...)
/// ```
pub fn save_config(path: &str, config: &Config) -> Result<()> {
    let content = serde_json::to_string_pretty(config)?;
    fs::write(path, content)?;
    Ok(())
}

impl Config {
    pub fn is_admin(&self, chat_id: &str) -> bool {
        self.admin_ids.iter().any(|id| id == chat_id)
    }

    pub fn is_allowed(&self, chat_id: &str) -> bool {
        self.allowed_users.iter().any(|id| id == chat_id)
    }

    pub fn add_user(&mut self, chat_id: &str) {
        if !self.is_allowed(chat_id) {
            self.allowed_users.push(chat_id.to_string());
        }
    }

    pub fn remove_user(&mut self, chat_id: &str) {
        self.allowed_users.retain(|id| id != chat_id);
    }
}
