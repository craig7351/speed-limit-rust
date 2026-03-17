//! 設定檔管理 - JSON 格式的規則匯入匯出
//! 設定檔存放於 exe 同目錄下的 speed-limit-config.json

use crate::traffic_shaper::ProcessRule;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONFIG_FILENAME: &str = "speed-limit-config.json";

/// 應用程式設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 全域下載限速 (Mbps)
    pub download_limit_mbps: f64,
    /// 全域上傳限速 (Mbps)
    pub upload_limit_mbps: f64,
    /// Per-process 限速規則
    pub process_rules: Vec<ProcessRule>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            download_limit_mbps: 0.0,
            upload_limit_mbps: 0.0,
            process_rules: Vec::new(),
        }
    }
}

/// 取得設定檔路徑（exe 同目錄）
fn config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(CONFIG_FILENAME)))
        .unwrap_or_else(|| PathBuf::from(CONFIG_FILENAME))
}

/// 載入設定檔，若不存在或格式錯誤則回傳預設值
pub fn load_config() -> AppConfig {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("設定檔格式錯誤，使用預設值: {}", e);
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    }
}

/// 儲存設定檔
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    let json = serde_json::to_string_pretty(config).map_err(|e| format!("序列化失敗: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("寫入設定檔失敗: {}", e))
}
