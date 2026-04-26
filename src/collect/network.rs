use crate::domain::network::{NetInfo, NetStat};
use std::collections::HashMap;

/// Network data collector using Windows IPHLPAPI.
pub struct NetCollector {
    pub interfaces: Vec<String>,
    pub current_net: HashMap<String, NetInfo>,
    pub selected_iface: String,
    last_time: std::time::Instant,
}

impl NetCollector {
    pub fn new() -> Self {
        Self {
            interfaces: Vec::new(),
            current_net: HashMap::new(),
            selected_iface: String::new(),
            last_time: std::time::Instant::now(),
        }
    }

    /// Collect network interface data.
    pub fn collect(&mut self) -> &HashMap<String, NetInfo> {
        use windows::Win32::NetworkManagement::IpHelper::*;
        use windows::Win32::Networking::WinSock::*;

        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_time).as_secs_f64().max(0.001);
        self.last_time = now;

        unsafe {
            let mut size = 0u32;
            let flags = GAA_FLAG_INCLUDE_PREFIX | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_ANYCAST;

            // First call to get required size
            let _ = GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, None, &mut size);

            if size == 0 {
                return &self.current_net;
            }

            let mut buffer = vec![0u8; size as usize];
            let adapter_ptr = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;

            if GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(adapter_ptr), &mut size)
                != 0
            {
                return &self.current_net;
            }

            self.interfaces.clear();
            let mut current = adapter_ptr;

            while !current.is_null() {
                let adapter = &*current;

                // Skip loopback
                if adapter.IfType == 24 {
                    current = adapter.Next;
                    continue;
                }

                // Get friendly name
                let name = if !adapter.FriendlyName.0.is_null() {
                    let mut len = 0;
                    let mut p = adapter.FriendlyName.0;
                    while *p != 0 {
                        len += 1;
                        p = p.add(1);
                    }
                    String::from_utf16_lossy(std::slice::from_raw_parts(
                        adapter.FriendlyName.0,
                        len,
                    ))
                } else {
                    current = adapter.Next;
                    continue;
                };

                let oper_status = adapter.OperStatus.0;
                let connected = oper_status == 1; // IfOperStatusUp = 1

                // Get IP addresses
                let mut ipv4 = String::new();
                let mut ipv6 = String::new();
                let mut unicast = adapter.FirstUnicastAddress;
                while !unicast.is_null() {
                    let addr = &*unicast;
                    let sa = &*addr.Address.lpSockaddr;
                    match sa.sa_family {
                        AF_INET => {
                            let sa4 = &*(addr.Address.lpSockaddr as *const SOCKADDR_IN);
                            let ip = sa4.sin_addr.S_un.S_addr.to_be();
                            ipv4 = format!(
                                "{}.{}.{}.{}",
                                (ip >> 24) & 0xFF,
                                (ip >> 16) & 0xFF,
                                (ip >> 8) & 0xFF,
                                ip & 0xFF
                            );
                        }
                        AF_INET6 => {
                            let sa6 = &*(addr.Address.lpSockaddr as *const SOCKADDR_IN6);
                            let bytes = sa6.sin6_addr.u.Byte;
                            ipv6 = format!(
                                "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
                                u16::from_be_bytes([bytes[0], bytes[1]]),
                                u16::from_be_bytes([bytes[2], bytes[3]]),
                                u16::from_be_bytes([bytes[4], bytes[5]]),
                                u16::from_be_bytes([bytes[6], bytes[7]]),
                                u16::from_be_bytes([bytes[8], bytes[9]]),
                                u16::from_be_bytes([bytes[10], bytes[11]]),
                                u16::from_be_bytes([bytes[12], bytes[13]]),
                                u16::from_be_bytes([bytes[14], bytes[15]]),
                            );
                        }
                        _ => {}
                    }
                    unicast = addr.Next;
                }

                self.interfaces.push(name.clone());

                // Get interface stats
                let if_index = adapter.Anonymous1.Anonymous.IfIndex;
                let (rx_bytes, tx_bytes) = get_if_stats(if_index);

                let entry = self.current_net.entry(name.clone()).or_insert_with(|| {
                    NetInfo::default()
                });

                entry.connected = connected;
                entry.ipv4 = ipv4;
                entry.ipv6 = ipv6;

                // Calculate speeds
                let dl_stat = entry.stat.get("download").cloned().unwrap_or_default();
                let ul_stat = entry.stat.get("upload").cloned().unwrap_or_default();

                let dl_speed = speed_from_delta(rx_bytes, dl_stat.last, elapsed);
                let ul_speed = speed_from_delta(tx_bytes, ul_stat.last, elapsed);

                entry.stat.insert("download".into(), NetStat {
                    speed: dl_speed,
                    top: dl_stat.top.max(dl_speed),
                    total: rx_bytes.saturating_sub(dl_stat.offset) + dl_stat.rollover,
                    last: rx_bytes,
                    offset: dl_stat.offset,
                    rollover: dl_stat.rollover,
                });

                entry.stat.insert("upload".into(), NetStat {
                    speed: ul_speed,
                    top: ul_stat.top.max(ul_speed),
                    total: tx_bytes.saturating_sub(ul_stat.offset) + ul_stat.rollover,
                    last: tx_bytes,
                    offset: ul_stat.offset,
                    rollover: ul_stat.rollover,
                });

                let bw_dl = entry.bandwidth.entry("download".into()).or_default();
                bw_dl.push_back(dl_speed as i64);
                while bw_dl.len() > 300 {
                    bw_dl.pop_front();
                }

                let bw_ul = entry.bandwidth.entry("upload".into()).or_default();
                bw_ul.push_back(ul_speed as i64);
                while bw_ul.len() > 300 {
                    bw_ul.pop_front();
                }

                current = adapter.Next;
            }
        }

        if self.selected_iface.is_empty() && !self.interfaces.is_empty() {
            self.selected_iface = self.interfaces[0].clone();
        }

        &self.current_net
    }
}

fn get_if_stats(if_index: u32) -> (u64, u64) {
    use windows::Win32::NetworkManagement::IpHelper::*;
    use windows::Win32::NetworkManagement::Ndis::*;

    let mut row = MIB_IF_ROW2::default();
    row.InterfaceIndex = if_index;

    unsafe {
        if GetIfEntry2(&mut row) == windows::Win32::Foundation::WIN32_ERROR(0) {
            (row.InOctets, row.OutOctets)
        } else {
            (0, 0)
        }
    }
}

/// Calculate speed from byte counter delta (for unit testing).
pub fn speed_from_delta(current: u64, previous: u64, elapsed_secs: f64) -> u64 {
    if previous == 0 || elapsed_secs <= 0.0 {
        return 0;
    }
    let delta = current.saturating_sub(previous);
    (delta as f64 / elapsed_secs) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_from_octet_delta() {
        assert_eq!(speed_from_delta(2000, 1000, 1.0), 1000);
        assert_eq!(speed_from_delta(3000, 1000, 2.0), 1000);
    }

    #[test]
    fn speed_zero_when_no_previous() {
        assert_eq!(speed_from_delta(1000, 0, 1.0), 0);
    }

    #[test]
    fn speed_zero_when_zero_elapsed() {
        assert_eq!(speed_from_delta(2000, 1000, 0.0), 0);
    }

    #[test]
    fn rollover_handling() {
        // When counter wraps, saturating_sub returns 0
        assert_eq!(speed_from_delta(500, 1000, 1.0), 0);
    }

    #[test]
    #[ignore]
    fn collect_returns_at_least_one_interface() {
        let mut collector = NetCollector::new();
        collector.collect();
        assert!(!collector.interfaces.is_empty());
    }
}
