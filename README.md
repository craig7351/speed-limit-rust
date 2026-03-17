# Windows Speed Limit

![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange)
![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-blue)
![License](https://img.shields.io/badge/License-MIT-green)

使用 **Rust** 實作的 Windows 全域頻寬限制器。透過 WinDivert 驅動攔截網路封包，並使用 **Token Bucket** 演算法實作精確的流量整形，支援全域限速與 per-process 限速。

## ✨ 功能特色

*   **全域 & Per-Process 限速**: 可設定整體頻寬上限，也可針對個別程式設定獨立速度限制。
*   **高效能與低延遲**: Rust 原生實作，上傳與下載流量在獨立執行緒處理。
*   **即時速度監控**: GUI 即時顯示目前的下行與上行速度。
*   **安全停止機制**: 使用 `WinDivertShutdown` + `WinDivertClose` 確保停止後正確移除核心過濾器，網路立即恢復。
*   **單一執行檔**: 編譯為單一 `.exe`，搭配 WinDivert 檔案即可使用，無需安裝其他執行環境。

## 🛠️ 技術架構

*   **GUI 框架**: [egui/eframe](https://github.com/emilk/egui)
*   **封包攔截**: [WinDivert](https://github.com/basil00/Divert) (透過 `windivert-sys` crate)
*   **權限管理**: 自動檢查並請求管理員 (UAC) 提權

## 🚀 快速開始

### 前置需求
*   **Windows 10 / 11** (64-bit)
*   **管理員權限** (程式啟動時會自動請求 UAC 提權)

### 使用預編譯版本

1.  從 [Releases](https://github.com/peterxcli/speed-limit/releases) 下載最新的 `SpeedLimit-Windows-x64.zip`
2.  解壓縮到任意資料夾
3.  以系統管理員身份執行 `speed-limit.exe`

> ⚠️ `speed-limit.exe`、`WinDivert.dll`、`WinDivert64.sys` 三個檔案**必須放在同一個資料夾**，程式才能正常運作。

### 從原始碼編譯

需要 [Rust 工具鏈](https://rustup.rs/) (建議 1.75 以上)。

```bash
git clone https://github.com/peterxcli/speed-limit.git
cd speed-limit
cargo run --release
```

> 首次編譯會自動透過 `vendored` feature 編譯 WinDivert DLL，編譯後的 DLL/SYS 會在 build output 中，執行時需將其複製到 exe 同目錄。

## 📂 檔案結構

| 檔案 | 說明 |
|---|---|
| `src/main.rs` | 程式入口與 UAC 提權邏輯 |
| `src/app.rs` | GUI 介面實作 (egui) |
| `src/traffic_shaper.rs` | 核心流量整形邏輯 (Token Bucket + WinDivert Network layer) |
| `src/process_monitor.rs` | Process 連線監控 (WinDivert Flow layer) |
| `src/admin.rs` | 管理員權限檢查工具 |

## ⚠️ 注意事項

*   **必要檔案**: `WinDivert.dll` 和 `WinDivert64.sys` 必須與 `speed-limit.exe` 放在同一個資料夾。DLL 在呼叫 `WinDivertOpen()` 時動態載入，驅動程式 (.sys) 則由 DLL 安裝到 Windows 核心。
*   **系統相容性**: 支援 Windows 10 與 Windows 11 (64-bit)。WinDivert 2.2 向下相容至 Windows 7。
*   **管理員權限**: WinDivert 需要管理員權限才能載入核心驅動，程式會自動透過 UAC 請求提權。
*   **防毒軟體**: WinDivert 驅動可能被部分防毒軟體誤判為威脅，如無法運行請加入排除名單。
*   **Secure Boot**: 在啟用 Secure Boot + 強制驅動程式簽章的企業環境中，可能需要額外設定才能載入 `WinDivert64.sys`。一般個人電腦不受影響。
*   **異常恢復**: 若程式意外崩潰導致網路中斷，重新執行程式並點擊「停止」即可恢復網路；或直接重啟電腦。

## 📝 License

[MIT License](LICENSE)
