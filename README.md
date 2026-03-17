# Windows Speed Limit

![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange)
![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-blue)
![License](https://img.shields.io/badge/License-MIT-green)

使用 **Rust** 實作的 Windows 全域頻寬限制器。透過 WinDivert 核心驅動攔截網路封包，並使用 **Token Bucket** 演算法實作精確的流量整形，支援全域限速與 per-process 限速。附帶 macOS 風格的圖形化介面，開箱即用。

## ✨ 功能特色

- **全域 & Per-Process 限速** — 可設定整體頻寬上限，也可針對個別程式（如 chrome.exe、steam.exe）設定獨立速度限制
- **Token Bucket 演算法** — 精確的流量整形，最大突發量為 1 秒流量，單一封包最多延遲 100ms 避免 TCP 超時
- **即時速度監控** — 每 500ms 更新下行 / 上行速度，並顯示前 5 名流量最高的程式
- **智慧程序偵測** — 結合 WinDivert Flow 層即時連線追蹤 + Windows `EnumProcesses` 系統掃描，完整列出所有可限速程式
- **設定自動儲存** — 全域限速、程式規則自動寫入 JSON 設定檔，下次啟動自動載入，不需重新設定
- **安全停止機制** — 使用 `WinDivertShutdown` + `WinDivertClose` 確保停止後正確移除核心過濾器，網路立即恢復
- **macOS 風格 GUI** — 淺色主題、卡片式佈局、Apple 系統配色、圓角元件
- **單一執行檔** — 搭配 WinDivert 檔案即可使用，無需安裝 Runtime

## 🛠️ 技術架構

| 元件 | 技術 | 說明 |
|------|------|------|
| GUI 框架 | [egui/eframe 0.31](https://github.com/emilk/egui) | 即時模式 GUI，自訂 macOS 淺色主題 |
| 封包攔截 | [WinDivert 2.2](https://github.com/basil00/Divert) | 核心層封包過濾 (Network layer + Flow layer) |
| FFI 綁定 | `windivert-sys 0.10` (vendored) | 從原始碼編譯 WinDivert DLL/SYS |
| Windows API | `windows 0.61` | UAC 提權、程序枚舉、Handle 管理 |
| 設定檔 | `serde` + `serde_json` | JSON 格式，自動儲存/載入 |

### 執行緒模型

```
Main Thread (GUI)          ← egui 事件迴圈、畫面渲染
├── ProcessMonitor Thread  ← WinDivert Flow 層：追蹤連線 ↔ PID 對應
└── TrafficShaper Thread   ← WinDivert Network 層：攔截封包 → Token Bucket → 延遲/放行
```

- **ProcessMonitor** 在程式啟動時建立，持續運行至程式關閉（不隨限速開關重建）
- **TrafficShaper** 在按下「開始」時啟動，「停止」時安全關閉 Handle 並加入執行緒

## 🚀 快速開始

### 前置需求
- **Windows 10 / 11** (64-bit)
- **管理員權限**（程式啟動時會自動請求 UAC 提權）

### 使用預編譯版本

1. 從 [Releases](https://github.com/craig7351/speed-limit-rust/releases) 下載最新的 `SpeedLimit-Windows-x64.zip`
2. 解壓縮到任意資料夾
3. 執行 `speed-limit.exe`（會自動請求管理員權限）

> ⚠️ **`speed-limit.exe`、`WinDivert.dll`、`WinDivert64.sys` 三個檔案必須放在同一個資料夾**，程式才能正常運作。DLL 在呼叫 `WinDivertOpen()` 時動態載入，SYS 由 DLL 安裝到 Windows 核心。

### 從原始碼編譯

需要 [Rust 工具鏈](https://rustup.rs/)（建議 1.75 以上）。

```bash
git clone https://github.com/craig7351/speed-limit-rust.git
cd speed-limit-rust
cargo build --release
```

或使用專案內的 build 腳本：

```bash
build.bat
```

> 首次編譯會自動透過 `vendored` feature 從原始碼編譯 WinDivert DLL/SYS。編譯後需將 `WinDivert.dll` 和 `WinDivert64.sys` 複製到 exe 同目錄。

## 🎮 使用說明

### 全域限速

1. 在「全域限速設定」區域輸入下載 / 上傳限速（單位：Mbps）
2. 輸入 `0` 代表不限制該方向的頻寬
3. 按下「▶ 開始限速」啟動

### Per-Process 限速

1. 在「程序限速規則」區域從下拉選單選擇程式，或選「✍ 自訂輸入...」手動輸入程式名稱
2. 設定該程式的下載 / 上傳限速
3. 按下「＋ 新增」加入規則
4. 符合規則的程式會使用獨立的速度上限，不受全域限制影響
5. 規則匹配為 **大小寫不敏感**（如 `Chrome.exe` = `chrome.exe`）

### 即時監控

限速啟動後，「程序流量明細」區域會顯示：
- 全域下載 / 上傳即時速度
- 流量排名前 5 的程式及其各自速度
- 有設定規則的程式會標示 🔒 圖示

## ⚙️ 設定檔

程式會在 exe 同目錄自動產生 `speed-limit-config.json`：

```json
{
  "download_limit_mbps": 100.0,
  "upload_limit_mbps": 50.0,
  "process_rules": [
    {
      "process_name": "chrome.exe",
      "download_mbps": 30.0,
      "upload_mbps": 10.0
    },
    {
      "process_name": "steam.exe",
      "download_mbps": 20.0,
      "upload_mbps": 0.0
    }
  ]
}
```

| 欄位 | 類型 | 說明 |
|------|------|------|
| `download_limit_mbps` | number | 全域下載限速（Mbps），`0` = 不限制 |
| `upload_limit_mbps` | number | 全域上傳限速（Mbps），`0` = 不限制 |
| `process_rules[].process_name` | string | 程式執行檔名稱（大小寫不敏感） |
| `process_rules[].download_mbps` | number | 該程式下載限速（Mbps），`0` = 不限制 |
| `process_rules[].upload_mbps` | number | 該程式上傳限速（Mbps），`0` = 不限制 |

- **自動儲存**：每次修改限速值或新增/刪除規則時自動寫入
- **自動載入**：程式啟動時自動讀取，找不到或格式錯誤會使用預設值
- **手動編輯**：可直接編輯 JSON 檔案，重啟程式後生效

## 📂 專案結構

```
speed-limit/
├── src/
│   ├── main.rs              # 程式入口、UAC 提權、視窗設定
│   ├── app.rs               # GUI 介面 (macOS 風格主題、所有使用者互動)
│   ├── traffic_shaper.rs    # 核心流量整形 (Token Bucket + WinDivert Network layer)
│   ├── process_monitor.rs   # 程序連線監控 (WinDivert Flow layer + EnumProcesses)
│   ├── config.rs            # JSON 設定檔讀寫
│   └── admin.rs             # 管理員權限檢查與 UAC 提權
├── .github/workflows/
│   └── build.yml            # GitHub Actions CI/CD (自動編譯 + Release)
├── build.bat                # 本地 Release 編譯腳本
├── speed-limit-config.json  # 使用者設定檔 (自動產生)
├── WinDivert.dll            # WinDivert 使用者模式函式庫 (必要)
├── WinDivert64.sys          # WinDivert 核心驅動程式 (必要)
├── Cargo.toml               # Rust 專案設定與相依套件
└── Cargo.lock               # 相依套件鎖定版本
```

## 🔧 CI/CD

GitHub Actions 自動化建置流程（`.github/workflows/build.yml`）：

| 觸發條件 | 動作 |
|----------|------|
| Push 到 `main` / `master` | 編譯並上傳 Artifact（保留 5 天） |
| Pull Request | 編譯驗證 |
| 推送 `v*` 標籤（如 `v1.0.0`） | 編譯 + 建立 GitHub Release + 附加 zip |
| 手動觸發 | workflow_dispatch |

發布新版本：

```bash
git tag v1.0.0
git push origin v1.0.0
```

GitHub Actions 會自動建立 Release 並附上 `SpeedLimit-Windows-x64.zip`（含 exe + DLL + SYS + README）。

## 📦 關於 WinDivert

本專案使用 [**WinDivert**](https://reqrypt.org/windivert.html)（Windows Packet Divert）作為核心封包攔截引擎。

WinDivert 是一個開源的 Windows 封包擷取與修改工具，提供使用者模式（user-mode）API 來攔截、修改、丟棄或注入網路封包。它由一個核心驅動程式（`WinDivert64.sys`）和一個使用者模式函式庫（`WinDivert.dll`）組成。

| 項目 | 說明 |
|------|------|
| 官方網站 | [reqrypt.org/windivert.html](https://reqrypt.org/windivert.html) |
| GitHub | [github.com/basil00/WinDivert](https://github.com/basil00/WinDivert) |
| 使用版本 | 2.2.2 |
| 授權 | LGPL v3 |
| 支援系統 | Windows 7 / 8 / 10 / 11（64-bit） |

### 本專案使用的 WinDivert 功能

- **Network Layer**：攔截所有進出封包，配合 Token Bucket 實現延遲放行達到限速效果
- **Flow Layer**：監聽連線建立/關閉事件，將 5-tuple（協議、來源/目標 IP + Port）對應到 PID，實現 per-process 識別
- **Recv-Only + Sniff 模式**：Flow 層以唯讀方式監聽，不干擾封包傳輸

## ⚠️ 注意事項

- **必要檔案**：`WinDivert.dll` 和 `WinDivert64.sys` 必須與 `speed-limit.exe` 放在同一資料夾
- **系統相容性**：支援 Windows 10 與 Windows 11（64-bit）。WinDivert 2.2 向下相容至 Windows 7
- **管理員權限**：WinDivert 需要管理員權限才能載入核心驅動，程式會自動透過 UAC 請求提權
- **防毒軟體**：WinDivert 驅動可能被部分防毒軟體誤判為威脅，如無法運行請加入排除名單
- **Secure Boot**：在啟用 Secure Boot + 強制驅動程式簽章的企業環境中，可能需要額外設定才能載入 `WinDivert64.sys`。一般個人電腦不受影響
- **異常恢復**：若程式意外崩潰導致網路中斷，重新執行程式並點擊「停止」即可恢復網路；或直接重啟電腦

## 🐛 疑難排解

| 問題 | 解決方式 |
|------|----------|
| 啟動後出現「WinDivert 開啟失敗」 | 確認 DLL/SYS 在同一資料夾，且以管理員身份執行 |
| 停止後網路無法使用 | 重新執行程式按「停止」，或重啟電腦 |
| 防毒軟體阻擋 | 將 exe + DLL + SYS 加入排除名單 |
| 限速後速度不準確 | Token Bucket 有 1 秒突發容量，短時間內可能超過限制 |
| 下拉選單看不到某程式 | 該程式可能尚未建立網路連線，可使用「✍ 自訂輸入」手動輸入 |
| 設定檔無法儲存 | 確認 exe 所在資料夾有寫入權限 |

## 📝 License

[MIT License](LICENSE)
