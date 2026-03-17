//! Windows 全域頻寬限制器 - Rust 版
//! 使用 WinDivert + Token Bucket 進行流量整形

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod admin;
mod app;
mod config;
mod traffic_shaper;
mod process_monitor;

fn main() {
    // 檢查管理員權限
    if !admin::is_admin() {
        eprintln!("需要管理員權限，正在嘗試提權...");
        if admin::relaunch_as_admin() {
            // 提權成功，結束當前進程
            return;
        } else {
            eprintln!("無法取得管理員權限，程式可能無法正常運作。");
            // 仍然嘗試啟動（會在開啟 WinDivert 時失敗）
        }
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([480.0, 520.0])
            .with_min_inner_size([420.0, 480.0])
            .with_title("Speed Limit - Process & Global Throttling"),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "Speed Limit",
        options,
        Box::new(|cc| Ok(Box::new(app::SpeedLimitApp::new(cc)))),
    ) {
        eprintln!("GUI 啟動失敗: {}", e);
    }
}
