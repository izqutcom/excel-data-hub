/**
 * 多语言管理器 - 前端国际化支持
 * 支持语言切换、RTL/LTR布局、动态翻译加载
 */
class I18nManager {
    constructor() {
        this.currentLanguage = 'zh';
        this.translations = {};
        this.supportedLanguages = [];
        this.isRTL = false;
        this.initialized = false;
        this.multilingualEnabled = true; // 多语言功能开关状态
        
        // 批量加载缺失翻译键的相关属性
        this.missingKeys = new Set();
        this.batchTimer = null;
        
        // 绑定方法上下文
        this.translate = this.translate.bind(this);
        this.setLanguage = this.setLanguage.bind(this);
        this.detectLanguage = this.detectLanguage.bind(this);
    }

    /**
     * 初始化多语言系统
     */
    async init() {
        try {
            // 获取多语言系统状态
            await this.loadI18nStatus();
            
            // 获取支持的语言列表
            await this.loadSupportedLanguages();
            
            // 检测用户语言偏好
            const detectedLang = this.detectLanguage();
            
            // 设置初始语言
            await this.setLanguage(detectedLang);
            
            // 生成语言选项和更新UI
            this.updateLanguageSwitcherUI();
            
            this.initialized = true;
            console.log('多语言系统初始化完成', {
                currentLanguage: this.currentLanguage,
                supportedLanguages: this.supportedLanguages.map(l => l.code),
                isRTL: this.isRTL,
                multilingualEnabled: this.multilingualEnabled
            });
            
            // 触发初始化完成事件
            this.dispatchEvent('i18n:initialized', {
                language: this.currentLanguage,
                isRTL: this.isRTL,
                multilingualEnabled: this.multilingualEnabled
            });
            
        } catch (error) {
            console.error('多语言系统初始化失败:', error);
            // 使用默认语言作为后备
            this.currentLanguage = 'zh';
            this.isRTL = false;
            this.multilingualEnabled = true;
        }
    }

    /**
     * 加载多语言系统状态
     */
    async loadI18nStatus() {
        try {
            const response = await fetch('/api/i18n/status');
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }
            
            const status = await response.json();
            this.multilingualEnabled = status.multilingual_enabled;
            console.log('多语言系统状态:', { multilingualEnabled: this.multilingualEnabled });
            
        } catch (error) {
            console.error('加载多语言系统状态失败:', error);
            // 默认启用多语言功能
            this.multilingualEnabled = true;
        }
    }

    /**
     * 加载支持的语言列表
     */
    async loadSupportedLanguages() {
        try {
            const response = await fetch('/api/i18n/languages');
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }
            
            this.supportedLanguages = await response.json();
            console.log('已加载支持的语言:', this.supportedLanguages);
            
        } catch (error) {
            console.error('加载语言列表失败:', error);
            // 使用默认语言列表作为后备
            this.supportedLanguages = [
                { code: 'zh', name: 'Chinese', native_name: '中文', is_rtl: false },
                { code: 'en', name: 'English', native_name: 'English', is_rtl: false },
                { code: 'ar', name: 'Arabic', native_name: 'العربية', is_rtl: true },
                { code: 'ug', name: 'Uyghur', native_name: 'ئۇيغۇرچە', is_rtl: true }
            ];
        }
    }

    /**
     * 检测用户语言偏好
     */
    detectLanguage() {
        // 如果多语言功能关闭，直接返回默认语言
        if (!this.multilingualEnabled) {
            if (this.supportedLanguages.length > 0) {
                return this.supportedLanguages[0].code;
            }
            return 'zh'; // 后备默认语言
        }
        
        // 1. 检查localStorage中保存的语言设置
        const savedLang = localStorage.getItem('preferred_language');
        if (savedLang && this.isLanguageSupported(savedLang)) {
            return savedLang;
        }

        // 2. 检查浏览器语言设置
        const browserLangs = navigator.languages || [navigator.language];
        for (const lang of browserLangs) {
            const langCode = lang.split('-')[0].toLowerCase();
            if (this.isLanguageSupported(langCode)) {
                return langCode;
            }
        }

        // 3. 返回默认语言
        return 'zh';
    }

    /**
     * 检查语言是否受支持
     */
    isLanguageSupported(langCode) {
        return this.supportedLanguages.some(lang => lang.code === langCode);
    }

    /**
     * 设置当前语言
     */
    async setLanguage(langCode) {
        // 如果多语言功能关闭，强制使用默认语言
        if (!this.multilingualEnabled) {
            // 从支持的语言列表中获取默认语言（通常是第一个）
            if (this.supportedLanguages.length > 0) {
                langCode = this.supportedLanguages[0].code;
            } else {
                langCode = 'zh'; // 后备默认语言
            }
        }
        
        if (!this.isLanguageSupported(langCode)) {
            console.warn(`不支持的语言: ${langCode}`);
            return false;
        }

        try {
            // 加载翻译数据
            await this.loadTranslations(langCode);
            
            // 更新当前语言
            this.currentLanguage = langCode;
            
            // 更新RTL状态
            const langInfo = this.supportedLanguages.find(lang => lang.code === langCode);
            this.isRTL = langInfo ? langInfo.is_rtl : false;
            
            // 保存语言偏好（仅在多语言功能开启时）
            if (this.multilingualEnabled) {
                localStorage.setItem('preferred_language', langCode);
            }
            
            // 更新页面布局方向
            this.updatePageDirection();
            
            // 更新所有翻译文本
            this.updateAllTranslations();
            
            // 触发语言切换事件
            this.dispatchEvent('i18n:languageChanged', {
                language: langCode,
                isRTL: this.isRTL
            });
            
            console.log(`语言已切换到: ${langCode} (RTL: ${this.isRTL})`);
            return true;
            
        } catch (error) {
            console.error(`切换语言失败 (${langCode}):`, error);
            return false;
        }
    }

    /**
     * 加载指定语言的翻译数据
     */
    async loadTranslations(langCode) {
        try {
            // 获取所有需要翻译的键
            const keys = this.getAllTranslationKeys();
            
            if (keys.length === 0) {
                console.log('没有找到需要翻译的键');
                return;
            }

            // 批量获取翻译
            const response = await fetch('/api/i18n/translate', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'Accept-Language': langCode
                },
                body: JSON.stringify({
                    keys: keys,
                    language: langCode
                })
            });

            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }

            const data = await response.json();
            this.translations[langCode] = data.translations;
            
            console.log(`已加载 ${langCode} 语言翻译:`, Object.keys(data.translations).length, '项');
            
        } catch (error) {
            console.error(`加载翻译失败 (${langCode}):`, error);
            // 确保有一个空的翻译对象
            this.translations[langCode] = {};
        }
    }

    /**
     * 获取页面中所有需要翻译的键
     */
    getAllTranslationKeys() {
        const keys = new Set();
        
        // 查找所有带有 data-i18n 属性的元素
        document.querySelectorAll('[data-i18n]').forEach(element => {
            const key = element.getAttribute('data-i18n');
            if (key) {
                keys.add(key);
            }
        });
        
        // 查找所有带有 data-i18n-placeholder 属性的元素
        document.querySelectorAll('[data-i18n-placeholder]').forEach(element => {
            const key = element.getAttribute('data-i18n-placeholder');
            if (key) {
                keys.add(key);
            }
        });
        
        // 查找所有带有 data-i18n-title 属性的元素
        document.querySelectorAll('[data-i18n-title]').forEach(element => {
            const key = element.getAttribute('data-i18n-title');
            if (key) {
                keys.add(key);
            }
        });
        
        // 添加在JavaScript中动态使用的翻译键
        const dynamicKeys = [
            'stats.total_files',
            'stats.total_rows', 
            'stats.last_update',
            'stats.error',
            'search.results',
            'search.total_records',
            'search.files',
            'search.keyword_required',
            'search.no_results',  // 添加搜索无结果的翻译键
            'notification.copied_cells',  // 添加复制单元格通知的翻译键
            'table.records',
            'table.row_number',
            'table.import_time',
            'table.fields',
            'pagination.page',
            'pagination.of',
            'pagination.pages',
            'pagination.records'
        ];
        
        dynamicKeys.forEach(key => keys.add(key));
        
        return Array.from(keys);
    }

    /**
     * 翻译指定键
     */
    translate(key, params = {}) {
        const langTranslations = this.translations[this.currentLanguage] || {};
        let translation = langTranslations[key];
        
        // 如果翻译不存在，队列缺失的键并返回键名作为后备
        if (!translation) {
            this.queueMissingKey(key);
            translation = key;
        }
        
        // 替换参数占位符
        Object.keys(params).forEach(paramKey => {
            translation = translation.replace(new RegExp(`\\{${paramKey}\\}`, 'g'), params[paramKey]);
        });
        
        return translation;
    }

    /**
     * 队列缺失的翻译键
     */
    queueMissingKey(key) {
        if (!this.missingKeys.has(key)) {
            this.missingKeys.add(key);
            
            // 清除之前的定时器
            if (this.batchTimer) {
                clearTimeout(this.batchTimer);
            }
            
            // 设置新的定时器，延迟批量加载
            this.batchTimer = setTimeout(() => {
                this.loadMissingKeys();
            }, 100); // 100ms 延迟
        }
    }

    /**
     * 批量加载缺失的翻译键
     */
    async loadMissingKeys() {
        if (this.missingKeys.size === 0) return;
        
        const keysToLoad = Array.from(this.missingKeys);
        this.missingKeys.clear();
        
        try {
            const response = await fetch('/api/i18n/batch_translate', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'Accept-Language': this.currentLanguage
                },
                body: JSON.stringify({
                    keys: keysToLoad
                })
            });
            
            if (response.ok) {
                const data = await response.json();
                
                // 更新翻译缓存
                if (!this.translations[this.currentLanguage]) {
                    this.translations[this.currentLanguage] = {};
                }
                
                Object.assign(this.translations[this.currentLanguage], data.translations);
                
                // 重新翻译页面中使用这些键的元素
                this.updateAllTranslations();
            }
        } catch (error) {
            console.error('批量加载翻译失败:', error);
        }
    }

    /**
     * 更新页面方向
     */
    updatePageDirection() {
        const html = document.documentElement;
        const body = document.body;
        
        if (this.isRTL) {
            html.setAttribute('dir', 'rtl');
            html.setAttribute('lang', this.currentLanguage);
            body.classList.add('rtl');
            body.classList.remove('ltr');
        } else {
            html.setAttribute('dir', 'ltr');
            html.setAttribute('lang', this.currentLanguage);
            body.classList.add('ltr');
            body.classList.remove('rtl');
        }
    }

    /**
     * 更新页面中所有翻译文本
     */
    updateAllTranslations() {
        // 更新文本内容
        document.querySelectorAll('[data-i18n]').forEach(element => {
            const key = element.getAttribute('data-i18n');
            if (key) {
                element.textContent = this.translate(key);
            }
        });
        
        // 更新占位符
        document.querySelectorAll('[data-i18n-placeholder]').forEach(element => {
            const key = element.getAttribute('data-i18n-placeholder');
            if (key) {
                element.placeholder = this.translate(key);
            }
        });
        
        // 更新标题
        document.querySelectorAll('[data-i18n-title]').forEach(element => {
            const key = element.getAttribute('data-i18n-title');
            if (key) {
                element.title = this.translate(key);
            }
        });
    }

    /**
     * 重新加载翻译数据
     */
    async reloadTranslations() {
        try {
            const response = await fetch('/api/i18n/reload', {
                method: 'POST'
            });
            
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }
            
            // 清除缓存的翻译
            this.translations = {};
            
            // 重新加载当前语言的翻译
            await this.loadTranslations(this.currentLanguage);
            
            // 更新页面翻译
            this.updateAllTranslations();
            
            console.log('翻译数据已重新加载');
            
            // 触发重新加载事件
            this.dispatchEvent('i18n:reloaded', {
                language: this.currentLanguage
            });
            
            return true;
            
        } catch (error) {
            console.error('重新加载翻译失败:', error);
            return false;
        }
    }

    /**
     * 获取当前语言信息
     */
    getCurrentLanguageInfo() {
        return this.supportedLanguages.find(lang => lang.code === this.currentLanguage);
    }

    /**
     * 获取支持的语言列表
     */
    getSupportedLanguages() {
        return this.supportedLanguages;
    }

    /**
     * 触发自定义事件
     */
    dispatchEvent(eventName, detail) {
        const event = new CustomEvent(eventName, { detail });
        document.dispatchEvent(event);
    }

    /**
     * 更新语言切换器UI
     */
    updateLanguageSwitcherUI() {
        const languageSwitcher = document.querySelector('.language-switcher');
        
        if (!this.multilingualEnabled) {
            // 多语言功能关闭时，隐藏语言切换器
            if (languageSwitcher) {
                languageSwitcher.style.display = 'none';
            }
            console.log('多语言功能已关闭，隐藏语言切换器');
        } else {
            // 多语言功能开启时，显示语言切换器
            if (languageSwitcher) {
                languageSwitcher.style.display = '';
            }
            // 生成语言选项
            this.generateLanguageOptions();
            // 更新当前语言显示
            this.updateCurrentLanguageDisplay();
            console.log('多语言功能已开启，显示语言切换器');
        }
    }

    /**
     * 生成语言选项
     */
    generateLanguageOptions() {
        const dropdown = document.getElementById('languageDropdown');
        if (!dropdown) return;

        // 清空现有选项
        dropdown.innerHTML = '';

        // 为每种支持的语言创建选项
        this.supportedLanguages.forEach(lang => {
            const option = document.createElement('div');
            option.className = 'language-option';
            if (lang.code === this.currentLanguage) {
                option.classList.add('active');
            }

            // 获取语言标志
            const flag = this.getLanguageFlag(lang.code);
            
            option.innerHTML = `
                <span class="language-flag">${flag}</span>
                <div class="language-info">
                    <div class="language-name">${lang.name}</div>
                    <div class="language-native-name">${lang.native_name}</div>
                </div>
            `;

            // 添加点击事件
            option.addEventListener('click', async () => {
                await this.setLanguage(lang.code);
                this.updateCurrentLanguageDisplay();
                // 关闭下拉菜单
                dropdown.classList.remove('show');
            });

            dropdown.appendChild(option);
        });
    }

    /**
     * 获取语言标志
     */
    getLanguageFlag(langCode) {
        const flags = {
            'zh': '🇨🇳',
            'en': '🇺🇸', 
            'ar': '🇸🇦',
            'ug': '🇨🇳'
        };
        return flags[langCode] || '🌐';
    }

    /**
     * 更新当前语言显示
     */
    updateCurrentLanguageDisplay() {
        const currentLangInfo = this.getCurrentLanguageInfo();
        if (!currentLangInfo) return;

        const flagElement = document.getElementById('currentLanguageFlag');
        const nameElement = document.getElementById('currentLanguageName');

        if (flagElement) {
            flagElement.textContent = this.getLanguageFlag(currentLangInfo.code);
        }
        if (nameElement) {
            nameElement.textContent = currentLangInfo.native_name;
        }
    }
}

// 创建全局实例
window.i18n = new I18nManager();

// DOM加载完成后初始化
document.addEventListener('DOMContentLoaded', () => {
    window.i18n.init();
});

// 导出供其他模块使用
if (typeof module !== 'undefined' && module.exports) {
    module.exports = I18nManager;
}