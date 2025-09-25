use crate::models::{SearchResponse, StatsResponse, LanguageResponse, TranslationResponse, 
                   BatchTranslationResponse, TranslationRequest, BatchTranslationRequest, 
                   LanguageSettingRequest, I18nStatusResponse};
use crate::i18n_manager::I18nManager;
use axum::{
    extract::{Query, State, Path},
    http::{StatusCode, header, HeaderMap},
    response::{Html, Response},
    routing::{get, post},
    Json, Router,
};
use tower_http::services::ServeDir;
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower_http::cors::CorsLayer;
use tracing::{info, debug, error};

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
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

pub async fn start_server(db: DatabaseConnection, port: u16) -> Result<(), Box<dyn std::error::Error>> {
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
    };
    
    // 创建路由
    info!("创建路由...");
    let app = Router::new()
        .route("/", get(home_handler))
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
    </style>
</head>
<body class="min-h-screen excel-bg">
    <div class="min-h-screen flex flex-col">
        <!-- Excel-style Toolbar -->
        <div class="excel-toolbar px-4 py-3">
            <div class="flex items-center justify-between">
                <div class="flex items-center space-x-4">
                    <h1 class="text-xl font-bold text-gray-800 flex items-center">
                        <span class="text-green-600 mr-2">📊</span>
                        <span data-i18n="app.title">Excel Master Pro</span>
                    </h1>
                    <div class="h-6 w-px bg-gray-300"></div>
                    <div class="flex items-center space-x-2">
                        <button class="excel-button px-3 py-1 rounded text-sm" onclick="exportData()">
                            <span data-i18n="toolbar.export">导出</span>
                        </button>
                        <button class="excel-button px-3 py-1 rounded text-sm" onclick="refreshData()">
                            <span data-i18n="toolbar.refresh">刷新</span>
                        </button>
                    </div>
                </div>
                <div class="flex items-center space-x-4">
                    <!-- Stats Display -->
                    <div id="stats" class="excel-stats px-4 py-2 text-sm">
                        <span class="text-gray-600" data-i18n="stats.loading">加载中...</span>
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
                <div class="flex items-center space-x-4 mb-3">
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

        // 页面加载完成后初始化
        document.addEventListener('DOMContentLoaded', async function() {
            // 等待i18n系统初始化完成
            if (window.i18n) {
                await window.i18n.init();
            }
            // 监听语言切换事件
            document.addEventListener('languageChanged', function(event) {
                console.log('Language changed to:', event.detail.language);
                loadStats(); // 重新加载统计信息以应用新语言
            });

            loadStats();
            document.getElementById('searchInput').focus();
        });

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
            fetch('/api/stats')
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

            fetch(`/api/search?q=${encodeURIComponent(query)}&limit=${pageSize}&offset=${offset}`)
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

        // 导出数据
        function exportData() {
            exportResults();
        }

        // 刷新数据
        function refreshData() {
            loadStats();
            if (currentQuery) {
                search(currentQuery, currentPage);
            }
        }

        // 定期更新统计信息
        setInterval(loadStats, 30000);

        // 添加键盘事件监听器
        document.addEventListener('keydown', function(event) {
            // Ctrl+C 或 Cmd+C 复制选中内容
            if ((event.ctrlKey || event.metaKey) && event.key === 'c') {
                if (selectedCells.size > 0) {
                    event.preventDefault();
                    copySelectedCells();
                }
            }
            
            // Ctrl+A 或 Cmd+A 全选当前表格
            if ((event.ctrlKey || event.metaKey) && event.key === 'a') {
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

            // 构建导出URL
            const exportUrl = `/api/export?q=${encodeURIComponent(query)}`;
            
            // 创建隐藏的下载链接
            const link = document.createElement('a');
            link.href = exportUrl;
            link.style.display = 'none';
            document.body.appendChild(link);
            
            // 触发下载
            link.click();
            
            // 清理
            document.body.removeChild(link);
            
            // 恢复按钮状态
            setTimeout(() => {
                exportBtn.disabled = false;
                exportBtn.textContent = '导出Excel';
            }, 1000);
        }
    </script>
</body>
</html>
    "#)
}



async fn stats_handler(
    State(app_state): State<AppState>,
) -> Result<Json<StatsResponse>, (StatusCode, String)> {
    // 首先检查缓存
    {
        let cache = app_state.stats_cache.lock().unwrap();
        if let Some(cached_stats) = cache.get() {
            debug!("返回缓存的统计数据");
            return Ok(Json(cached_stats.clone()));
        }
    }

    // 缓存过期或不存在，重新获取数据
    debug!("缓存过期，重新获取统计数据");
    let db = app_state.db;
    
    // 使用ExcelProcessor获取统计信息
    let processor = crate::excel_processor_sea::ExcelProcessor::new(db.clone());
    
    match processor.get_statistics().await {
        Ok(stats) => {
            // 更新缓存
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
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let db = app_state.db;
    let query_text = params.q.unwrap_or_default();
    let limit = params.limit.unwrap_or(20).max(1).min(100) as u64;
    let offset = params.offset.unwrap_or(0).max(0) as u64;
    
    if query_text.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "查询参数不能为空".to_string()));
    }
    
    // 使用ExcelProcessor搜索数据
    let processor = crate::excel_processor_sea::ExcelProcessor::new(db);
    
    match processor.search_data(&query_text, limit.try_into().unwrap(), offset.try_into().unwrap()).await {
        Ok(results) => Ok(Json(results)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, format!("搜索失败: {}", e))),
    }
}

async fn export_handler(
    State(app_state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<Response<axum::body::Body>, (StatusCode, String)> {
    let db = app_state.db;
    let query_text = params.q.unwrap_or_default();
    
    if query_text.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "查询参数不能为空".to_string()));
    }
    
    // 使用ExcelProcessor导出数据
    let processor = crate::excel_processor_sea::ExcelProcessor::new(db);
    
    match processor.export_search_results(&query_text).await {
        Ok(excel_data) => {
            // 生成文件名
            let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
            let filename = format!("搜索结果导出_{}.xlsx", timestamp);
            
            // 构建响应
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