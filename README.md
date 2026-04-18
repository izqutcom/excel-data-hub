# Excel Data Hub 📊

[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![PostgreSQL](https://img.shields.io/badge/postgresql-12+-blue.svg)](https://www.postgresql.org)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

基于 Rust + PostgreSQL 的 Excel 数据检索平台，已升级为 **Workspace 模式**：
- 用户可注册/登录
- 用户可创建、编辑、删除 Workspace
- 在 Workspace 内上传 Excel（`.xlsx/.xls`）并自动入库
- Workspace 内搜索、导出
- 支持公开 Workspace 的全局搜索
- 支持中/英/阿/维四语界面

## 版本说明

当前版本：`v0.2.0`

`v0.2.0` 重点变化：
- 移除“程序启动自动扫描本地目录导入 Excel”机制
- 全面切换为“用户在 Workspace 中上传导入”
- 新增认证、Workspace 管理、上传进度、公开搜索、前端管理弹窗

## 核心能力

### 数据管理
- 支持用户注册/登录（Token 认证）
- 支持 Workspace 创建、编辑、删除
- 删除 Workspace 时，同时删除关联上传文件与数据库数据
- 上传支持多文件、进度反馈与防重复上传

### 搜索与导出
- Workspace 内搜索：只检索当前 Workspace 数据
- 公开全局搜索：不传 `workspace_id` 时检索公开 Workspace
- 搜索结果导出 Excel
- 统计接口支持 Workspace 维度与公开全局维度

### 国际化
- 支持 `zh / en / ar / ug`
- 前端动态切换语言，含新增功能文案的实时翻译

### 数据库
- 程序启动自动建表与增量升级（Rust 内完成）
- 自动创建索引（搜索、时间、关联字段）

---

## 快速开始

### 1. 环境要求
- Rust 1.70+
- PostgreSQL 12+

### 2. 克隆项目
```bash
git clone https://github.com/izqutcom/excel-data-hub.git
cd excel-data-hub
```

### 3. 准备 PostgreSQL
```sql
CREATE DATABASE excel;
```

### 4. 配置环境变量
复制配置模板：
```bash
cp .env.example .env
```

示例 `.env`：
```env
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/excel
UPLOAD_DIR=./uploads
PORT=8000
RUST_LOG=info

ENABLE_MULTILINGUAL=true
DEFAULT_LANGUAGE=zh
SUPPORTED_LANGUAGES=zh,en,ar,ug
LOCALES_PATH=./locales
ENABLE_AUTO_DETECT=true
CACHE_TRANSLATIONS=true
CACHE_EXPIRE_MINUTES=60
```

### 5. 运行
```bash
cargo run
```

访问：
`http://localhost:8000`

---

## 使用流程

1. 首次进入：注册账号并登录  
2. 打开“工作区管理”：创建 Workspace  
3. 在 Workspace 内上传 Excel  
4. 上传后自动导入并建立检索索引  
5. 在搜索框中按 Workspace 搜索或公开全局搜索  
6. 需要时导出搜索结果为 Excel

---

## API 概览

### 认证
- `POST /api/auth/register` 注册并返回 Token
- `POST /api/auth/login` 登录并返回 Token

### Workspace
- `GET /api/workspaces` 列表（公开 + 当前用户自己的）
- `POST /api/workspaces` 创建
- `PUT /api/workspaces/{id}` 编辑
- `DELETE /api/workspaces/{id}` 删除（级联删除数据与文件）
- `POST /api/workspaces/{id}/upload` 上传并导入

### 搜索与统计
- `GET /api/search?q=...&workspace_id=...`
- `GET /api/stats?workspace_id=...`
- `GET /api/export?q=...&workspace_id=...`

说明：
- 传 `workspace_id`：按该 Workspace 作用域
- 不传 `workspace_id`：按公开 Workspace 全局作用域

### i18n
- `GET /api/i18n/status`
- `GET /api/i18n/languages`
- `GET /api/i18n/translate/{key}`
- `POST /api/i18n/batch_translate`
- `POST /api/i18n/reload`

---

## 环境变量

| 变量名 | 说明 | 默认值 |
|---|---|---|
| `DATABASE_URL` | PostgreSQL 连接字符串 | - |
| `UPLOAD_DIR` | 上传文件存储目录 | `./uploads` |
| `PORT` | Web 服务端口 | `8000` |
| `RUST_LOG` | 日志级别 | `info` |
| `ENABLE_MULTILINGUAL` | 是否启用多语言 | `true` |
| `DEFAULT_LANGUAGE` | 默认语言 | `zh` |
| `SUPPORTED_LANGUAGES` | 支持语言列表 | `zh,en,ar,ug` |
| `LOCALES_PATH` | 语言包目录 | `./locales` |
| `ENABLE_AUTO_DETECT` | 自动语言检测 | `true` |
| `CACHE_TRANSLATIONS` | 翻译缓存开关 | `true` |
| `CACHE_EXPIRE_MINUTES` | 翻译缓存过期分钟数 | `60` |

---

## 数据表

系统自动维护以下核心表：
- `users`
- `auth_tokens`
- `workspaces`
- `files`
- `excel_data`

---

## 项目结构

```text
src/
├── main.rs
├── web_server.rs
├── excel_processor_sea.rs
├── database_sea.rs
├── i18n_manager.rs
└── models/

static/
├── css/
└── js/

locales/
├── zh.json
├── en.json
├── ar.json
└── ug.json
```

---

## 许可证

MIT License，详见 `LICENSE`。
