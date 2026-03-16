//! 核心流量整形器 - 使用 Token Bucket 演算法 + WinDivert 封包攔截

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

/// 流量統計 (bytes per second)
#[derive(Debug, Clone, Default)]
pub struct TrafficStats {
    pub download_bps: f64,
    pub upload_bps: f64,
}

/// Token Bucket 狀態
struct TokenBucket {
    tokens: f64,
    rate: f64,        // bytes per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(rate: f64) -> Self {
        Self {
            tokens: if rate > 0.0 { rate } else { f64::INFINITY },
            rate,
            last_refill: Instant::now(),
        }
    }

    fn set_rate(&mut self, rate: f64) {
        self.rate = rate;
        self.tokens = if rate > 0.0 { rate } else { f64::INFINITY };
        self.last_refill = Instant::now();
    }

    /// 補充 token 並嘗試消耗 packet_size
    /// 回傳需要等待的秒數（0.0 = 不需等待）
    fn consume(&mut self, packet_size: usize) -> f64 {
        if self.rate <= 0.0 {
            return 0.0; // 無限制
        }

        // 補充 token
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;

        self.tokens += elapsed * self.rate;
        // 最大突發量 = 1 秒份量
        if self.tokens > self.rate {
            self.tokens = self.rate;
        }

        let size = packet_size as f64;

        if self.tokens >= size {
            self.tokens -= size;
            0.0
        } else {
            // 計算需要等待的時間
            let shortage = size - self.tokens;
            self.tokens -= size; // 允許負值
            let wait = shortage / self.rate;
            // 最長等待 100ms，避免 TCP timeout
            wait.min(0.1)
        }
    }
}

/// 頻寬限制器
pub struct BandwidthLimiter {
    running: Arc<AtomicBool>,
    stats: Arc<Mutex<TrafficStats>>,
    thread_handle: Option<thread::JoinHandle<()>>,
    download_limit_mbps: f64,
    upload_limit_mbps: f64,
}

impl BandwidthLimiter {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Mutex::new(TrafficStats::default())),
            thread_handle: None,
            download_limit_mbps: 0.0,
            upload_limit_mbps: 0.0,
        }
    }

    /// 設定限速 (Mbps)
    pub fn set_limits(&mut self, download_mbps: f64, upload_mbps: f64) {
        self.download_limit_mbps = download_mbps;
        self.upload_limit_mbps = upload_mbps;
    }

    /// 取得即時流量統計
    pub fn get_stats(&self) -> TrafficStats {
        self.stats.lock().unwrap().clone()
    }

    /// 是否正在運行
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// 啟動限速
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::Relaxed) {
            return Err("限速器已在運行中".to_string());
        }

        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let stats = self.stats.clone();

        // 1 Mbps = 125,000 bytes/s
        let dl_rate = self.download_limit_mbps * 125_000.0;
        let ul_rate = self.upload_limit_mbps * 125_000.0;

        let handle = thread::spawn(move || {
            if let Err(e) = Self::worker(running.clone(), stats, dl_rate, ul_rate) {
                eprintln!("流量整形器錯誤: {}", e);
                running.store(false, Ordering::SeqCst);
            }
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    /// 停止限速
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.thread_handle.take() {
            // 等待工作執行緒結束 (最多 2 秒)
            let _ = handle.join();
        }

        // 清空統計
        if let Ok(mut s) = self.stats.lock() {
            *s = TrafficStats::default();
        }
    }

    /// 工作執行緒 - 攔截並整形封包
    fn worker(
        running: Arc<AtomicBool>,
        stats: Arc<Mutex<TrafficStats>>,
        dl_rate: f64,
        ul_rate: f64,
    ) -> Result<(), String> {
        use windivert::prelude::*;

        // 開啟 WinDivert handle，攔截所有 IP 流量
        let wdh = WinDivert::network(
            "true",
            0,    // priority
            WinDivertFlags::new(),
        )
        .map_err(|e| format!("無法開啟 WinDivert: {:?}", e))?;

        let mut dl_bucket = TokenBucket::new(dl_rate);
        let mut ul_bucket = TokenBucket::new(ul_rate);

        // 統計用的滑動視窗
        let mut dl_bytes_window: u64 = 0;
        let mut ul_bytes_window: u64 = 0;
        let mut window_start = Instant::now();

        let mut buffer = vec![0u8; 65535];

        while running.load(Ordering::Relaxed) {
            // 接收封包
            let packet = match wdh.recv(Some(&mut buffer)) {
                Ok(p) => p,
                Err(e) => {
                    let err_str = format!("{:?}", e);
                    if err_str.contains("87") {
                        // 封包太大，跳過
                        continue;
                    }
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }
                    return Err(format!("recv 錯誤: {}", err_str));
                }
            };

            if !running.load(Ordering::Relaxed) {
                // 停止信號已設定，直接轉發剩餘封包
                let _ = wdh.send(&packet);
                break;
            }

            let packet_len = packet.data.len();
            let is_outbound = packet.address.outbound();

            // Token Bucket 限速
            let wait_time = if is_outbound {
                if ul_rate > 0.0 {
                    ul_bytes_window += packet_len as u64;
                    ul_bucket.consume(packet_len)
                } else {
                    ul_bytes_window += packet_len as u64;
                    0.0
                }
            } else {
                if dl_rate > 0.0 {
                    dl_bytes_window += packet_len as u64;
                    dl_bucket.consume(packet_len)
                } else {
                    dl_bytes_window += packet_len as u64;
                    0.0
                }
            };

            // 若需要延遲，sleep 來降速
            if wait_time > 0.001 {
                thread::sleep(std::time::Duration::from_secs_f64(wait_time));
            }

            // 轉發封包
            let _ = wdh.send(&packet);

            // 更新統計 (每秒更新一次)
            let elapsed = window_start.elapsed().as_secs_f64();
            if elapsed >= 1.0 {
                if let Ok(mut s) = stats.lock() {
                    s.download_bps = dl_bytes_window as f64 / elapsed;
                    s.upload_bps = ul_bytes_window as f64 / elapsed;
                }
                dl_bytes_window = 0;
                ul_bytes_window = 0;
                window_start = Instant::now();
            }
        }

        Ok(())
    }
}

impl Drop for BandwidthLimiter {
    fn drop(&mut self) {
        if self.running.load(Ordering::Relaxed) {
            self.stop();
        }
    }
}
