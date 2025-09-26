# 贡献指南

感谢您对 Excel Data Hub 项目的关注！我们欢迎所有形式的贡献，包括但不限于：

- 🐛 报告 Bug
- 💡 提出新功能建议
- 📝 改进文档
- 🔧 提交代码修复
- 🌐 添加多语言支持

## 开始之前

在开始贡献之前，请确保您已经：

1. 阅读了项目的 [README.md](README.md)
2. 了解项目的基本架构和功能
3. 设置了本地开发环境

## 开发环境设置

### 1. 克隆项目

```bash
git clone https://github.com/your-username/excel-data-hub.git
cd excel-data-hub
```

### 2. 安装依赖

确保您已安装：
- Rust 1.70+
- PostgreSQL 12+
- Git

### 3. 设置数据库

```bash
# 使用 Docker 启动 PostgreSQL
docker run --name excel-postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=excel \
  -p 5432:5432 \
  -d postgres:14
```

### 4. 配置环境变量

```bash
cp .env.example .env
# 编辑 .env 文件，设置正确的数据库连接信息
```

### 5. 运行项目

```bash
cargo run
```

## 贡献流程

### 1. 创建 Issue

在开始编码之前，请先创建一个 Issue 来描述：
- 要修复的 Bug
- 要添加的新功能
- 要改进的文档

### 2. Fork 项目

点击项目页面右上角的 "Fork" 按钮，将项目 Fork 到您的 GitHub 账户。

### 3. 创建分支

```bash
git checkout -b feature/your-feature-name
# 或
git checkout -b fix/your-bug-fix
```

分支命名规范：
- `feature/` - 新功能
- `fix/` - Bug 修复
- `docs/` - 文档更新
- `refactor/` - 代码重构

### 4. 编写代码

请遵循以下代码规范：

#### Rust 代码规范
- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 检查代码质量
- 为公共函数添加文档注释
- 编写单元测试

#### 提交信息规范
使用以下格式编写提交信息：

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

类型（type）：
- `feat`: 新功能
- `fix`: Bug 修复
- `docs`: 文档更新
- `style`: 代码格式调整
- `refactor`: 代码重构
- `test`: 测试相关
- `chore`: 构建过程或辅助工具的变动

示例：
```
feat(search): add multi-keyword search support

- Implement keyword splitting logic
- Add relevance scoring for search results
- Update search API documentation

Closes #123
```

### 5. 运行测试

在提交之前，请确保所有测试通过：

```bash
# 运行测试
cargo test

# 检查代码格式
cargo fmt --check

# 运行 Clippy 检查
cargo clippy -- -D warnings
```

### 6. 提交 Pull Request

1. 推送您的分支到 GitHub
2. 在 GitHub 上创建 Pull Request
3. 填写 PR 模板，详细描述您的更改
4. 等待代码审查

## 代码审查

所有的 Pull Request 都需要经过代码审查。审查过程中可能会要求您：

- 修改代码实现
- 添加测试用例
- 更新文档
- 调整代码格式

请耐心配合审查过程，这有助于保持项目的代码质量。

## 多语言支持

如果您想为项目添加新的语言支持：

1. 在 `locales/` 目录下创建新的语言文件（如 `fr.json`）
2. 参考现有的语言文件结构添加翻译
3. 更新 `.env` 文件中的 `SUPPORTED_LANGUAGES` 配置
4. 在 `src/i18n_manager.rs` 中添加语言信息

## 报告 Bug

报告 Bug 时，请提供以下信息：

- 操作系统和版本
- Rust 版本
- PostgreSQL 版本
- 详细的错误信息
- 重现步骤
- 预期行为和实际行为

## 功能建议

提出新功能建议时，请说明：

- 功能的用途和价值
- 详细的功能描述
- 可能的实现方案
- 是否愿意参与开发

## 文档贡献

文档改进包括：

- 修正错别字和语法错误
- 添加使用示例
- 改进 API 文档
- 翻译文档到其他语言

## 社区准则

参与项目时，请遵守以下准则：

- 保持友善和尊重
- 欢迎新贡献者
- 提供建设性的反馈
- 专注于项目目标

## 获得帮助

如果您在贡献过程中遇到问题，可以：

- 在 Issue 中提问
- 发送邮件到 your-email@example.com
- 查看项目文档

## 许可证

通过向本项目贡献代码，您同意您的贡献将在 MIT 许可证下发布。

---

再次感谢您的贡献！🎉