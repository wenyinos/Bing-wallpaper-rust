//! 界面字体：统一使用系统微软雅黑（回退宋体，Win7+ 全系自带）。
//!
//! egui 内置字体不含 CJK 字形（中文显示为方框），且与中文混排观感不一致；
//! 这里直接替换字体族为系统中文字体，拉丁/数字一并由雅黑渲染，观感统一。

use eframe::egui;
use tracing::{info, warn};

/// 候选系统中文字体（按优先级；ttc 集合取第 0 个 face，即 Regular）
const CANDIDATES: &[(&str, &str)] = &[
    ("msyh", r"C:\Windows\Fonts\msyh.ttc"),
    ("msyh-ttf", r"C:\Windows\Fonts\msyh.ttf"),
    ("simsun", r"C:\Windows\Fonts\simsun.ttc"),
];

pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    for (name, path) in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        fonts
            .font_data
            .insert((*name).into(), egui::FontData::from_owned(bytes).into());
        // 清空内置字体族，整个界面（含拉丁/数字）统一用系统中文字体
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let list = fonts.families.entry(family).or_default();
            list.clear();
            list.push((*name).into());
        }
        ctx.set_fonts(fonts);
        info!("界面字体已统一为: {path}");
        return;
    }
    warn!("未找到系统中文字体（msyh/simsun），回退 egui 内置字体，中文将显示为方框");
}
