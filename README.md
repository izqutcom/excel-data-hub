# Excel Data Hub - Rust版

这是一个基于Rust开发的Excel数据处理和搜索系统，支持从Excel文件中读取数据并存储到PostgreSQL数据库中，提供强大的搜索功能。

## 功能特性

- 📊 **Excel文件处理**: 支持.xlsx和.xls格式的Excel文件
- 🔍 **全文搜索**: 快速搜索Excel数据内容
- 🗄️ **PostgreSQL存储**: 使用PostgreSQL数据库存储数据，支持高性能查询
- 🌐 **Web界面**: 提供友好的Web搜索界面
- 📈 **统计信息**: 实时显示文件数量和数据统计

## 系统要求

- Rust 1.70+
- PostgreSQL 12+
- Docker (如果使用容器化PostgreSQL)

## 配置

系统通过`.env`文件进行配置：

```env
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/excel
EXCEL_FOLDER_PATH=/path/to/your/excel/files
PORT=8000
RUST_LOG=info
```

## 运行

1. 确保PostgreSQL数据库运行在Docker容器中
2. 配置`.env`文件中的数据库连接信息
3. 运行程序：

```bash
cargo run
```

4. 访问Web界面：http://localhost:8000

## API接口

- `GET /api/stats` - 获取统计信息
- `GET /api/search?q=关键词&limit=20&offset=0` - 搜索数据

## 数据库架构

系统会自动创建以下表结构：

- `files` - 存储Excel文件元数据
- `excel_data` - 存储Excel数据内容
- 相关索引用于优化搜索性能

## 注意事项

- 系统仅支持PostgreSQL数据库
- Excel文件会自动从配置的文件夹中导入
- 支持增量更新，重复运行不会产生重复数据