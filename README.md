# WorkMirror - 你的本地AI工作效能镜子

> A privacy-first, 100% local AI work tracker that shows you how you really work.

<p align="center">
  <img src="demo.gif" alt="WorkMirror Demo" width="720">
</p>

<p align="center">
  <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License MIT">
  <img src="https://img.shields.io/badge/rust-1.85+-orange.svg" alt="Rust">
  <img src="https://img.shields.io/badge/platform-windows%20%7C%20macOS%20%7C%20linux-lightgrey" alt="Platform">
  <img src="https://img.shields.io/badge/AI-llama3.2%20%7C%20Ollama-brightgreen" alt="AI">
</p>

---

## ✨ 核心特性

| | Feature | Description |
|---|---|---|
| 🔒 | **100%本地运行** | 所有数据存储在你的机器上，绝不离开。无需账户，无需注册。 |
| 🧠 | **本地AI分析** | 通过 Ollama 运行本地 LLM，分析你的工作模式。断网也能用。 |
| 🎛️ | **完全控制** | 你决定追踪什么、什么时候追踪、数据保留多久。 |
| 📊 | **自动周报** | 每周自动生成本地报告，包含深度工作时间、应用分布、AI洞察。 |
| 🆓 | **开源免费** | MIT 许可证，自由使用、修改、分发。 |

## 🚀 快速开始

### 前置条件

- [Rust](https://rustup.rs) 1.85+
- [Node.js](https://nodejs.org) 20+
- [pnpm](https://pnpm.io) 9+
- (可选) [Ollama](https://ollama.ai) - 用于本地AI分析

### 手动安装

```bash
# 1. 克隆项目
git clone https://github.com/Q04291/WorkMirror.git
cd workmirror

# 2. 安装前端依赖
pnpm install

# 3. 运行（同时启动前端 + Rust后端）
pnpm tauri dev
```

### 使用 Docker（仅后端）

```bash
# 构建 Docker 镜像
docker build -t workmirror .

# 运行
docker run -d \
  -v workmirror-data:/app/data \
  -p 1420:1420 \
  workmirror
```

## 📋 系统要求

| 平台 | 支持 | 最低要求 |
|------|------|---------|
| Windows 10/11 | ✅ | 4GB RAM, 500MB 磁盘 |
| macOS 12+ | ✅ | Intel 或 Apple Silicon |
| Linux (X11/Wayland) | ✅ | 4GB RAM, 500MB 磁盘 |

**可选：AI 分析** — 需要 [Ollama](https://ollama.ai) + llama3.2:3b 或更大模型（推荐 8GB+ RAM）。

## 🏗️ 架构

```
workmirror/
├── src/                    # 前端 (Solid.js + TypeScript)
│   ├── App.tsx             # 主应用 + 路由
│   ├── pages/              # 页面组件
│   │   ├── Dashboard.tsx   # 仪表盘
│   │   ├── Details.tsx     # 详细数据
│   │   ├── Report.tsx      # 报告
│   │   └── Settings.tsx    # 设置
│   └── styles.css          # Tailwind 样式
├── src-tauri/              # 后端 (Rust)
│   ├── src/
│   │   ├── main.rs         # Tauri 入口
│   │   ├── lib.rs          # 模块注册 + 初始化
│   │   ├── commands.rs     # Tauri 命令桥接
│   │   ├── db/             # 加密数据库层
│   │   ├── security/       # AES-256-GCM 加密
│   │   ├── tracker/        # 窗口活动追踪
│   │   ├── ai/             # Ollama AI 引擎
│   │   └── reporter/       # HTML/PDF 报告生成
│   ├── templates/           # Handlebar 模板
│   └── Cargo.toml          # Rust 依赖
├── package.json            # 前端依赖
└── tailwind.config.js      # Tailwind 配置
```

**数据流：**

```
[窗口活动] → [Tracker] → [加密 SQLite] → [Analyzer] → [Ollama]
                      ↑                              ↓
                  [Tauri Commands] ← [Report Generator] ← [Frontend]
```

## 🤝 贡献指南

欢迎贡献！请遵循以下步骤：

1. Fork 项目
2. 创建特性分支: `git checkout -b feat/my-feature`
3. 提交更改: `git commit -m 'feat: add something'`
4. 推送: `git push origin feat/my-feature`
5. 提交 Pull Request

### 开发准则

- Rust 代码：零 `unsafe` 块，所有测试通过
- 前端代码：TypeScript，Tailwind 类名排序
- 提交信息：遵循 [Conventional Commits](https://www.conventionalcommits.org/)

## 📄 许可证

[MIT License](LICENSE)

Copyright (c) 2026 WorkMirror

---

<p align="center">
  <sub>Built with ❤️ and 🦀 Rust + Solid.js by <b>QRiven</b></sub>
</p>
