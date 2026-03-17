//! Process Monitor 模組
//! 使用 WinDivert Flow layer 監聽網路連線事件，建立 5-tuple → PID 映射表。
//! 供 Network layer 封包攔截時查詢封包所屬 process。

use std::collections::HashMap;
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use windivert_sys as sys;
use windivert_sys::address::WINDIVERT_ADDRESS;
use windivert_sys::{WinDivertFlags, WinDivertLayer, WinDivertShutdownMode};

/// 網路流的 5-tuple 識別鍵
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct FlowKey {
    pub protocol: u8,
    pub local_addr: IpAddr,
    pub local_port: u16,
    pub remote_addr: IpAddr,
    pub remote_port: u16,
}

/// Process 資訊
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
}

/// 取得 process 名稱（從 PID）
fn get_process_name(pid: u32) -> String {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT};
    use windows::Win32::Foundation::CloseHandle;
    use windows::core::PWSTR;

    if pid == 0 || pid == 4 {
        return match pid {
            0 => "System Idle".to_string(),
            4 => "System".to_string(),
            _ => "Unknown".to_string(),
        };
    }

    unsafe {
        let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return format!("PID:{}", pid),
        };

        let mut buf = [0u16; 260];
        let mut size = buf.len() as u32;

        let name = if QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), PWSTR(buf.as_mut_ptr()), &mut size).is_ok() {
            let full_path = String::from_utf16_lossy(&buf[..size as usize]);
            // 只取檔案名稱
            full_path
                .rsplit('\\')
                .next()
                .unwrap_or(&full_path)
                .to_string()
        } else {
            format!("PID:{}", pid)
        };

        let _ = CloseHandle(handle);
        name
    }
}

/// Thread-safe wrapper for raw WinDivert handle value.
#[derive(Clone, Copy)]
struct RawHandle(isize);
unsafe impl Send for RawHandle {}
unsafe impl Sync for RawHandle {}

/// Process 連線監控器
pub struct ProcessMonitor {
    running: Arc<AtomicBool>,
    flow_map: Arc<Mutex<HashMap<FlowKey, ProcessInfo>>>,
    thread_handle: Option<thread::JoinHandle<()>>,
    /// Raw WinDivert handle for external shutdown
    wdh_handle: Arc<Mutex<Option<RawHandle>>>,
}

impl ProcessMonitor {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            flow_map: Arc::new(Mutex::new(HashMap::new())),
            thread_handle: None,
            wdh_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// 啟動 Flow layer 監聽
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let flow_map = self.flow_map.clone();
        let wdh_handle = self.wdh_handle.clone();

        let handle = thread::spawn(move || {
            if let Err(e) = Self::monitor_worker(running.clone(), flow_map, wdh_handle) {
                eprintln!("ProcessMonitor 錯誤: {}", e);
                running.store(false, Ordering::SeqCst);
            }
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    /// 停止監聽
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        // Shutdown WinDivert handle 以中斷阻塞的 recv()
        if let Ok(mut h) = self.wdh_handle.lock() {
            if let Some(raw) = h.take() {
                unsafe {
                    sys::WinDivertShutdown(std::mem::transmute(raw.0), WinDivertShutdownMode::Both);
                }
            }
        }

        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }

        // 清空映射表
        if let Ok(mut map) = self.flow_map.lock() {
            map.clear();
        }
    }

    /// 根據封包的 5-tuple 查詢所屬 process
    pub fn lookup(&self, key: &FlowKey) -> Option<ProcessInfo> {
        if let Ok(map) = self.flow_map.lock() {
            map.get(key).cloned()
        } else {
            None
        }
    }

    /// 取得所有目前活躍的 process 名稱列表（去重）
    pub fn get_active_processes(&self) -> Vec<String> {
        if let Ok(map) = self.flow_map.lock() {
            let mut names: Vec<String> = map
                .values()
                .map(|info| info.name.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            names.sort();
            names
        } else {
            Vec::new()
        }
    }

    /// 取得 flow_map 的 Arc 引用（供 traffic_shaper 查詢使用）
    pub fn get_flow_map(&self) -> Arc<Mutex<HashMap<FlowKey, ProcessInfo>>> {
        self.flow_map.clone()
    }

    /// Flow layer 監聽執行緒（使用 raw windivert_sys API 確保 handle 正確關閉）
    fn monitor_worker(
        running: Arc<AtomicBool>,
        flow_map: Arc<Mutex<HashMap<FlowKey, ProcessInfo>>>,
        wdh_handle: Arc<Mutex<Option<RawHandle>>>,
    ) -> Result<(), String> {
        // 使用 raw API 開啟 Flow layer handle
        let filter = CString::new("true").map_err(|e| format!("{}", e))?;
        let flags = WinDivertFlags::new().set_recv_only().set_sniff();
        let handle = unsafe {
            sys::WinDivertOpen(
                filter.as_ptr(),
                WinDivertLayer::Flow,
                -1,    // 較低優先權
                flags,
            )
        };
        if handle.is_invalid() {
            return Err(format!("無法開啟 WinDivert Flow layer: {:?}", std::io::Error::last_os_error()));
        }

        // 存儲 handle 以供外部 shutdown
        let handle_raw: isize = unsafe { std::mem::transmute(handle) };
        if let Ok(mut h) = wdh_handle.lock() {
            *h = Some(RawHandle(handle_raw));
        }

        while running.load(Ordering::Relaxed) {
            let mut addr = MaybeUninit::<WINDIVERT_ADDRESS>::uninit();
            let mut recv_len: u32 = 0;

            let ok = unsafe {
                sys::WinDivertRecv(
                    handle,
                    std::ptr::null_mut(),
                    0,
                    &mut recv_len,
                    addr.as_mut_ptr(),
                )
            };

            if !ok.as_bool() {
                if !running.load(Ordering::Relaxed) {
                    break; // 正常關閉
                }
                let err = std::io::Error::last_os_error();
                let err_code = err.raw_os_error().unwrap_or(0);
                // 超時或緩衝區錯誤，繼續嘗試
                if err_code == 87 {
                    continue;
                }
                // 清理後返回
                Self::close_handle(&wdh_handle, handle_raw);
                return Err(format!("Flow recv 錯誤: {:?}", err));
            }

            let addr = unsafe { addr.assume_init() };
            let flow = unsafe { addr.union_field.Flow };

            let pid = flow.process_id;
            let protocol = flow.protocol;
            let local_port = flow.local_port;
            let remote_port = flow.remote_port;

            // 轉換 IP 地址
            let (local_addr, remote_addr) = if addr.ipv6() {
                let local = IpAddr::V6(std::net::Ipv6Addr::from(
                    flow.local_addr.iter().rev().fold(0u128, |acc, &x| acc << 32 | (x as u128)),
                ));
                let remote = IpAddr::V6(std::net::Ipv6Addr::from(
                    flow.remote_addr.iter().rev().fold(0u128, |acc, &x| acc << 32 | (x as u128)),
                ));
                (local, remote)
            } else {
                let local = IpAddr::V4(std::net::Ipv4Addr::from(flow.local_addr[0]));
                let remote = IpAddr::V4(std::net::Ipv4Addr::from(flow.remote_addr[0]));
                (local, remote)
            };

            let key = FlowKey {
                protocol,
                local_addr,
                local_port,
                remote_addr,
                remote_port,
            };

            if let Ok(mut map) = flow_map.lock() {
                if pid > 0 {
                    let name = get_process_name(pid);
                    map.insert(key, ProcessInfo { pid, name });
                } else {
                    map.remove(&key);
                }
            }
        }

        // 正確關閉 WinDivert handle
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

impl Drop for ProcessMonitor {
    fn drop(&mut self) {
        if self.running.load(Ordering::Relaxed) {
            self.stop();
        }
    }
}

/// 從 IP 封包的 raw bytes 解析出 FlowKey
/// 支援 IPv4 (TCP/UDP) 和 IPv6 (TCP/UDP)
pub fn parse_flow_key_from_packet(data: &[u8], is_outbound: bool) -> Option<FlowKey> {
    if data.len() < 20 {
        return None;
    }

    let version = (data[0] >> 4) & 0x0F;

    match version {
        4 => parse_ipv4_flow_key(data, is_outbound),
        6 => parse_ipv6_flow_key(data, is_outbound),
        _ => None,
    }
}

fn parse_ipv4_flow_key(data: &[u8], is_outbound: bool) -> Option<FlowKey> {
    if data.len() < 20 {
        return None;
    }

    let ihl = (data[0] & 0x0F) as usize * 4;
    let protocol = data[9];
    let src_ip = IpAddr::V4(std::net::Ipv4Addr::new(data[12], data[13], data[14], data[15]));
    let dst_ip = IpAddr::V4(std::net::Ipv4Addr::new(data[16], data[17], data[18], data[19]));

    // TCP (6) 或 UDP (17) 才有 port
    if protocol != 6 && protocol != 17 {
        return None;
    }

    if data.len() < ihl + 4 {
        return None;
    }

    let src_port = u16::from_be_bytes([data[ihl], data[ihl + 1]]);
    let dst_port = u16::from_be_bytes([data[ihl + 2], data[ihl + 3]]);

    // Flow layer 記錄的是 (local, remote)
    // outbound: local=src, remote=dst
    // inbound:  local=dst, remote=src
    let (local_addr, local_port, remote_addr, remote_port) = if is_outbound {
        (src_ip, src_port, dst_ip, dst_port)
    } else {
        (dst_ip, dst_port, src_ip, src_port)
    };

    Some(FlowKey {
        protocol,
        local_addr,
        local_port,
        remote_addr,
        remote_port,
    })
}

fn parse_ipv6_flow_key(data: &[u8], is_outbound: bool) -> Option<FlowKey> {
    if data.len() < 40 {
        return None;
    }

    let protocol = data[6]; // Next Header
    let src_ip = {
        let mut octets = [0u8; 16];
        octets.copy_from_slice(&data[8..24]);
        IpAddr::V6(std::net::Ipv6Addr::from(octets))
    };
    let dst_ip = {
        let mut octets = [0u8; 16];
        octets.copy_from_slice(&data[24..40]);
        IpAddr::V6(std::net::Ipv6Addr::from(octets))
    };

    if protocol != 6 && protocol != 17 {
        return None;
    }

    let header_len = 40;
    if data.len() < header_len + 4 {
        return None;
    }

    let src_port = u16::from_be_bytes([data[header_len], data[header_len + 1]]);
    let dst_port = u16::from_be_bytes([data[header_len + 2], data[header_len + 3]]);

    let (local_addr, local_port, remote_addr, remote_port) = if is_outbound {
        (src_ip, src_port, dst_ip, dst_port)
    } else {
        (dst_ip, dst_port, src_ip, src_port)
    };

    Some(FlowKey {
        protocol,
        local_addr,
        local_port,
        remote_addr,
        remote_port,
    })
}
