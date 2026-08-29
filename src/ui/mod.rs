//! 纯软件渲染界面（v0.3）：Win32 原生控件 + GDI，零 GPU 依赖。
//!
//! 单线程事件模型：控件事件、`Notice`/`Timer` 全在 dispatch 线程处理；
//! 后台任务通过 `UpdateEnv.ui_dirty` 原子标志通知刷新（100ms 定时器轮询）。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use native_windows_gui as nwg;
use tracing::{error, info};

use crate::app::{spawn_next, spawn_provider_check, spawn_update, UpdateEnv};
use crate::cache::{CacheEntry, CacheManager};
use crate::i18n::Lang;
use crate::tray::TrayAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    History,
    Settings,
    About,
}

/// 事件闭包内需要可变访问的状态（Rc<AppUi> 只能提供共享借用）
struct UiState {
    page: Page,
    last_status: String,
    history_entries: Vec<CacheEntry>,
    home_bitmap: Option<Rc<nwg::Bitmap>>,
    history_bitmap: Option<Rc<nwg::Bitmap>>,
}

pub struct AppUi {
    window: nwg::Window,
    #[allow(dead_code)]
    icon: nwg::Icon,
    font: nwg::Font,
    font_bold: nwg::Font,
    timer: nwg::Timer,

    tab_home: nwg::Button,
    tab_history: nwg::Button,
    tab_settings: nwg::Button,
    tab_about: nwg::Button,

    home_frame: nwg::Frame,
    home_status: nwg::Label,
    home_last: nwg::Label,
    home_preview: nwg::ImageFrame,
    home_update_btn: nwg::Button,
    home_next_btn: nwg::Button,
    home_source: nwg::Label,

    history_frame: nwg::Frame,
    history_list: nwg::ListBox<String>,
    history_preview: nwg::ImageFrame,
    history_title: nwg::Label,
    history_set_btn: nwg::Button,
    history_open_btn: nwg::Button,
    history_del_btn: nwg::Button,
    history_refresh_btn: nwg::Button,

    settings_frame: nwg::Frame,
    s_provider_label: nwg::Label,
    s_provider_combo: nwg::ComboBox<String>,
    s_preset_label: nwg::Label,
    s_preset_combo: nwg::ComboBox<String>,
    s_fit_label: nwg::Label,
    s_fit_combo: nwg::ComboBox<String>,
    s_lang_label: nwg::Label,
    s_lang_combo: nwg::ComboBox<String>,
    s_auto_cb: nwg::CheckBox,
    s_startup_cb: nwg::CheckBox,
    s_cache_label: nwg::Label,
    s_cache_input: nwg::TextInput,
    s_rotate_label: nwg::Label,
    s_rotate_input: nwg::TextInput,
    s_repo_label: nwg::Label,
    s_repo_input: nwg::TextInput,
    s_pubkey_label: nwg::Label,
    s_pubkey_input: nwg::TextInput,
    s_check_btn: nwg::Button,
    s_provider_msg: nwg::Label,

    about_frame: nwg::Frame,
    about_label: nwg::Label,

    env: Arc<UpdateEnv>,
    rx: std::sync::mpsc::Receiver<TrayAction>,
    lang: Lang,
    state: RefCell<UiState>,
}

/// 启动 Win32 消息循环（阻塞；退出由托盘"退出"触发 process::exit）
pub fn run(env: Arc<UpdateEnv>, rx: std::sync::mpsc::Receiver<TrayAction>, lang: Lang, show: bool) {
    nwg::init().expect("Win32 UI 初始化失败");

    let t = crate::i18n::table(lang);
    let mut font = nwg::Font::default();
    nwg::Font::builder()
        .family("Microsoft YaHei")
        .size(18)
        .build(&mut font)
        .expect("创建界面字体失败");
    let mut font_bold = nwg::Font::default();
    nwg::Font::builder()
        .family("Microsoft YaHei")
        .size(22)
        .weight(700)
        .build(&mut font_bold)
        .expect("创建粗体字体失败");

    let icon = nwg::Icon::from_bin(crate::icon::ICON_BYTES).unwrap_or_default();

    let mut ui = AppUi {
        env,
        rx,
        lang,
        state: RefCell::new(UiState {
            page: Page::Home,
            last_status: String::new(),
            history_entries: Vec::new(),
            home_bitmap: None,
            history_bitmap: None,
        }),
        window: Default::default(),
        icon,
        font,
        font_bold,
        timer: Default::default(),
        tab_home: Default::default(),
        tab_history: Default::default(),
        tab_settings: Default::default(),
        tab_about: Default::default(),
        home_frame: Default::default(),
        home_status: Default::default(),
        home_last: Default::default(),
        home_preview: Default::default(),
        home_update_btn: Default::default(),
        home_next_btn: Default::default(),
        home_source: Default::default(),
        history_frame: Default::default(),
        history_list: Default::default(),
        history_preview: Default::default(),
        history_title: Default::default(),
        history_set_btn: Default::default(),
        history_open_btn: Default::default(),
        history_del_btn: Default::default(),
        history_refresh_btn: Default::default(),
        settings_frame: Default::default(),
        s_provider_label: Default::default(),
        s_provider_combo: Default::default(),
        s_preset_label: Default::default(),
        s_preset_combo: Default::default(),
        s_fit_label: Default::default(),
        s_fit_combo: Default::default(),
        s_lang_label: Default::default(),
        s_lang_combo: Default::default(),
        s_auto_cb: Default::default(),
        s_startup_cb: Default::default(),
        s_cache_label: Default::default(),
        s_cache_input: Default::default(),
        s_rotate_label: Default::default(),
        s_rotate_input: Default::default(),
        s_repo_label: Default::default(),
        s_repo_input: Default::default(),
        s_pubkey_label: Default::default(),
        s_pubkey_input: Default::default(),
        s_check_btn: Default::default(),
        s_provider_msg: Default::default(),
        about_frame: Default::default(),
        about_label: Default::default(),
    };

    // 主窗口（固定布局，不可调整大小）
    let mut flags = nwg::WindowFlags::WINDOW | nwg::WindowFlags::MINIMIZE_BOX;
    if show {
        flags |= nwg::WindowFlags::VISIBLE;
    }
    nwg::Window::builder()
        .flags(flags)
        .size((720, 560))
        .position((180, 120))
        .title("BingWallpaper-Rust")
        .icon(Some(&ui.icon))
        .build(&mut ui.window)
        .expect("创建主窗口失败");

    build_children(&mut ui);

    // 100ms 轮询定时器：托盘事件 + 状态刷新
    nwg::Timer::builder()
        .interval(100)
        .parent(&ui.window)
        .build(&mut ui.timer)
        .expect("创建定时器失败");

    let ui_ev = Rc::new(ui);
    // 启动首次更新（缓存命中零下载）；定时器须在闭包捕获前启动
    spawn_update(&ui_ev.env, lang);
    ui_ev.timer.start();
    nwg::full_bind_event_handler(&ui_ev.window.handle, move |evt, data, handle| {
        use nwg::Event::*;
        match evt {
            OnWindowClose => {
                // 方案 §15：关闭窗口 = 进托盘
                if let nwg::EventData::OnWindowClose(d) = data {
                    d.close(false);
                }
                ui_ev.window.set_visible(false);
            }
            OnButtonClick => ui_ev.on_button_click(&handle),
            OnComboxBoxSelection => ui_ev.on_combo_change(&handle),
            OnListBoxSelect => {
                if handle == ui_ev.history_list.handle {
                    ui_ev.show_history_preview();
                }
            }
            OnTimerTick => {
                while let Ok(action) = ui_ev.rx.try_recv() {
                    ui_ev.handle_tray_action(action);
                }
                ui_ev.refresh_status();
                ui_ev.refresh_provider_msg();
            }
            _ => {}
        }
    });

    // 进入消息循环（托盘常驻；退出仅经托盘"退出"）
    info!("UI 已就绪（纯软件渲染，Win32 控件）");
    nwg::dispatch_thread_events();
}

impl AppUi {
    fn t(&self) -> &'static crate::i18n::Strings {
        crate::i18n::table(self.lang)
    }

    fn on_button_click(self: &Rc<Self>, handle: &nwg::ControlHandle) {
        if handle == &self.tab_home.handle {
            self.show_page(Page::Home);
        } else if handle == &self.tab_history.handle {
            self.show_page(Page::History);
        } else if handle == &self.tab_settings.handle {
            self.show_page(Page::Settings);
        } else if handle == &self.tab_about.handle {
            self.show_page(Page::About);
        } else if handle == &self.home_update_btn.handle {
            spawn_update(&self.env, self.lang);
        } else if handle == &self.home_next_btn.handle {
            spawn_next(&self.env, self.lang);
        } else if handle == &self.history_refresh_btn.handle {
            self.reload_history();
        } else if handle == &self.history_set_btn.handle {
            self.apply_history_selection();
        } else if handle == &self.history_open_btn.handle {
            self.open_history_location();
        } else if handle == &self.history_del_btn.handle {
            self.delete_history_selection();
        } else if handle == &self.s_check_btn.handle {
            spawn_provider_check(&self.env, self.lang);
        } else if handle == &self.s_auto_cb.handle {
            if let Ok(mut cfg) = self.env.cfg.lock() {
                cfg.auto_update = self.s_auto_cb.check_state() == nwg::CheckBoxState::Checked;
                save_cfg(&mut cfg, &self.env.data_dir);
            }
        } else if handle == &self.s_startup_cb.handle {
            let want = self.s_startup_cb.check_state() == nwg::CheckBoxState::Checked;
            match crate::autostart::set_enabled(want) {
                Ok(()) => {
                    if let Ok(mut cfg) = self.env.cfg.lock() {
                        cfg.startup = want;
                        save_cfg(&mut cfg, &self.env.data_dir);
                    }
                }
                Err(err) => {
                    error!("开机启动设置失败: {err}");
                    self.s_provider_msg
                        .set_text(&format!("{}: {err}", self.t().status_failed_prefix));
                }
            }
        }
    }

    fn on_combo_change(&self, handle: &nwg::ControlHandle) {
        let Ok(mut cfg) = self.env.cfg.lock() else {
            return;
        };
        if handle == &self.s_provider_combo.handle {
            if let Some(i) = self.s_provider_combo.selection() {
                if let Some(id) = provider_ids(&self.env).get(i).cloned() {
                    cfg.provider = id;
                }
            }
        } else if handle == &self.s_preset_combo.handle {
            if let Some(i) = self.s_preset_combo.selection() {
                cfg.bing_preset = if i == 1 { "global" } else { "china" }.into();
            }
        } else if handle == &self.s_fit_combo.handle {
            if let Some(i) = self.s_fit_combo.selection() {
                cfg.fit_mode = ["fill", "fit", "stretch", "center", "span"][i.min(4)].into();
            }
        } else if handle == &self.s_lang_combo.handle {
            if let Some(i) = self.s_lang_combo.selection() {
                cfg.language = if i == 1 { "en" } else { "zh" }.into();
            }
        }
        save_cfg(&mut cfg, &self.env.data_dir);
    }

    fn show_page(&self, page: Page) {
        self.home_frame.set_visible(page == Page::Home);
        self.history_frame.set_visible(page == Page::History);
        self.settings_frame.set_visible(page == Page::Settings);
        self.about_frame.set_visible(page == Page::About);
        {
            let mut st = self.state.borrow_mut();
            st.page = page;
            if page == Page::History && st.history_entries.is_empty() {
                drop(st);
                self.reload_history();
            }
        }
        if page == Page::Home {
            self.load_home_preview();
        }
        if page == Page::Settings {
            self.sync_settings_controls();
        }
    }

    fn refresh_status(&self) {
        let Ok(s) = self.env.status.lock() else {
            return;
        };
        let mut st = self.state.borrow_mut();
        if s.message != st.last_status {
            st.last_status = s.message.clone();
            self.home_status.set_text(&s.message);
            match &s.last_set {
                Some(d) => self
                    .home_last
                    .set_text(&format!("{}: {d}", self.t().last_set_label)),
                None => self.home_last.set_text(""),
            }
            if !s.running {
                self.load_home_preview();
            }
        }
    }

    fn refresh_provider_msg(&self) {
        if let Ok(slot) = self.env.provider_check_msg.lock() {
            if let Some((msg, at)) = slot.as_ref() {
                if at.elapsed().as_secs() < 8 {
                    self.s_provider_msg.set_text(msg);
                }
            }
        }
    }

    fn load_home_preview(&self) {
        let entry = {
            let cache = CacheManager::new(&self.env.data_dir);
            let index = cache.load_index();
            index
                .last_set
                .and_then(|ls| {
                    index
                        .entries
                        .into_iter()
                        .find(|e| e.provider == ls.provider && e.wallpaper_id == ls.wallpaper_id)
                })
                .and_then(|entry| {
                    let source = cache.dir().join(&entry.file);
                    crate::thumbs::ensure_thumbnail(cache.dir(), &source, &entry.file)
                })
        };
        let Some(thumb) = entry else { return };
        match nwg::Bitmap::from_file(thumb.to_string_lossy().as_ref(), false) {
            Ok(bm) => {
                let bm = Rc::new(bm);
                self.home_preview.set_bitmap(Some(&bm));
                self.state.borrow_mut().home_bitmap = Some(bm);
            }
            Err(err) => error!("加载预览失败: {err:?}"),
        }
    }

    fn reload_history(&self) {
        let mut entries = CacheManager::new(&self.env.data_dir).entries();
        entries.sort_by(|a, b| b.added_at.cmp(&a.added_at));
        self.history_list.clear();
        for e in &entries {
            let date = e
                .date
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            let title = e.title.clone().unwrap_or_else(|| e.wallpaper_id.clone());
            self.history_list.push(format!("{date}  {title}"));
        }
        let mut st = self.state.borrow_mut();
        st.history_entries = entries;
        st.history_bitmap = None;
        self.history_preview.set_bitmap(None);
        self.history_title.set_text("");
    }

    fn selected_history(&self) -> Option<CacheEntry> {
        let i = self.history_list.selection()?;
        self.state.borrow().history_entries.get(i).cloned()
    }

    fn show_history_preview(&self) {
        let Some(entry) = self.selected_history() else {
            return;
        };
        let cache = CacheManager::new(&self.env.data_dir);
        let source = cache.dir().join(&entry.file);
        if let Some(thumb) = crate::thumbs::ensure_thumbnail(cache.dir(), &source, &entry.file) {
            match nwg::Bitmap::from_file(thumb.to_string_lossy().as_ref(), false) {
                Ok(bm) => {
                    let bm = Rc::new(bm);
                    self.history_preview.set_bitmap(Some(&bm));
                    self.state.borrow_mut().history_bitmap = Some(bm);
                }
                Err(err) => error!("加载缩略图失败: {err:?}"),
            }
        }
        self.history_title.set_text(
            &entry
                .title
                .clone()
                .unwrap_or_else(|| entry.wallpaper_id.clone()),
        );
    }

    fn apply_history_selection(&self) {
        let Some(entry) = self.selected_history() else {
            return;
        };
        let path = CacheManager::new(&self.env.data_dir)
            .dir()
            .join(&entry.file);
        let fit = self
            .env
            .cfg
            .lock()
            .map(|c| c.fit_mode.clone())
            .unwrap_or_else(|_| "fill".into());
        let env = Arc::clone(&self.env);
        let lang = self.lang;
        let title = entry
            .title
            .clone()
            .unwrap_or_else(|| entry.wallpaper_id.clone());
        let provider = entry.provider.clone();
        let wallpaper_id = entry.wallpaper_id.clone();
        self.env.rt.spawn(async move {
            let t = crate::i18n::table(lang);
            set_status(&env, true, t.status_updating.into());
            let result =
                tokio::task::spawn_blocking(move || crate::wallpaper::set_wallpaper(&path, &fit))
                    .await;
            match result {
                Ok(Ok(())) => {
                    info!("历史壁纸已设置: {title}");
                    CacheManager::new(&env.data_dir).record_last_set(&provider, &wallpaper_id);
                    set_status(&env, false, format!("{}{title}", t.status_done_prefix));
                }
                Ok(Err(err)) => {
                    error!("历史壁纸设置失败: {err}");
                    set_status(&env, false, format!("{}{err}", t.status_failed_prefix));
                }
                Err(err) => {
                    error!("壁纸任务异常: {err}");
                    set_status(&env, false, format!("{}{err}", t.status_failed_prefix));
                }
            }
        });
    }

    fn open_history_location(&self) {
        let Some(entry) = self.selected_history() else {
            return;
        };
        let path = CacheManager::new(&self.env.data_dir)
            .dir()
            .join(&entry.file)
            .to_string_lossy()
            .into_owned();
        let _ = std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn();
    }

    fn delete_history_selection(&self) {
        let Some(entry) = self.selected_history() else {
            return;
        };
        CacheManager::new(&self.env.data_dir).remove_entry(&entry.provider, &entry.wallpaper_id);
        self.reload_history();
    }

    fn handle_tray_action(&self, action: TrayAction) {
        match action {
            TrayAction::Open => self.window.set_visible(true),
            TrayAction::UpdateNow => spawn_update(&self.env, self.lang),
            TrayAction::Next => spawn_next(&self.env, self.lang),
            TrayAction::History => {
                self.show_page(Page::History);
                self.window.set_visible(true);
            }
            TrayAction::Settings => {
                self.show_page(Page::Settings);
                self.window.set_visible(true);
            }
            TrayAction::About => {
                self.show_page(Page::About);
                self.window.set_visible(true);
            }
            TrayAction::Quit => {
                info!("用户从托盘请求退出");
                // 配置即改即存、缓存索引原子写，直接退出无状态丢失
                std::process::exit(0);
            }
            TrayAction::ReloadProviders => {
                let loaded = crate::provider::manifest::load_all(&self.env.data_dir);
                if let Ok(mut guard) = self.env.providers.lock() {
                    *guard = loaded.into_iter().map(Arc::new).collect();
                }
                self.sync_settings_controls();
            }
        }
    }

    fn sync_settings_controls(&self) {
        let Ok(cfg) = self.env.cfg.lock() else { return };
        // ComboBox 无 clear 方法，按现有条数逐个移除
        for i in (0..self.s_provider_combo.len()).rev() {
            self.s_provider_combo.remove(i);
        }
        for (_, label) in provider_entries(&self.env) {
            self.s_provider_combo.push(label);
        }
        let ids = provider_ids(&self.env);
        let cur = ids
            .iter()
            .position(|id| *id == cfg.provider)
            .or(if ids.is_empty() { None } else { Some(0) });
        self.s_provider_combo.set_selection(cur);
        self.s_preset_combo
            .set_selection(Some(if cfg.bing_preset == "global" { 1 } else { 0 }));
        self.s_fit_combo.set_selection(Some(
            ["fill", "fit", "stretch", "center", "span"]
                .iter()
                .position(|f| *f == cfg.fit_mode)
                .unwrap_or(0),
        ));
        self.s_lang_combo
            .set_selection(Some(if cfg.language == "en" { 1 } else { 0 }));
        self.s_auto_cb.set_check_state(if cfg.auto_update {
            nwg::CheckBoxState::Checked
        } else {
            nwg::CheckBoxState::Unchecked
        });
        self.s_startup_cb.set_check_state(if cfg.startup {
            nwg::CheckBoxState::Checked
        } else {
            nwg::CheckBoxState::Unchecked
        });
        self.s_cache_input.set_text(&cfg.cache_days.to_string());
        self.s_rotate_input
            .set_text(&cfg.rotate_minutes.to_string());
        self.s_repo_input.set_text(&cfg.provider_repo_url);
        self.s_pubkey_input
            .set_text(cfg.provider_repo_public_key.as_deref().unwrap_or(""));
    }
}

fn set_status(env: &Arc<UpdateEnv>, running: bool, message: String) {
    if let Ok(mut s) = env.status.lock() {
        s.running = running;
        s.message = message;
    }
    env.notify_ui();
}

fn save_cfg(cfg: &mut crate::app::config::Config, data_dir: &std::path::Path) {
    if let Err(err) = crate::app::config::Config::save(cfg, data_dir) {
        error!("保存配置失败: {err}");
    }
}

fn provider_entries(env: &Arc<UpdateEnv>) -> Vec<(String, String)> {
    env.providers
        .lock()
        .map(|providers| {
            providers
                .iter()
                .map(|p| {
                    (
                        p.manifest.id.clone(),
                        format!("{} ({})", p.manifest.name, p.manifest.id),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn provider_ids(env: &Arc<UpdateEnv>) -> Vec<String> {
    env.providers
        .lock()
        .map(|providers| providers.iter().map(|p| p.manifest.id.clone()).collect())
        .unwrap_or_default()
}

fn build_children(ui: &mut AppUi) {
    let t = ui.t();
    let f = &ui.font;

    // 页签
    for (btn, text, x) in [
        (&mut ui.tab_home, t.tab_home, 8),
        (&mut ui.tab_history, t.tab_history, 108),
        (&mut ui.tab_settings, t.tab_settings, 208),
        (&mut ui.tab_about, t.tab_about, 308),
    ] {
        nwg::Button::builder()
            .parent(&ui.window)
            .position((x, 8))
            .size((96, 28))
            .text(text)
            .font(Some(f))
            .build(btn)
            .expect("创建页签失败");
    }

    // 页面容器（主页可见，其余隐藏，由页签切换）
    for (frame, visible) in [
        (&mut ui.home_frame, true),
        (&mut ui.history_frame, false),
        (&mut ui.settings_frame, false),
        (&mut ui.about_frame, false),
    ] {
        nwg::Frame::builder()
            .parent(&ui.window)
            .position((8, 42))
            .size((692, 470))
            .build(frame)
            .expect("创建页面容器失败");
        frame.set_visible(visible);
    }

    // ---- 主页 ----
    nwg::ImageFrame::builder()
        .parent(&ui.home_frame)
        .position((10, 10))
        .size((320, 180))
        .background_color(Some([240, 240, 240]))
        .build(&mut ui.home_preview)
        .unwrap();
    nwg::Label::builder()
        .parent(&ui.home_frame)
        .position((344, 16))
        .size((336, 64))
        .text(t.status_ready)
        .flags(nwg::LabelFlags::VISIBLE)
        .font(Some(&ui.font_bold))
        .build(&mut ui.home_status)
        .unwrap();
    nwg::Label::builder()
        .parent(&ui.home_frame)
        .position((344, 88))
        .size((336, 22))
        .font(Some(f))
        .flags(nwg::LabelFlags::VISIBLE)
        .build(&mut ui.home_last)
        .unwrap();
    nwg::Button::builder()
        .parent(&ui.home_frame)
        .position((344, 128))
        .size((150, 32))
        .text(t.update_now)
        .font(Some(f))
        .build(&mut ui.home_update_btn)
        .unwrap();
    nwg::Button::builder()
        .parent(&ui.home_frame)
        .position((504, 128))
        .size((150, 32))
        .text(t.next_wallpaper)
        .font(Some(f))
        .build(&mut ui.home_next_btn)
        .unwrap();
    let source_text = format!("{}: {}", t.data_dir_label, ui.env.data_dir.display());
    nwg::Label::builder()
        .parent(&ui.home_frame)
        .position((10, 432))
        .size((668, 24))
        .text(&source_text)
        .flags(nwg::LabelFlags::VISIBLE)
        .font(Some(f))
        .build(&mut ui.home_source)
        .unwrap();

    // ---- 历史 ----
    nwg::ListBox::builder()
        .parent(&ui.history_frame)
        .position((10, 10))
        .size((320, 396))
        .font(Some(f))
        .build(&mut ui.history_list)
        .unwrap();
    nwg::ImageFrame::builder()
        .parent(&ui.history_frame)
        .position((344, 10))
        .size((336, 190))
        .background_color(Some([240, 240, 240]))
        .build(&mut ui.history_preview)
        .unwrap();
    nwg::Label::builder()
        .parent(&ui.history_frame)
        .position((344, 208))
        .size((336, 22))
        .font(Some(f))
        .flags(nwg::LabelFlags::VISIBLE)
        .build(&mut ui.history_title)
        .unwrap();
    nwg::Button::builder()
        .parent(&ui.history_frame)
        .position((344, 240))
        .size((104, 30))
        .text(t.history_set)
        .font(Some(f))
        .build(&mut ui.history_set_btn)
        .unwrap();
    nwg::Button::builder()
        .parent(&ui.history_frame)
        .position((458, 240))
        .size((104, 30))
        .text(t.history_open_location)
        .font(Some(f))
        .build(&mut ui.history_open_btn)
        .unwrap();
    nwg::Button::builder()
        .parent(&ui.history_frame)
        .position((572, 240))
        .size((104, 30))
        .text(t.history_delete)
        .font(Some(f))
        .build(&mut ui.history_del_btn)
        .unwrap();
    nwg::Button::builder()
        .parent(&ui.history_frame)
        .position((10, 416))
        .size((320, 30))
        .text(t.history_refresh)
        .font(Some(f))
        .build(&mut ui.history_refresh_btn)
        .unwrap();

    // ---- 设置 ----
    let mut y = 16;
    macro_rules! combo_row {
        ($label:expr, $text:expr, $combo:expr) => {{
            nwg::Label::builder()
                .parent(&ui.settings_frame)
                .position((14, y + 2))
                .size((240, 24))
                .text($text)
                .font(Some(f))
                .flags(nwg::LabelFlags::VISIBLE)
                .build($label)
                .unwrap();
            nwg::ComboBox::builder()
                .parent(&ui.settings_frame)
                .position((264, y))
                .size((400, 24))
                .font(Some(f))
                .build($combo)
                .unwrap();
            y += 40;
        }};
    }
    combo_row!(
        &mut ui.s_provider_label,
        t.provider_label,
        &mut ui.s_provider_combo
    );
    combo_row!(
        &mut ui.s_preset_label,
        t.preset_label,
        &mut ui.s_preset_combo
    );
    combo_row!(&mut ui.s_fit_label, t.fit_mode_label, &mut ui.s_fit_combo);
    combo_row!(&mut ui.s_lang_label, t.language_label, &mut ui.s_lang_combo);

    nwg::CheckBox::builder()
        .parent(&ui.settings_frame)
        .position((14, y))
        .size((650, 24))
        .text(t.auto_update_label)
        .font(Some(f))
        .build(&mut ui.s_auto_cb)
        .unwrap();
    y += 34;
    nwg::CheckBox::builder()
        .parent(&ui.settings_frame)
        .position((14, y))
        .size((650, 24))
        .text(t.autostart_label)
        .font(Some(f))
        .build(&mut ui.s_startup_cb)
        .unwrap();
    y += 34;
    nwg::Label::builder()
        .parent(&ui.settings_frame)
        .position((14, y + 2))
        .size((240, 24))
        .text(t.cache_days_label)
        .font(Some(f))
        .flags(nwg::LabelFlags::VISIBLE)
        .build(&mut ui.s_cache_label)
        .unwrap();
    nwg::TextInput::builder()
        .parent(&ui.settings_frame)
        .position((264, y))
        .size((120, 24))
        .font(Some(f))
        .build(&mut ui.s_cache_input)
        .unwrap();
    y += 40;
    nwg::Label::builder()
        .parent(&ui.settings_frame)
        .position((14, y + 2))
        .size((240, 24))
        .text(t.rotate_label)
        .font(Some(f))
        .flags(nwg::LabelFlags::VISIBLE)
        .build(&mut ui.s_rotate_label)
        .unwrap();
    nwg::TextInput::builder()
        .parent(&ui.settings_frame)
        .position((264, y))
        .size((120, 24))
        .font(Some(f))
        .build(&mut ui.s_rotate_input)
        .unwrap();
    y += 40;
    nwg::Label::builder()
        .parent(&ui.settings_frame)
        .position((14, y + 2))
        .size((240, 24))
        .text(t.provider_repo_label)
        .font(Some(f))
        .flags(nwg::LabelFlags::VISIBLE)
        .build(&mut ui.s_repo_label)
        .unwrap();
    nwg::TextInput::builder()
        .parent(&ui.settings_frame)
        .position((264, y))
        .size((400, 24))
        .font(Some(f))
        .build(&mut ui.s_repo_input)
        .unwrap();
    y += 38;
    nwg::Label::builder()
        .parent(&ui.settings_frame)
        .position((14, y + 2))
        .size((240, 24))
        .text(t.provider_public_key_label)
        .font(Some(f))
        .flags(nwg::LabelFlags::VISIBLE)
        .build(&mut ui.s_pubkey_label)
        .unwrap();
    nwg::TextInput::builder()
        .parent(&ui.settings_frame)
        .position((264, y))
        .size((400, 24))
        .font(Some(f))
        .build(&mut ui.s_pubkey_input)
        .unwrap();
    y += 40;
    nwg::Button::builder()
        .parent(&ui.settings_frame)
        .position((14, y))
        .size((220, 30))
        .text(t.provider_check_update)
        .font(Some(f))
        .build(&mut ui.s_check_btn)
        .unwrap();
    nwg::Label::builder()
        .parent(&ui.settings_frame)
        .position((244, y + 6))
        .size((420, 22))
        .flags(nwg::LabelFlags::VISIBLE)
        .font(Some(f))
        .build(&mut ui.s_provider_msg)
        .unwrap();

    // ---- 关于 ----
    nwg::Label::builder()
        .parent(&ui.about_frame)
        .position((20, 24))
        .size((652, 320))
        .text(t.about_text)
        .flags(nwg::LabelFlags::VISIBLE)
        .font(Some(f))
        .build(&mut ui.about_label)
        .unwrap();
}
