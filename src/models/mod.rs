pub mod entity;

// Re-export the main models from models.rs
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcelData {
    pub id: Option<i32>,
    pub file_id: i32,
    pub import_time: DateTime<Utc>,
    pub row_number: i32,
    pub data_json: String,
    pub search_text: String,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<ExcelData>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResponse {
    pub total_rows: i64,
    pub total_files: i64,
    pub last_update: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportStats {
    pub success: i64,
    pub failed: i64,
    pub total: i64,
    pub skipped: i64,
}

/// 语言响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageResponse {
    pub code: String,
    pub name: String,
    pub native_name: String,
    pub is_rtl: bool,
}

/// 翻译响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationResponse {
    pub key: String,
    pub value: String,
    pub language: String,
}

/// 批量翻译响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchTranslationResponse {
    pub translations: HashMap<String, String>,
    pub language: String,
}

/// 翻译请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationRequest {
    pub key: String,
    pub language: Option<String>,
    pub params: Option<HashMap<String, String>>,
}

/// 批量翻译请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchTranslationRequest {
    pub keys: Vec<String>,
    pub language: Option<String>,
    pub params: Option<HashMap<String, String>>,
}

/// 语言设置请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageSettingRequest {
    pub language: String,
}

/// 国际化状态响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I18nStatusResponse {
    pub default_language: String,
    pub supported_languages: Vec<LanguageResponse>,
    pub auto_detect_enabled: bool,
    pub cache_enabled: bool,
    pub total_translations: usize,
    pub multilingual_enabled: bool,
}