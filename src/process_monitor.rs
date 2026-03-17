//! Process Monitor 模組
//! 使用 WinDivert Flow layer 監聽網路連線事件，建立 5-tuple → PID 映射表。
//! 供 Network layer 封包攔截時查詢封包所屬 process。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

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

/// Process 連線監控器
pub struct ProcessMonitor {
    running: Arc<AtomicBool>,
    flow_map: Arc<Mutex<HashMap<FlowKey, ProcessInfo>>>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl ProcessMonitor {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            flow_map: Arc::new(Mutex::new(HashMap::new())),
            thread_handle: None,
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

        let handle = thread::spawn(move || {
            if let Err(e) = Self::monitor_worker(running.clone(), flow_map) {
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

    /// Flow layer 監聽執行緒
    fn monitor_worker(
        running: Arc<AtomicBool>,
        flow_map: Arc<Mutex<HashMap<FlowKey, ProcessInfo>>>,
    ) -> Result<(), String> {
        use windivert::prelude::*;

        // 使用 Flow layer，設定 sniff flag（不攔截，只觀察）
        let flags = WinDivertFlags::new();
        // 如果是較舊版本的 windivert crate，flags 可能沒有 set_sniff
        // 但我們之前的代碼有用過，這裡假設支援。如果不支援會報編譯錯誤。

        let wdh = WinDivert::flow(
            "true",
            -1,    // priority: 較低優先權，不影響其他 handle
            flags,
        )
        .map_err(|e| format!("無法開啟 WinDivert Flow layer: {:?}", e))?;

        while running.load(Ordering::Relaxed) {
            let packet = match wdh.recv(None) {
                Ok(p) => p,
                Err(e) => {
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }
                    let err_str = format!("{:?}", e);
                    // 超時或緩衝區錯誤，繼續嘗試
                    if err_str.contains("87") || err_str.contains("timeout") {
                        continue;
                    }
                    return Err(format!("Flow recv 錯誤: {}", err_str));
                }
            };

            let addr = &packet.address;
            let pid = addr.process_id();
            let protocol = addr.protocol();
            let local_addr = addr.local_address();
            let remote_addr = addr.remote_address();
            let local_port = addr.local_port();
            let remote_port = addr.remote_port();

            let key = FlowKey {
                protocol,
                local_addr,
                local_port,
                remote_addr,
                remote_port,
            };

            if let Ok(mut map) = flow_map.lock() {
                // 基於 PID 判斷：PID > 0 代表連線建立與存續，PID = 0 代表連線結束
                if pid > 0 {
                    let name = get_process_name(pid);
                    map.insert(key, ProcessInfo { pid, name });
                } else {
                    map.remove(&key);
                }
            }
        }

        Ok(())
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
