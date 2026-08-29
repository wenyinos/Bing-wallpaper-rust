# BingWallpaper-Rust

轻量级 Rust 壁纸客户端 + 可插拔 Wallpaper Provider 框架（Windows 7 SP1+ / x64）。

项目定位、总体架构与全部实施决策见 [bing-wallpaper-rust-总体实施方案.md](bing-wallpaper-rust-总体实施方案.md)（决策记录见第 42 节）。

## 技术栈

Rust 1.77.2 + eframe/egui（glow 后端）+ reqwest（rustls + ring）+ tokio + windows-sys。

## 开发流程（2026-08-29 决策：构建单轨）

- **本机（Linux）只做语法/格式验证**：`cargo fmt --all`。rustup 会按 `rust-toolchain.toml` 自动使用 1.77.2。
- **编译全部由 GitHub Actions 承担**（windows runner + MSVC），产物在 Actions Artifacts（`BingWallpaper-Rust-x64`）。
- 不要在本机安装 mingw-w64 或任何 Windows target。
- 依赖版本必须满足 MSRV ≤ 1.77.2；`Cargo.lock` 提交入库，禁止常规 `cargo update`。

## 当前状态（P0）

- [x] 项目脚手架 + CI 构建工作流
- [x] Bing Provider（HPImageArchive，国内/国际双预设，默认国内）
- [x] 图片下载（超时、User-Agent、.part 原子写入）
- [x] 设置壁纸（SystemParametersInfoW + 注册表 WallpaperStyle，默认 Fill）
- [x] 单实例互斥锁
- [ ] 系统托盘、配置 UI、定时更新、历史壁纸（P1/P2）

## Win7 兼容要点

- Rust 锁定 1.77.2（1.78 起 Windows 7 目标降为 Tier 3）
- 渲染仅 glow（Win7 无 DX12/Vulkan 路径）
- TLS 用 rustls + ring（Win7 的 schannel 加密套件老旧，不可依赖）
- P0 交付后需在 Win7 VM 实测：HTTPS 握手、托盘、壁纸设置、DPI
