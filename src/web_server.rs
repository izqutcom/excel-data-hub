use crate::models::{
    AuthResponse, BatchTranslationRequest, BatchTranslationResponse, I18nStatusResponse, LanguageResponse, SearchResponse,
    StatsResponse, TranslationResponse, UserResponse, WorkspaceResponse,
};
use crate::models::entity::{auth_tokens, files, users, workspaces};
use crate::i18n_manager::I18nManager;
use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{StatusCode, header, HeaderMap},
    response::{Html, Response},
    routing::{get, post, put},
    Json, Router,
};
use tower_http::services::ServeDir;
use sea_orm::DatabaseConnection;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::fs;
use std::net::SocketAddr;
use std::path::Path as StdPath;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower_http::cors::CorsLayer;
use tracing::{debug, info, warn};
use uuid::Uuid;

#[derive(Clone)]
struct StatsCache {
    data: Option<StatsResponse>,
    last_updated: Option<Instant>,
    cache_duration: Duration,
}

impl StatsCache {
    fn new() -> Self {
        Self {
            data: None,
            last_updated: None,
            cache_duration: Duration::from_secs(300), // 5分钟缓存
        }
    }

    fn is_expired(&self) -> bool {
        match self.last_updated {
            Some(last_updated) => last_updated.elapsed() > self.cache_duration,
            None => true,
        }
    }

    fn update(&mut self, data: StatsResponse) {
        self.data = Some(data);
        self.last_updated = Some(Instant::now());
    }

    fn get(&self) -> Option<&StatsResponse> {
        if self.is_expired() {
            None
        } else {
            self.data.as_ref()
        }
    }
}

#[derive(Clone)]
struct AppState {
    db: DatabaseConnection,
    i18n_manager: Arc<Mutex<I18nManager>>,
    stats_cache: Arc<Mutex<StatsCache>>,
    upload_dir: String,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: Option<String>,
    workspace_id: Option<i32>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
pub struct StatsQuery {
    workspace_id: Option<i32>,
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
pub struct CreateWorkspaceRequest {
    name: String,
    description: Option<String>,
    is_public: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateWorkspaceRequest {
    name: Option<String>,
    description: Option<String>,
    is_public: Option<bool>,
}

fn hash_password(password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn bearer_token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
}

async fn authenticate_user(headers: &HeaderMap, db: &DatabaseConnection) -> Result<users::Model, (StatusCode, String)> {
    let token = bearer_token_from_headers(headers)
        .ok_or((StatusCode::UNAUTHORIZED, "缺少认证Token".to_string()))?;

    let now = chrono::Utc::now();
    let token_model = auth_tokens::Entity::find()
        .filter(auth_tokens::Column::Token.eq(token))
        .filter(auth_tokens::Column::ExpiresAt.gte(now))
        .one(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("查询Token失败: {}", e)))?
        .ok_or((StatusCode::UNAUTHORIZED, "Token无效或已过期".to_string()))?;

    users::Entity::find_by_id(token_model.user_id)
        .one(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("查询用户失败: {}", e)))?
        .ok_or((StatusCode::UNAUTHORIZED, "用户不存在".to_string()))
}

async fn get_workspace_by_id(
    db: &DatabaseConnection,
    workspace_id: i32,
) -> Result<workspaces::Model, (StatusCode, String)> {
    workspaces::Entity::find_by_id(workspace_id)
        .one(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("查询workspace失败: {}", e)))?
        .ok_or((StatusCode::NOT_FOUND, "workspace不存在".to_string()))
}

pub async fn start_server(db: DatabaseConnection, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "./uploads".to_string());
    if !StdPath::new(&upload_dir).exists() {
        fs::create_dir_all(&upload_dir).await?;
    }

    // 初始化多语言管理器
    info!("初始化多语言管理器...");
    let i18n_manager = Arc::new(Mutex::new(I18nManager::new()?));
    debug!("多语言管理器初始化完成");
    
    // 初始化统计缓存
    let stats_cache = Arc::new(Mutex::new(StatsCache::new()));
    
    // 配置CORS
    info!("配置CORS策略...");
    let cors = CorsLayer::very_permissive();
    debug!("CORS策略配置完成");
    
    // 创建应用状态
    let app_state = AppState {
        db: db.clone(),
        i18n_manager: i18n_manager.clone(),
        stats_cache: stats_cache.clone(),
        upload_dir,
    };
    
    // 创建路由
    info!("创建路由...");
    let app = Router::new()
        .route("/", get(home_handler))
        .route("/api/auth/register", post(register_handler))
        .route("/api/auth/login", post(login_handler))
        .route("/api/workspaces", get(list_workspaces_handler).post(create_workspace_handler))
        .route("/api/workspaces/{id}", put(update_workspace_handler).delete(delete_workspace_handler))
        .route("/api/workspaces/{id}/upload", post(upload_to_workspace_handler))
        .route("/api/search", get(search_handler))
        .route("/api/stats", get(stats_handler))
        .route("/api/export", get(export_handler))
        // 多语言API路由
        .route("/api/i18n/languages", get(get_languages_handler))
        .route("/api/i18n/status", get(get_i18n_status_handler))
        .route("/api/i18n/translate/{key}", get(translate_handler))
        .route("/api/i18n/batch_translate", post(batch_translate_handler))
        .route("/api/i18n/reload", post(reload_translations_handler))
        // 静态文件服务
        .nest_service("/static", ServeDir::new("static"))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .layer(cors)
        .with_state(app_state);
    debug!("路由创建完成");

    // 绑定地址
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("🚀 Web服务器正在启动，监听地址: {}", addr);
    debug!("正在绑定地址...");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    debug!("地址绑定成功");
    debug!("正在准备服务...");

    // 启动服务器
    debug!("服务准备完成，开始监听请求...");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn home_handler() -> Html<&'static str> {
    Html(r#"
<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title data-i18n="app.title">Excel Master Pro - Professional Data Search System</title>
    <script src="/static/css/tailwindcss.js"></script>
    <link rel="stylesheet" href="/static/css/i18n.css?v=1.4">
    <script>
        tailwind.config = {
            theme: {
                extend: {
                    animation: {
                        'fade-in': 'fadeIn 0.5s ease-in-out',
                        'slide-up': 'slideUp 0.3s ease-out',
                        'pulse-soft': 'pulse 2s cubic-bezier(0.4, 0, 0.6, 1) infinite',
                    },
                    keyframes: {
                        fadeIn: {
                            '0%': { opacity: '0' },
                            '100%': { opacity: '1' }
                        },
                        slideUp: {
                            '0%': { transform: 'translateY(10px)', opacity: '0' },
                            '100%': { transform: 'translateY(0)', opacity: '1' }
                        }
                    }
                }
            }
        }
    </script>
    <style>
        .excel-bg {
            background: #f8f9fa;
        }
        .excel-toolbar {
            background: linear-gradient(to bottom, #ffffff 0%, #f0f0f0 100%);
            border-bottom: 1px solid #d0d0d0;
            box-shadow: 0 1px 3px rgba(0,0,0,0.1);
        }
        .excel-cell {
            border: 1px solid #d0d0d0;
            background: white;
            position: relative;
        }
        .excel-cell:hover {
            background: #e8f4fd;
            border-color: #0078d4;
        }
        .excel-cell.selected {
            background: #cce8ff;
            border-color: #0078d4;
            border-width: 2px;
        }
        .excel-header {
            background: linear-gradient(to bottom, #f8f9fa 0%, #e9ecef 100%);
            border: 1px solid #d0d0d0;
            font-weight: 600;
            color: #495057;
        }
        .excel-row-header {
            background: linear-gradient(to right, #f8f9fa 0%, #e9ecef 100%);
            border: 1px solid #d0d0d0;
            font-weight: 600;
            color: #495057;
            text-align: center;
            min-width: 50px;
        }
        .excel-grid {
            border-collapse: separate;
            border-spacing: 0;
        }
        .excel-search-bar {
            background: white;
            border: 2px solid #d0d0d0;
            border-radius: 4px;
        }
        .excel-search-bar:focus {
            border-color: #0078d4;
            outline: none;
            box-shadow: 0 0 0 1px #0078d4;
        }
        .excel-button {
            background: linear-gradient(to bottom, #ffffff 0%, #f0f0f0 100%);
            border: 1px solid #d0d0d0;
            color: #333;
            transition: all 0.2s;
        }
        .excel-button:hover {
            background: linear-gradient(to bottom, #f0f0f0 0%, #e0e0e0 100%);
            border-color: #0078d4;
        }
        .excel-button:active {
            background: linear-gradient(to bottom, #e0e0e0 0%, #d0d0d0 100%);
        }
        .excel-export-button {
            background: linear-gradient(to bottom, #107c10 0%, #0e6e0e 100%);
            border: 1px solid #0e6e0e;
            color: white;
            transition: all 0.2s;
        }
        .excel-export-button:hover {
            background: linear-gradient(to bottom, #0e6e0e 0%, #0c5c0c 100%);
            border-color: #0c5c0c;
        }
        .excel-export-button:active {
            background: linear-gradient(to bottom, #0c5c0c 0%, #0a4a0a 100%);
        }
        .excel-export-button:disabled {
            background: linear-gradient(to bottom, #cccccc 0%, #bbbbbb 100%);
            border-color: #bbbbbb;
            color: #666666;
            cursor: not-allowed;
        }
        .excel-stats {
            background: #f8f9fa;
            border: 1px solid #d0d0d0;
            border-radius: 4px;
            height: 32px;
            display: flex;
            align-items: center;
            line-height: 1;
            box-sizing: border-box;
        }
        .search-highlight {
            background-color: #ffeb3b;
            color: #333;
            font-weight: 600;
            padding: 1px 2px;
            border-radius: 2px;
        }
        .search-tips {
            background: #f0f8ff;
            border: 1px solid #b3d9ff;
            border-radius: 4px;
            color: #0066cc;
        }
        .excel-cell.selected {
            background: linear-gradient(135deg, #e3f2fd 0%, #bbdefb 100%) !important;
            color: #1565c0 !important;
            border: 2px solid #2196f3 !important;
            box-shadow: 0 2px 8px rgba(33, 150, 243, 0.3) !important;
        }
        .excel-cell.multi-selected {
            background-color: #cce7ff !important;
            color: #0066cc !important;
        }
        .excel-cell {
            user-select: none;
            -webkit-user-select: none;
            -moz-user-select: none;
            -ms-user-select: none;
        }
        .app-modal-backdrop {
            position: fixed;
            inset: 0;
            background: rgba(0, 0, 0, 0.45);
            display: none;
            align-items: center;
            justify-content: center;
            z-index: 1000;
            padding: 16px;
        }
        #authModalBackdrop {
            z-index: 1300;
        }
        #workspaceFormBackdrop {
            z-index: 1300;
        }
        #workspaceManagerBackdrop {
            z-index: 1200;
        }
        .app-modal {
            width: 100%;
            max-width: 560px;
            background: #fff;
            border: 1px solid #d0d0d0;
            border-radius: 10px;
            box-shadow: 0 8px 32px rgba(0, 0, 0, 0.18);
            overflow: hidden;
        }
        .app-modal-header {
            padding: 12px 16px;
            border-bottom: 1px solid #e5e7eb;
            font-weight: 600;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }
        .app-modal-body {
            padding: 16px;
        }
        .app-modal-footer {
            padding: 12px 16px;
            border-top: 1px solid #e5e7eb;
            display: flex;
            justify-content: flex-end;
            gap: 8px;
        }
        .app-form-label {
            display: block;
            font-size: 13px;
            color: #374151;
            margin-bottom: 6px;
        }
        .app-form-input {
            width: 100%;
            border: 1px solid #d1d5db;
            border-radius: 6px;
            padding: 8px 10px;
            font-size: 14px;
            outline: none;
        }
        .app-form-input:focus {
            border-color: #2563eb;
            box-shadow: 0 0 0 1px #2563eb;
        }
        .workspace-row {
            border: 1px solid #e5e7eb;
            border-radius: 8px;
            padding: 10px 12px;
            margin-bottom: 10px;
            display: flex;
            justify-content: space-between;
            align-items: center;
            gap: 12px;
        }
        .workspace-row:last-child {
            margin-bottom: 0;
        }
        .upload-progress-track {
            width: 100%;
            height: 10px;
            border-radius: 999px;
            background: #e5e7eb;
            overflow: hidden;
        }
        .upload-progress-bar {
            height: 100%;
            width: 0%;
            background: linear-gradient(90deg, #3b82f6 0%, #2563eb 100%);
            transition: width 0.2s ease;
        }
        .upload-progress-indeterminate .upload-progress-bar {
            width: 40%;
            animation: uploadIndeterminate 1.1s infinite ease-in-out;
        }
        @keyframes uploadIndeterminate {
            0% { transform: translateX(-120%); }
            100% { transform: translateX(320%); }
        }
    </style>
</head>
<body class="min-h-screen excel-bg">
    <div class="min-h-screen flex flex-col">
        <!-- Excel-style Toolbar -->
        <div class="excel-toolbar px-4 py-3">
            <div class="flex items-center justify-between">
                <div class="flex flex-col">
                    <h1 class="text-xl font-bold text-gray-800 flex items-center">
                        <span class="text-green-600 mr-2">📊</span>
                        <span data-i18n="app.title">Excel Master Pro</span>
                    </h1>
                </div>
                <div class="flex items-center space-x-4">
                    <!-- Stats Display -->
                    <div id="stats" class="excel-stats px-3 text-sm">
                        <span class="text-gray-600" data-i18n="stats.loading">加载中...</span>
                    </div>
                    <div class="h-6 w-px bg-gray-300"></div>
                    <div class="flex items-center space-x-2">
                        <button id="registerBtn" class="excel-button px-3 py-1 rounded text-sm" onclick="showRegister()" data-i18n="auth.register">注册</button>
                        <button id="loginBtn" class="excel-button px-3 py-1 rounded text-sm" onclick="showLogin()" data-i18n="auth.login">登录</button>
                        <button id="manageWorkspaceBtn" class="excel-button px-3 py-1 rounded text-sm" onclick="openWorkspaceManager()" style="display:none;" data-i18n="workspace.manage">工作区管理</button>
                        <button id="logoutBtn" class="excel-button px-3 py-1 rounded text-sm" onclick="logout()" style="display:none;" data-i18n="auth.logout">退出</button>
                    </div>
                    <!-- Language Switcher -->
                    <div class="language-switcher">
                        <button class="language-switcher-button" onclick="toggleLanguageDropdown()">
                            <span class="language-flag" id="currentLanguageFlag">🌐</span>
                            <span id="currentLanguageName">中文</span>
                            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"></path>
                            </svg>
                        </button>
                        <div class="language-switcher-dropdown" id="languageDropdown">
                            <!-- 语言选项将通过JavaScript动态生成 -->
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- Search Section -->
        <div class="bg-white border-b border-gray-300 px-4 py-4">
            <div class="max-w-4xl mx-auto">
                <input id="uploadInput" type="file" accept=".xlsx,.xls" multiple style="display:none;" onchange="uploadSelectedFiles()">
                <div class="flex items-center space-x-4 mb-3">
                    <div id="workspaceControls" style="display:none;">
                        <select id="workspaceSelect" class="excel-search-bar px-3 py-2 text-sm min-w-40" onchange="onWorkspaceChange()">
                            <option value="" data-i18n="workspace.public_search_all">公开工作区总搜索</option>
                        </select>
                    </div>
                    <div class="flex-1 relative">
                        <input type="text" id="searchInput" 
                               class="excel-search-bar w-full px-4 py-2 text-sm"
                               data-i18n-placeholder="search.placeholder"
                               placeholder="在Excel数据中搜索... (支持多关键词，用空格分隔)" 
                               onkeypress="handleKeyPress(event)">
                        <div class="absolute inset-y-0 right-0 flex items-center pr-3">
                            <svg class="w-4 h-4 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"></path>
                            </svg>
                        </div>
                    </div>
                    <button class="excel-button px-6 py-2 rounded font-medium" onclick="performSearch()">
                        <span data-i18n="search.button">搜索</span>
                    </button>
                    <button class="excel-export-button px-6 py-2 rounded font-medium" onclick="exportResults()" id="exportResultsBtn" style="display: none;">
                        <span data-i18n="search.export">导出Excel</span>
                    </button>
                </div>
                <!-- 搜索提示和按钮在同一行 -->
                <div class="flex items-center justify-between">
                    <div class="search-tips px-3 py-2 text-sm">
                        <span data-i18n="search.tips">💡 搜索提示：输入多个关键词用空格分隔，如"阿迪力 阿布拉"可匹配包含这两个词的内容</span>
                    </div>
                </div>
            </div>
        </div>

        <!-- Excel-style Data Grid -->
        <main class="flex-1 bg-white overflow-auto">
            <div id="results" class="p-4">
                <!-- Empty State -->
                <div class="text-center py-16">
                    <div class="text-gray-400 mb-4">
                        <svg class="w-16 h-16 mx-auto" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"></path>
                        </svg>
                    </div>
                    <p class="text-gray-500 text-lg" data-i18n="search.empty_state">输入关键词开始搜索Excel数据</p>
                </div>
            </div>

            <!-- Pagination -->
            <div id="pagination" class="hidden border-t border-gray-300 bg-gray-50 px-4 py-3">
                <div class="flex items-center justify-between">
                    <div class="flex items-center space-x-2">
                        <span class="text-sm text-gray-600" id="recordsInfo" data-i18n="pagination.showing">显示记录</span>
                    </div>
                    <div class="flex items-center space-x-2">
                        <button id="prevBtn" class="excel-button px-4 py-2 rounded text-sm disabled:opacity-50 disabled:cursor-not-allowed" onclick="changePage(-1)">
                            <span data-i18n="pagination.previous">上一页</span>
                        </button>
                        <span id="pageInfo" class="text-sm text-gray-600 px-4">第 1 页</span>
                        <button id="nextBtn" class="excel-button px-4 py-2 rounded text-sm disabled:opacity-50 disabled:cursor-not-allowed" onclick="changePage(1)">
                            <span data-i18n="pagination.next">下一页</span>
                        </button>
                    </div>
                </div>
            </div>
        </main>

        <!-- Status Bar -->
        <footer class="bg-gray-100 border-t border-gray-300 px-4 py-2">
            <div class="flex items-center justify-between text-sm text-gray-600">
                <div class="flex items-center space-x-4">
                    <span data-i18n="status.ready">就绪</span>
                </div>
                <div class="flex items-center space-x-4">
                    <span data-i18n="app.title">Excel Master Pro</span>
                    <span class="text-gray-400">|</span>
                    <a
                        href="https://github.com/izqutcom/excel-data-hub"
                        target="_blank"
                        rel="noopener noreferrer"
                        class="text-blue-600 hover:underline"
                    >
                        GitHub
                    </a>
                    <span class="text-gray-400">|</span>
                    <a
                        href="https://gitee.com/izqutcom/excel-data-hub"
                        target="_blank"
                        rel="noopener noreferrer"
                        class="text-blue-600 hover:underline"
                    >
                        Gitee
                    </a>
                </div>
            </div>
        </footer>
    </div>

    <!-- 语言切换成功提示 -->
    <div class="language-switch-toast" id="languageToast">
        <span data-i18n="language.switch_success">语言已切换</span>
    </div>

    <!-- 文本方向指示器 -->
    <div class="text-direction-indicator" id="directionIndicator">
        <span id="directionText">LTR</span>
    </div>

    <div id="authModalBackdrop" class="app-modal-backdrop">
        <div class="app-modal">
            <div class="app-modal-header">
                <span id="authModalTitle">登录</span>
                <button class="excel-button px-2 py-1 rounded text-sm" onclick="closeAuthModal()" data-i18n="common.close">关闭</button>
            </div>
            <div class="app-modal-body">
                <label class="app-form-label" for="authUsernameInput" data-i18n="auth.username">用户名</label>
                <input id="authUsernameInput" class="app-form-input" type="text" data-i18n-placeholder="auth.placeholder_username" placeholder="请输入用户名">
                <div style="height:12px;"></div>
                <label class="app-form-label" for="authPasswordInput" data-i18n="auth.password">密码</label>
                <input id="authPasswordInput" class="app-form-input" type="password" data-i18n-placeholder="auth.placeholder_password" placeholder="请输入密码">
                <div id="authConfirmPasswordWrap" style="display:none;">
                    <div style="height:12px;"></div>
                    <label class="app-form-label" for="authConfirmPasswordInput" data-i18n="auth.confirm_password">确认密码</label>
                    <input id="authConfirmPasswordInput" class="app-form-input" type="password" data-i18n-placeholder="auth.placeholder_confirm_password" placeholder="请再次输入密码">
                </div>
                <div style="height:10px;"></div>
                <div id="authModalError" class="text-sm text-red-600"></div>
            </div>
            <div class="app-modal-footer">
                <button class="excel-button px-3 py-2 rounded text-sm" onclick="closeAuthModal()" data-i18n="common.cancel">取消</button>
                <button id="authSubmitBtn" class="excel-button px-3 py-2 rounded text-sm" onclick="submitAuthForm()">提交</button>
            </div>
        </div>
    </div>

    <div id="workspaceFormBackdrop" class="app-modal-backdrop">
        <div class="app-modal">
            <div class="app-modal-header">
                <span id="workspaceFormTitle">创建工作区</span>
                <button class="excel-button px-2 py-1 rounded text-sm" onclick="closeWorkspaceForm()" data-i18n="common.close">关闭</button>
            </div>
            <div class="app-modal-body">
                <label class="app-form-label" for="workspaceNameInput" data-i18n="workspace.name">工作区名称</label>
                <input id="workspaceNameInput" class="app-form-input" type="text" data-i18n-placeholder="workspace.name_placeholder" placeholder="请输入工作区名称">
                <div style="height:12px;"></div>
                <label class="app-form-label" for="workspaceDescInput" data-i18n="workspace.description">描述</label>
                <input id="workspaceDescInput" class="app-form-input" type="text" data-i18n-placeholder="workspace.description_placeholder" placeholder="可选描述">
                <div style="height:12px;"></div>
                <label class="app-form-label">
                    <input id="workspacePublicInput" type="checkbox">
                    <span data-i18n="workspace.is_public">公开工作区</span>
                </label>
                <div id="workspaceFormError" class="text-sm text-red-600"></div>
            </div>
            <div class="app-modal-footer">
                <button class="excel-button px-3 py-2 rounded text-sm" onclick="closeWorkspaceForm()" data-i18n="common.cancel">取消</button>
                <button id="workspaceSubmitBtn" class="excel-button px-3 py-2 rounded text-sm" onclick="submitWorkspaceForm()">保存</button>
            </div>
        </div>
    </div>

    <div id="workspaceManagerBackdrop" class="app-modal-backdrop">
        <div class="app-modal" style="max-width: 760px;">
            <div class="app-modal-header">
                <span data-i18n="workspace.manage">工作区管理</span>
                <button class="excel-button px-2 py-1 rounded text-sm" onclick="closeWorkspaceManager()" data-i18n="common.close">关闭</button>
            </div>
            <div class="app-modal-body">
                <div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:10px;">
                    <span class="text-sm text-gray-600" data-i18n="workspace.manager_hint">可编辑、删除、上传文件到你拥有的工作区</span>
                    <button class="excel-button px-3 py-2 rounded text-sm" onclick="openWorkspaceForm('create')" data-i18n="workspace.new_button">新建工作区</button>
                </div>
                <div id="workspaceManagerList"></div>
            </div>
        </div>
    </div>

    <div id="uploadProgressBackdrop" class="app-modal-backdrop" style="z-index:1400;">
        <div class="app-modal" style="max-width: 520px;">
            <div class="app-modal-header">
                <span data-i18n="upload.progress_title">上传与检索进度</span>
            </div>
            <div class="app-modal-body">
                <div id="uploadProgressText" class="text-sm text-gray-700" data-i18n="upload.preparing">准备上传...</div>
                <div id="uploadProgressDetail" class="text-xs text-gray-500 mt-1"></div>
                <div style="height:10px;"></div>
                <div id="uploadProgressTrack" class="upload-progress-track">
                    <div id="uploadProgressBar" class="upload-progress-bar"></div>
                </div>
                <div id="uploadProgressPercent" class="text-xs text-gray-600 mt-2">0%</div>
            </div>
        </div>
    </div>

    <script src="/static/js/i18n.js"></script>

    <script>
        // 全局变量
        let selectedCell = null;
        let selectedCells = new Set(); // 存储多选的单元格
        let isSelecting = false; // 是否正在拖拽选择
        let selectionStart = null; // 选择起始单元格
        let lastClickedCell = null; // 最后点击的单元格，用于Shift选择

        let currentQuery = '';
        let currentPage = 0;
        const pageSize = 50;
        const WORKSPACE_STORAGE_KEY = 'selected_workspace_id';
        let currentToken = localStorage.getItem('auth_token') || '';
        let currentUser = null;
        let workspaceList = [];
        let currentWorkspaceId = null;
        let authModalMode = 'login';
        let workspaceFormMode = 'create';
        let editingWorkspaceId = null;
        let uploadWorkspaceId = null;
        let uploadInProgress = false;

        function t(key, fallback, params = {}) {
            if (!window.i18n) return fallback;
            const translated = window.i18n.translate(key, params);
            return translated === key ? fallback : translated;
        }

        // 页面加载完成后初始化
        document.addEventListener('DOMContentLoaded', async function() {
            // 等待i18n系统初始化完成
            if (window.i18n) {
                await window.i18n.init();
            }
            // 监听语言切换事件（兼容旧事件名）
            const onLanguageChanged = function(event) {
                console.log('Language changed to:', event.detail.language);
                loadStats(); // 重新加载统计信息以应用新语言
                renderWorkspaceOptions();
                renderWorkspaceManagerList();
                updateAuthModalTexts();
                updateWorkspaceFormTexts();
            };
            document.addEventListener('i18n:languageChanged', onLanguageChanged);
            document.addEventListener('languageChanged', onLanguageChanged);

            restoreUserFromStorage();
            updateAuthUI();
            await loadWorkspaces();
            updateAuthModalTexts();
            updateWorkspaceFormTexts();
            loadStats();
            document.getElementById('searchInput').focus();
        });

        function getAuthHeaders() {
            const headers = {};
            if (currentToken) {
                headers['Authorization'] = `Bearer ${currentToken}`;
            }
            return headers;
        }

        function restoreUserFromStorage() {
            try {
                const userRaw = localStorage.getItem('auth_user');
                currentUser = userRaw ? JSON.parse(userRaw) : null;
            } catch (e) {
                currentUser = null;
            }
        }

        function saveSession(authData) {
            currentToken = authData.token || '';
            currentUser = authData.user || null;
            localStorage.setItem('auth_token', currentToken);
            localStorage.setItem('auth_user', JSON.stringify(currentUser || {}));
            updateAuthUI();
        }

        function clearSession() {
            currentToken = '';
            currentUser = null;
            localStorage.removeItem('auth_token');
            localStorage.removeItem('auth_user');
            localStorage.removeItem(WORKSPACE_STORAGE_KEY);
            updateAuthUI();
        }

        function updateAuthUI() {
            const loginBtn = document.getElementById('loginBtn');
            const registerBtn = document.getElementById('registerBtn');
            const logoutBtn = document.getElementById('logoutBtn');
            const workspaceControls = document.getElementById('workspaceControls');
            const manageWorkspaceBtn = document.getElementById('manageWorkspaceBtn');

            if (currentToken && currentUser && currentUser.username) {
                loginBtn.style.display = 'none';
                registerBtn.style.display = 'none';
                logoutBtn.style.display = 'inline-block';
                manageWorkspaceBtn.style.display = 'inline-block';
                workspaceControls.style.display = 'flex';
            } else {
                loginBtn.style.display = 'inline-block';
                registerBtn.style.display = 'inline-block';
                logoutBtn.style.display = 'none';
                manageWorkspaceBtn.style.display = 'none';
                workspaceControls.style.display = 'none';
            }

            refreshUploadButtonState();
        }

        function updateAuthModalTexts() {
            const title = document.getElementById('authModalTitle');
            const submit = document.getElementById('authSubmitBtn');
            if (!title || !submit) return;
            if (authModalMode === 'register') {
                title.textContent = t('auth.modal.register_title', '注册账号');
                submit.textContent = t('auth.modal.submit_register', '注册并登录');
            } else {
                title.textContent = t('auth.modal.login_title', '用户登录');
                submit.textContent = t('auth.modal.submit_login', '登录');
            }
        }

        function showRegister() {
            authModalMode = 'register';
            updateAuthModalTexts();
            document.getElementById('authModalError').textContent = '';
            document.getElementById('authUsernameInput').value = '';
            document.getElementById('authPasswordInput').value = '';
            document.getElementById('authConfirmPasswordInput').value = '';
            document.getElementById('authConfirmPasswordWrap').style.display = 'block';
            document.getElementById('authModalBackdrop').style.display = 'flex';
        }

        function showLogin() {
            authModalMode = 'login';
            updateAuthModalTexts();
            document.getElementById('authModalError').textContent = '';
            document.getElementById('authUsernameInput').value = '';
            document.getElementById('authPasswordInput').value = '';
            document.getElementById('authConfirmPasswordInput').value = '';
            document.getElementById('authConfirmPasswordWrap').style.display = 'none';
            document.getElementById('authModalBackdrop').style.display = 'flex';
        }

        function closeAuthModal() {
            document.getElementById('authModalBackdrop').style.display = 'none';
        }

        async function submitAuthForm() {
            const username = document.getElementById('authUsernameInput').value.trim();
            const password = document.getElementById('authPasswordInput').value;
            const confirmPassword = document.getElementById('authConfirmPasswordInput').value;
            const errorEl = document.getElementById('authModalError');
            errorEl.textContent = '';

            if (!username || !password) {
                errorEl.textContent = t('auth.errors.required', '用户名和密码不能为空');
                return;
            }
            if (authModalMode === 'register' && password !== confirmPassword) {
                errorEl.textContent = t('auth.errors.mismatch', '两次输入的密码不一致');
                return;
            }

            const url = authModalMode === 'register' ? '/api/auth/register' : '/api/auth/login';
            try {
                const response = await fetch(url, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ username, password })
                });
                if (!response.ok) {
                    throw new Error(await response.text());
                }
                const data = await response.json();
                saveSession(data);
                await loadWorkspaces();
                closeAuthModal();
            } catch (e) {
                errorEl.textContent = e.message;
            }
        }

        function logout() {
            clearSession();
            workspaceList = workspaceList.filter(w => w.is_public);
            const select = document.getElementById('workspaceSelect');
            select.value = '';
            currentWorkspaceId = null;
            renderWorkspaceOptions();
            loadStats();
            if (currentQuery) {
                search(currentQuery, currentPage);
            }
        }

        async function loadWorkspaces() {
            try {
                const response = await fetch('/api/workspaces', {
                    headers: getAuthHeaders()
                });
                if (!response.ok) {
                    throw new Error(await response.text());
                }
                workspaceList = await response.json();
                renderWorkspaceOptions();
                refreshUploadButtonState();
                renderWorkspaceManagerList();
            } catch (e) {
                console.error('加载工作区失败:', e);
            }
        }

        function renderWorkspaceOptions() {
            const select = document.getElementById('workspaceSelect');
            const previous = currentWorkspaceId ? String(currentWorkspaceId) : '';
            const remembered = localStorage.getItem(WORKSPACE_STORAGE_KEY) || '';
            select.innerHTML = `<option value="">${t('workspace.public_search_all', '公开工作区总搜索')}</option>`;
            workspaceList.forEach((ws) => {
                const opt = document.createElement('option');
                opt.value = String(ws.id);
                const visibilityText = ws.is_public ? t('workspace.public', '公开') : t('workspace.private', '私有');
                opt.textContent = `${visibilityText} | ${ws.name}`;
                select.appendChild(opt);
            });

            let targetValue = '';
            const hasOption = (v) => v && Array.from(select.options).some(o => o.value === v);
            if (hasOption(previous)) {
                targetValue = previous;
            } else if (hasOption(remembered)) {
                targetValue = remembered;
            } else if (currentToken && currentUser) {
                const ownWorkspace = workspaceList.find(w => w.owner_id === currentUser.id);
                targetValue = ownWorkspace ? String(ownWorkspace.id) : '';
            }

            select.value = targetValue;
            currentWorkspaceId = select.value ? parseInt(select.value, 10) : null;
            if (currentWorkspaceId) {
                localStorage.setItem(WORKSPACE_STORAGE_KEY, String(currentWorkspaceId));
            } else {
                localStorage.removeItem(WORKSPACE_STORAGE_KEY);
            }
        }

        function onWorkspaceChange() {
            const value = document.getElementById('workspaceSelect').value;
            currentWorkspaceId = value ? parseInt(value, 10) : null;
            if (currentWorkspaceId) {
                localStorage.setItem(WORKSPACE_STORAGE_KEY, String(currentWorkspaceId));
            } else {
                localStorage.removeItem(WORKSPACE_STORAGE_KEY);
            }
            refreshUploadButtonState();
            loadStats();
            if (currentQuery) {
                search(currentQuery, currentPage);
            }
        }

        function getSelectedWorkspace() {
            if (!currentWorkspaceId) return null;
            return workspaceList.find(w => w.id === currentWorkspaceId) || null;
        }

        function refreshUploadButtonState() {
            const uploadBtn = document.getElementById('uploadBtn');
            const ws = getSelectedWorkspace();
            const canUpload = Boolean(
                currentToken &&
                currentUser &&
                ws &&
                ws.owner_id === currentUser.id
            );
            if (uploadBtn) {
                uploadBtn.disabled = !canUpload;
            }
        }

        function createWorkspace() {
            if (!currentToken) {
                alert(t('workspace.login_required', '请先登录'));
                return;
            }
            openWorkspaceForm('create');
        }

        function updateWorkspaceFormTexts() {
            const title = document.getElementById('workspaceFormTitle');
            const submit = document.getElementById('workspaceSubmitBtn');
            if (!title || !submit) return;
            if (workspaceFormMode === 'edit') {
                title.textContent = t('workspace.edit_title', '编辑工作区');
                submit.textContent = t('workspace.save_changes', '保存修改');
            } else {
                title.textContent = t('workspace.create_title', '创建工作区');
                submit.textContent = t('workspace.create', '创建');
            }
        }

        function openWorkspaceForm(mode, workspaceId = null) {
            workspaceFormMode = mode;
            editingWorkspaceId = workspaceId;
            const title = document.getElementById('workspaceFormTitle');
            const submitBtn = document.getElementById('workspaceSubmitBtn');
            const errorEl = document.getElementById('workspaceFormError');
            const nameEl = document.getElementById('workspaceNameInput');
            const descEl = document.getElementById('workspaceDescInput');
            const publicEl = document.getElementById('workspacePublicInput');

            errorEl.textContent = '';
            if (mode === 'edit' && workspaceId) {
                const ws = workspaceList.find(w => w.id === workspaceId);
                if (!ws) {
                    alert(t('workspace.not_found', '未找到工作区'));
                    return;
                }
                workspaceFormMode = 'edit';
                updateWorkspaceFormTexts();
                nameEl.value = ws.name || '';
                descEl.value = ws.description || '';
                publicEl.checked = !!ws.is_public;
            } else {
                workspaceFormMode = 'create';
                updateWorkspaceFormTexts();
                nameEl.value = '';
                descEl.value = '';
                publicEl.checked = false;
            }

            document.getElementById('workspaceFormBackdrop').style.display = 'flex';
        }

        function closeWorkspaceForm() {
            document.getElementById('workspaceFormBackdrop').style.display = 'none';
        }

        async function submitWorkspaceForm() {
            if (!currentToken) {
                alert(t('workspace.login_required', '请先登录'));
                return;
            }
            const name = document.getElementById('workspaceNameInput').value.trim();
            const description = document.getElementById('workspaceDescInput').value.trim();
            const isPublic = document.getElementById('workspacePublicInput').checked;
            const errorEl = document.getElementById('workspaceFormError');
            errorEl.textContent = '';

            if (!name) {
                errorEl.textContent = t('workspace.errors.name_required', '工作区名称不能为空');
                return;
            }

            try {
                const isEdit = workspaceFormMode === 'edit' && editingWorkspaceId;
                const response = await fetch(isEdit ? `/api/workspaces/${editingWorkspaceId}` : '/api/workspaces', {
                    method: isEdit ? 'PUT' : 'POST',
                    headers: {
                        'Content-Type': 'application/json',
                        ...getAuthHeaders()
                    },
                    body: JSON.stringify({
                        name,
                        description: description || null,
                        is_public: Boolean(isPublic)
                    })
                });
                if (!response.ok) {
                    throw new Error(await response.text());
                }
                const workspace = await response.json();
                await loadWorkspaces();
                currentWorkspaceId = workspace.id;
                localStorage.setItem(WORKSPACE_STORAGE_KEY, String(workspace.id));
                renderWorkspaceOptions();
                loadStats();
                closeWorkspaceForm();
                renderWorkspaceManagerList();
            } catch (e) {
                errorEl.textContent = e.message;
            }
        }

        function triggerUpload() {
            if (uploadInProgress) {
                alert(t('upload.in_progress_block', '正在上传处理中，请勿重复操作'));
                return;
            }
            const ws = getSelectedWorkspace();
            if (!ws) {
                alert(t('workspace.select_required', '请先选择一个工作区'));
                return;
            }
            if (!currentToken || !currentUser || ws.owner_id !== currentUser.id) {
                alert(t('workspace.owner_upload_only', '仅工作区拥有者可上传'));
                return;
            }
            uploadWorkspaceId = ws.id;
            document.getElementById('uploadInput').click();
        }

        function openWorkspaceManager() {
            if (!currentToken) {
                alert(t('workspace.login_required', '请先登录'));
                return;
            }
            renderWorkspaceManagerList();
            document.getElementById('workspaceManagerBackdrop').style.display = 'flex';
        }

        function closeWorkspaceManager() {
            document.getElementById('workspaceManagerBackdrop').style.display = 'none';
        }

        function renderWorkspaceManagerList() {
            const listEl = document.getElementById('workspaceManagerList');
            if (!listEl) return;
            if (!currentUser) {
                listEl.innerHTML = `<div class="text-sm text-gray-500">${t('workspace.login_required', '请先登录')}</div>`;
                return;
            }

            const ownWorkspaces = workspaceList.filter(w => w.owner_id === currentUser.id);
            if (ownWorkspaces.length === 0) {
                listEl.innerHTML = `<div class="text-sm text-gray-500">${t('workspace.no_workspace', '你还没有工作区，点击右上角“新建工作区”创建。')}</div>`;
                return;
            }

            listEl.innerHTML = ownWorkspaces.map((ws) => `
                <div class="workspace-row">
                    <div>
                        <div class="text-sm font-semibold text-gray-800">${ws.name}</div>
                        <div class="text-xs text-gray-500 mt-1">${ws.description || t('workspace.empty_desc', '无描述')}</div>
                        <div class="text-xs mt-1 ${ws.is_public ? 'text-green-600' : 'text-gray-500'}">${ws.is_public ? t('workspace.public', '公开') : t('workspace.private', '私有')}</div>
                    </div>
                    <div class="flex items-center space-x-2">
                        <button class="excel-button px-2 py-1 rounded text-sm" onclick="selectWorkspaceFromManager(${ws.id})">${t('workspace.enter', '进入')}</button>
                        <button class="excel-button px-2 py-1 rounded text-sm" onclick="openWorkspaceForm('edit', ${ws.id})">${t('workspace.edit', '编辑')}</button>
                        <button class="excel-button px-2 py-1 rounded text-sm" onclick="openUploadForWorkspace(${ws.id})">${t('workspace.upload', '上传')}</button>
                        <button class="excel-button px-2 py-1 rounded text-sm text-red-600" onclick="deleteWorkspace(${ws.id})">${t('workspace.delete', '删除')}</button>
                    </div>
                </div>
            `).join('');
        }

        function selectWorkspaceFromManager(workspaceId) {
            currentWorkspaceId = workspaceId;
            renderWorkspaceOptions();
            onWorkspaceChange();
            closeWorkspaceManager();
        }

        function openUploadForWorkspace(workspaceId) {
            if (uploadInProgress) {
                alert(t('upload.in_progress_block', '正在上传处理中，请勿重复操作'));
                return;
            }
            uploadWorkspaceId = workspaceId;
            document.getElementById('uploadInput').click();
        }

        async function deleteWorkspace(workspaceId) {
            if (!confirm(t('workspace.delete_confirm', '确认删除此工作区吗？\n工作区内已上传数据会被一并删除且不可恢复。'))) {
                return;
            }
            try {
                const response = await fetch(`/api/workspaces/${workspaceId}`, {
                    method: 'DELETE',
                    headers: getAuthHeaders()
                });
                if (!response.ok) {
                    throw new Error(await response.text());
                }
                if (currentWorkspaceId === workspaceId) {
                    currentWorkspaceId = null;
                    localStorage.removeItem(WORKSPACE_STORAGE_KEY);
                }
                await loadWorkspaces();
                renderWorkspaceOptions();
                renderWorkspaceManagerList();
                loadStats();
                if (currentQuery) {
                    search(currentQuery, currentPage);
                }
            } catch (e) {
                alert(`${t('workspace.delete_failed', '删除失败')}: ${e.message}`);
            }
        }

        function showUploadProgress() {
            document.getElementById('uploadProgressBackdrop').style.display = 'flex';
        }

        function hideUploadProgress() {
            document.getElementById('uploadProgressBackdrop').style.display = 'none';
        }

        function setUploadProgress(percent, text, detail, indeterminate = false) {
            const track = document.getElementById('uploadProgressTrack');
            const bar = document.getElementById('uploadProgressBar');
            document.getElementById('uploadProgressText').textContent = text;
            document.getElementById('uploadProgressDetail').textContent = detail || '';
            document.getElementById('uploadProgressPercent').textContent = `${Math.max(0, Math.min(100, Math.floor(percent)))}%`;
            if (indeterminate) {
                track.classList.add('upload-progress-indeterminate');
                bar.style.width = '40%';
            } else {
                track.classList.remove('upload-progress-indeterminate');
                bar.style.width = `${Math.max(0, Math.min(100, percent))}%`;
            }
        }

        function uploadOneFileWithProgress(workspaceId, file, fileIndex, totalFiles) {
            return new Promise((resolve, reject) => {
                const formData = new FormData();
                formData.append('files', file);

                const xhr = new XMLHttpRequest();
                xhr.open('POST', `/api/workspaces/${workspaceId}/upload`);
                if (currentToken) {
                    xhr.setRequestHeader('Authorization', `Bearer ${currentToken}`);
                }

                xhr.upload.onprogress = function(event) {
                    if (!event.lengthComputable) return;
                    const filePercent = (event.loaded / event.total) * 100;
                    const overallPercent = ((fileIndex - 1) / totalFiles) * 100 + (filePercent / totalFiles);
                    const remain = totalFiles - fileIndex;
                    setUploadProgress(
                        overallPercent,
                        t('upload.uploading_n', '正在上传第 {current}/{total} 个文件', { current: fileIndex, total: totalFiles }),
                        t('upload.current_file_remain', '当前文件: {file}，剩余 {remain} 个', { file: file.name, remain })
                    );
                };

                xhr.onreadystatechange = function() {
                    if (xhr.readyState !== XMLHttpRequest.DONE) return;
                    if (xhr.status >= 200 && xhr.status < 300) {
                        resolve(xhr.responseText);
                    } else {
                        reject(new Error(xhr.responseText || `${t('upload.failed', '上传失败')}，${t('upload.status_code', '状态码')} ${xhr.status}`));
                    }
                };
                xhr.onerror = function() {
                    reject(new Error(t('upload.network_failed', '网络异常，上传失败')));
                };

                xhr.send(formData);
            });
        }

        async function uploadSelectedFiles() {
            const workspaceId = uploadWorkspaceId || currentWorkspaceId;
            const ws = workspaceId ? workspaceList.find(w => w.id === workspaceId) : null;
            const input = document.getElementById('uploadInput');
            if (!ws || !input.files || input.files.length === 0) return;
            if (uploadInProgress) {
                alert(t('upload.in_progress_block', '正在上传处理中，请勿重复操作'));
                input.value = '';
                uploadWorkspaceId = null;
                return;
            }
            if (!currentUser || ws.owner_id !== currentUser.id) {
                alert(t('workspace.owner_upload_only', '仅工作区拥有者可上传'));
                input.value = '';
                uploadWorkspaceId = null;
                return;
            }
            const filesToUpload = Array.from(input.files);
            uploadInProgress = true;
            showUploadProgress();
            setUploadProgress(0, t('upload.preparing', '准备上传...'), t('upload.total_files', '共 {count} 个文件', { count: filesToUpload.length }));

            try {
                for (let i = 0; i < filesToUpload.length; i += 1) {
                    const file = filesToUpload[i];
                    await uploadOneFileWithProgress(ws.id, file, i + 1, filesToUpload.length);
                    const donePercent = ((i + 1) / filesToUpload.length) * 100;
                    setUploadProgress(
                        donePercent,
                        t('upload.file_uploaded', '文件 {current}/{total} 上传完成', { current: i + 1, total: filesToUpload.length }),
                        t('upload.wait_indexing', '等待服务端检索: {file}', { file: file.name }),
                        true
                    );
                }
                setUploadProgress(100, t('upload.all_uploaded', '全部文件上传完成'), t('upload.refreshing', '正在刷新数据...'), false);
                loadStats();
                if (currentQuery) {
                    search(currentQuery, currentPage);
                }
                setTimeout(() => {
                    hideUploadProgress();
                }, 600);
            } catch (e) {
                hideUploadProgress();
                alert(`${t('upload.failed', '上传失败')}: ${e.message}`);
            } finally {
                input.value = '';
                uploadWorkspaceId = null;
                uploadInProgress = false;
            }
        }

        // 语言切换器相关函数
        function toggleLanguageDropdown() {
            const dropdown = document.getElementById('languageDropdown');
            dropdown.classList.toggle('show');
        }

        // 点击其他地方关闭下拉菜单
        document.addEventListener('click', function(event) {
            const languageSwitcher = document.querySelector('.language-switcher');
            if (!languageSwitcher.contains(event.target)) {
                document.getElementById('languageDropdown').classList.remove('show');
            }
        });

        // 加载统计信息
        function loadStats() {
            const workspaceParam = currentWorkspaceId ? `?workspace_id=${currentWorkspaceId}` : '';
            fetch(`/api/stats${workspaceParam}`, {
                headers: getAuthHeaders()
            })
                .then(response => response.json())
                .then(data => {
                    const totalFiles = window.i18n ? window.i18n.translate('stats.total_files') : '文件';
                    const totalRows = window.i18n ? window.i18n.translate('stats.total_rows') : '行';
                    const lastUpdate = window.i18n ? window.i18n.translate('stats.last_update') : '更新';
                    
                    document.getElementById('stats').innerHTML = `
                        <span class="text-gray-600">
                            ${data.total_files} ${totalFiles} | ${data.total_rows.toLocaleString()} ${totalRows} | ${lastUpdate}: ${new Date(data.last_update).toLocaleDateString()}
                        </span>
                    `;
                })
                .catch(error => {
                    console.error('加载统计信息失败:', error);
                    const errorText = window.i18n ? window.i18n.translate('stats.error') : '加载失败';
                    document.getElementById('stats').innerHTML = `
                        <span class="text-red-600">${errorText}</span>
                    `;
                });
        }

        // 处理回车键搜索
        function handleKeyPress(event) {
            if (event.key === 'Enter') {
                performSearch();
            }
        }

        // 执行搜索
        function performSearch() {
            const query = document.getElementById('searchInput').value.trim();
            if (!query) {
                const message = window.i18n ? window.i18n.translate('search.keyword_required') : '请输入搜索关键词';
                alert(message);
                return;
            }

            currentQuery = query;
            currentPage = 0;
            search(query, currentPage);
        }

        // 搜索函数
        function search(query, page) {
            const offset = page * pageSize;
            const loadingText = window.i18n ? window.i18n.translate('search.loading') : '搜索中...';

            // 显示加载状态
            document.getElementById('results').innerHTML = `
                <div class="text-center py-16">
                    <div class="flex items-center justify-center text-gray-600">
                        <div class="animate-spin rounded-full h-6 w-6 border-b-2 border-blue-600 mr-3"></div>
                        <span class="text-lg">${loadingText}</span>
                    </div>
                </div>
            `;

            // 处理多关键词搜索 - 提取关键词用于高亮显示
            const keywords = query.trim().split(/\s+/).filter(k => k.length > 0);

            const workspaceParam = currentWorkspaceId ? `&workspace_id=${currentWorkspaceId}` : '';
            fetch(`/api/search?q=${encodeURIComponent(query)}&limit=${pageSize}&offset=${offset}${workspaceParam}`, {
                headers: getAuthHeaders()
            })
                .then(response => response.json())
                .then(data => {
                    displayResults(data, keywords);
                    updatePagination(data);
                })
                .catch(error => {
                    console.error('搜索失败:', error);
                    const errorText = window.i18n ? window.i18n.translate('search.failed') : '搜索失败，请重试';
                    document.getElementById('results').innerHTML = `
                        <div class="text-center py-16">
                            <div class="text-red-600">
                                <p class="text-lg">${errorText}</p>
                            </div>
                        </div>
                    `;
                });
        }

        // 高亮显示关键词
        function highlightKeywords(text, keywords) {
            if (!keywords || keywords.length === 0) return text;
            
            let highlightedText = text;
            keywords.forEach(keyword => {
                if (keyword.trim()) {
                    const regex = new RegExp(`(${keyword.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})`, 'gi');
                    highlightedText = highlightedText.replace(regex, '<span class="search-highlight">$1</span>');
                }
            });
            return highlightedText;
        }

        // 显示搜索结果为Excel表格（按文件分组）
        function displayResults(data, keywords = []) {
            const resultsDiv = document.getElementById('results');
            
            if (data.results.length === 0) {
                const noResultsText = window.i18n ? window.i18n.translate('search.no_results') : '没有找到匹配的结果';
                resultsDiv.innerHTML = `
                    <div class="text-center py-16">
                        <div class="text-gray-500">
                            <p class="text-lg">${noResultsText}</p>
                        </div>
                    </div>
                `;
                // 隐藏导出按钮
                document.getElementById('exportResultsBtn').style.display = 'none';
                return;
            }

            // 显示导出按钮
            document.getElementById('exportResultsBtn').style.display = 'inline-block';

            // 按文件名分组数据
            const groupedData = {};
            data.results.forEach(item => {
                if (!groupedData[item.file_name]) {
                    groupedData[item.file_name] = [];
                }
                groupedData[item.file_name].push(item);
            });

            const searchResultsText = window.i18n ? window.i18n.translate('search.results') : '搜索结果';
            const totalRecordsText = window.i18n ? window.i18n.translate('search.total_records') : '条记录';
            const filesText = window.i18n ? window.i18n.translate('search.files') : '个文件';

            let html = `
                <div class="mb-4 text-sm text-gray-600">
                    ${searchResultsText}: ${data.total.toLocaleString()} ${totalRecordsText} (${Object.keys(groupedData).length} ${filesText})
                </div>
            `;

            // 为每个文件创建独立的表格
            Object.entries(groupedData).forEach(([fileName, items], fileIndex) => {
                // 获取该文件的所有唯一列名
                const rowNumberText = window.i18n ? window.i18n.translate('table.row_number') : '行号';
                const importTimeText = window.i18n ? window.i18n.translate('table.import_time') : '导入时间';
                
                const fileColumns = new Set([rowNumberText, importTimeText]);
                items.forEach(item => {
                    try {
                        const dataObj = JSON.parse(item.data_json);
                        Object.keys(dataObj).forEach(key => fileColumns.add(key));
                    } catch (e) {}
                });

                const columns = Array.from(fileColumns);
                const recordsText = window.i18n ? window.i18n.translate('table.records') : '条记录';
                const fieldsText = window.i18n ? window.i18n.translate('table.fields') : '个字段';

                html += `
                    <div class="mb-8 bg-white border border-gray-300 rounded-lg overflow-hidden">
                        <!-- 文件标题栏 -->
                        <div class="bg-gradient-to-r from-blue-50 to-blue-100 border-b border-gray-300 px-4 py-3">
                            <div class="flex items-center justify-between">
                                <div class="flex items-center space-x-3">
                                    <button class="file-toggle-btn text-blue-600 hover:text-blue-800 transition-colors" 
                                            onclick="toggleFileSection('file_${fileIndex}')" 
                                            data-target="file_${fileIndex}">
                                        <svg class="w-5 h-5 transform transition-transform" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"></path>
                                        </svg>
                                    </button>
                                    <div class="flex items-center space-x-2">
                                        <svg class="w-5 h-5 text-green-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"></path>
                                        </svg>
                                        <h3 class="text-lg font-semibold text-gray-800">${highlightKeywords(fileName, keywords)}</h3>
                                    </div>
                                </div>
                                <div class="flex items-center space-x-4 text-sm text-gray-600">
                                    <span>${items.length} ${recordsText}</span>
                                    <span>${columns.length} ${fieldsText}</span>
                                </div>
                            </div>
                        </div>

                        <!-- 表格内容 -->
                        <div id="file_${fileIndex}" class="file-content">
                            <div class="overflow-auto">
                                <table class="excel-grid w-full">
                                    <thead>
                                        <tr>
                                            <th class="excel-row-header p-2">#</th>
                `;

                // 添加工作表名称列到表头
                html += `<th class="excel-header p-2 text-left min-w-32">工作表</th>`;
                
                // 表头
                columns.forEach(col => {
                    html += `<th class="excel-header p-2 text-left min-w-32">${col}</th>`;
                });

                html += `
                                        </tr>
                                    </thead>
                                    <tbody>
                `;

                // 数据行
                items.forEach((item, index) => {
                    const globalRowNum = data.results.findIndex(r => r === item) + 1;
                    html += `<tr>`;
                    html += `<td class="excel-row-header p-2">${globalRowNum}</td>`;
                    
                    // 添加工作表名称列
                    const sheetName = item.sheet_name || 'Sheet1';
                    const highlightedSheetName = highlightKeywords(sheetName, keywords);
                    html += `
                        <td class="excel-cell p-2 text-sm cursor-pointer bg-blue-50" 
                            onclick="selectCell(this, event)" 
                            onmousedown="startSelection(this, event)"
                            onmouseenter="extendSelection(this, event)"
                            onmouseup="endSelection(event)"
                            data-row="${globalRowNum}" 
                            data-col="工作表"
                            data-file="${fileName}"
                            title="${sheetName}">
                            <div class="truncate max-w-48 font-medium text-blue-700">${highlightedSheetName}</div>
                        </td>
                    `;

                    let dataObj = {};
                    try {
                        dataObj = JSON.parse(item.data_json);
                    } catch (e) {}

                    columns.forEach(col => {
                        let cellValue = '';
                        if (col === rowNumberText) {
                            cellValue = item.row_number;
                        } else if (col === importTimeText) {
                            cellValue = new Date(item.import_time).toLocaleString();
                        } else {
                            cellValue = dataObj[col] || '';
                        }

                        // 对单元格内容进行关键词高亮
                        const highlightedValue = highlightKeywords(String(cellValue), keywords);

                        html += `
                            <td class="excel-cell p-2 text-sm cursor-pointer" 
                                onclick="selectCell(this, event)" 
                                onmousedown="startSelection(this, event)"
                                onmouseenter="extendSelection(this, event)"
                                onmouseup="endSelection(event)"
                                data-row="${globalRowNum}" 
                                data-col="${col}"
                                data-file="${fileName}"
                                title="${cellValue}">
                                <div class="truncate max-w-48">${highlightedValue}</div>
                            </td>
                        `;
                    });

                    html += `</tr>`;
                });

                html += `
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    </div>
                `;
            });

            resultsDiv.innerHTML = html;
        }

        // 切换文件分组的展开/折叠状态
        function toggleFileSection(targetId) {
            const content = document.getElementById(targetId);
            const button = document.querySelector(`[data-target="${targetId}"]`);
            const icon = button.querySelector('svg');
            
            if (content.style.display === 'none') {
                content.style.display = 'block';
                icon.style.transform = 'rotate(0deg)';
            } else {
                content.style.display = 'none';
                icon.style.transform = 'rotate(-90deg)';
            }
        }

        // 选择单元格
        function selectCell(cell, event) {
            // 移除输入框焦点，确保表格操作时焦点不在输入框内
            const searchInput = document.getElementById('searchInput');
            if (searchInput && document.activeElement === searchInput) {
                searchInput.blur();
            }
            
            if (event.shiftKey && lastClickedCell) {
                // Shift键选择范围
                selectRange(lastClickedCell, cell);
            } else if (event.ctrlKey || event.metaKey) {
                // Ctrl/Cmd键多选
                toggleCellSelection(cell);
            } else {
                // 普通单击，清除其他选择
                clearAllSelections();
                selectSingleCell(cell);
            }
            lastClickedCell = cell;
        }

        // 选择单个单元格
        function selectSingleCell(cell) {
            cell.classList.add('selected');
            selectedCells.add(cell);
            selectedCell = cell;
        }

        // 切换单元格选择状态
        function toggleCellSelection(cell) {
            if (selectedCells.has(cell)) {
                cell.classList.remove('selected', 'multi-selected');
                selectedCells.delete(cell);
                if (selectedCell === cell) {
                    selectedCell = selectedCells.size > 0 ? Array.from(selectedCells)[0] : null;
                }
            } else {
                cell.classList.add('multi-selected');
                selectedCells.add(cell);
                selectedCell = cell;
            }
        }

        // 选择范围
        function selectRange(startCell, endCell) {
            clearAllSelections();
            
            const startRow = parseInt(startCell.dataset.row);
            const endRow = parseInt(endCell.dataset.row);
            const startCol = startCell.dataset.col;
            const endCol = endCell.dataset.col;
            
            const minRow = Math.min(startRow, endRow);
            const maxRow = Math.max(startRow, endRow);
            
            // 获取所有单元格
            const allCells = document.querySelectorAll('.excel-cell');
            const colOrder = Array.from(new Set(Array.from(allCells).map(cell => cell.dataset.col)));
            const startColIndex = colOrder.indexOf(startCol);
            const endColIndex = colOrder.indexOf(endCol);
            const minColIndex = Math.min(startColIndex, endColIndex);
            const maxColIndex = Math.max(startColIndex, endColIndex);
            
            allCells.forEach(cell => {
                const cellRow = parseInt(cell.dataset.row);
                const cellColIndex = colOrder.indexOf(cell.dataset.col);
                
                if (cellRow >= minRow && cellRow <= maxRow && 
                    cellColIndex >= minColIndex && cellColIndex <= maxColIndex) {
                    cell.classList.add('multi-selected');
                    selectedCells.add(cell);
                }
            });
            
            selectedCell = endCell;
        }

        // 清除所有选择
        function clearAllSelections() {
            selectedCells.forEach(cell => {
                cell.classList.remove('selected', 'multi-selected');
            });
            selectedCells.clear();
            selectedCell = null;
        }

        // 开始拖拽选择
        function startSelection(cell, event) {
            if (event.button !== 0) return; // 只处理左键
            
            isSelecting = true;
            selectionStart = cell;
            
            if (!event.ctrlKey && !event.metaKey && !event.shiftKey) {
                clearAllSelections();
            }
            
            selectSingleCell(cell);
            event.preventDefault();
        }

        // 扩展选择
        function extendSelection(cell, event) {
            if (!isSelecting || !selectionStart) return;
            
            // 清除之前的拖拽选择，保留Ctrl选择的
            selectedCells.forEach(selectedCell => {
                if (!selectedCell.classList.contains('selected')) {
                    selectedCell.classList.remove('multi-selected');
                    selectedCells.delete(selectedCell);
                }
            });
            
            selectRange(selectionStart, cell);
        }

        // 结束选择
        function endSelection(event) {
            isSelecting = false;
            selectionStart = null;
        }

        // 复制选中的内容
        function copySelectedCells() {
            if (selectedCells.size === 0) return;
            
            const cellsArray = Array.from(selectedCells);
            const cellsData = cellsArray.map(cell => {
                const textContent = cell.querySelector('div').textContent || cell.textContent;
                return {
                    row: parseInt(cell.dataset.row),
                    col: cell.dataset.col,
                    file: cell.dataset.file,
                    content: textContent.trim()
                };
            });
            
            // 按行和列排序
            cellsData.sort((a, b) => {
                if (a.row !== b.row) return a.row - b.row;
                return a.col.localeCompare(b.col);
            });
            
            // 生成复制文本
            let copyText = '';
            let currentRow = -1;
            
            cellsData.forEach((cellData, index) => {
                if (cellData.row !== currentRow) {
                    if (currentRow !== -1) copyText += '\n';
                    currentRow = cellData.row;
                } else {
                    copyText += '\t';
                }
                copyText += cellData.content;
            });
            
            // 复制到剪贴板
            navigator.clipboard.writeText(copyText).then(() => {
                showCopyNotification();
            }).catch(err => {
                console.error('复制失败:', err);
                // 降级方案
                const textArea = document.createElement('textarea');
                textArea.value = copyText;
                document.body.appendChild(textArea);
                textArea.select();
                document.execCommand('copy');
                document.body.removeChild(textArea);
                showCopyNotification();
            });
        }

        // 显示复制成功提示
        function showCopyNotification() {
            const notification = document.createElement('div');
            notification.className = 'fixed top-4 right-4 bg-green-500 text-white px-4 py-2 rounded shadow-lg z-50';
            const copiedText = window.i18n ? window.i18n.translate('notification.copied_cells', { count: selectedCells.size }) : `已复制 ${selectedCells.size} 个单元格`;
            notification.textContent = copiedText;
            document.body.appendChild(notification);
            
            setTimeout(() => {
                notification.remove();
            }, 2000);
        }

        // 更新分页
        function updatePagination(data) {
            const paginationDiv = document.getElementById('pagination');
            const pageInfo = document.getElementById('pageInfo');
            const prevBtn = document.getElementById('prevBtn');
            const nextBtn = document.getElementById('nextBtn');
            const recordsInfo = document.getElementById('recordsInfo');

            if (data.total <= pageSize) {
                paginationDiv.classList.add('hidden');
                return;
            }

            paginationDiv.classList.remove('hidden');
            const totalPages = Math.ceil(data.total / pageSize);
            const currentPageNumber = Math.floor(data.offset / pageSize) + 1;
            const startRecord = data.offset + 1;
            const endRecord = Math.min(data.offset + pageSize, data.total);

            const pageText = window.i18n ? window.i18n.translate('pagination.page') : '第';
            const ofText = window.i18n ? window.i18n.translate('pagination.of') : '页，共';
            const pagesText = window.i18n ? window.i18n.translate('pagination.pages') : '页';
            const recordsText = window.i18n ? window.i18n.translate('pagination.records') : '显示';

            pageInfo.textContent = `${pageText} ${currentPageNumber} ${ofText} ${totalPages} ${pagesText}`;
            recordsInfo.textContent = `${recordsText} ${startRecord}-${endRecord} / ${data.total.toLocaleString()}`;

            prevBtn.disabled = currentPageNumber <= 1;
            nextBtn.disabled = currentPageNumber >= totalPages;
        }

        // 切换页面
        function changePage(direction) {
            currentPage += direction;
            if (currentPage < 0) currentPage = 0;
            search(currentQuery, currentPage);
        }

        // 定期更新统计信息
        setInterval(loadStats, 30000);

        // 添加键盘事件监听器
        document.addEventListener('keydown', function(event) {
            // Ctrl+C 或 Cmd+C 复制选中内容
            if ((event.ctrlKey || event.metaKey) && event.key === 'c') {
                // 检查焦点是否在输入框内
                const activeElement = document.activeElement;
                const isInputFocused = activeElement && (
                    activeElement.tagName === 'INPUT' || 
                    activeElement.tagName === 'TEXTAREA' ||
                    activeElement.contentEditable === 'true'
                );
                
                // 如果焦点在输入框内，允许默认行为（复制输入框选中的文本）
                if (isInputFocused) {
                    return; // 不阻止默认行为，让浏览器处理输入框的复制
                }
                
                // 如果焦点不在输入框内，且有选中的表格单元格，则复制表格数据
                if (selectedCells.size > 0) {
                    event.preventDefault();
                    copySelectedCells();
                }
            }
            
            // Ctrl+A 或 Cmd+A 全选当前表格
            if ((event.ctrlKey || event.metaKey) && event.key === 'a') {
                // 检查焦点是否在输入框内
                const activeElement = document.activeElement;
                const isInputFocused = activeElement && (
                    activeElement.tagName === 'INPUT' || 
                    activeElement.tagName === 'TEXTAREA' ||
                    activeElement.contentEditable === 'true'
                );
                
                // 如果焦点在输入框内，允许默认行为（选择输入框文本）
                if (isInputFocused) {
                    return; // 不阻止默认行为，让浏览器处理输入框的全选
                }
                
                // 如果焦点不在输入框内，且有表格数据，则全选表格
                const allCells = document.querySelectorAll('.excel-cell');
                if (allCells.length > 0) {
                    event.preventDefault();
                    clearAllSelections();
                    allCells.forEach(cell => {
                        cell.classList.add('multi-selected');
                        selectedCells.add(cell);
                    });
                    selectedCell = allCells[allCells.length - 1];
                }
            }
            
            // Escape 清除选择
            if (event.key === 'Escape') {
                clearAllSelections();
            }
        });

        // 阻止表格区域的右键菜单，添加自定义菜单
        document.addEventListener('contextmenu', function(event) {
            const cell = event.target.closest('.excel-cell');
            if (cell) {
                event.preventDefault();
                
                // 如果右键点击的单元格没有被选中，先选中它
                if (!cell.classList.contains('selected') && !cell.classList.contains('multi-selected')) {
                    clearAllSelections();
                    selectSingleCell(cell);
                }
                
                // 复制选中的内容
                if (selectedCells.size > 0) {
                    copySelectedCells();
                }
            }
        });

        // 阻止拖拽时的文本选择
        document.addEventListener('selectstart', function(event) {
            if (isSelecting) {
                event.preventDefault();
            }
        });

        // 导出功能
        function exportResults() {
            const query = currentQuery;
            if (!query) {
                alert('请先进行搜索');
                return;
            }

            const exportBtn = document.getElementById('exportResultsBtn');
            
            // 显示加载状态
            exportBtn.disabled = true;
            exportBtn.textContent = '导出中...';

            const workspaceParam = currentWorkspaceId ? `&workspace_id=${currentWorkspaceId}` : '';
            const exportUrl = `/api/export?q=${encodeURIComponent(query)}${workspaceParam}`;

            fetch(exportUrl, {
                headers: getAuthHeaders()
            })
            .then(async (resp) => {
                if (!resp.ok) throw new Error(await resp.text());
                return resp.blob();
            })
            .then((blob) => {
                const blobUrl = URL.createObjectURL(blob);
                const link = document.createElement('a');
                link.href = blobUrl;
                link.download = `搜索结果导出_${Date.now()}.xlsx`;
                link.style.display = 'none';
                document.body.appendChild(link);
                link.click();
                document.body.removeChild(link);
                URL.revokeObjectURL(blobUrl);
            })
            .catch((err) => {
                alert(`导出失败: ${err.message}`);
            })
            .finally(() => {
                exportBtn.disabled = false;
                exportBtn.textContent = '导出Excel';
            });
        }
    </script>
</body>
</html>
    "#)
}

async fn register_handler(
    State(app_state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, String)> {
    let username = payload.username.trim();
    if username.is_empty() || payload.password.len() < 6 {
        return Err((StatusCode::BAD_REQUEST, "用户名不能为空且密码至少6位".to_string()));
    }

    let existing = users::Entity::find()
        .filter(users::Column::Username.eq(username))
        .one(&app_state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("查询用户失败: {}", e)))?;
    if existing.is_some() {
        return Err((StatusCode::CONFLICT, "用户名已存在".to_string()));
    }

    let now = chrono::Utc::now();
    let user = users::ActiveModel {
        id: Default::default(),
        username: Set(username.to_string()),
        password_hash: Set(hash_password(&payload.password)),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&app_state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("创建用户失败: {}", e)))?;

    let token = Uuid::new_v4().to_string();
    let expires_at = now + chrono::Duration::days(30);
    auth_tokens::ActiveModel {
        id: Default::default(),
        user_id: Set(user.id),
        token: Set(token.clone()),
        expires_at: Set(expires_at),
        created_at: Set(now),
    }
    .insert(&app_state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("保存Token失败: {}", e)))?;

    Ok(Json(AuthResponse {
        token,
        expires_at,
        user: UserResponse {
            id: user.id,
            username: user.username,
        },
    }))
}

async fn login_handler(
    State(app_state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, String)> {
    let username = payload.username.trim();
    let user = users::Entity::find()
        .filter(users::Column::Username.eq(username))
        .one(&app_state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("查询用户失败: {}", e)))?
        .ok_or((StatusCode::UNAUTHORIZED, "用户名或密码错误".to_string()))?;

    if user.password_hash != hash_password(&payload.password) {
        return Err((StatusCode::UNAUTHORIZED, "用户名或密码错误".to_string()));
    }

    let now = chrono::Utc::now();
    let token = Uuid::new_v4().to_string();
    let expires_at = now + chrono::Duration::days(30);
    auth_tokens::ActiveModel {
        id: Default::default(),
        user_id: Set(user.id),
        token: Set(token.clone()),
        expires_at: Set(expires_at),
        created_at: Set(now),
    }
    .insert(&app_state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("保存Token失败: {}", e)))?;

    Ok(Json(AuthResponse {
        token,
        expires_at,
        user: UserResponse {
            id: user.id,
            username: user.username,
        },
    }))
}

async fn create_workspace_handler(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateWorkspaceRequest>,
) -> Result<Json<WorkspaceResponse>, (StatusCode, String)> {
    let user = authenticate_user(&headers, &app_state.db).await?;
    let name = payload.name.trim();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "workspace名称不能为空".to_string()));
    }

    let duplicate = workspaces::Entity::find()
        .filter(workspaces::Column::OwnerId.eq(user.id))
        .filter(workspaces::Column::Name.eq(name))
        .one(&app_state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("查询workspace失败: {}", e)))?;
    if duplicate.is_some() {
        return Err((StatusCode::CONFLICT, "该名称已存在".to_string()));
    }

    let now = chrono::Utc::now();
    let model = workspaces::ActiveModel {
        id: Default::default(),
        owner_id: Set(user.id),
        name: Set(name.to_string()),
        description: Set(payload.description),
        is_public: Set(payload.is_public.unwrap_or(false)),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&app_state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("创建workspace失败: {}", e)))?;

    Ok(Json(WorkspaceResponse {
        id: model.id,
        owner_id: model.owner_id,
        name: model.name,
        description: model.description,
        is_public: model.is_public,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }))
}

async fn update_workspace_handler(
    State(app_state): State<AppState>,
    Path(workspace_id): Path<i32>,
    headers: HeaderMap,
    Json(payload): Json<UpdateWorkspaceRequest>,
) -> Result<Json<WorkspaceResponse>, (StatusCode, String)> {
    let user = authenticate_user(&headers, &app_state.db).await?;
    let existing = get_workspace_by_id(&app_state.db, workspace_id).await?;
    if existing.owner_id != user.id {
        return Err((StatusCode::FORBIDDEN, "仅workspace拥有者可编辑".to_string()));
    }

    let mut new_name = existing.name.clone();
    if let Some(name) = payload.name.as_deref() {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "workspace名称不能为空".to_string()));
        }
        new_name = trimmed.to_string();
    }

    let duplicate = workspaces::Entity::find()
        .filter(workspaces::Column::OwnerId.eq(user.id))
        .filter(workspaces::Column::Name.eq(new_name.clone()))
        .one(&app_state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("查询workspace失败: {}", e)))?;
    if let Some(dup) = duplicate {
        if dup.id != workspace_id {
            return Err((StatusCode::CONFLICT, "该名称已存在".to_string()));
        }
    }

    let new_description = match payload.description {
        Some(desc) => {
            let trimmed = desc.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        None => existing.description.clone(),
    };

    let now = chrono::Utc::now();
    let updated = workspaces::ActiveModel {
        id: Set(existing.id),
        owner_id: Set(existing.owner_id),
        name: Set(new_name),
        description: Set(new_description),
        is_public: Set(payload.is_public.unwrap_or(existing.is_public)),
        created_at: Set(existing.created_at),
        updated_at: Set(now),
    }
    .update(&app_state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("更新workspace失败: {}", e)))?;

    Ok(Json(WorkspaceResponse {
        id: updated.id,
        owner_id: updated.owner_id,
        name: updated.name,
        description: updated.description,
        is_public: updated.is_public,
        created_at: updated.created_at,
        updated_at: updated.updated_at,
    }))
}

async fn delete_workspace_handler(
    State(app_state): State<AppState>,
    Path(workspace_id): Path<i32>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user = authenticate_user(&headers, &app_state.db).await?;
    let workspace = get_workspace_by_id(&app_state.db, workspace_id).await?;
    if workspace.owner_id != user.id {
        return Err((StatusCode::FORBIDDEN, "仅workspace拥有者可删除".to_string()));
    }

    let workspace_files = files::Entity::find()
        .filter(files::Column::WorkspaceId.eq(workspace_id))
        .all(&app_state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("查询文件失败: {}", e)))?;

    for file in workspace_files {
        if let Err(e) = fs::remove_file(&file.file_path).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("删除workspace文件失败: path={}, err={}", file.file_path, e);
            }
        }
    }

    workspaces::Entity::delete_by_id(workspace_id)
        .exec(&app_state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("删除workspace失败: {}", e)))?;

    Ok(Json(serde_json::json!({
        "workspace_id": workspace_id,
        "deleted": true
    })))
}

async fn list_workspaces_handler(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<WorkspaceResponse>>, (StatusCode, String)> {
    let current_user = authenticate_user(&headers, &app_state.db).await.ok();

    let mut rows = workspaces::Entity::find()
        .filter(workspaces::Column::IsPublic.eq(true))
        .all(&app_state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("查询workspace失败: {}", e)))?;

    if let Some(user) = current_user {
        let own_rows = workspaces::Entity::find()
            .filter(workspaces::Column::OwnerId.eq(user.id))
            .all(&app_state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("查询workspace失败: {}", e)))?;
        for item in own_rows {
            if !rows.iter().any(|r| r.id == item.id) {
                rows.push(item);
            }
        }
    }

    let resp = rows
        .into_iter()
        .map(|w| WorkspaceResponse {
            id: w.id,
            owner_id: w.owner_id,
            name: w.name,
            description: w.description,
            is_public: w.is_public,
            created_at: w.created_at,
            updated_at: w.updated_at,
        })
        .collect();
    Ok(Json(resp))
}

async fn upload_to_workspace_handler(
    State(app_state): State<AppState>,
    Path(workspace_id): Path<i32>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user = authenticate_user(&headers, &app_state.db).await?;
    let workspace = get_workspace_by_id(&app_state.db, workspace_id).await?;
    if workspace.owner_id != user.id {
        return Err((StatusCode::FORBIDDEN, "仅workspace拥有者可上传".to_string()));
    }

    let mut imported = 0i32;
    let processor = crate::excel_processor_sea::ExcelProcessor::new(app_state.db.clone());

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("读取上传字段失败: {}", e)))?
    {
        let file_name = field.file_name().unwrap_or("upload.xlsx").to_string();
        let file_name_lower = file_name.to_lowercase();
        if !file_name_lower.ends_with(".xlsx") && !file_name_lower.ends_with(".xls") {
            continue;
        }

        let data = field
            .bytes()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("读取上传文件失败: {}", e)))?;
        if data.is_empty() {
            continue;
        }

        let ext = if file_name_lower.ends_with(".xls") { "xls" } else { "xlsx" };
        let stored_name = format!("{}_{}.{}", workspace_id, Uuid::new_v4(), ext);
        let full_path = StdPath::new(&app_state.upload_dir).join(stored_name);
        fs::write(&full_path, data)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("保存上传文件失败: {}", e)))?;

        let path_str = full_path.to_string_lossy().to_string();
        processor
            .import_uploaded_file(workspace_id, &path_str, user.id, &file_name)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("导入Excel失败: {}", e)))?;
        imported += 1;
    }

    Ok(Json(serde_json::json!({
        "workspace_id": workspace_id,
        "imported_files": imported
    })))
}



async fn stats_handler(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<StatsQuery>,
) -> Result<Json<StatsResponse>, (StatusCode, String)> {
    let db = app_state.db.clone();
    let processor = crate::excel_processor_sea::ExcelProcessor::new(db.clone());

    if let Some(workspace_id) = params.workspace_id {
        let workspace = get_workspace_by_id(&db, workspace_id).await?;
        if !workspace.is_public {
            let user = authenticate_user(&headers, &db).await?;
            if user.id != workspace.owner_id {
                return Err((StatusCode::FORBIDDEN, "无权限访问该workspace".to_string()));
            }
        }
        return processor
            .get_workspace_statistics(workspace_id)
            .await
            .map(Json)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("获取统计信息失败: {}", e)));
    }

    {
        let cache = app_state.stats_cache.lock().unwrap();
        if let Some(cached_stats) = cache.get() {
            debug!("返回缓存的统计数据");
            return Ok(Json(cached_stats.clone()));
        }
    }

    match processor.get_public_statistics().await {
        Ok(stats) => {
            {
                let mut cache = app_state.stats_cache.lock().unwrap();
                cache.update(stats.clone());
            }
            debug!("统计数据已更新到缓存");
            Ok(Json(stats))
        },
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, format!("获取统计信息失败: {}", e))),
    }
}

async fn search_handler(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let db = app_state.db.clone();
    let query_text = params.q.unwrap_or_default();
    let limit = params.limit.unwrap_or(20).max(1).min(100) as u64;
    let offset = params.offset.unwrap_or(0).max(0) as u64;
    
    if query_text.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "查询参数不能为空".to_string()));
    }
    
    let processor = crate::excel_processor_sea::ExcelProcessor::new(db.clone());

    if let Some(workspace_id) = params.workspace_id {
        let workspace = get_workspace_by_id(&db, workspace_id).await?;
        if !workspace.is_public {
            let user = authenticate_user(&headers, &db).await?;
            if user.id != workspace.owner_id {
                return Err((StatusCode::FORBIDDEN, "无权限访问该workspace".to_string()));
            }
        }
        match processor.search_workspace_data(workspace_id, &query_text, limit, offset).await {
            Ok(results) => Ok(Json(results)),
            Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, format!("搜索失败: {}", e))),
        }
    } else {
        match processor.search_public_data(&query_text, limit, offset).await {
            Ok(results) => Ok(Json(results)),
            Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, format!("搜索失败: {}", e))),
        }
    }
}

async fn export_handler(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<SearchQuery>,
) -> Result<Response<axum::body::Body>, (StatusCode, String)> {
    let db = app_state.db.clone();
    let query_text = params.q.unwrap_or_default();
    
    if query_text.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "查询参数不能为空".to_string()));
    }
    
    let processor = crate::excel_processor_sea::ExcelProcessor::new(db.clone());
    let export_result = if let Some(workspace_id) = params.workspace_id {
        let workspace = get_workspace_by_id(&db, workspace_id).await?;
        if !workspace.is_public {
            let user = authenticate_user(&headers, &db).await?;
            if user.id != workspace.owner_id {
                return Err((StatusCode::FORBIDDEN, "无权限访问该workspace".to_string()));
            }
        }
        processor.export_workspace_search_results(workspace_id, &query_text).await
    } else {
        processor.export_public_search_results(&query_text).await
    };

    match export_result {
        Ok(excel_data) => {
            let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
            let filename = format!("搜索结果导出_{}.xlsx", timestamp);
            
            let response = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
                .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", filename))
                .header(header::CONTENT_LENGTH, excel_data.len())
                .body(axum::body::Body::from(excel_data))
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("构建响应失败: {}", e)))?;
                
            Ok(response)
        },
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, format!("导出失败: {}", e))),
    }
}

// 多语言API处理器
async fn get_languages_handler(
    State(app_state): State<AppState>,
) -> Result<Json<Vec<LanguageResponse>>, (StatusCode, String)> {
    let i18n_manager = app_state.i18n_manager.lock().unwrap();
    let languages = i18n_manager.get_effective_supported_languages()
        .into_iter()
        .map(|lang_info| LanguageResponse {
            code: lang_info.code,
            name: lang_info.name,
            native_name: lang_info.native_name,
            is_rtl: lang_info.is_rtl,
        })
        .collect();
    Ok(Json(languages))
}

async fn get_i18n_status_handler(
    State(app_state): State<AppState>,
) -> Result<Json<I18nStatusResponse>, (StatusCode, String)> {
    let i18n_manager = app_state.i18n_manager.lock().unwrap();
    let current_language = i18n_manager.get_default_language().to_string();
    let available_languages = i18n_manager.get_effective_supported_languages()
        .into_iter()
        .map(|lang_info| LanguageResponse {
            code: lang_info.code,
            name: lang_info.name,
            native_name: lang_info.native_name,
            is_rtl: lang_info.is_rtl,
        })
        .collect();
    
    Ok(Json(I18nStatusResponse {
        default_language: current_language,
        supported_languages: available_languages,
        auto_detect_enabled: true,
        cache_enabled: true,
        total_translations: i18n_manager.get_total_translations(),
        multilingual_enabled: i18n_manager.is_multilingual_enabled(),
    }))
}

async fn translate_handler(
    State(app_state): State<AppState>,
    Path(key): Path<String>,
    headers: HeaderMap,
) -> Result<Json<TranslationResponse>, (StatusCode, String)> {
    let i18n_manager = app_state.i18n_manager.lock().unwrap();
    let lang = i18n_manager.detect_language_from_headers(&headers);
    let translation = i18n_manager.translate(&key, &lang, None);
    
    Ok(Json(TranslationResponse {
        key,
        value: translation,
        language: lang,
    }))
}

async fn batch_translate_handler(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<BatchTranslationRequest>,
) -> Result<Json<BatchTranslationResponse>, (StatusCode, String)> {
    let i18n_manager = app_state.i18n_manager.lock().unwrap();
    let lang = request.language.unwrap_or_else(|| i18n_manager.detect_language_from_headers(&headers));
    let mut translations: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    
    for key in request.keys.into_iter() {
        let translation = i18n_manager.translate(&key, &lang, request.params.as_ref());
        translations.insert(key, translation);
    }
    
    Ok(Json(BatchTranslationResponse {
        translations,
        language: lang,
    }))
}

async fn reload_translations_handler(
    State(app_state): State<AppState>,
) -> Result<Json<I18nStatusResponse>, (StatusCode, String)> {
    let mut i18n_manager = app_state.i18n_manager.lock().unwrap();
    
    match i18n_manager.reload_translations() {
        Ok(_) => {
            let current_language = i18n_manager.get_default_language().to_string();
            let available_languages = i18n_manager.get_supported_languages()
                .into_iter()
                .map(|lang_info| LanguageResponse {
                    code: lang_info.code,
                    name: lang_info.name,
                    native_name: lang_info.native_name,
                    is_rtl: lang_info.is_rtl,
                })
                .collect();
            
            Ok(Json(I18nStatusResponse {
                default_language: current_language,
                supported_languages: available_languages,
                auto_detect_enabled: true,
                cache_enabled: true,
                total_translations: i18n_manager.get_total_translations(),
                multilingual_enabled: i18n_manager.is_multilingual_enabled(),
            }))
        },
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, format!("重新加载翻译失败: {}", e))),
    }
}
