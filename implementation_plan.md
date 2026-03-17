# Process 層級上下傳流量控制實作計畫

## 背景

目前的 speed-limit 應用程式使用 WinDivert Network layer 進行**全域**頻寬限制。本計畫旨在實作**程序（Process）級別**的頻寬限制功能。

### 技術實現原理

WinDivert 的 Network layer 本身不提供封包的 PID 資訊。本功能透過以下機制實現：

1.  **Flow Layer 監聽**：新增 `process_monitor` 模組，使用 WinDivert Flow layer 監聽系統所有網路流（Flow）的建立與銷毀事件。此 Layer 會提供對應的 PID。
2.  **PID 映射查表**：維護一個執行緒安全的 `5-tuple (src/dst IP, port, protocol) -> PID` 映射表。
3.  **封包解析與匹配**：在 Network layer 攔截封包時，手動解析封包 Header 取得 5-tuple，並向 `process_monitor` 查詢該封包所屬的 Process 名稱。
4.  **獨立 Token Bucket**：為每個設定了限速規則的程序維護獨立的 Token Bucket，實現精確的流量整形。

## 變更項目

### 1. 核心模組
- **process_monitor.rs** [NEW]: 處理 Flow 事件監聽、PID 到 Process 名稱的轉換，以及映射表的維護。
- **traffic_shaper.rs** [UPDATE]: 整合查表邏輯，支援按程序名稱分配 Token Bucket。

### 2. 使用者介面 (app.rs)
- **規則管理**：新增程序規則輸入區塊，支援動態新增與移除規則。
- **流量明細**：即時顯示目前活躍程序的下載與上傳速度，並以金色標記已受規則限制的程序。
- **介面優化**：調整視窗尺寸並加入捲動區域，以適應多條規則的顯示。

### 3. 編譯設定 (Cargo.toml)
- 加入 `Win32_System_ProcessStatus` 以支援跨程序名稱查詢。

## 使用方式

1. 以**管理員權限**執行程式。
2. 在「程序限速規則」區塊輸入目標程序名稱（例如 `chrome.exe`）。
3. 設定該程序的下載與上傳限速值 (Mbps)。
4. 點擊「新增」後，再點擊「開始限速」即可生效。
5. 全域限速依然有效，未在規則列表中的程序將受全域限速限制。
