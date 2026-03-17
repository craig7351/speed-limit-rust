//! 核心流量整形器 - 使用 Token Bucket 演算法 + WinDivert 封包攔截
//! 支援全域限速和 per-process 限速

use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use windivert_sys as sys;
use windivert_sys::address::WINDIVERT_ADDRESS;
use windivert_sys::{WinDivertFlags, WinDivertLayer, WinDivertShutdownMode};

use crate::process_monitor::{parse_flow_key_from_packet, ProcessMonitor};

/// 流量統計 (bytes per second)
#[derive(Debug, Clone, Default)]
pub struct TrafficStats {
    pub download_bps: f64,
    pub upload_bps: f64,
    /// per-process 流量統計: process_name → (dl_bps, ul_bps)
    pub process_stats: Vec<(String, f64, f64)>,
}

/// Process 限速規則
#[derive(Debug, Clone)]
pub struct ProcessRule {
    pub process_name: String,
    pub download_mbps: f64,
    pub upload_mbps: f64,
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

/// Per-process 的 Token Bucket 組
struct ProcessBuckets {
    dl_bucket: TokenBucket,
    ul_bucket: TokenBucket,
}

/// 統計用滑動視窗計數器
struct ProcessCounter {
    dl_bytes: u64,
    ul_bytes: u64,
}

/// Thread-safe wrapper for raw WinDivert handle value.
/// We store as isize to avoid windows crate version conflicts
/// (windivert-sys uses windows 0.48 HANDLE(isize), our code uses windows 0.61).
#[derive(Clone, Copy)]
struct RawHandle(isize);
unsafe impl Send for RawHandle {}
unsafe impl Sync for RawHandle {}

/// 頻寬限制器
pub struct BandwidthLimiter {
    running: Arc<AtomicBool>,
    stats: Arc<Mutex<TrafficStats>>,
    thread_handle: Option<thread::JoinHandle<()>>,
    download_limit_mbps: f64,
    upload_limit_mbps: f64,
    process_rules: Arc<Mutex<Vec<ProcessRule>>>,
    process_monitor: ProcessMonitor,
    /// Raw WinDivert handle for external shutdown (interrupt blocking recv)
    wdh_handle: Arc<Mutex<Option<RawHandle>>>,
}

impl BandwidthLimiter {
    pub fn new() -> Self {
        let mut process_monitor = ProcessMonitor::new();
        // 初始化時即啟動背景程序監控，以供 UI 下拉選單使用
        if let Err(e) = process_monitor.start() {
            eprintln!("警告：ProcessMonitor 初始化啟動失敗: {}", e);
        }

        Self {
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Mutex::new(TrafficStats::default())),
            thread_handle: None,
            download_limit_mbps: 0.0,
            upload_limit_mbps: 0.0,
            process_rules: Arc::new(Mutex::new(Vec::new())),
            process_monitor,
            wdh_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// 設定全域限速 (Mbps)
    pub fn set_limits(&mut self, download_mbps: f64, upload_mbps: f64) {
        self.download_limit_mbps = download_mbps;
        self.upload_limit_mbps = upload_mbps;
    }

    /// 設定 per-process 限速規則
    pub fn set_process_rules(&self, rules: Vec<ProcessRule>) {
        if let Ok(mut r) = self.process_rules.lock() {
            *r = rules;
        }
    }

    /// 取得即時流量統計
    pub fn get_stats(&self) -> TrafficStats {
        self.stats.lock().unwrap().clone()
    }

    /// 是否正在運行
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// 取得活躍 process 列表
    pub fn get_active_processes(&self) -> Vec<String> {
        self.process_monitor.get_active_processes()
    }

    /// 啟動限速
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::Relaxed) {
            return Err("限速器已在運行中".to_string());
        }

        // ProcessMonitor 已經在 new() 中啟動，這裡確保它還活著
        if let Err(e) = self.process_monitor.start() {
            eprintln!("警告：ProcessMonitor 嘗試重新啟動失敗: {}", e);
        }

        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let stats = self.stats.clone();
        let process_rules = self.process_rules.clone();

        // 1 Mbps = 125,000 bytes/s
        let dl_rate = self.download_limit_mbps * 125_000.0;
        let ul_rate = self.upload_limit_mbps * 125_000.0;

        // 複製 process_monitor 的 flow_map 引用
        let monitor_flow_map = self.process_monitor.get_flow_map();
        let wdh_handle = self.wdh_handle.clone();

        let handle = thread::spawn(move || {
            if let Err(e) = Self::worker(running.clone(), stats, dl_rate, ul_rate, process_rules, monitor_flow_map, wdh_handle) {
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

        // 先 shutdown WinDivert handle 以中斷阻塞的 recv()
        if let Ok(mut h) = self.wdh_handle.lock() {
            if let Some(raw) = h.take() {
                unsafe {
                    sys::WinDivertShutdown(std::mem::transmute(raw.0), WinDivertShutdownMode::Both);
                }
            }
        }

        if let Some(handle) = self.thread_handle.take() {
            // 等待工作執行緒結束 (最多 2 秒)
            let _ = handle.join();
        }

        // ProcessMonitor 保持運行，持續提供 UI 下拉選單的程序列表

        // 清空統計
        if let Ok(mut s) = self.stats.lock() {
            *s = TrafficStats::default();
        }
    }

    /// 工作執行緒 - 攔截並整形封包（使用 raw windivert_sys API 確保 handle 正確關閉）
    fn worker(
        running: Arc<AtomicBool>,
        stats: Arc<Mutex<TrafficStats>>,
        dl_rate: f64,
        ul_rate: f64,
        process_rules: Arc<Mutex<Vec<ProcessRule>>>,
        flow_map: Arc<Mutex<HashMap<crate::process_monitor::FlowKey, crate::process_monitor::ProcessInfo>>>,
        wdh_handle: Arc<Mutex<Option<RawHandle>>>,
    ) -> Result<(), String> {
        // 使用 raw API 開啟 WinDivert handle，確保可以從外部 shutdown + 正確 close
        let filter = CString::new("true").map_err(|e| format!("{}", e))?;
        let handle = unsafe {
            sys::WinDivertOpen(
                filter.as_ptr(),
                WinDivertLayer::Network,
                0,
                WinDivertFlags::new(),
            )
        };
        if handle.is_invalid() {
            return Err(format!("無法開啟 WinDivert: {:?}", std::io::Error::last_os_error()));
        }

        // 存儲 handle 以供外部 shutdown (轉為 isize 以避免 windows crate 版本衝突)
        let handle_raw: isize = unsafe { std::mem::transmute(handle) };
        if let Ok(mut h) = wdh_handle.lock() {
            *h = Some(RawHandle(handle_raw));
        }

        let mut dl_bucket = TokenBucket::new(dl_rate);
        let mut ul_bucket = TokenBucket::new(ul_rate);

        // Per-process bucket 快取
        let mut process_buckets: HashMap<String, ProcessBuckets> = HashMap::new();

        // 統計用的滑動視窗
        let mut dl_bytes_window: u64 = 0;
        let mut ul_bytes_window: u64 = 0;
        let mut process_counters: HashMap<String, ProcessCounter> = HashMap::new();
        let mut window_start = Instant::now();

        let mut buffer = vec![0u8; 65535];

        while running.load(Ordering::Relaxed) {
            // 接收封包
            let mut recv_len: u32 = 0;
            let mut addr = MaybeUninit::<WINDIVERT_ADDRESS>::uninit();

            let ok = unsafe {
                sys::WinDivertRecv(
                    handle,
                    buffer.as_mut_ptr() as *mut c_void,
                    buffer.len() as u32,
                    &mut recv_len,
                    addr.as_mut_ptr(),
                )
            };

            if !ok.as_bool() {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(87) {
                    continue; // 封包太大，跳過
                }
                if !running.load(Ordering::Relaxed) {
                    break; // 正常關閉 (WinDivertShutdown 導致 recv 失敗)
                }
                // 異常錯誤，清理後返回
                Self::close_handle(&wdh_handle, handle_raw);
                return Err(format!("recv 錯誤: {:?}", err));
            }

            let addr = unsafe { addr.assume_init() };

            if !running.load(Ordering::Relaxed) {
                // 停止信號已設定，直接轉發剩餘封包後退出
                unsafe {
                    sys::WinDivertSend(
                        handle,
                        buffer.as_ptr() as *const c_void,
                        recv_len,
                        std::ptr::null_mut(),
                        &addr,
                    );
                }
                break;
            }

            let packet_len = recv_len as usize;
            let packet_data = &buffer[..packet_len];
            let is_outbound = addr.outbound();

            // 嘗試解析 5-tuple 並查找 process
            let process_name = parse_flow_key_from_packet(packet_data, is_outbound)
                .and_then(|flow_key| {
                    if let Ok(map) = flow_map.lock() {
                        map.get(&flow_key).map(|info| info.name.clone())
                    } else {
                        None
                    }
                });

            // 查找匹配的 process rule
            let matched_rule = if let Some(ref pname) = process_name {
                if let Ok(rules) = process_rules.lock() {
                    rules.iter().find(|r| {
                        pname.to_lowercase() == r.process_name.to_lowercase()
                    }).cloned()
                } else {
                    None
                }
            } else {
                None
            };

            // Token Bucket 限速
            let wait_time = if let Some(rule) = &matched_rule {
                // Per-process 限速
                let pname = rule.process_name.to_lowercase();
                let buckets = process_buckets.entry(pname.clone()).or_insert_with(|| {
                    ProcessBuckets {
                        dl_bucket: TokenBucket::new(rule.download_mbps * 125_000.0),
                        ul_bucket: TokenBucket::new(rule.upload_mbps * 125_000.0),
                    }
                });

                if is_outbound {
                    if rule.upload_mbps > 0.0 {
                        buckets.ul_bucket.consume(packet_len)
                    } else {
                        0.0
                    }
                } else if rule.download_mbps > 0.0 {
                    buckets.dl_bucket.consume(packet_len)
                } else {
                    0.0
                }
            } else {
                // 全域限速（不在規則中的 process）
                if is_outbound {
                    if ul_rate > 0.0 {
                        ul_bucket.consume(packet_len)
                    } else {
                        0.0
                    }
                } else if dl_rate > 0.0 {
                    dl_bucket.consume(packet_len)
                } else {
                    0.0
                }
            };

            // 記錄 per-process 統計
            if let Some(ref pname) = process_name {
                let counter = process_counters.entry(pname.clone()).or_insert_with(|| {
                    ProcessCounter { dl_bytes: 0, ul_bytes: 0 }
                });
                if is_outbound {
                    counter.ul_bytes += packet_len as u64;
                } else {
                    counter.dl_bytes += packet_len as u64;
                }
            }

            // 全域統計
            if is_outbound {
                ul_bytes_window += packet_len as u64;
            } else {
                dl_bytes_window += packet_len as u64;
            }

            // 若需要延遲，sleep 來降速
            if wait_time > 0.001 {
                thread::sleep(std::time::Duration::from_secs_f64(wait_time));
            }

            // 轉發封包
            unsafe {
                sys::WinDivertSend(
                    handle,
                    buffer.as_ptr() as *const c_void,
                    recv_len,
                    std::ptr::null_mut(),
                    &addr,
                );
            }

            // 更新統計 (每秒更新一次)
            let elapsed = window_start.elapsed().as_secs_f64();
            if elapsed >= 1.0 {
                if let Ok(mut s) = stats.lock() {
                    s.download_bps = dl_bytes_window as f64 / elapsed;
                    s.upload_bps = ul_bytes_window as f64 / elapsed;

                    // Per-process 統計
                    s.process_stats = process_counters
                        .iter()
                        .map(|(name, counter)| {
                            (
                                name.clone(),
                                counter.dl_bytes as f64 / elapsed,
                                counter.ul_bytes as f64 / elapsed,
                            )
                        })
                        .filter(|(_, dl, ul)| *dl > 0.0 || *ul > 0.0)
                        .collect();

                    // 按下載量排序
                    s.process_stats.sort_by(|a, b| {
                        (b.1 + b.2).partial_cmp(&(a.1 + a.2)).unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
                dl_bytes_window = 0;
                ul_bytes_window = 0;
                process_counters.clear();
                window_start = Instant::now();
            }
        }

        // 正確關閉 WinDivert handle，移除 kernel filter
        Self::close_handle(&wdh_handle, handle_raw);
        Ok(())
    }

    /// 安全關閉 WinDivert handle
    fn close_handle(wdh_handle: &Arc<Mutex<Option<RawHandle>>>, handle_raw: isize) {
        if let Ok(mut h) = wdh_handle.lock() {
            *h = None;
        }
        unsafe {
            sys::WinDivertShutdown(std::mem::transmute(handle_raw), WinDivertShutdownMode::Both);
            sys::WinDivertClose(std::mem::transmute(handle_raw));
        }
    }
}

impl Drop for BandwidthLimiter {
    fn drop(&mut self) {
        if self.running.load(Ordering::Relaxed) {
            self.stop();
        }
        // 程式結束時才停止 ProcessMonitor
        self.process_monitor.stop();
    }
}
