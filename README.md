# Excel Data Hub 📊

[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![PostgreSQL](https://img.shields.io/badge/postgresql-12+-blue.svg)](https://www.postgresql.org)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

一个基于 Rust 开发的高性能 Excel 数据处理和搜索系统，支持多语言界面，提供强大的全文搜索功能和现代化的 Web 界面。

## ✨ 功能特性

### 📈 核心功能
- **Excel 文件处理**: 支持 `.xlsx` 和 `.xls` 格式的 Excel 文件批量导入
- **全文搜索**: 基于 PostgreSQL 的高性能全文搜索，支持多关键词匹配
- **数据导出**: 支持搜索结果导出为 Excel 格式
- **实时统计**: 提供文件数量、记录总数等实时统计信息
- **增量更新**: 智能检测文件变化，避免重复导入

### 🌐 多语言支持
- **四种语言**: 中文、英文、阿拉伯语、维吾尔语
- **动态切换**: 无需刷新页面即可切换语言
- **本地化界面**: 完整的界面本地化支持
- **自动检测**: 支持浏览器语言自动检测

### 🎨 现代化界面
- **响应式设计**: 适配桌面和移动设备
- **Excel 风格**: 熟悉的 Excel 表格样式
- **实时搜索**: 即时显示搜索结果
- **浏览器兼容**: 支持 Chrome 69+ 等旧版浏览器

### ⚡ 高性能架构
- **异步处理**: 基于 Tokio 的异步 I/O
- **并发导入**: 支持多文件并发处理
- **数据库优化**: PostgreSQL 全文索引优化
- **内存管理**: 高效的内存使用和垃圾回收

## 🚀 快速开始

### 系统要求

- **Rust**: 1.70 或更高版本
- **PostgreSQL**: 12 或更高版本
- **操作系统**: Windows, macOS, Linux

### 安装步骤

1. **克隆项目**
```bash
git clone https://github.com/your-username/excel-data-hub.git
cd excel-data-hub
```

2. **安装 Rust**
```bash
# 如果还没有安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

3. **设置 PostgreSQL**

使用 Docker 快速启动 PostgreSQL：
```bash
docker run --name excel-postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=excel \
  -p 5432:5432 \
  -d postgres:14
```

或手动安装 PostgreSQL 并创建数据库：
```sql
CREATE DATABASE excel;
```

4. **配置环境变量**

复制并编辑配置文件：
```bash
cp .env.example .env
```

编辑 `.env` 文件：
```env
# 数据库连接
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/excel

# Excel 文件路径
EXCEL_FOLDER_PATH=./excel_files

# 服务器配置
PORT=8000
RUST_LOG=info

# 多语言配置
ENABLE_MULTILINGUAL=true
DEFAULT_LANGUAGE=zh
SUPPORTED_LANGUAGES=zh,en,ar,ug
```

5. **创建 Excel 文件目录**
```bash
mkdir excel_files
# 将你的 Excel 文件放入此目录
```

6. **运行项目**
```bash
cargo run
```

7. **访问应用**

打开浏览器访问: http://localhost:8000

## 📖 使用指南

### Excel 文件导入

1. 将 Excel 文件放入配置的 `EXCEL_FOLDER_PATH` 目录
2. 启动程序，系统会自动扫描并导入文件
3. 支持的文件格式：`.xlsx`, `.xls`
4. 系统会自动检测文件变化，避免重复导入

### 搜索功能

- **单关键词搜索**: 直接输入关键词
- **多关键词搜索**: 用空格分隔多个关键词
- **搜索示例**: 
  - `张三` - 搜索包含"张三"的记录
  - `张三 1990` - 搜索同时包含"张三"和"1990"的记录

### 数据导出

1. 执行搜索查询
2. 点击"导出Excel"按钮
3. 系统会生成包含搜索结果的 Excel 文件

### 语言切换

1. 点击右上角的语言切换按钮 🌐
2. 选择目标语言
3. 界面会立即切换到选定语言

## 🔧 API 文档

### 搜索接口
```http
GET /api/search?q=关键词&limit=20&offset=0
```

**参数说明:**
- `q`: 搜索关键词（必需）
- `limit`: 返回结果数量限制（默认: 20）
- `offset`: 分页偏移量（默认: 0）

**响应示例:**
```json
{
  "results": [
    {
      "id": 1,
      "file_name": "员工信息.xlsx",
      "sheet_name": "Sheet1",
      "row_number": 2,
      "data_json": "{\"姓名\":\"张三\",\"年龄\":\"30\"}",
      "import_time": "2024-01-01T10:00:00Z"
    }
  ],
  "total": 100,
  "limit": 20,
  "offset": 0
}
```

### 统计接口
```http
GET /api/stats
```

**响应示例:**
```json
{
  "total_files": 10,
  "total_records": 1000,
  "last_updated": "2024-01-01T10:00:00Z"
}
```

### 导出接口
```http
GET /api/export?q=关键词
```

返回 Excel 文件的二进制数据。

## ⚙️ 配置选项

### 环境变量说明

| 变量名 | 说明 | 默认值 |
|--------|------|--------|
| `DATABASE_URL` | PostgreSQL 连接字符串 | - |
| `EXCEL_FOLDER_PATH` | Excel 文件存储路径 | `./excel_files` |
| `PORT` | Web 服务器端口 | `8000` |
| `RUST_LOG` | 日志级别 | `info` |
| `FORCE_REIMPORT` | 是否强制重新导入 | `false` |
| `MAX_CONCURRENT_FILES` | 并发处理文件数 | `4` |
| `ENABLE_MULTILINGUAL` | 启用多语言支持 | `true` |
| `DEFAULT_LANGUAGE` | 默认语言 | `zh` |
| `SUPPORTED_LANGUAGES` | 支持的语言列表 | `zh,en,ar,ug` |

### 数据库配置

系统会自动创建以下表结构：

- `files`: 存储 Excel 文件元数据
- `excel_data`: 存储 Excel 数据内容
- 自动创建全文搜索索引以优化查询性能

## 🏗️ 项目架构

```
src/
├── main.rs              # 程序入口
├── web_server.rs        # Web 服务器和路由
├── excel_processor_sea.rs # Excel 处理逻辑
├── database_sea.rs      # 数据库连接管理
├── i18n_manager.rs      # 多语言管理
├── models/              # 数据模型
└── utils.rs             # 工具函数

static/
├── css/                 # 样式文件
└── js/                  # JavaScript 文件

locales/                 # 多语言文件
├── zh.json             # 中文
├── en.json             # 英文
├── ar.json             # 阿拉伯语
└── ug.json             # 维吾尔语
```

## 🤝 贡献指南

我们欢迎所有形式的贡献！

### 如何贡献

1. Fork 本项目
2. 创建特性分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 创建 Pull Request

### 开发环境设置

```bash
# 克隆项目
git clone https://github.com/your-username/excel-data-hub.git
cd excel-data-hub

# 安装依赖
cargo build

# 运行测试
cargo test

# 运行开发服务器
cargo run
```

### 代码规范

- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 检查代码质量
- 为新功能添加测试
- 更新相关文档

## 📝 更新日志

### v0.1.0 (2024-01-01)
- ✨ 初始版本发布
- 📊 Excel 文件处理功能
- 🔍 全文搜索功能
- 🌐 多语言支持
- 📱 响应式 Web 界面

## 📄 许可证

本项目采用 MIT 许可证 - 查看 [LICENSE](LICENSE) 文件了解详情。

## 🙏 致谢

- [Rust](https://www.rust-lang.org/) - 系统编程语言
- [Axum](https://github.com/tokio-rs/axum) - Web 框架
- [SeaORM](https://www.sea-ql.org/SeaORM/) - ORM 框架
- [Calamine](https://github.com/tafia/calamine) - Excel 文件处理
- [PostgreSQL](https://www.postgresql.org/) - 数据库系统

## 📞 联系方式

- 项目主页: https://github.com/your-username/excel-data-hub
- 问题反馈: https://github.com/your-username/excel-data-hub/issues
- 邮箱: your-email@example.com

---

⭐ 如果这个项目对你有帮助，请给我们一个 Star！