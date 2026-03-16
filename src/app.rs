//! GUI 模組 - 使用 egui/eframe 建立視窗介面

use eframe::egui;

use crate::traffic_shaper::BandwidthLimiter;

/// 主應用程式狀態
pub struct SpeedLimitApp {
    limiter: BandwidthLimiter,
    dl_input: String,
    ul_input: String,
    is_running: bool,
    error_message: Option<String>,
    current_dl_speed: String,
    current_ul_speed: String,
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
        }
    }
}

impl SpeedLimitApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
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

            // 同步檢查 limiter 是否異常停止
            if !self.limiter.is_running() {
                self.is_running = false;
                self.error_message = Some("限速器異常停止".to_string());
            }
        }
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
        style.spacing.item_spacing = egui::vec2(8.0, 12.0);
        ctx.set_style(style);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);

                // 標題
                ui.heading(
                    egui::RichText::new("⚡ Windows 全域頻寬限制器")
                        .size(22.0)
                        .strong(),
                );

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(10.0);

                // 下載限速輸入
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("⬇ 下載限速 (Mbps):")
                            .size(15.0),
                    );
                    let dl_edit = egui::TextEdit::singleline(&mut self.dl_input)
                        .desired_width(80.0)
                        .interactive(!self.is_running);
                    ui.add(dl_edit);
                });

                // 上傳限速輸入
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("⬆ 上傳限速 (Mbps):")
                            .size(15.0),
                    );
                    let ul_edit = egui::TextEdit::singleline(&mut self.ul_input)
                        .desired_width(80.0)
                        .interactive(!self.is_running);
                    ui.add(ul_edit);
                });

                ui.label(
                    egui::RichText::new("(0 = 不限制)")
                        .size(12.0)
                        .weak(),
                );

                ui.add_space(10.0);

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
                .min_size(egui::vec2(200.0, 45.0))
                .corner_radius(8.0);

                if ui.add(button).clicked() {
                    self.toggle_limiting();
                }

                ui.add_space(10.0);

                // 即時速度顯示
                if self.is_running {
                    ui.separator();
                    ui.add_space(5.0);

                    egui::Grid::new("speed_grid")
                        .num_columns(2)
                        .spacing([20.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("⬇ 目前下載:")
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(100, 180, 255)),
                            );
                            ui.label(
                                egui::RichText::new(&self.current_dl_speed)
                                    .size(14.0)
                                    .strong(),
                            );
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("⬆ 目前上傳:")
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(100, 220, 130)),
                            );
                            ui.label(
                                egui::RichText::new(&self.current_ul_speed)
                                    .size(14.0)
                                    .strong(),
                            );
                            ui.end_row();
                        });
                }

                // 錯誤訊息
                if let Some(ref err) = self.error_message {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("❌ {}", err))
                            .size(13.0)
                            .color(egui::Color32::from_rgb(255, 100, 100)),
                    );
                }

                // 狀態列
                ui.add_space(10.0);
                ui.separator();

                let (status_text, status_color) = if self.is_running {
                    (
                        format!(
                            "🟢 運行中 (DL: {} Mbps, UL: {} Mbps)",
                            self.dl_input, self.ul_input
                        ),
                        egui::Color32::from_rgb(80, 200, 100),
                    )
                } else {
                    ("🔴 已停止".to_string(), egui::Color32::from_rgb(200, 80, 80))
                };

                ui.label(
                    egui::RichText::new(status_text)
                        .size(13.0)
                        .color(status_color),
                );
            });
        });
    }
}
