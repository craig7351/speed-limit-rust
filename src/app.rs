//! GUI 模組 - 使用 egui/eframe 建立視窗介面

use eframe::egui;

use crate::traffic_shaper::{BandwidthLimiter, ProcessRule};

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
        Self::default()
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
    }
}

/// 設定中文字型以解決方塊亂碼問題
fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 嘗試在 Windows 系統路徑尋找「微軟正黑體」
    let font_path = "C:\\Windows\\Fonts\\msjh.ttc";
    
    if let Ok(font_data) = std::fs::read(font_path) {
        fonts.font_data.insert(
            "msjh".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(font_data)),
        );

        // 將其加入到 Proportional 和 Monospace 的優先名單首位
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap()
            .insert(0, "msjh".to_owned());
        fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap()
            .insert(0, "msjh".to_owned());
        
        ctx.set_fonts(fonts);
    }
}

impl eframe::App for SpeedLimitApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 每 500ms 更新統計
        self.update_stats();
        if self.is_running {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        // 視覺主題
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        ctx.set_style(style);

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);

                    // 標題
                    ui.heading(
                        egui::RichText::new("⚡ Windows 全域頻寬限制器")
                            .size(20.0)
                            .strong(),
                    );

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // ===== 全域限速設定 =====
                    ui.label(
                        egui::RichText::new("📊 全域限速設定")
                            .size(15.0)
                            .strong(),
                    );

                    ui.add_space(4.0);

                    // 下載限速輸入
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("⬇ 下載 (Mbps):")
                                .size(14.0),
                        );
                        let dl_edit = egui::TextEdit::singleline(&mut self.dl_input)
                            .desired_width(70.0)
                            .interactive(!self.is_running);
                        ui.add(dl_edit);
                    });

                    // 上傳限速輸入
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("⬆ 上傳 (Mbps):")
                                .size(14.0),
                        );
                        let ul_edit = egui::TextEdit::singleline(&mut self.ul_input)
                            .desired_width(70.0)
                            .interactive(!self.is_running);
                        ui.add(ul_edit);
                    });

                    ui.label(
                        egui::RichText::new("(0 = 不限制)")
                            .size(11.0)
                            .weak(),
                    );

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // ===== 程序限速規則 =====
                    ui.label(
                        egui::RichText::new("🎯 程序限速規則")
                            .size(15.0)
                            .strong(),
                    );

                    ui.add_space(4.0);

                    if !self.is_running {
                        // 新增規則的輸入區域
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("程序:").size(13.0));
                            let pe = egui::TextEdit::singleline(&mut self.rule_process_input)
                                .desired_width(110.0)
                                .hint_text("chrome.exe");
                            ui.add(pe);
                        });

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("⬇").size(13.0));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.rule_dl_input)
                                    .desired_width(50.0),
                            );
                            ui.label(egui::RichText::new("⬆").size(13.0));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.rule_ul_input)
                                    .desired_width(50.0),
                            );
                            ui.label(egui::RichText::new("Mbps").size(12.0).weak());

                            if ui.button(
                                egui::RichText::new("➕ 新增").size(13.0),
                            ).clicked() {
                                self.add_process_rule();
                            }
                        });
                    }

                    // 已新增的規則列表
                    if !self.process_rules.is_empty() {
                        ui.add_space(4.0);

                        let mut to_remove: Option<usize> = None;

                        egui::Grid::new("rules_grid")
                            .num_columns(4)
                            .spacing([8.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                // 表頭
                                ui.label(egui::RichText::new("程序").size(12.0).strong());
                                ui.label(egui::RichText::new("⬇ DL").size(12.0).strong());
                                ui.label(egui::RichText::new("⬆ UL").size(12.0).strong());
                                ui.label(egui::RichText::new("").size(12.0));
                                ui.end_row();

                                for (i, rule) in self.process_rules.iter().enumerate() {
                                    ui.label(
                                        egui::RichText::new(&rule.process_name)
                                            .size(12.0)
                                            .color(egui::Color32::from_rgb(180, 220, 255)),
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
                                    ui.label(egui::RichText::new(dl_txt).size(12.0));
                                    ui.label(egui::RichText::new(ul_txt).size(12.0));
                                    if !self.is_running {
                                        if ui.button(
                                            egui::RichText::new("✖").size(12.0).color(egui::Color32::from_rgb(255, 100, 100)),
                                        ).clicked() {
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
                        }
                    } else {
                        ui.label(
                            egui::RichText::new("（未設定程序規則，僅套用全域限速）")
                                .size(11.0)
                                .weak(),
                        );
                    }

                    ui.add_space(8.0);

                    // START / STOP 按鈕
                    let (btn_text, btn_color) = if self.is_running {
                        ("⏹ 停止限速", egui::Color32::from_rgb(220, 50, 50))
                    } else {
                        ("▶ 開始限速", egui::Color32::from_rgb(50, 160, 80))
                    };

                    let button = egui::Button::new(
                        egui::RichText::new(btn_text).size(18.0).strong().color(egui::Color32::WHITE),
                    )
                    .fill(btn_color)
                    .min_size(egui::vec2(200.0, 42.0))
                    .corner_radius(8.0);

                    if ui.add(button).clicked() {
                        self.toggle_limiting();
                    }

                    ui.add_space(8.0);

                    // 即時速度顯示
                    if self.is_running {
                        ui.separator();
                        ui.add_space(4.0);

                        ui.label(
                            egui::RichText::new("📈 即時流量")
                                .size(14.0)
                                .strong(),
                        );

                        egui::Grid::new("speed_grid")
                            .num_columns(2)
                            .spacing([20.0, 4.0])
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("⬇ 全域下載:")
                                        .size(13.0)
                                        .color(egui::Color32::from_rgb(100, 180, 255)),
                                );
                                ui.label(
                                    egui::RichText::new(&self.current_dl_speed)
                                        .size(13.0)
                                        .strong(),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new("⬆ 全域上傳:")
                                        .size(13.0)
                                        .color(egui::Color32::from_rgb(100, 220, 130)),
                                );
                                ui.label(
                                    egui::RichText::new(&self.current_ul_speed)
                                        .size(13.0)
                                        .strong(),
                                );
                                ui.end_row();
                            });

                        // Per-process 流量統計
                        if !self.process_stats_display.is_empty() {
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("📋 程序流量明細")
                                    .size(13.0)
                                    .strong(),
                            );

                            egui::Grid::new("process_stats_grid")
                                .num_columns(3)
                                .spacing([10.0, 3.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new("程序").size(11.0).strong());
                                    ui.label(egui::RichText::new("⬇ DL").size(11.0).strong());
                                    ui.label(egui::RichText::new("⬆ UL").size(11.0).strong());
                                    ui.end_row();

                                    // 最多顯示前 10 個
                                    for (name, dl, ul) in self.process_stats_display.iter().take(10) {
                                        // 標記有規則的 process
                                        let has_rule = self.process_rules.iter().any(|r| {
                                            r.process_name.to_lowercase() == name.to_lowercase()
                                        });
                                        let name_color = if has_rule {
                                            egui::Color32::from_rgb(255, 200, 80) // 金色 = 有規則
                                        } else {
                                            egui::Color32::from_rgb(180, 180, 180) // 灰色 = 無規則
                                        };

                                        let display_name = if name.len() > 18 {
                                            format!("{}…", &name[..17])
                                        } else {
                                            name.clone()
                                        };

                                        ui.label(
                                            egui::RichText::new(display_name)
                                                .size(11.0)
                                                .color(name_color),
                                        );
                                        ui.label(egui::RichText::new(dl).size(11.0));
                                        ui.label(egui::RichText::new(ul).size(11.0));
                                        ui.end_row();
                                    }
                                });
                        }
                    }

                    // 錯誤訊息
                    if let Some(ref err) = self.error_message {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(format!("❌ {}", err))
                                .size(12.0)
                                .color(egui::Color32::from_rgb(255, 100, 100)),
                        );
                    }

                    // 狀態列
                    ui.add_space(6.0);
                    ui.separator();

                    let (status_text, status_color) = if self.is_running {
                        let rules_count = self.process_rules.len();
                        let status = if rules_count > 0 {
                            format!(
                                "🟢 運行中 (全域 DL:{} UL:{} Mbps, {} 條程序規則)",
                                self.dl_input, self.ul_input, rules_count
                            )
                        } else {
                            format!(
                                "🟢 運行中 (DL: {} Mbps, UL: {} Mbps)",
                                self.dl_input, self.ul_input
                            )
                        };
                        (status, egui::Color32::from_rgb(80, 200, 100))
                    } else {
                        ("🔴 已停止".to_string(), egui::Color32::from_rgb(200, 80, 80))
                    };

                    ui.label(
                        egui::RichText::new(status_text)
                            .size(12.0)
                            .color(status_color),
                    );

                    ui.add_space(4.0);
                });
            });
        });
    }
}
