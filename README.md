# Windows Speed Limit (Rust 版)

![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange)
![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-blue)
![License](https://img.shields.io/badge/License-MIT-green)

這是一個將原 Python 版重新以 **Rust** 實作的全域頻寬限制器。透過 `windivert` (WinDivert 驅動) 攔截網路封包，並使用 **Token Bucket** 演算法實作精確的流量整形。

## ✨ Rust 版核心改進

*   **高效能與低延遲**: 採用 Rust 原生實作，上傳與下載流量在**獨立執行緒**處理，解決了 Python 版單一執行緒互相干擾的瓶頸。
*   **執行緒安全**: 嚴謹的記憶體管理與 Mutex 通訊，確保在高流量下穩定運作。
*   **即時速度監控**: 新增 GUI 顯示，可即時查看目前的下行與上行速度（Mbps/Kbps）。
*   **更好的停止機制**: 優化了封包攔截迴圈，點擊停止後能更迅速地釋放資源並恢復網路。
*   **單一執行檔**: (開發中) 未來將支援打包成單一 `.exe` 分發，無需安裝 Python 環境。

## 🛠️ 技術架構

*   **GUI 框架**: 使用 [egui/eframe](https://github.com/emilk/egui) (核心 Rust 實作，極速、輕量)。
*   **驅動介面**: 使用 `pydivert` 的 Rust 綁定 [`windivert`](https://crates.io/crates/windivert)。
*   **權限管理**: 自動檢查並請求管理員 (UAC) 提權。

## 🚀 快速開始

### 前置需求
*   Windows 10 / 11 (需管理員權限)。
*   [Rust 工具鏈](https://rustup.rs/) (建議 1.75 以上)。

### 編譯與執行

1.  複製專案：
    ```bash
    git clone https://github.com/yourusername/speed-limit.git
    cd speed-limit
    ```

2.  直接編譯並執行：
    ```bash
    cargo run --release
    ```
    *(註：首次啟動會自動請求管理員權限)*

## 📂 檔案結構 (Rust)

*   `src/main.rs`: 程式入口與 UAC 提權邏輯。
*   `src/app.rs`: GUI 介面實作。
*   `src/traffic_shaper.rs`: 核心流量整形邏輯與 WinDivert 整合。
*   `src/admin.rs`: 管理員權限檢查工具。

## ⚠️ 注意事項

*   **防毒軟體**: WinDivert 驅動可能會被部分防毒軟體誤判，如無法運行請加入排除名單。
*   **網路恢復**: 若程式異常崩潰，通常只需重新啟動程式並點擊停止，或重啟電腦即可恢復網路正常。

## 📝 License

[MIT License](LICENSE)
