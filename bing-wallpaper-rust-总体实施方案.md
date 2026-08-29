# Rust Bing Wallpaper 桌面应用总体实施方案

## 1. 项目定位

项目名称暂定：**Bing Wallpaper Rust**

项目定位不是单纯的 Bing 壁纸客户端，而是一个：

> **面向 Windows 7 及以上系统的轻量级 Rust 壁纸客户端 + 可插拔 Wallpaper Provider 数据源框架**

第一阶段以内置 Bing Wallpaper 为主要数据源，后续可逐步增加 NASA、第三方图片 API、自建 API、本地目录、NAS、固定图片 URL 等来源。

核心原则：

- Windows 7 是硬性最低支持版本
- 核心业务逻辑长期稳定
- Rust 工具链和依赖版本可锁定
- Provider 与核心程序解耦
- 数据源可以独立更新
- 尽量避免 WebView、Chromium、Node.js 等运行时依赖
- 支持 Windows 7 / 8 / 8.1 / 10 / 11
- 优先支持 x64，可根据实际需求增加 x86
- 软件体积小、内存占用低、启动速度快

---

# 2. 总体技术架构

```text
                         Wallpaper Client
                                │
                ┌───────────────┴───────────────┐
                │                               │
          Application Core                    UI
                │                               │
       ┌────────┼────────┐              egui / eframe
       │        │        │
   Provider   Cache   Scheduler
       │        │        │
       │        │        └──────────────┐
       │        │                       │
       │   Image Processor          Wallpaper Manager
       │                                │
       │                              Win32
       │                                │
       │                         Windows Desktop
       │
 ┌─────┴───────────────────────────────┐
 │                                     │
Bing        JSON API       URL       Local/NAS
Provider    Provider       Provider   Provider
```

核心层不关心壁纸来源。

统一数据流：

```text
Provider
   ↓
Wallpaper Metadata
   ↓
Downloader
   ↓
Image Validation
   ↓
Cache
   ↓
Image Processor
   ↓
Wallpaper Manager
   ↓
Windows Desktop
```

---

# 3. 推荐技术栈

## 3.1 Rust

建议第一阶段锁定：

```text
Rust 1.77.2
```

原因：

- Windows 7 兼容路线相对成熟
- 避免未来 Rust 默认 Windows 最低版本变化影响 Win7
- 保证构建环境长期可复现

项目根目录加入：

```text
rust-toolchain.toml
Cargo.lock
```

禁止开发环境无约束地自动升级 Rust 或依赖。

---

## 3.2 GUI

推荐：

```text
egui
eframe
winit
```

方案定位：

- egui：UI
- eframe：应用框架（锁定 0.27~0.28，MSRV ≤ 1.77.2）
- winit：窗口和输入
- Windows API：系统级功能

渲染后端已决策：仅使用 glow（OpenGL 2.0+，Win7 显卡驱动原生支持），
Cargo features 明确禁用 wgpu（Win7 无 DX12 运行时、老显卡普遍无 Vulkan 驱动）。

不采用：

```text
Electron
Tauri + WebView2
Chromium
Node.js
```

主要原因是降低运行时依赖，并减少 Windows 7 兼容性风险。

---

## 3.3 Windows API

推荐：

```text
windows-sys / windows-rs
```

负责：

- 设置桌面壁纸
- 系统托盘
- Windows 窗口
- 多显示器
- 开机启动
- 注册表
- 系统菜单
- DPI
- 系统通知
- 文件路径
- Windows 特有功能

---

## 3.4 网络

推荐：

```text
reqwest
```

负责：

- Bing API
- JSON API
- 图片下载
- HTTP/HTTPS
- 超时
- 重试
- HTTP Header
- User-Agent

已决策：采用 reqwest + tokio 异步方案，Provider trait 的 fetch 为 async 签名。

TLS 后端采用 rustls 并显式指定 ring provider（rustls 0.23 默认的 aws-lc-rs
需要 CMake/NASM 且构建链复杂；ring 纯 Rust 且 Win7 可用）。
不采用 native-tls/schannel：Win7 SP1 的 schannel 加密套件老旧，与 Bing CDN
（Azure Front Door）握手存在失败风险，rustls 自带现代套件反而更稳。
`ureq` 不再评估。

---

## 3.5 JSON

```text
serde
serde_json
```

用于：

- Bing API
- Provider Manifest
- 软件配置
- 壁纸元数据
- Provider 配置

---

## 3.6 图片

```text
image
```

负责：

- JPG
- PNG
- WebP（根据版本及需求）
- 图片尺寸检测
- 图片解码
- 缩放
- 裁剪
- 缩略图生成

---

# 4. 项目目录设计

推荐结构：

```text
bing-wallpaper/
│
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── README.md
├── LICENSE
│
├── src/
│   ├── main.rs
│   │
│   ├── app/
│   │   ├── mod.rs
│   │   ├── state.rs
│   │   └── config.rs
│   │
│   ├── provider/
│   │   ├── mod.rs
│   │   ├── trait.rs
│   │   ├── manager.rs
│   │   ├── bing.rs
│   │   ├── json.rs
│   │   └── url.rs
│   │
│   ├── downloader/
│   │   ├── mod.rs
│   │   ├── client.rs
│   │   └── retry.rs
│   │
│   ├── cache/
│   │   ├── mod.rs
│   │   ├── manager.rs
│   │   └── index.rs        # JSON 索引（已决策不使用 SQLite）
│   │
│   ├── image/
│   │   ├── mod.rs
│   │   ├── processor.rs
│   │   └── thumbnail.rs
│   │
│   ├── wallpaper/
│   │   ├── mod.rs
│   │   ├── windows.rs
│   │   └── monitor.rs
│   │
│   ├── scheduler/
│   │   └── mod.rs
│   │
│   ├── tray/
│   │   └── mod.rs
│   │
│   └── ui/
│       ├── mod.rs
│       ├── main_window.rs
│       ├── settings.rs
│       ├── history.rs
│       └── provider.rs
│
├── assets/
│   ├── icon.ico
│   └── images/
│
├── providers/
│   ├── bing.json
│   └── examples/
│
├── installer/
│   └── windows/
│
└── docs/
    ├── architecture.md
    ├── provider.md
    └── build-win7.md
```

---

# 5. Provider 架构

Provider 是整个项目最重要的扩展设计。

核心接口：

```rust
pub trait WallpaperProvider {
    fn id(&self) -> &str;
    fn name(&self) -> &str;

    fn fetch(
        &self,
        context: &ProviderContext
    ) -> Result<Vec<Wallpaper>, ProviderError>;
}
```

统一壁纸结构：

```rust
pub struct Wallpaper {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub image_url: String,
    pub thumbnail_url: Option<String>,
    pub copyright: Option<String>,
    pub source: String,
    pub published_at: Option<DateTime<Utc>>,
}
```

核心程序只处理 `Wallpaper`，不直接依赖 Bing 的 JSON 结构。

---

# 6. Provider 类型

## 6.1 Bing Provider

第一阶段唯一正式内置 Provider。

数据来源：

```text
Bing HPImageArchive API
```

主要参数：

```text
市场
语言
日期
数量
分辨率
```

已决策默认值（双预设可切换）：

```text
默认预设（国内）： https://cn.bing.com + mkt=zh-CN + 1920x1080
国际预设：         https://www.bing.com + mkt=en-US + 1920x1080
```

UHD（_UHD.jpg）作为可选配置项；端点 + 市场在配置/UI 中一键切换。

示例配置：

```json
{
    "id": "bing",
    "name": "Bing",
    "type": "bing",
    "enabled": true,
    "market": "zh-CN"
}
```

---

## 6.2 通用 JSON Provider

用于兼容第三方 API。

例如：

```json
{
    "id": "custom",
    "name": "Custom Wallpaper",
    "type": "json",
    "endpoint": "https://example.com/api/wallpaper"
}
```

最终 API 返回：

```json
{
    "data": [
        {
            "id": "001",
            "title": "Mountain",
            "url": "https://example.com/image.jpg",
            "thumbnail": "https://example.com/thumb.jpg"
        }
    ]
}
```

程序通过映射规则读取。

---

# 7. URL Provider

最简单的 Provider：

```text
https://example.com/today.jpg
```

程序：

```text
URL
 ↓
HTTP GET
 ↓
Content-Type 检查
 ↓
图片解码
 ↓
缓存
 ↓
设置壁纸
```

适合：

- 每日一图
- 自建图片服务器
- CDN
- GitHub Raw
- NAS
- 动态图片地址

---

# 8. Provider Manifest

推荐未来支持独立 Provider 配置。

例如：

```json
{
    "id": "bing",
    "version": 1,
    "name": "Bing",
    "type": "json",
    "endpoint": "https://www.bing.com/HPImageArchive.aspx",
    "params": {
        "format": "js",
        "idx": "0",
        "n": "8",
        "mkt": "zh-CN"
    }
}
```

这样未来 Bing 接口发生变化时，可以优先尝试：

```text
更新 provider.json
```

而不是重新编译整个程序。

---

# 9. Provider 更新机制

建议分成两级。

## 第一阶段

Provider 文件随程序发布：

```text
providers/
└── bing.json
```

## 第二阶段

增加远程 Provider 更新：

```text
官方 Provider Repository
          ↓
Manifest
          ↓
版本检查
          ↓
下载
          ↓
校验
          ↓
安装
          ↓
立即生效
```

注意：

远程 Provider 不应拥有任意代码执行能力。

推荐 Provider 只允许：

```text
HTTP 请求
JSON 解析
字段映射
图片下载
```

不要设计成：

```text
下载 DLL
下载 EXE
动态执行代码
```

这样可以显著降低供应链攻击风险。

---

# 10. 配置文件

建议：

```text
%LOCALAPPDATA%\BingWallpaper\
```

例如：

```text
BingWallpaper/
├── config.json
├── cache/
├── thumbnails/
├── history/
└── logs/
```

配置示例：

```json
{
    "provider": "bing",
    "market": "zh-CN",
    "auto_update": true,
    "update_interval": 86400,
    "startup": true,
    "cache_days": 30,
    "resolution": "auto",
    "fit_mode": "fill"
}
```

---

# 11. 缓存系统

缓存建议按照：

```text
Provider
+
Wallpaper ID
+
图片 Hash
```

确定文件名。

例如：

```text
cache/
├── bing/
│   ├── 20260829_xxxxx.jpg
│   ├── 20260828_xxxxx.jpg
│   └── ...
└── custom/
```

缓存功能：

- 避免重复下载
- 离线启动
- 历史壁纸
- 手动重新设置
- 清理旧文件
- 限制缓存容量

索引存储已决策：使用 serde_json 生成的 JSON 索引文件（如 cache/index.json），
不引入 SQLite。当前量级（每天数张 × 保留期）JSON 足够，且避免 C 库交叉编译链复杂度。

建议默认保留：

```text
30 天
```

并允许用户修改。

---

# 12. Wallpaper Manager

统一接口：

```rust
pub trait WallpaperManager {
    fn set_wallpaper(
        &self,
        image_path: &Path
    ) -> Result<(), WallpaperError>;
}
```

Windows 实现：

```text
SystemParametersInfoW
```

核心功能：

- 设置桌面壁纸
- 恢复壁纸
- 查询当前壁纸
- 多显示器处理
- 壁纸模式

---

# 13. 图片处理

建议提供：

```text
Fill
Fit
Stretch
Center
```

默认：

```text
Fill
```

流程：

```text
原始图片
   ↓
获取 Monitor Resolution
   ↓
计算缩放比例
   ↓
Crop / Resize
   ↓
生成最终图片
   ↓
设置壁纸
```

执行阶段已决策：本节流程（程序级 Crop/Resize）在 P2 实施；
第一阶段 MVP 使用系统注册表 WallpaperStyle（默认 Fill，Win7 起支持），
由系统完成适配，零像素处理开销。

不要修改原始缓存图片。

---

# 14. 多显示器

第一阶段：

```text
所有显示器使用同一张壁纸
```

第二阶段：

```text
每个显示器独立壁纸
```

未来可以支持：

```text
Monitor 1 → Bing
Monitor 2 → NASA
Monitor 3 → Custom
```

Provider 与 Monitor 配置独立。

---

# 15. 系统托盘

后台运行模式：

```text
关闭窗口
     ↓
程序继续运行
     ↓
系统托盘
```

菜单：

```text
打开
立即更新
下一张
历史壁纸
设置
关于
退出
```

已决策：使用 tray-icon crate（tauri-apps 生态，Win7 兼容）而非手写
Shell_NotifyIconW。代价是引入其依赖的 windows crate（与 windows-sys 并存），
已接受该依赖树成本换取集成可靠性。

---

# 16. 自动更新

启动：

```text
程序启动
 ↓
读取配置
 ↓
加载 Provider
 ↓
检查今日壁纸
 ↓
缓存命中？
 ├── 是 → 使用缓存
 └── 否 → 下载
             ↓
          设置壁纸
```

调度语义已决策：按"壁纸日期"驱动，而非纯间隔定时。

```text
触发点：启动后 30 秒 / 定时器（可配 6/12/24 小时）/ 系统睡眠唤醒
动作：  比较 Bing 今日壁纸日期与已设置壁纸日期，不同才下载并设置
```

天然处理跨天与睡眠唤醒，保证每天必更新一次，且不浪费请求。

---

# 17. 离线策略

没有网络时：

```text
启动
 ↓
Provider 请求失败
 ↓
检查本地缓存
 ↓
存在缓存？
 ├── 是 → 使用最近壁纸
 └── 否 → 保持当前 Windows 壁纸
```

不能因为 Bing API 不可用导致程序启动失败。

---

# 18. 错误处理

所有网络操作必须具有：

```text
Timeout
Retry
Error Classification
```

区分：

```text
网络错误
HTTP 错误
JSON 错误
图片下载错误
图片解码错误
Provider 错误
Windows API 错误
磁盘错误
```

日志：

```text
logs/
└── app.log
```

正式版本建议限制日志大小。

---

# 19. UI 设计

主界面：

```text
┌──────────────────────────────────────────┐
│ Bing Wallpaper                     ─ □ × │
├──────────────────────────────────────────┤
│                                          │
│       ┌──────────────────────────┐       │
│       │                          │       │
│       │        Wallpaper         │       │
│       │                          │       │
│       └──────────────────────────┘       │
│                                          │
│       图片标题                           │
│       图片描述 / Copyright               │
│                                          │
│       [ 设置为壁纸 ]   [ 下载 ]          │
│                                          │
├──────────────────────────────────────────┤
│ 来源：Bing          更新：2026-08-29     │
└──────────────────────────────────────────┘
```

设置页面：

```text
来源
├── Bing
├── Custom JSON
└── URL

自动更新
开机启动
缓存数量
图片模式
显示器
市场/语言
```

---

# 20. 历史壁纸

提供：

```text
┌────────┬────────┬────────┐
│ 图片 1 │ 图片 2 │ 图片 3 │
├────────┼────────┼────────┤
│ 图片 4 │ 图片 5 │ 图片 6 │
└────────┴────────┴────────┘
```

点击：

```text
预览
设置为当前壁纸
打开文件位置
删除
```

---

# 21. Windows 兼容策略

最低：

```text
Windows 7
```

目标：

```text
Windows 7
Windows 8
Windows 8.1
Windows 10
Windows 11
```

建议发布：

```text
BingWallpaper-Rust-x64.exe
```

根据实际用户需求再增加：

```text
BingWallpaper-x86.exe
```

---

# 22. Rust Toolchain 锁定

根目录：

```text
rust-toolchain.toml
```

建议：

```toml
[toolchain]
channel = "1.77.2"
components = [
    "rustfmt",
    "clippy"
]
```

同时：

```text
Cargo.lock
```

必须提交到 Git。

原则：

```text
开发环境固定
↓
依赖固定
↓
构建环境固定
↓
发布产物可复现
```

不要使用：

```text
cargo update
```

作为常规维护手段。

构建路线已决策（双轨）：

```text
日常开发： Fedora Linux + rustup 1.77.2 + mingw-w64
           target x86_64-pc-windows-gnu
正式发布： GitHub Actions windows runner + MSVC 工具链
```

所有依赖选择必须满足 MSRV ≤ 1.77.2（以 Cargo.lock 实际锁定为准）。

---

# 23. 依赖升级策略

核心原则：

> **不是追求最新版本，而是追求长期稳定。**

正常情况下：

```text
Rust
egui
eframe
winit
windows-rs
```

全部冻结。

只有出现：

```text
严重安全漏洞
无法兼容新 Windows
关键功能 Bug
关键第三方 API 变化
```

才考虑升级。

升级必须：

```text
建立新分支
 ↓
更新依赖
 ↓
Windows 7 测试
 ↓
Windows 10 测试
 ↓
Windows 11 测试
 ↓
完整回归
 ↓
合并
```

---

# 24. Provider 更新策略

Provider 是整个项目未来主要的可维护部分。

例如 Bing API 发生变化：

```text
原程序
    │
    ├── Core 不变
    ├── UI 不变
    ├── Cache 不变
    ├── Scheduler 不变
    └── Provider 更新
```

目标：

> 大部分数据源变化只修改 Provider 配置，而不修改核心程序。

---

# 25. 安全设计

必须避免：

```text
远程 Provider 执行 Rust/WASM/EXE
```

第一版只允许：

```text
HTTP
HTTPS
JSON
图片
```

Provider Manifest 建议支持：

```text
schema_version
provider_version
endpoint
headers
params
mapping
```

未来远程更新可以加入：

```text
SHA-256
数字签名
HTTPS
版本检查
```

---

# 26. 软件更新与 Provider 更新分离

推荐：

```text
Application Update
        │
        └── EXE / Installer

Provider Update
        │
        └── JSON / Manifest
```

例如：

```text
程序：
1.0.0
```

Provider：

```text
Bing Provider
2026.08.29
```

Bing 接口变化时：

```text
程序仍然 1.0.0
Provider → 2026.08.30
```

这样可以最大限度减少核心软件升级。

---

# 27. 编译与发布

已决策流程：

```text
Fedora 交叉编译（gnu 目标）日常开发
        ↓
GitHub Actions（windows runner + MSVC）构建发布包
        ↓
Windows 7 VM（已具备）
        ↓
功能测试（重点：rustls+ring 的 HTTPS 握手实测）
        ↓
Windows 10 VM
        ↓
Windows 11
        ↓
发布
```

如果条件允许：

```text
GitHub Actions
```

建立：

```text
build
test
package
release
```

流程。

---

# 28. Windows 7 测试矩阵

至少测试（Win7 VM/真机已具备，可完整执行；重点实测 rustls+ring 的 HTTPS 握手）：

### Windows 7

- x64
- 网络请求
- HTTPS
- Bing API
- JPG 下载
- JPG 解码
- 设置壁纸
- 系统托盘
- 开机启动
- DPI
- 最小化
- 退出

### Windows 10

- 正常安装
- 自动更新
- 多显示器
- 高 DPI

### Windows 11

- 正常安装
- 高 DPI
- 托盘
- 深色/浅色 UI

---

# 29. 安装程序

第一阶段可以：

```text
Portable
```

直接：

```text
BingWallpaper.exe
```

第二阶段：

```text
Installer
```

安装内容：

```text
Program Files/
└── BingWallpaper-Rust/
    └── BingWallpaper-Rust.exe
```

用户数据：

```text
%LOCALAPPDATA%\BingWallpaper-Rust\
```

程序和用户数据分离。

---

# 30. 开机启动

建议使用：

```text
HKCU\Software\Microsoft\Windows\CurrentVersion\Run
```

优点：

- 不需要管理员权限
- Win7 兼容
- 简单可靠

不要第一版就引入 Windows Service。

---

# 31. 第一阶段 MVP

第一版本只实现：

```text
[核心]

✓ Bing API
✓ 图片下载
✓ 图片缓存
✓ 设置壁纸
✓ 自动更新
✓ 系统托盘
✓ 开机启动
✓ 基本设置
✓ Windows 7+
✓ x64
```

UI：

```text
✓ 当前壁纸预览
✓ 设置为壁纸
✓ 手动刷新
✓ 来源选择
✓ 自动更新开关
✓ 市场/语言
```

---

# 32. 第二阶段

加入：

```text
✓ 历史壁纸
✓ 下载管理
✓ 多显示器
✓ 图片裁剪模式
✓ 缓存管理
✓ 离线模式
✓ 更完整的 Provider
```

---

# 33. 第三阶段

加入：

```text
✓ 通用 JSON Provider
✓ URL Provider
✓ 自定义 Provider
✓ Provider Manifest
✓ Provider 在线更新
✓ Provider 版本管理
```

---

# 34. 第四阶段

加入：

```text
✓ Provider Repository
✓ Provider 签名
✓ Provider 自动更新
✓ 更多第三方图片来源
✓ 用户共享 Provider
```

---

# 35. 未来扩展

最终可以发展为：

```text
                Wallpaper Client
                       │
          ┌────────────┼────────────┐
          │            │            │
        Bing          NASA        Custom
          │            │            │
          └────────────┼────────────┘
                       │
                  Provider API
                       │
                 Wallpaper Core
                       │
              ┌────────┼────────┐
              │        │        │
             Cache   Scheduler  Windows
```

最终软件并不局限于 Bing。

---

# 36. 核心接口稳定原则

建议从第一版开始就稳定以下接口：

```rust
WallpaperProvider
Wallpaper
ProviderManager
Downloader
CacheManager
WallpaperManager
Scheduler
```

以后：

```text
Bing API 改变
```

不影响：

```text
Cache
UI
Scheduler
Wallpaper Manager
```

甚至：

```text
egui
```

未来更换 GUI，也不影响核心业务。

---

# 37. 最终技术方案

推荐最终组合：

```text
Language:
Rust 1.77.2

GUI:
egui + eframe

Window:
winit

Windows:
windows-sys / windows-rs

HTTP:
reqwest + tokio（TLS: rustls + ring provider）

JSON:
serde + serde_json

Image:
image

Logging:
tracing

Config:
serde + dirs

Build:
Cargo

Target:
Windows 7 → Windows 11

Architecture:
Provider + Core

Rendering:
eframe glow 后端（OpenGL 2.0+），禁用 wgpu，Win7 VM 实测验证
```

---

# 38. 最终项目理念

整个项目遵循：

```text
                  ┌──────────────────────┐
                  │    Frozen Core       │
                  │                      │
                  │ Rust                 │
                  │ egui                 │
                  │ winit                │
                  │ Win32                │
                  │ Cache                │
                  │ Scheduler             │
                  │ Wallpaper Manager    │
                  └──────────┬───────────┘
                             │
                        Stable API
                             │
                  ┌──────────▼───────────┐
                  │      Provider        │
                  │                      │
                  │ Bing                 │
                  │ NASA                 │
                  │ JSON API             │
                  │ URL                  │
                  │ Local                │
                  │ NAS                  │
                  └──────────────────────┘
```

核心程序可以长期保持稳定。

数据源可以独立变化。

---

# 39. 实施优先级

建议按照以下顺序开发：

```text
P0
├── Rust/Win7 编译环境
├── 基础窗口
├── Bing API
├── 图片下载
└── 设置壁纸

P1
├── 配置系统
├── 缓存
├── 自动更新
├── 系统托盘
└── 开机启动

P2
├── UI 完善
├── 历史壁纸
├── 多显示器
└── 图片处理

P3
├── JSON Provider
├── URL Provider
└── Provider Manifest

P4
├── Provider 在线更新
├── Provider Repository
└── Provider 签名/安全机制
```

---

# 40. 推荐的第一版目标

最终第一版应做到：

> **一个单文件或极少依赖的 Rust Windows Wallpaper 客户端。**

安装/运行后：

```text
启动
 ↓
获取 Bing 今日壁纸
 ↓
下载到本地缓存
 ↓
自动设置桌面
 ↓
进入系统托盘
 ↓
后台等待下一次更新
```

用户也可以：

```text
打开窗口
 ↓
查看当前壁纸
 ↓
查看历史
 ↓
手动刷新
 ↓
选择图片
 ↓
设置为桌面
```

同时整个核心架构从第一天就为：

```text
Bing
↓
第三方 API
↓
自定义来源
```

做好扩展准备。

---

# 41. 结论

对于“Windows 7+ + Rust + 长期维护 + Bing Wallpaper + 后续增加第三方来源”的目标，推荐：

**Rust 1.77.2 + egui/eframe + winit + Windows API + reqwest + serde + image**

核心程序采用：

**Provider → Downloader → Cache → Image Processor → Wallpaper Manager**

数据源采用：

**Bing Provider → JSON Provider → URL Provider → 自定义 Provider**

同时将：

**Rust 工具链、Cargo.lock、核心依赖、Windows API 层和业务逻辑冻结。**

未来维护重点放在 Provider，而不是频繁升级整个客户端。

这样能够最大程度降低 Windows 7 兼容性风险，并让项目具备长期运行和扩展能力。

---

# 42. 实施决策记录（2026-08-29 逐项确认）

以下决策已经过访谈逐项确认，正文与之冲突之处以本节为准：

| # | 决策项 | 结论 |
|---|--------|------|
| 1 | 构建环境 | 双轨：Fedora 交叉编译 x86_64-pc-windows-gnu（mingw-w64）日常开发；GitHub Actions（windows runner + MSVC）出正式发布包 |
| 2 | 网络栈 | reqwest + tokio 异步；WallpaperProvider::fetch 为 async 签名 |
| 3 | TLS 后端 | rustls + 显式 ring provider（不用 aws-lc-rs，不用 native-tls/schannel） |
| 4 | 渲染后端 | eframe 0.27~0.28 + glow（OpenGL 2.0+），Cargo features 禁用 wgpu |
| 5 | 系统托盘 | tray-icon crate（接受其 windows crate 依赖树），不手写 Shell_NotifyIconW |
| 6 | 壁纸适配 | MVP 用系统注册表 WallpaperStyle（默认 Fill）；程序级 crop/resize 推至 P2 |
| 7 | 缓存索引 | JSON 索引文件（serde_json），不引入 SQLite；cache/database.rs 更名 index.rs |
| 8 | 单实例 | CreateMutexW 命名互斥体；第二实例弹托盘气泡后退出 |
| 9 | Bing 默认值 | 双预设可切换：默认 cn.bing.com + zh-CN + 1920x1080；国际预设 www.bing.com + en-US；UHD 可选配置 |
| 10 | 调度语义 | 按壁纸日期驱动：启动后 30 秒 / 定时器 / 睡眠唤醒三触发点比对日期，变更才下载 |
| 11 | 项目命名 | 统一 BingWallpaper-Rust（exe 名、%LOCALAPPDATA%\BingWallpaper-Rust、托盘、窗口标题） |
| 12 | 许可证 | GPL-3.0（tray-icon / eframe / rustls 等关键依赖均为 MIT OR Apache-2.0 双授权，兼容 GPL-3.0） |
| 13 | Git | 立即初始化；.gitignore 排除 target/、logs/；Cargo.lock 按第 22 节要求入库 |
| 14 | UI 语言 | 中英双语 i18n：自定义字符串表（zh/en），按系统 locale / 配置选择，不引入 fluent |
| 15 | Win7 测试 | Win7 VM/真机已具备，第 28 节测试矩阵完整执行；P0 重点实测 rustls+ring HTTPS 握手 |
| 16 | 唤起窗口 | 单实例唤起已有窗口所需跨进程通信（命名管道/自定义消息）MVP 不做，留待后续 |

P0 待落地事项：

```text
✓ git init + .gitignore（本次已完成）
□ LICENSE 文件（GPL-3.0 官方全文，脚手架阶段放入仓库根目录）
□ Cargo.toml 初版（features 按决策 #2/#3/#4/#5 配置）
□ rust-toolchain.toml（1.77.2 + rustfmt + clippy）
□ mingw-w64 交叉编译环境验证（本机 Fedora）
```
