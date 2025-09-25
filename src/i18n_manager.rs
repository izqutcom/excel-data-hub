use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use serde_json::Value;
use tracing::{info, warn, error, debug};
use axum::http::HeaderMap;

/// 语言信息结构
#[derive(Debug, Clone)]
pub struct LanguageInfo {
    pub code: String,
    pub name: String,
    pub native_name: String,
    pub is_rtl: bool,
}

/// 翻译缓存项
#[derive(Debug, Clone)]
struct CacheItem {
    value: String,
    timestamp: u64,
}

/// 多语言管理器
pub struct I18nManager {
    /// 默认语言
    default_language: String,
    /// 语言包路径
    locales_path: String,
    /// 支持的语言列表
    supported_languages: Vec<String>,
    /// 是否启用自动检测
    auto_detect: bool,
    /// 是否启用缓存
    cache_enabled: bool,
    /// 缓存过期时间（分钟）
    cache_expire_minutes: u64,
    /// 是否启用多语言功能
    multilingual_enabled: bool,
    /// 语言包数据
    translations: Arc<RwLock<HashMap<String, HashMap<String, Value>>>>,
    /// 翻译缓存
    cache: Arc<RwLock<HashMap<String, CacheItem>>>,
    /// 语言信息
    language_info: HashMap<String, LanguageInfo>,
}

impl I18nManager {
    /// 创建新的多语言管理器
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let default_language = std::env::var("DEFAULT_LANGUAGE").unwrap_or_else(|_| "zh".to_string());
        let locales_path = std::env::var("LOCALES_PATH").unwrap_or_else(|_| "./locales".to_string());
        let supported_languages: Vec<String> = std::env::var("SUPPORTED_LANGUAGES")
            .unwrap_or_else(|_| "zh,en,ar,ug".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        let auto_detect = std::env::var("ENABLE_AUTO_DETECT")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true);
        let cache_enabled = std::env::var("CACHE_TRANSLATIONS")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true);
        let cache_expire_minutes = std::env::var("CACHE_EXPIRE_MINUTES")
            .unwrap_or_else(|_| "60".to_string())
            .parse()
            .unwrap_or(60);
        let multilingual_enabled = std::env::var("ENABLE_MULTILINGUAL")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true);

        let mut manager = Self {
            default_language,
            locales_path,
            supported_languages,
            auto_detect,
            cache_enabled,
            cache_expire_minutes,
            multilingual_enabled,
            translations: Arc::new(RwLock::new(HashMap::new())),
            cache: Arc::new(RwLock::new(HashMap::new())),
            language_info: Self::init_language_info(),
        };

        // 加载所有语言包
        manager.load_all_translations()?;
        
        info!("多语言管理器初始化完成，支持语言: {:?}", manager.supported_languages);
        Ok(manager)
    }

    /// 初始化语言信息
    fn init_language_info() -> HashMap<String, LanguageInfo> {
        let mut info = HashMap::new();
        
        info.insert("zh".to_string(), LanguageInfo {
            code: "zh".to_string(),
            name: "Chinese".to_string(),
            native_name: "中文".to_string(),
            is_rtl: false,
        });
        
        info.insert("en".to_string(), LanguageInfo {
            code: "en".to_string(),
            name: "English".to_string(),
            native_name: "English".to_string(),
            is_rtl: false,
        });
        
        info.insert("ar".to_string(), LanguageInfo {
            code: "ar".to_string(),
            name: "Arabic".to_string(),
            native_name: "العربية".to_string(),
            is_rtl: true,
        });
        
        info.insert("ug".to_string(), LanguageInfo {
            code: "ug".to_string(),
            name: "Uyghur".to_string(),
            native_name: "ئۇيغۇرچە".to_string(),
            is_rtl: true,
        });
        
        info
    }

    /// 加载所有语言包
    pub fn load_all_translations(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut translations = self.translations.write().unwrap();
        translations.clear();

        for lang in &self.supported_languages {
            match self.load_language_pack(lang) {
                Ok(data) => {
                    translations.insert(lang.clone(), data);
                    debug!("成功加载语言包: {}", lang);
                }
                Err(e) => {
                    warn!("加载语言包失败 {}: {}", lang, e);
                }
            }
        }

        info!("已加载 {} 个语言包", translations.len());
        Ok(())
    }

    /// 加载单个语言包
    fn load_language_pack(&self, lang: &str) -> Result<HashMap<String, Value>, Box<dyn std::error::Error>> {
        let file_path = format!("{}/{}.json", self.locales_path, lang);
        
        if !Path::new(&file_path).exists() {
            return Err(format!("语言包文件不存在: {}", file_path).into());
        }

        let content = fs::read_to_string(&file_path)?;
        let data: Value = serde_json::from_str(&content)?;
        
        Ok(self.flatten_json(&data, ""))
    }

    /// 扁平化JSON结构
    fn flatten_json(&self, value: &Value, prefix: &str) -> HashMap<String, Value> {
        let mut result = HashMap::new();
        
        match value {
            Value::Object(map) => {
                for (key, val) in map {
                    let new_key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", prefix, key)
                    };
                    
                    if val.is_object() {
                        result.extend(self.flatten_json(val, &new_key));
                    } else {
                        result.insert(new_key, val.clone());
                    }
                }
            }
            _ => {
                result.insert(prefix.to_string(), value.clone());
            }
        }
        
        result
    }

    /// 获取翻译文本
    pub fn translate(&self, key: &str, lang: &str, params: Option<&HashMap<String, String>>) -> String {
        // 检查缓存
        if self.cache_enabled {
            let cache_key = format!("{}:{}", lang, key);
            if let Some(cached) = self.get_from_cache(&cache_key) {
                return self.replace_params(&cached, params);
            }
        }

        let translations = self.translations.read().unwrap();
        
        // 尝试获取指定语言的翻译
        let translation = if let Some(lang_data) = translations.get(lang) {
            lang_data.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
        } else {
            None
        };

        // 如果没有找到，尝试使用默认语言
        let result = translation.or_else(|| {
            if lang != self.default_language {
                translations.get(&self.default_language)
                    .and_then(|lang_data| lang_data.get(key))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        }).unwrap_or_else(|| {
            warn!("翻译键未找到: {} (语言: {})", key, lang);
            key.to_string()
        });

        // 缓存结果
        if self.cache_enabled {
            let cache_key = format!("{}:{}", lang, key);
            self.set_cache(&cache_key, &result);
        }

        self.replace_params(&result, params)
    }

    /// 替换参数占位符
    fn replace_params(&self, text: &str, params: Option<&HashMap<String, String>>) -> String {
        if let Some(params) = params {
            let mut result = text.to_string();
            for (key, value) in params {
                result = result.replace(&format!("{{{}}}", key), value);
            }
            result
        } else {
            text.to_string()
        }
    }

    /// 从缓存获取
    fn get_from_cache(&self, key: &str) -> Option<String> {
        let cache = self.cache.read().unwrap();
        if let Some(item) = cache.get(key) {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            if now - item.timestamp < self.cache_expire_minutes * 60 {
                return Some(item.value.clone());
            }
        }
        None
    }

    /// 设置缓存
    fn set_cache(&self, key: &str, value: &str) {
        let mut cache = self.cache.write().unwrap();
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        cache.insert(key.to_string(), CacheItem {
            value: value.to_string(),
            timestamp,
        });
    }

    /// 清理过期缓存
    pub fn cleanup_cache(&self) {
        let mut cache = self.cache.write().unwrap();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let expire_time = self.cache_expire_minutes * 60;
        
        cache.retain(|_, item| now - item.timestamp < expire_time);
        debug!("缓存清理完成，剩余项目: {}", cache.len());
    }

    /// 从HTTP头部检测语言
    pub fn detect_language_from_headers(&self, headers: &HeaderMap) -> String {
        if !self.auto_detect {
            return self.default_language.clone();
        }

        if let Some(accept_language) = headers.get("accept-language") {
            if let Ok(accept_language_str) = accept_language.to_str() {
                for lang_range in accept_language_str.split(',') {
                    let lang = lang_range.split(';').next().unwrap_or("").trim();
                    let lang_code = lang.split('-').next().unwrap_or("").to_lowercase();
                    
                    if self.supported_languages.contains(&lang_code) {
                        debug!("检测到语言: {}", lang_code);
                        return lang_code;
                    }
                }
            }
        }

        self.default_language.clone()
    }

    /// 获取支持的语言列表
    pub fn get_supported_languages(&self) -> Vec<LanguageInfo> {
        self.supported_languages.iter()
            .filter_map(|code| self.language_info.get(code).cloned())
            .collect()
    }

    /// 获取语言信息
    pub fn get_language_info(&self, lang: &str) -> Option<&LanguageInfo> {
        self.language_info.get(lang)
    }

    /// 重新加载语言包
    pub fn reload_translations(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("重新加载语言包...");
        self.load_all_translations()?;
        
        // 清空缓存
        if self.cache_enabled {
            let mut cache = self.cache.write().unwrap();
            cache.clear();
            debug!("翻译缓存已清空");
        }
        
        Ok(())
    }

    /// 获取默认语言
    pub fn get_default_language(&self) -> &str {
        &self.default_language
    }

    /// 检查语言是否支持
    pub fn is_language_supported(&self, lang: &str) -> bool {
        self.supported_languages.contains(&lang.to_string())
    }

    /// 获取翻译总数
    pub fn get_total_translations(&self) -> usize {
        let translations = self.translations.read().unwrap();
        translations.values().map(|lang_map| lang_map.len()).sum()
    }

    /// 检查是否启用多语言功能
    pub fn is_multilingual_enabled(&self) -> bool {
        self.multilingual_enabled
    }

    /// 获取有效语言（如果多语言关闭，只返回默认语言）
    pub fn get_effective_language(&self, requested_lang: &str) -> String {
        if !self.multilingual_enabled {
            return self.default_language.clone();
        }
        
        if self.is_language_supported(requested_lang) {
            requested_lang.to_string()
        } else {
            self.default_language.clone()
        }
    }

    /// 获取有效的支持语言列表（如果多语言关闭，只返回默认语言）
    pub fn get_effective_supported_languages(&self) -> Vec<LanguageInfo> {
        if !self.multilingual_enabled {
            if let Some(default_info) = self.language_info.get(&self.default_language) {
                return vec![default_info.clone()];
            }
            return vec![];
        }
        
        self.get_supported_languages()
    }
}