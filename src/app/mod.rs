//! eframe 应用层：页面（主页/设置/历史/关于）、托盘事件消费、更新动作。
//!
//! P2：+历史壁纸（缩略图网格）、+下一张轮换、+Win10/Win7 双壁纸路径接入。

pub mod config;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui;
use tokio::runtime::Handle;
use tracing::{error, info};

use crate::cache::{CacheEntry, CacheManager};
use crate::downloader::Downloader;
use crate::i18n::Lang;
use crate::provider::manifest::LoadedProvider;
use crate::provider::{ProviderContext, Wallpaper};
use crate::scheduler;
use crate::tray::TrayAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Settings,
    History,
    About,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub running: bool,
    pub message: String,
    pub last_set: Option<String>,
}

impl Status {
    pub fn idle(lang: Lang) -> Self {
        Self {
            running: false,
            message: crate::i18n::table(lang).status_ready.into(),
            last_set: None,
        }
    }
}

/// 更新动作共享环境：UI / 托盘 / 调度器三方共用的状态集合
#[derive(Clone)]
pub struct UpdateEnv {
    pub cfg: Arc<Mutex<config::Config>>,
    pub data_dir: PathBuf,
    pub status: Arc<Mutex<Status>>,
    /// 最近一次 fetch 的壁纸列表（"下一张"轮换数据源）
    pub last_fetch: Arc<Mutex<Vec<Wallpaper>>>,
    pub fetch_cursor: Arc<Mutex<usize>>,
    /// Provider 注册表（P3：内置 Bing + 用户 Manifest，同 id 覆盖）
    pub providers: Arc<Mutex<Vec<Arc<LoadedProvider>>>>,
    /// 事件回注通道（P4：Provider 更新完成后请求 UI 重载注册表）
    pub events_tx: std::sync::mpsc::Sender<TrayAction>,
    /// P4：Provider 更新检查结果消息（后台任务写、UI 读，5 秒后淡出）
    pub provider_check_msg: Arc<Mutex<Option<(String, Instant)>>>,
}

pub struct App {
    pub env: Arc<UpdateEnv>,
    pub rt: Handle,
    pub cache: CacheManager,
    pub lang: Lang,
    pub page: Page,
    pub events_rx: std::sync::mpsc::Receiver<TrayAction>,
    pub settings_hint: Option<(String, Instant)>,
    /// 历史页：条目快照 + 缩略图纹理缓存
    pub history_entries: Vec<CacheEntry>,
    pub history_stale: bool,
    pub textures: HashMap<String, egui::TextureHandle>,
}

impl App {
    pub fn new(
        env: Arc<UpdateEnv>,
        rt: Handle,
        events_rx: std::sync::mpsc::Receiver<TrayAction>,
        ctx: egui::Context,
    ) -> Self {
        // 中文界面必须先挂上 CJK 字体（egui 内置字体不含 CJK 字形）
        crate::ui_font::apply(&ctx);
        let lang = Lang::parse(
            &env.cfg
                .lock()
                .map(|c| c.language.clone())
                .unwrap_or_default(),
        );
        // 定时调度：启动 30 秒后首查，之后每 5 分钟比对壁纸日期（决策 #10）
        scheduler::spawn(scheduler::SchedulerDeps {
            rt: rt.clone(),
            env: env.clone(),
            ctx: ctx.clone(),
        });
        // 启动即尝试一次更新（缓存命中则零下载）
        spawn_update(&rt, &env, &ctx, lang);
        Self {
            cache: CacheManager::new(&env.data_dir),
            env,
            rt,
            lang,
            page: Page::Home,
            events_rx,
            settings_hint: None,
            history_entries: Vec::new(),
            history_stale: true,
            textures: HashMap::new(),
        }
    }

    fn spawn_manual_update(&self, ctx: &egui::Context) {
        spawn_update(&self.rt, &self.env, ctx, self.lang);
    }

    fn spawn_next_wallpaper(&self, ctx: &egui::Context) {
        spawn_next(&self.rt, &self.env, ctx, self.lang);
    }

    fn handle_tray_action(&mut self, action: TrayAction, ctx: &egui::Context) {
        let show = |page: &mut Page, target: Page, ctx: &egui::Context| {
            *page = target;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        };
        match action {
            TrayAction::Open => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            TrayAction::UpdateNow => self.spawn_manual_update(ctx),
            TrayAction::Next => self.spawn_next_wallpaper(ctx),
            TrayAction::History => show(&mut self.page, Page::History, ctx),
            TrayAction::Settings => show(&mut self.page, Page::Settings, ctx),
            TrayAction::About => show(&mut self.page, Page::About, ctx),
            TrayAction::Quit => {
                // 配置即改即存、缓存索引原子写，直接退出无状态丢失风险
                info!("用户从托盘请求退出");
                std::process::exit(0);
            }
            TrayAction::ReloadProviders => {
                let loaded = crate::provider::manifest::load_all(&self.env.data_dir);
                if let Ok(mut guard) = self.env.providers.lock() {
                    *guard = loaded.into_iter().map(Arc::new).collect();
                }
                info!("Provider 注册表已重载（共 {} 项）", self.provider_count());
            }
        }
    }

    fn provider_count(&self) -> usize {
        self.env.providers.lock().map(|p| p.len()).unwrap_or(0)
    }

    /// P4：检查 Provider 在线更新（方案 §9 第二阶段）
    fn spawn_provider_check(&self, ctx: &egui::Context) {
        let t = crate::i18n::table(self.lang);
        if let Ok(mut slot) = self.env.provider_check_msg.lock() {
            *slot = Some((t.provider_checking.into(), Instant::now()));
        }
        let env = self.env.clone();
        let ctx = ctx.clone();
        let lang = self.lang;
        self.rt.spawn(async move {
            let snapshot = match env.cfg.lock() {
                Ok(cfg) => cfg.clone(),
                Err(err) => {
                    error!("Provider 更新检查失败: {err}");
                    return;
                }
            };
            let t = crate::i18n::table(lang);
            let build = reqwest::Client::builder()
                .user_agent(concat!("BingWallpaper-Rust/", env!("CARGO_PKG_VERSION")))
                .timeout(std::time::Duration::from_secs(30))
                .build();
            let (message, has_updates) = match build {
                Err(err) => (format!("{}{err}", t.status_failed_prefix), false),
                Ok(http) => {
                    match crate::provider::repo::check_for_updates(
                        &http,
                        &snapshot.provider_repo_url,
                        snapshot.provider_repo_public_key.as_deref(),
                        &env.data_dir,
                    )
                    .await
                    {
                        Ok(report) => {
                            info!("Provider 更新检查完成: {report}");
                            if report.updated.is_empty() {
                                (t.provider_up_to_date.into(), false)
                            } else {
                                (format!("{}: {report}", t.provider_check_update), true)
                            }
                        }
                        Err(err) => {
                            error!("Provider 更新检查失败: {err}");
                            (format!("{}{err}", t.status_failed_prefix), false)
                        }
                    }
                }
            };
            if let Ok(mut slot) = env.provider_check_msg.lock() {
                *slot = Some((message, Instant::now()));
            }
            if has_updates {
                let _ = env.events_tx.send(TrayAction::ReloadProviders);
            }
            ctx.request_repaint();
        });
    }

    fn draw_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let t = crate::i18n::table(self.lang);
            let pages = [
                (Page::Home, t.tab_home),
                (Page::History, t.tab_history),
                (Page::Settings, t.tab_settings),
                (Page::About, t.tab_about),
            ];
            for (page, label) in pages {
                if ui.selectable_label(self.page == page, label).clicked() {
                    self.page = page;
                }
            }
        });
        ui.separator();
    }

    fn draw_home(&mut self, ui: &mut egui::Ui) {
        let t = crate::i18n::table(self.lang);
        let status = self
            .env
            .status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| Status {
                running: false,
                message: "internal state error".into(),
                last_set: None,
            });

        ui.add_space(8.0);
        ui.label(&status.message);
        if let Some(last) = &status.last_set {
            ui.label(format!("{}: {last}", t.last_set_label));
        }
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!status.running, |ui| {
                if ui.button(t.update_now).clicked() {
                    self.spawn_manual_update(ui.ctx());
                }
                if ui.button(t.next_wallpaper).clicked() {
                    self.spawn_next_wallpaper(ui.ctx());
                }
            });
        });
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            let source_name = {
                let wanted = self
                    .env
                    .cfg
                    .lock()
                    .map(|c| c.provider.clone())
                    .unwrap_or_default();
                self.env
                    .providers
                    .lock()
                    .ok()
                    .and_then(|providers| {
                        providers
                            .iter()
                            .find(|p| p.manifest.id == wanted)
                            .or_else(|| providers.first())
                            .map(|p| p.manifest.name.clone())
                    })
                    .unwrap_or_else(|| wanted)
            };
            ui.label(format!(
                "{}: {}    {}: {}",
                t.source_label,
                source_name,
                t.data_dir_label,
                self.env.data_dir.display()
            ));
        });
    }

    fn draw_history(&mut self, ui: &mut egui::Ui) {
        let t = crate::i18n::table(self.lang);
        if self.history_stale {
            let mut entries = self.cache.entries();
            entries.sort_by(|a, b| b.added_at.cmp(&a.added_at));
            self.history_entries = entries;
            self.history_stale = false;
        }
        if self.history_entries.is_empty() {
            ui.add_space(8.0);
            ui.label(t.history_empty);
            return;
        }
        let entries = self.history_entries.clone();
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("history_grid")
                .num_columns(3)
                .spacing([12.0, 12.0])
                .show(ui, |ui| {
                    let mut column = 0;
                    for entry in &entries {
                        ui.vertical(|ui| {
                            if let Some(tex) = self.texture_for(ui.ctx(), entry) {
                                ui.add(egui::Image::new(&tex).max_width(220.0));
                            } else {
                                // 无缩略图时占出等高空间，保持网格整齐
                                ui.allocate_exact_size(
                                    egui::vec2(220.0, 124.0),
                                    egui::Sense::hover(),
                                );
                            }
                            ui.label(
                                entry
                                    .title
                                    .clone()
                                    .unwrap_or_else(|| entry.wallpaper_id.clone()),
                            );
                            if let Some(date) = entry.date {
                                ui.weak(date.format("%Y-%m-%d").to_string());
                            }
                            ui.horizontal(|ui| {
                                if ui.button(t.history_set).clicked() {
                                    self.apply_history_entry(ui.ctx(), entry);
                                }
                                if ui.button(t.history_open_location).clicked() {
                                    let loc = self
                                        .cache
                                        .dir()
                                        .join(&entry.file)
                                        .to_string_lossy()
                                        .into_owned();
                                    open_location(&loc);
                                }
                                if ui.button(t.history_delete).clicked() {
                                    self.cache
                                        .remove_entry(&entry.provider, &entry.wallpaper_id);
                                    self.textures.remove(&entry.file);
                                    self.history_stale = true;
                                }
                            });
                        });
                        column += 1;
                        if column % 3 == 0 {
                            ui.end_row();
                        }
                    }
                });
        });
    }

    fn texture_for(
        &mut self,
        ctx: &egui::Context,
        entry: &CacheEntry,
    ) -> Option<egui::TextureHandle> {
        if let Some(tex) = self.textures.get(&entry.file) {
            return Some(tex.clone());
        }
        let source = self.cache.dir().join(&entry.file);
        let thumb = crate::thumbs::ensure_thumbnail(self.cache.dir(), &source, &entry.file)?;
        let img = image::open(&thumb).ok()?;
        let (width, height) = image::GenericImageView::dimensions(&img);
        let rgba = img.to_rgba8().into_raw();
        let color =
            egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
        let tex = ctx.load_texture(
            format!("history-{}", entry.file),
            color,
            egui::TextureOptions::default(),
        );
        self.textures.insert(entry.file.clone(), tex.clone());
        Some(tex)
    }

    /// 历史条目 -> 设为当前壁纸（后台线程执行 Win32 调用）
    fn apply_history_entry(&self, ctx: &egui::Context, entry: &CacheEntry) {
        let t = crate::i18n::table(self.lang);
        let path = self.cache.dir().join(&entry.file);
        let fit = self
            .env
            .cfg
            .lock()
            .map(|c| c.fit_mode.clone())
            .unwrap_or_else(|_| "fill".into());
        let status = self.env.status.clone();
        let data_dir = self.env.data_dir.clone();
        let ctx = ctx.clone();
        let title = entry
            .title
            .clone()
            .unwrap_or_else(|| entry.wallpaper_id.clone());
        let provider = entry.provider.clone();
        let wallpaper_id = entry.wallpaper_id.clone();
        self.rt.spawn(async move {
            set_status(&status, true, t.status_updating.into(), &ctx);
            let result =
                tokio::task::spawn_blocking(move || crate::wallpaper::set_wallpaper(&path, &fit))
                    .await;
            match result {
                Ok(Ok(())) => {
                    info!("历史壁纸已设置: {title}");
                    CacheManager::new(&data_dir).record_last_set(&provider, &wallpaper_id);
                    if let Ok(mut s) = status.lock() {
                        s.running = false;
                        s.message = format!("{}{title}", t.status_done_prefix);
                        s.last_set = Some(CacheManager::today().format("%Y-%m-%d").to_string());
                    }
                }
                Ok(Err(err)) => {
                    error!("历史壁纸设置失败: {err}");
                    set_status(
                        &status,
                        false,
                        format!("{}{err}", t.status_failed_prefix),
                        &ctx,
                    );
                }
                Err(err) => {
                    error!("壁纸任务异常: {err}");
                    set_status(
                        &status,
                        false,
                        format!("{}{err}", t.status_failed_prefix),
                        &ctx,
                    );
                }
            }
            ctx.request_repaint();
        });
    }

    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        let t = crate::i18n::table(self.lang);
        ui.add_space(8.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("settings_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    let Ok(mut cfg) = self.env.cfg.lock() else {
                        return;
                    };

                    // 壁纸来源（P3：Provider 注册表，方案 §5/§24）
                    ui.label(t.provider_label);
                    let options: Vec<(String, String)> = match self.env.providers.lock() {
                        Ok(providers) => providers
                            .iter()
                            .map(|p| {
                                (
                                    p.manifest.id.clone(),
                                    format!("{} ({})", p.manifest.name, p.manifest.id),
                                )
                            })
                            .collect(),
                        Err(_) => Vec::new(),
                    };
                    egui::ComboBox::from_id_source("provider")
                        .selected_text(
                            options
                                .iter()
                                .find(|(id, _)| *id == cfg.provider)
                                .map(|(_, label)| label.clone())
                                .unwrap_or_else(|| cfg.provider.clone()),
                        )
                        .show_ui(ui, |ui| {
                            for (id, label) in &options {
                                ui.selectable_value(&mut cfg.provider, id.clone(), label);
                            }
                        });
                    ui.end_row();

                    // Bing 预设（决策 #9 双预设）
                    ui.label(t.preset_label);
                    egui::ComboBox::from_id_source("preset")
                        .selected_text(if cfg.bing_preset == "global" {
                            t.preset_global
                        } else {
                            t.preset_china
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut cfg.bing_preset,
                                "china".to_string(),
                                t.preset_china,
                            );
                            ui.selectable_value(
                                &mut cfg.bing_preset,
                                "global".to_string(),
                                t.preset_global,
                            );
                        });
                    ui.end_row();

                    // 适配模式（MVP 系统样式，方案 §13）
                    ui.label(t.fit_mode_label);
                    egui::ComboBox::from_id_source("fit_mode")
                        .selected_text(fit_label(&t, &cfg.fit_mode))
                        .show_ui(ui, |ui| {
                            for (value, label) in [
                                ("fill", t.fit_fill),
                                ("fit", t.fit_fit),
                                ("stretch", t.fit_stretch),
                                ("center", t.fit_center),
                                ("span", t.fit_span),
                            ] {
                                ui.selectable_value(&mut cfg.fit_mode, value.into(), label);
                            }
                        });
                    ui.end_row();

                    // 界面语言（决策 #14）
                    ui.label(t.language_label);
                    let mut lang = self.lang;
                    egui::ComboBox::from_id_source("language")
                        .selected_text(if lang == Lang::En {
                            "English"
                        } else {
                            "中文"
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut lang, Lang::Zh, "中文");
                            ui.selectable_value(&mut lang, Lang::En, "English");
                        });
                    if lang != self.lang {
                        self.lang = lang;
                        cfg.language = lang.as_config_str().into();
                    }
                    ui.end_row();

                    // 自动更新（决策 #10：日期驱动）
                    ui.label(t.auto_update_label);
                    ui.checkbox(&mut cfg.auto_update, "");
                    ui.end_row();

                    // 开机启动（方案 §30：HKCU Run）
                    ui.label(t.autostart_label);
                    let mut startup = cfg.startup;
                    if ui.checkbox(&mut startup, "").changed() {
                        match crate::autostart::set_enabled(startup) {
                            Ok(()) => cfg.startup = startup,
                            Err(err) => {
                                error!("开机启动设置失败: {err}");
                                self.settings_hint = Some((
                                    format!("{}: {err}", t.status_failed_prefix),
                                    Instant::now(),
                                ));
                            }
                        }
                    }
                    ui.end_row();

                    // 缓存保留天数（方案 §11）
                    ui.label(t.cache_days_label);
                    ui.add(egui::DragValue::new(&mut cfg.cache_days).clamp_range(7..=365));
                    ui.end_row();
                });

            // P4：Provider 在线更新（方案 §9 第二阶段 / §26）
            ui.add_space(12.0);
            ui.separator();
            if let Ok(mut cfg) = self.env.cfg.lock() {
                ui.label(t.provider_repo_label);
                ui.add(
                    egui::TextEdit::singleline(&mut cfg.provider_repo_url)
                        .hint_text(t.provider_repo_hint)
                        .desired_width(420.0),
                );
                ui.add_space(4.0);
                ui.label(t.provider_public_key_label);
                let mut key = cfg.provider_repo_public_key.clone().unwrap_or_default();
                if ui
                    .add(egui::TextEdit::singleline(&mut key).desired_width(420.0))
                    .changed()
                {
                    cfg.provider_repo_public_key = if key.trim().is_empty() {
                        None
                    } else {
                        Some(key.trim().to_string())
                    };
                }
            }
            ui.add_space(4.0);
            if ui.button(t.provider_check_update).clicked() {
                self.spawn_provider_check(ui.ctx());
            }
            if let Ok(slot) = self.env.provider_check_msg.lock() {
                if let Some((msg, at)) = slot.as_ref() {
                    if at.elapsed().as_secs() < 8 {
                        ui.label(msg);
                    }
                }
            }

            // 即改即存
            if let Ok(cfg) = self.env.cfg.lock() {
                if let Err(err) = config::Config::save(&cfg, &self.env.data_dir) {
                    error!("保存配置失败: {err}");
                }
            }
            if let Some((msg, at)) = &self.settings_hint {
                if at.elapsed().as_secs() < 5 {
                    ui.colored_label(egui::Color32::RED, msg);
                }
            }
        });
    }

    fn draw_about(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(crate::i18n::table(self.lang).about_text);
        ui.add_space(8.0);
        ui.label(concat!("版本: ", env!("CARGO_PKG_VERSION")));
    }
}

fn fit_label(t: &crate::i18n::Strings, fit: &str) -> &'static str {
    match fit {
        "fit" => t.fit_fit,
        "stretch" => t.fit_stretch,
        "center" => t.fit_center,
        "span" => t.fit_span,
        _ => t.fit_fill,
    }
}

#[cfg(windows)]
fn open_location(path: &str) {
    let _ = std::process::Command::new("explorer")
        .args(["/select,", path])
        .spawn();
}

#[cfg(not(windows))]
fn open_location(_file: &str) {}

/// 供 UI 按钮 / 托盘 / 调度器共用的更新触发入口
pub fn spawn_update(rt: &Handle, env: &Arc<UpdateEnv>, ctx: &egui::Context, lang: Lang) {
    let env = env.clone();
    let ctx = ctx.clone();
    rt.spawn(update_task(env, ctx, lang));
}

/// "下一张"：轮换最近 fetch 列表中的壁纸（方案 §15 托盘"下一张"）
pub fn spawn_next(rt: &Handle, env: &Arc<UpdateEnv>, ctx: &egui::Context, lang: Lang) {
    let env = env.clone();
    let ctx = ctx.clone();
    rt.spawn(next_task(env, ctx, lang));
}

async fn update_task(env: Arc<UpdateEnv>, ctx: egui::Context, lang: Lang) {
    let t = crate::i18n::table(lang);
    set_status(&env.status, true, t.status_updating.into(), &ctx);
    match run_update(&env).await {
        Ok((title, date)) => {
            info!("壁纸更新完成: {title}（{date}）");
            if let Ok(mut s) = env.status.lock() {
                s.running = false;
                s.message = format!("{}{title}", t.status_done_prefix);
                s.last_set = Some(date);
            }
            ctx.request_repaint();
        }
        Err(err) => {
            error!("壁纸更新失败: {err}");
            set_status(
                &env.status,
                false,
                format!("{}{err}", t.status_failed_prefix),
                &ctx,
            );
        }
    }
}

async fn next_task(env: Arc<UpdateEnv>, ctx: egui::Context, lang: Lang) {
    let t = crate::i18n::table(lang);
    set_status(&env.status, true, t.status_updating.into(), &ctx);
    match run_next(&env).await {
        Ok((title, date)) => {
            info!("切换壁纸: {title}（{date}）");
            if let Ok(mut s) = env.status.lock() {
                s.running = false;
                s.message = format!("{}{title}", t.status_done_prefix);
                s.last_set = Some(date);
            }
            ctx.request_repaint();
        }
        Err(err) => {
            error!("切换壁纸失败: {err}");
            set_status(
                &env.status,
                false,
                format!("{}{err}", t.status_failed_prefix),
                &ctx,
            );
        }
    }
}

fn set_status(status: &Arc<Mutex<Status>>, running: bool, message: String, ctx: &egui::Context) {
    if let Ok(mut s) = status.lock() {
        s.running = running;
        s.message = message;
    }
    ctx.request_repaint();
}

type UpdateError = Box<dyn std::error::Error + Send + Sync>;

async fn http_client() -> Result<reqwest::Client, UpdateError> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("BingWallpaper-Rust/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}

/// 从注册表选择 Provider：按 cfg.provider 匹配，未命中回退第一个（内置 Bing）
fn select_provider(
    providers: &[Arc<LoadedProvider>],
    wanted: &str,
) -> Result<Arc<LoadedProvider>, UpdateError> {
    providers
        .iter()
        .find(|p| p.manifest.id == wanted)
        .or_else(|| providers.first())
        .cloned()
        .ok_or("Provider 注册表为空".into())
}

/// 下载（或缓存命中）并把单张壁纸设为桌面
async fn apply_wallpaper(
    provider_id: &str,
    wp: &Wallpaper,
    http: reqwest::Client,
    cache: &CacheManager,
    fit_mode: &str,
) -> Result<(String, String), UpdateError> {
    let dest = cache.path_for(provider_id, &wp.id);
    let mut bytes: u64 = 0;
    if dest.exists() {
        info!("缓存命中: {}", dest.display());
    } else {
        tokio::fs::create_dir_all(cache.dir().join(provider_id)).await?;
        bytes = Downloader { http }
            .download_to_file(&wp.image_url, &dest)
            .await?;
    }

    crate::wallpaper::set_wallpaper(&dest, fit_mode)?;
    cache.record_download(provider_id, wp, bytes, CacheManager::today());

    let title = if wp.title.is_empty() {
        wp.id.clone()
    } else {
        wp.title.clone()
    };
    let date = wp
        .published_at
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    Ok((title, date))
}

async fn run_update(env: &Arc<UpdateEnv>) -> Result<(String, String), UpdateError> {
    let snapshot = env
        .cfg
        .lock()
        .map(|c| c.clone())
        .map_err(|_| "配置状态异常")?;
    let selected = {
        let guard = env.providers.lock().map_err(|_| "状态异常")?;
        select_provider(&guard, &snapshot.provider)
    }?;
    let http = http_client().await?;
    let context = ProviderContext { http: http.clone() };
    let wallpapers = selected.provider.fetch(&context).await?;
    if let Ok(mut slot) = env.last_fetch.lock() {
        *slot = wallpapers.clone();
    }
    let wp = wallpapers.first().ok_or("Provider 未返回任何壁纸")?;
    let cache = CacheManager::new(&env.data_dir);
    apply_wallpaper(&selected.manifest.id, wp, http, &cache, &snapshot.fit_mode).await
}

async fn run_next(env: &Arc<UpdateEnv>) -> Result<(String, String), UpdateError> {
    let snapshot = env
        .cfg
        .lock()
        .map(|c| c.clone())
        .map_err(|_| "配置状态异常")?;
    let mut list = env
        .last_fetch
        .lock()
        .map(|l| l.clone())
        .map_err(|_| "状态异常")?;
    if list.is_empty() {
        let selected = {
            let guard = env.providers.lock().map_err(|_| "状态异常")?;
            select_provider(&guard, &snapshot.provider)
        }?;
        let http = http_client().await?;
        let context = ProviderContext { http };
        list = selected.provider.fetch(&context).await?;
        if let Ok(mut slot) = env.last_fetch.lock() {
            *slot = list.clone();
        }
    }
    if list.is_empty() {
        return Err("Provider 未返回任何壁纸".into());
    }
    let index = {
        let mut c = env.fetch_cursor.lock().map_err(|_| "状态异常")?;
        let i = *c % list.len();
        *c = i + 1;
        i
    };
    let wp = list[index].clone();
    let selected = {
        let guard = env.providers.lock().map_err(|_| "状态异常")?;
        select_provider(&guard, &snapshot.provider)
    }?;
    let cache = CacheManager::new(&env.data_dir);
    let http = http_client().await?;
    apply_wallpaper(&selected.manifest.id, &wp, http, &cache, &snapshot.fit_mode).await
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 托盘事件（隐藏窗口时也持续轮询）
        while let Ok(action) = self.events_rx.try_recv() {
            self.handle_tray_action(action, ctx);
        }

        // 方案 §15：关闭窗口 = 进托盘。
        // eframe 0.27 无 on_close_event，用 close_requested + CancelClose 拦截 X 关闭
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_tabs(ui);
            match self.page {
                Page::Home => self.draw_home(ui),
                Page::Settings => self.draw_settings(ui),
                Page::History => self.draw_history(ui),
                Page::About => self.draw_about(ui),
            }
        });

        // 托盘事件需要 UI 循环持续运转（隐藏窗口时保持低频重绘）
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}
