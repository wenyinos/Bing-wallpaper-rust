# BingWallpaper-Rust

轻量级 Rust 壁纸客户端 + 可插拔 Wallpaper Provider 框架（Windows 7 SP1+ / x64）。

项目定位、总体架构与全部实施决策见 [bing-wallpaper-rust-总体实施方案.md](bing-wallpaper-rust-总体实施方案.md)（决策记录见第 42 节）。

## 技术栈

Rust 1.77.2 + **native-windows-gui**（Win32 原生控件，纯软件渲染零 GPU 依赖）+ reqwest（rustls + ring）+ tokio + windows-sys + tray-icon。

## 开发流程（2026-08-29 决策：构建单轨）

- **本机（Linux）只做语法/格式验证**：`cargo fmt --all`。rustup 会按 `rust-toolchain.toml` 自动使用 1.77.2。
- **编译全部由 GitHub Actions 承担**（windows runner + MSVC），产物在 Actions Artifacts（`BingWallpaper-Rust-x64`）。
- 不要在本机安装 mingw-w64 或任何 Windows target。
- 依赖版本必须满足 MSRV ≤ 1.77.2；`Cargo.lock` 提交入库，禁止常规 `cargo update`。

## 当前状态（v0.3 开发中；v0.2.0 已发布）

**v0.3 变更**：界面渲染从 egui/glow 改为 **native-windows-gui（Win32 原生控件 + GDI 纯软件渲染）**——彻底解决 Windows 下渲染出错；release 构建无 cmd 黑窗；新增 7 天内壁纸自动轮换（设置页可配间隔）；明确纯 Windows 项目（移除跨平台兼容层）。

- [x] 项目脚手架 + CI 构建工作流（P0）
- [x] Bing Provider（HPImageArchive，国内/国际双预设，默认国内）
- [x] 图片下载（超时、User-Agent、.part 原子写入）
- [x] 设置壁纸（Win10+ IDesktopWallpaper / Win7 回退 SystemParametersInfoW + WallpaperStyle）
- [x] 单实例互斥锁
- [x] 中英双语 i18n（自定义字符串表，决策 #14）（P1）
- [x] 缓存索引（cache/index.json，按天清理）（P1）
- [x] 定时调度（日期驱动：启动 30s / 每 5 分钟比对，决策 #10）（P1）
- [x] 系统托盘（tray-icon：打开/立即更新/下一张/历史/设置/关于/退出）（P1）
- [x] 开机启动（HKCU Run + --minimized）（P1）
- [x] 设置页（来源/预设/适配/语言/自动更新/自启动/缓存天数）
- [x] 历史壁纸（缩略图网格：设为壁纸/打开位置/删除）（P2）
- [x] 下一张轮换（fetch 列表游标）（P2）
- [x] 按天滚动日志文件（P2）
- [x] Provider Manifest（内置 bing + 用户目录覆盖）（P3）
- [x] 通用 JSON Provider（JSON Pointer 映射）/ URL Provider（P3）
- [x] Provider 在线更新（版本比较 + SHA-256 + 可选 ed25519 验签 + 热重载）（P4）

**重要**：全部代码仅通过本机 rustfmt 语法验证，编译验证依赖 GitHub Actions（windows/MSVC）；功能实测需 Windows 7/10/11 VM。

## Win7 兼容要点

- Rust 锁定 1.77.2（1.78 起 Windows 7 目标降为 Tier 3）
- 渲染仅 glow（Win7 无 DX12/Vulkan 路径）
- TLS 用 rustls + ring（Win7 的 schannel 加密套件老旧，不可依赖）
- CI 产物需在 Win7 VM 实测：HTTPS 握手、托盘、壁纸设置、DPI
