# BingWallpaper-Rust

轻量级 Rust 壁纸客户端 + 可插拔 Wallpaper Provider 框架。
目标平台：**Windows 7 SP1+（x64 / x86）**，纯软件渲染，零 GPU 依赖。

- 最新版本：[Releases](https://github.com/wenyinos/Bing-wallpaper-rust/releases)
- 架构设计与全部实施决策：[bing-wallpaper-rust-总体实施方案.md](bing-wallpaper-rust-总体实施方案.md)（决策记录见第 42 节）

## 功能

- **Bing 每日壁纸**：自动获取/下载/设置，国内（cn.bing.com / zh-CN）与国际（www.bing.com / en-US）双预设可切换
- **日期驱动自动更新**：启动 30 秒首查 + 每 5 分钟比对壁纸日期（睡眠唤醒自适应），跨天必更新、同日不重复
- **7 天内壁纸自动轮换**：按可配间隔（默认 60 分钟，0 关闭）在最近 7 天壁纸间轮换
- **获取前 7 天壁纸**：一键批量下载最近 7 天壁纸入库，历史列表即点即用
- **历史壁纸**：列表 + 缩略图预览，设为壁纸 / 打开文件位置 / 删除
- **可插拔 Provider**：内置 Bing，支持 JSON Pointer 映射的通用 JSON Provider、URL Provider，放置清单到 `providers\` 目录即自动加载
- **Provider 在线更新**：版本比较 + SHA-256 校验 + 可选 ed25519 签名验证，热重载
- **系统托盘**：左键单击/双击打开窗口，右键菜单（打开/立即更新/下一张/历史/设置/关于/退出）；关窗进托盘
- **开机自启动**（HKCU Run，`--minimized` 静默启动）、单实例互斥、中英双语界面
- **多显示器**：Win10+ 走 IDesktopWallpaper（支持跨屏），Win7 回退 SystemParametersInfoW
- **高 DPI 适配**：按系统 DPI 缩放布局，高分屏清晰不过小

## 技术栈

Rust 1.77.2 + **native-windows-gui**（Win32 原生控件 + GDI，纯软件渲染零 GPU 依赖）+ reqwest（rustls + ring）+ tokio + windows-sys + tray-icon。release 构建为窗口子系统（无 cmd 黑窗），嵌入应用清单（Common Controls v6 / OS 兼容声明 / DPI 感知）。

## 下载

从 [Releases](https://github.com/wenyinos/Bing-wallpaper-rust/releases) 下载对应架构：

- `BingWallpaper-Rust-x64.exe`：64 位系统
- `BingWallpaper-Rust-x86.exe`：32 位系统

单文件免安装（Portable），数据目录 `%LOCALAPPDATA%\BingWallpaper-Rust\`。程序基于 GPL-3.0 授权发布，不含任何担保。

## 开发与构建（2026-08-29 决策：构建单轨）

- 本项目为**纯 Windows 项目**；编译验证以 GitHub Actions 为准（windows runner + MSVC，双架构矩阵）。
- 本机仅做 `cargo fmt --all` 语法/格式检查；rustup 按 `rust-toolchain.toml` 自动使用 1.77.2。不要安装 mingw-w64 或任何 Windows target。
- **发布**：推送 `v*` tag，CI 双架构构建后自动附加产物到同名 Release，无需手动上传。
- 依赖必须满足 MSRV ≤ 1.77.2；`Cargo.lock` 入库，禁止常规 `cargo update`。2025-09 后部分传递依赖新版要求 edition2024/rustc ≥1.81，已在 `Cargo.toml` 用 `=版本` 钉死（升级工具链前勿动，详见方案文档 §42）。

## 版本历史

| 版本 | 要点 |
|------|------|
| v0.4.1 | 高 DPI 布局按系统 DPI 缩放；修复语言切换死锁（配置锁重入） |
| v0.4.0 | 历史页"获取前 7 天壁纸"；托盘左键单击/双击打开窗口；设置页下拉修复；关于页版权信息 |
| v0.3.x | 渲染迁移到 native-windows-gui 纯软件渲染（替换 egui/glow）；中文字体统一系统雅黑；无 cmd 黑窗；应用清单（Common Controls v6 / DPI / OS 声明）；修复启动崩溃（FFI 回调内 panic）；7 天壁纸自动轮换 |
| v0.2.0 | Provider 在线更新 + Manifest；界面 nwg 前的最后一代 egui 构建 |
| v0.1.0 | 首个版本：Bing 拉取/下载/设壁纸 + 托盘/调度/缓存/多语言 |

## Win7 兼容要点

- Rust 锁定 1.77.2（1.78 起 Windows 7 目标降为 Tier 3）
- 渲染为 Win32 GDI 原生控件（纯软件，任何显卡/虚拟机/远程桌面可用；v0.3 前的 egui/glow 已废弃）
- TLS 用 rustls + ring（Win7 的 schannel 加密套件老旧，不可依赖）
- 嵌入 Common Controls v6 清单（否则报"无法定位程序输入点 GetWindowSubclass"）
- 待持续实测项：Win7 VM 中的 HTTPS 握手、托盘行为、壁纸设置、DPI 表现
