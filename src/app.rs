//! GUI 模組 - 使用 egui/eframe 建立視窗介面

use eframe::egui;

use crate::config;
use crate::traffic_shaper::{BandwidthLimiter, ProcessRule};

// ── macOS 風格色彩 ──
const BG_LIGHT: egui::Color32 = egui::Color32::from_rgb(236, 236, 236);
const CARD_BG: egui::Color32 = egui::Color32::from_rgb(255, 255, 255);
const CARD_BORDER: egui::Color32 = egui::Color32::from_rgb(210, 210, 210);
const ACCENT_BLUE: egui::Color32 = egui::Color32::from_rgb(0, 122, 255);
const ACCENT_GREEN: egui::Color32 = egui::Color32::from_rgb(52, 199, 89);
const ACCENT_RED: egui::Color32 = egui::Color32::from_rgb(255, 59, 48);
const ACCENT_ORANGE: egui::Color32 = egui::Color32::from_rgb(255, 149, 0);
const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(28, 28, 30);
const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(99, 99, 102);
const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(142, 142, 147);
const INPUT_BG: egui::Color32 = egui::Color32::from_rgb(242, 242, 247);

/// 主應用程式狀態
pub struct SpeedLimitApp {
    limiter: BandwidthLimiter,
    dl_input: String,
    ul_input: String,
    is_running: bool,
    error_message: Option<String>,
    current_dl_speed: String,
    current_ul_speed: String,

    // Per-process 規則 UI
    rule_process_input: String,
    rule_process_is_custom: bool,
    rule_dl_input: String,
    rule_ul_input: String,
    process_rules: Vec<ProcessRule>,

    // Per-process 流量統計
    process_stats_display: Vec<(String, String, String)>, // (name, dl_speed, ul_speed)
}

impl Default for SpeedLimitApp {
    fn default() -> Self {
        Self {
            limiter: BandwidthLimiter::new(),
            dl_input: "0".to_string(),
            ul_input: "0".to_string(),
            is_running: false,
            error_message: None,
            current_dl_speed: "—".to_string(),
            current_ul_speed: "—".to_string(),

            rule_process_input: String::new(),
            rule_process_is_custom: false,
            rule_dl_input: "0".to_string(),
            rule_ul_input: "0".to_string(),
            process_rules: Vec::new(),

            process_stats_display: Vec::new(),
        }
    }
}

impl SpeedLimitApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_custom_fonts(&cc.egui_ctx);

        // 從設定檔載入上次的設定
        let cfg = config::load_config();
        let dl_str = if cfg.download_limit_mbps > 0.0 {
            cfg.download_limit_mbps.to_string()
        } else {
            "0".to_string()
        };
        let ul_str = if cfg.upload_limit_mbps > 0.0 {
            cfg.upload_limit_mbps.to_string()
        } else {
            "0".to_string()
        };

        Self {
            limiter: BandwidthLimiter::new(),
            dl_input: dl_str,
            ul_input: ul_str,
            is_running: false,
            error_message: None,
            current_dl_speed: "—".to_string(),
            current_ul_speed: "—".to_string(),
            rule_process_input: String::new(),
            rule_process_is_custom: false,
            rule_dl_input: "0".to_string(),
            rule_ul_input: "0".to_string(),
            process_rules: cfg.process_rules,
            process_stats_display: Vec::new(),
        }
    }

    /// 格式化 bytes/s 為人類可讀格式
    fn format_speed(bps: f64) -> String {
        if bps <= 0.0 {
            return "—".to_string();
        }
        let mbps = bps * 8.0 / 1_000_000.0; // bytes/s → Mbps
        if mbps >= 1.0 {
            format!("{:.1} Mbps", mbps)
        } else {
            let kbps = bps * 8.0 / 1_000.0; // bytes/s → Kbps
            format!("{:.0} Kbps", kbps)
        }
    }

    fn toggle_limiting(&mut self) {
        if self.is_running {
            // 停止
            self.limiter.stop();
            self.is_running = false;
            self.error_message = None;
            self.current_dl_speed = "—".to_string();
            self.current_ul_speed = "—".to_string();
            self.process_stats_display.clear();
        } else {
            // 驗證輸入
            let dl: f64 = match self.dl_input.trim().parse() {
                Ok(v) if v >= 0.0 => v,
                _ => {
                    self.error_message = Some("下載限速請輸入有效的非負數字".to_string());
                    return;
                }
            };
            let ul: f64 = match self.ul_input.trim().parse() {
                Ok(v) if v >= 0.0 => v,
                _ => {
                    self.error_message = Some("上傳限速請輸入有效的非負數字".to_string());
                    return;
                }
            };

            self.limiter.set_limits(dl, ul);
            self.limiter.set_process_rules(self.process_rules.clone());

            // 儲存目前設定
            self.save_current_config();

            match self.limiter.start() {
                Ok(()) => {
                    self.is_running = true;
                    self.error_message = None;
                }
                Err(e) => {
                    self.error_message = Some(format!("啟動失敗: {}", e));
                }
            }
        }
    }

    fn update_stats(&mut self) {
        if self.is_running {
            let stats = self.limiter.get_stats();
            self.current_dl_speed = Self::format_speed(stats.download_bps);
            self.current_ul_speed = Self::format_speed(stats.upload_bps);

            // 更新 per-process 統計
            self.process_stats_display = stats
                .process_stats
                .iter()
                .map(|(name, dl, ul)| {
                    (name.clone(), Self::format_speed(*dl), Self::format_speed(*ul))
                })
                .collect();

            // 同步檢查 limiter 是否異常停止
            if !self.limiter.is_running() {
                self.is_running = false;
                self.error_message = Some("限速器異常停止".to_string());
            }
        }
    }

    fn add_process_rule(&mut self) {
        let name = self.rule_process_input.trim().to_string();
        if name.is_empty() {
            self.error_message = Some("請輸入程序名稱（如 chrome.exe）".to_string());
            return;
        }

        let dl: f64 = match self.rule_dl_input.trim().parse() {
            Ok(v) if v >= 0.0 => v,
            _ => {
                self.error_message = Some("程序下載限速請輸入有效的非負數字".to_string());
                return;
            }
        };

        let ul: f64 = match self.rule_ul_input.trim().parse() {
            Ok(v) if v >= 0.0 => v,
            _ => {
                self.error_message = Some("程序上傳限速請輸入有效的非負數字".to_string());
                return;
            }
        };

        // 檢查是否已有相同 process
        if self.process_rules.iter().any(|r| r.process_name.to_lowercase() == name.to_lowercase()) {
            self.error_message = Some(format!("已存在 {} 的規則", name));
            return;
        }

        self.process_rules.push(ProcessRule {
            process_name: name,
            download_mbps: dl,
            upload_mbps: ul,
        });

        self.error_message = None;
        self.rule_process_input.clear();
        self.rule_dl_input = "0".to_string();
        self.rule_ul_input = "0".to_string();

        self.save_current_config();
    }

    /// 儲存目前設定到 JSON 檔
    fn save_current_config(&self) {
        let cfg = config::AppConfig {
            download_limit_mbps: self.dl_input.trim().parse().unwrap_or(0.0),
            upload_limit_mbps: self.ul_input.trim().parse().unwrap_or(0.0),
            process_rules: self.process_rules.clone(),
        };
        if let Err(e) = config::save_config(&cfg) {
            eprintln!("儲存設定失敗: {}", e);
        }
    }
}

/// 設定中文字型以解決方塊亂碼問題
fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let font_path = "C:\\Windows\\Fonts\\msjh.ttc";
    
    if let Ok(font_data) = std::fs::read(font_path) {
        fonts.font_data.insert(
            "msjh".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(font_data)),
        );
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap()
            .insert(0, "msjh".to_owned());
        fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap()
            .insert(0, "msjh".to_owned());
        ctx.set_fonts(fonts);
    }
}

/// 套用 macOS 風格淺色主題
fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(16);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);

    // 關閉深色模式
    style.visuals.dark_mode = false;

    // 圓角 (macOS 大圓角風格)
    style.visuals.window_corner_radius = egui::CornerRadius::same(12);
    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);

    // 背景
    style.visuals.panel_fill = BG_LIGHT;
    style.visuals.window_fill = CARD_BG;
    style.visuals.extreme_bg_color = INPUT_BG;

    // Widget 外觀
    style.visuals.widgets.noninteractive.bg_fill = CARD_BG;
    style.visuals.widgets.inactive.bg_fill = INPUT_BG;
    style.visuals.widgets.inactive.weak_bg_fill = INPUT_BG;
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(229, 229, 234);
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(209, 209, 214);

    // 文字色
    style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);
    style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);

    // 邊框 (macOS 細邊框)
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, CARD_BORDER);
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.5, egui::Color32::from_rgb(195, 195, 200));
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT_BLUE);

    // 選取色
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(0, 122, 255).gamma_multiply(0.3);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT_BLUE);

    ctx.set_style(style);
}

/// macOS 風格卡片 (白底 + 淺灰陰影邊框)
fn card_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(CARD_BG)
        .stroke(egui::Stroke::new(0.5, CARD_BORDER))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .outer_margin(egui::Margin::symmetric(4, 4))
        .shadow(egui::epaint::Shadow {
            offset: [0, 1],
            blur: 4,
            spread: 0,
            color: egui::Color32::from_rgba_premultiplied(0, 0, 0, 18),
        })
}

/// 繪製區塊標題 (macOS 風格)
fn section_header(ui: &mut egui::Ui, icon: &str, title: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(icon).size(15.0));
        ui.label(
            egui::RichText::new(title)
                .size(14.0)
                .strong()
                .color(TEXT_PRIMARY),
        );
    });
    ui.add_space(6.0);
}

impl eframe::App for SpeedLimitApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_stats();
        if self.is_running {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        apply_theme(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_min_width(ui.available_width());

                ui.add_space(8.0);

                // ── 標題列 ──
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("⚡ Speed Limit")
                            .size(22.0)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );
                    ui.label(
                        egui::RichText::new("全域 & 程序級頻寬限制器")
                            .size(11.0)
                            .color(TEXT_DIM),
                    );
                });

                ui.add_space(8.0);

                // ── 狀態指示條 (macOS pill style) ──
                let (status_bg, status_border, status_dot, status_text) = if self.is_running {
                    let rules_count = self.process_rules.len();
                    let info = if rules_count > 0 {
                        format!("運行中 — ⬇{} ⬆{} Mbps · {} 條規則", self.dl_input, self.ul_input, rules_count)
                    } else {
                        format!("運行中 — ⬇ {} ⬆ {} Mbps", self.dl_input, self.ul_input)
                    };
                    (
                        egui::Color32::from_rgb(234, 248, 237),
                        egui::Color32::from_rgb(190, 230, 196),
                        ACCENT_GREEN,
                        info,
                    )
                } else {
                    (
                        egui::Color32::from_rgb(254, 236, 235),
                        egui::Color32::from_rgb(240, 200, 198),
                        ACCENT_RED,
                        "已停止".to_string(),
                    )
                };

                egui::Frame::NONE
                    .fill(status_bg)
                    .stroke(egui::Stroke::new(0.5, status_border))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(14, 7))
                    .outer_margin(egui::Margin::symmetric(4, 2))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("●").size(10.0).color(status_dot));
                            ui.label(egui::RichText::new(status_text).size(12.0).color(TEXT_PRIMARY));
                        });
                    });

                ui.add_space(4.0);

                // ── 全域限速設定 ──
                card_frame().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    section_header(ui, "📊", "全域限速設定");

                    ui.columns(2, |cols| {
                        cols[0].horizontal(|ui| {
                            ui.label(egui::RichText::new("⬇ 下載").size(13.0).color(ACCENT_BLUE));
                            let dl_edit = egui::TextEdit::singleline(&mut self.dl_input)
                                .desired_width(60.0)
                                .interactive(!self.is_running);
                            ui.add(dl_edit);
                            ui.label(egui::RichText::new("Mbps").size(11.0).color(TEXT_DIM));
                        });
                        cols[1].horizontal(|ui| {
                            ui.label(egui::RichText::new("⬆ 上傳").size(13.0).color(ACCENT_GREEN));
                            let ul_edit = egui::TextEdit::singleline(&mut self.ul_input)
                                .desired_width(60.0)
                                .interactive(!self.is_running);
                            ui.add(ul_edit);
                            ui.label(egui::RichText::new("Mbps").size(11.0).color(TEXT_DIM));
                        });
                    });

                    ui.label(egui::RichText::new("0 = 不限制").size(10.0).color(TEXT_DIM));
                });

                // ── 程序限速規則 ──
                card_frame().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    section_header(ui, "🎯", "程序限速規則");

                    if !self.is_running {
                        // 新增規則輸入區 (macOS 淺灰底)
                        egui::Frame::NONE
                            .fill(INPUT_BG)
                            .corner_radius(egui::CornerRadius::same(10))
                            .inner_margin(egui::Margin::same(12))
                            .stroke(egui::Stroke::new(0.5, egui::Color32::from_rgb(215, 215, 220)))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("程序").size(12.0).color(TEXT_SECONDARY));

                                    let mut active_procs = self.limiter.get_active_processes();
                                    active_procs.retain(|p| p != "System Idle" && p != "System" && !p.starts_with("PID:"));

                                    egui::ComboBox::from_id_salt("process_combo")
                                        .width(140.0)
                                        .selected_text(if self.rule_process_is_custom {
                                            "自訂輸入...".to_string()
                                        } else if self.rule_process_input.is_empty() {
                                            "選擇程序...".to_string()
                                        } else {
                                            self.rule_process_input.clone()
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.rule_process_is_custom, true, "✍ 自訂輸入...");
                                            ui.separator();
                                            for p in active_procs {
                                                if ui.selectable_value(&mut self.rule_process_input, p.clone(), &p).clicked() {
                                                    self.rule_process_is_custom = false;
                                                }
                                            }
                                        });

                                    if self.rule_process_is_custom {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.rule_process_input)
                                                .desired_width(90.0)
                                                .hint_text("chrome.exe"),
                                        );
                                    }
                                });

                                ui.add_space(4.0);

                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("⬇").size(12.0).color(ACCENT_BLUE));
                                    ui.add(egui::TextEdit::singleline(&mut self.rule_dl_input).desired_width(45.0));
                                    ui.label(egui::RichText::new("⬆").size(12.0).color(ACCENT_GREEN));
                                    ui.add(egui::TextEdit::singleline(&mut self.rule_ul_input).desired_width(45.0));
                                    ui.label(egui::RichText::new("Mbps").size(10.0).color(TEXT_DIM));

                                    let add_btn = egui::Button::new(
                                        egui::RichText::new("＋ 新增").size(12.0).color(egui::Color32::WHITE),
                                    )
                                    .fill(ACCENT_BLUE)
                                    .corner_radius(egui::CornerRadius::same(7));

                                    if ui.add(add_btn).clicked() {
                                        self.add_process_rule();
                                    }
                                });
                            });

                        ui.add_space(4.0);
                    }

                    // 規則列表
                    if !self.process_rules.is_empty() {
                        let mut to_remove: Option<usize> = None;

                        egui::Grid::new("rules_grid")
                            .num_columns(4)
                            .spacing([10.0, 6.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("程序").size(11.0).strong().color(TEXT_SECONDARY));
                                ui.label(egui::RichText::new("⬇ 下載").size(11.0).strong().color(TEXT_SECONDARY));
                                ui.label(egui::RichText::new("⬆ 上傳").size(11.0).strong().color(TEXT_SECONDARY));
                                ui.label(egui::RichText::new("").size(11.0));
                                ui.end_row();

                                for (i, rule) in self.process_rules.iter().enumerate() {
                                    ui.label(
                                        egui::RichText::new(&rule.process_name)
                                            .size(12.0)
                                            .color(ACCENT_BLUE),
                                    );
                                    let dl_txt = if rule.download_mbps > 0.0 {
                                        format!("{} Mbps", rule.download_mbps)
                                    } else {
                                        "不限".to_string()
                                    };
                                    let ul_txt = if rule.upload_mbps > 0.0 {
                                        format!("{} Mbps", rule.upload_mbps)
                                    } else {
                                        "不限".to_string()
                                    };
                                    ui.label(egui::RichText::new(dl_txt).size(11.0).color(TEXT_PRIMARY));
                                    ui.label(egui::RichText::new(ul_txt).size(11.0).color(TEXT_PRIMARY));

                                    if !self.is_running {
                                        let del_btn = egui::Button::new(
                                            egui::RichText::new("✕").size(10.0).color(ACCENT_RED),
                                        )
                                        .fill(egui::Color32::from_rgb(255, 235, 235))
                                        .stroke(egui::Stroke::new(0.5, egui::Color32::from_rgb(240, 200, 198)))
                                        .corner_radius(egui::CornerRadius::same(5));

                                        if ui.add(del_btn).clicked() {
                                            to_remove = Some(i);
                                        }
                                    } else {
                                        ui.label("");
                                    }
                                    ui.end_row();
                                }
                            });

                        if let Some(i) = to_remove {
                            self.process_rules.remove(i);
                            self.save_current_config();
                        }
                    } else {
                        ui.label(
                            egui::RichText::new("尚未設定程序規則，僅套用全域限速")
                                .size(11.0)
                                .color(TEXT_DIM),
                        );
                    }
                });

                // ── 開始 / 停止 按鈕 ──
                ui.add_space(2.0);
                ui.vertical_centered(|ui| {
                    let (btn_text, btn_color) = if self.is_running {
                        ("⏹  停止限速", ACCENT_RED)
                    } else {
                        ("▶  開始限速", ACCENT_BLUE)
                    };

                    let button = egui::Button::new(
                        egui::RichText::new(btn_text).size(16.0).strong().color(egui::Color32::WHITE),
                    )
                    .fill(btn_color)
                    .min_size(egui::vec2(220.0, 38.0))
                    .corner_radius(egui::CornerRadius::same(10));

                    if ui.add(button).clicked() {
                        self.toggle_limiting();
                    }
                });
                ui.add_space(2.0);

                // ── 即時流量監控 ──
                if self.is_running {
                    card_frame().show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        section_header(ui, "📈", "即時流量");

                        ui.columns(2, |cols| {
                            // 下載速度卡片
                            egui::Frame::NONE
                                .fill(egui::Color32::from_rgb(235, 245, 255))
                                .corner_radius(egui::CornerRadius::same(10))
                                .inner_margin(egui::Margin::same(12))
                                .stroke(egui::Stroke::new(0.5, egui::Color32::from_rgb(190, 220, 255)))
                                .show(&mut cols[0], |ui| {
                                    ui.vertical_centered(|ui| {
                                        ui.label(egui::RichText::new("⬇ 下載").size(11.0).color(TEXT_SECONDARY));
                                        ui.label(
                                            egui::RichText::new(&self.current_dl_speed)
                                                .size(18.0)
                                                .strong()
                                                .color(ACCENT_BLUE),
                                        );
                                    });
                                });

                            // 上傳速度卡片
                            egui::Frame::NONE
                                .fill(egui::Color32::from_rgb(234, 250, 240))
                                .corner_radius(egui::CornerRadius::same(10))
                                .inner_margin(egui::Margin::same(12))
                                .stroke(egui::Stroke::new(0.5, egui::Color32::from_rgb(190, 235, 205)))
                                .show(&mut cols[1], |ui| {
                                    ui.vertical_centered(|ui| {
                                        ui.label(egui::RichText::new("⬆ 上傳").size(11.0).color(TEXT_SECONDARY));
                                        ui.label(
                                            egui::RichText::new(&self.current_ul_speed)
                                                .size(18.0)
                                                .strong()
                                                .color(ACCENT_GREEN),
                                        );
                                    });
                                });
                        });

                        // Per-process 流量統計
                        if !self.process_stats_display.is_empty() {
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("📋").size(13.0));
                                ui.label(egui::RichText::new("程序流量明細").size(13.0).strong().color(TEXT_PRIMARY));
                            });
                            ui.add_space(4.0);

                            let name_col_width = 180.0;
                            let speed_col_width = 80.0;

                            ui.horizontal(|ui| {
                                ui.add_sized([name_col_width, 16.0],
                                    egui::Label::new(egui::RichText::new("程序").size(11.0).strong().color(TEXT_SECONDARY)));
                                ui.add_sized([speed_col_width, 16.0],
                                    egui::Label::new(egui::RichText::new("⬇ 下載").size(11.0).strong().color(TEXT_SECONDARY)));
                                ui.add_sized([speed_col_width, 16.0],
                                    egui::Label::new(egui::RichText::new("⬆ 上傳").size(11.0).strong().color(TEXT_SECONDARY)));
                            });

                            for (name, dl, ul) in self.process_stats_display.iter().take(5) {
                                let has_rule = self.process_rules.iter().any(|r| {
                                    r.process_name.to_lowercase() == name.to_lowercase()
                                });

                                let (label, color) = if has_rule {
                                    (format!("🔒 {}", name), ACCENT_ORANGE)
                                } else {
                                    (name.clone(), TEXT_PRIMARY)
                                };

                                ui.horizontal(|ui| {
                                    ui.add_sized([name_col_width, 16.0],
                                        egui::Label::new(egui::RichText::new(label).size(11.0).color(color)).truncate());
                                    ui.add_sized([speed_col_width, 16.0],
                                        egui::Label::new(egui::RichText::new(dl).size(11.0).color(ACCENT_BLUE)));
                                    ui.add_sized([speed_col_width, 16.0],
                                        egui::Label::new(egui::RichText::new(ul).size(11.0).color(ACCENT_GREEN)));
                                });
                            }
                        }
                    });
                }

                // ── 錯誤訊息 ──
                if let Some(ref err) = self.error_message {
                    ui.add_space(4.0);
                    egui::Frame::NONE
                        .fill(egui::Color32::from_rgb(255, 240, 240))
                        .stroke(egui::Stroke::new(0.5, egui::Color32::from_rgb(240, 200, 198)))
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(12))
                        .outer_margin(egui::Margin::symmetric(4, 0))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("⚠  {}", err))
                                    .size(12.0)
                                    .color(ACCENT_RED),
                            );
                        });
                }

                ui.add_space(10.0);
            });
        });
    }
}
