use crate::domain::network::{NetInfo, NetStat};
use std::{mem::size_of, slice};

use super::{
    Collector,
    counters::{accumulate_total, bytes_per_sec},
};

const MAX_ADAPTER_ADDRESSES_BUFFER: u32 = 1024 * 1024;
const MAX_ADAPTER_TRAVERSAL: usize = 1024;
const MAX_UNICAST_TRAVERSAL: usize = 256;
/// Generous upper bound for an adapter description (driver name
/// from the INF). Real-world descriptions are < 100 chars.
const MAX_DESCRIPTION_CHARS: usize = 512;
/// AdapterName is a GUID-shaped string of the form
/// `"{12345678-1234-1234-1234-123456789012}"` — exactly 38 chars
/// plus a NUL terminator. The bound is set generously above that
/// to defend against malformed input without arbitrarily clamping
/// any well-formed value.
const MAX_ADAPTER_NAME_CHARS: usize = 64;

enum FormattedIp {
    V4(String),
    V6(String),
}

/// Network data collector using Windows IPHLPAPI.
pub struct NetCollector {
    pub nets: Vec<NetInfo>,
    pub status: super::CollectStatus,
    last_time: std::time::Instant,
}

impl NetCollector {
    /// Create a new network collector.
    pub fn new() -> Self {
        Self {
            nets: Vec::new(),
            status: super::CollectStatus::Ok,
            last_time: std::time::Instant::now(),
        }
    }

    fn collect_impl(&mut self) {
        self.status = super::CollectStatus::Ok;

        use windows::Win32::NetworkManagement::IpHelper::*;

        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_time).as_secs_f64().max(0.001);
        self.last_time = now;

        let flags = GAA_FLAG_INCLUDE_PREFIX | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_ANYCAST;
        let Some(mut buffer) = adapter_addresses_buffer(flags) else {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Network,
                "GetAdaptersAddresses failed",
            );
            self.status
                .downgrade(super::CollectStatus::Failed("adapter query failed"));
            return;
        };

        // SAFETY: adapter_ptr points into the owned buffer returned by
        // GetAdaptersAddresses. Linked-list traversal is capped and null-checked
        // before dereferencing nested adapter and unicast pointers.
        unsafe {
            let adapter_ptr = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
            let previous_nets = std::mem::take(&mut self.nets);
            let mut current = adapter_ptr;
            let mut adapter_count = 0usize;

            while !current.is_null() {
                if adapter_count >= MAX_ADAPTER_TRAVERSAL {
                    self.status
                        .downgrade(super::CollectStatus::Degraded("adapter list truncated"));
                    break;
                }
                adapter_count += 1;
                let adapter = &*current;

                // Skip loopback
                if adapter.IfType == 24 {
                    current = adapter.Next;
                    continue;
                }

                // Read the permanent adapter GUID (cycling and
                // persistence identifier; see
                // `domain::network::NetInfo::stable_id`) and the
                // driver description (display name).
                let Some(stable_id) =
                    ansi_ptr_to_string_bounded(adapter.AdapterName.0, MAX_ADAPTER_NAME_CHARS)
                else {
                    current = adapter.Next;
                    continue;
                };
                let description =
                    wide_ptr_to_string_bounded(adapter.Description.0, MAX_DESCRIPTION_CHARS)
                        .unwrap_or_default();

                let oper_status = adapter.OperStatus.0;
                let connected = oper_status == 1; // IfOperStatusUp = 1

                // Get IP addresses
                let mut ipv4 = String::new();
                let mut ipv6 = String::new();
                let mut unicast = adapter.FirstUnicastAddress;
                let mut unicast_count = 0usize;
                while !unicast.is_null() {
                    if unicast_count >= MAX_UNICAST_TRAVERSAL {
                        break;
                    }
                    unicast_count += 1;
                    let addr = &*unicast;
                    match socket_address_to_ip(&addr.Address) {
                        Some(FormattedIp::V4(ip)) => {
                            ipv4 = ip;
                        }
                        Some(FormattedIp::V6(ip)) => {
                            ipv6 = ip;
                        }
                        None => {}
                    }
                    unicast = addr.Next;
                }

                // Get interface stats
                let if_index = adapter.Anonymous1.Anonymous.IfIndex;
                let (rx_bytes, tx_bytes, link_speed) = get_if_stats(if_index);

                let mut entry = previous_nets
                    .iter()
                    .find(|n| n.stable_id == stable_id)
                    .cloned()
                    .unwrap_or_default();
                entry.stable_id = stable_id;
                entry.description = description;

                entry.connected = connected;
                entry.ipv4 = ipv4;
                entry.ipv6 = ipv6;
                entry.link_speed = link_speed;

                // Calculate speeds
                let dl_stat = entry.stat.download;
                let ul_stat = entry.stat.upload;

                let dl_speed = bytes_per_sec(rx_bytes, dl_stat.last, elapsed);
                let ul_speed = bytes_per_sec(tx_bytes, ul_stat.last, elapsed);

                entry.stat.download = NetStat {
                    speed: dl_speed,
                    top: dl_stat.top.max(dl_speed),
                    last: rx_bytes,
                    total: accumulate_total(dl_stat.total, rx_bytes, dl_stat.last),
                };

                entry.stat.upload = NetStat {
                    speed: ul_speed,
                    top: ul_stat.top.max(ul_speed),
                    last: tx_bytes,
                    total: accumulate_total(ul_stat.total, tx_bytes, ul_stat.last),
                };

                let bw_dl = &mut entry.bandwidth.download;
                bw_dl.push_back(dl_speed as i64);
                while bw_dl.len() > 300 {
                    bw_dl.pop_front();
                }

                let bw_ul = &mut entry.bandwidth.upload;
                bw_ul.push_back(ul_speed as i64);
                while bw_ul.len() > 300 {
                    bw_ul.pop_front();
                }

                self.nets.push(entry);
                current = adapter.Next;
            }
        }
    }
}

impl Collector for NetCollector {
    type Snapshot = crate::runner::NetSnapshot;

    fn collect(&mut self) {
        self.collect_impl();
    }

    fn snapshot(&self) -> Self::Snapshot {
        crate::runner::NetSnapshot {
            nets: self.nets.clone(),
            status: self.status.clone(),
        }
    }
}

fn adapter_addresses_buffer(
    flags: windows::Win32::NetworkManagement::IpHelper::GET_ADAPTERS_ADDRESSES_FLAGS,
) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
    use windows::Win32::NetworkManagement::IpHelper::GetAdaptersAddresses;
    use windows::Win32::Networking::WinSock::AF_UNSPEC;

    let mut size = 0u32;
    // SAFETY: A null adapter buffer is the documented sizing call. size is a
    // valid output pointer and the status is checked before allocation.
    let first = unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, None, &mut size) };
    if first != ERROR_BUFFER_OVERFLOW.0 || size == 0 || size > MAX_ADAPTER_ADDRESSES_BUFFER {
        return None;
    }

    for _ in 0..2 {
        let mut buffer = vec![0u8; size as usize];
        // SAFETY: buffer is allocated to the API-reported size and cast to the
        // adapter-address record type expected by GetAdaptersAddresses.
        let status = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                flags,
                None,
                Some(buffer.as_mut_ptr() as *mut _),
                &mut size,
            )
        };
        if status == ERROR_SUCCESS.0 {
            return Some(buffer);
        }
        if status != ERROR_BUFFER_OVERFLOW.0
            || size == 0
            || size > MAX_ADAPTER_ADDRESSES_BUFFER
            || (size as usize) <= buffer.len()
        {
            return None;
        }
    }

    None
}

unsafe fn wide_ptr_to_string_bounded(ptr: *const u16, max_units: usize) -> Option<String> {
    if ptr.is_null() || max_units == 0 {
        return None;
    }

    let mut len = 0usize;
    while len < max_units {
        // SAFETY: the caller guarantees ptr points to a foreign UTF-16 string.
        // The scan is bounded by max_units and stops before reading past it.
        if unsafe { *ptr.add(len) } == 0 {
            // SAFETY: len was discovered by a bounded scan from ptr, so this
            // slice covers only initialized UTF-16 units before the terminator.
            let slice = unsafe { slice::from_raw_parts(ptr, len) };
            return Some(String::from_utf16_lossy(slice));
        }
        len += 1;
    }

    None
}

/// Read a NUL-terminated single-byte (ANSI) string pointed to by
/// `ptr`, scanning at most `max_units` bytes. Returns `None` if
/// `ptr` is null, `max_units` is zero, or no NUL is found within
/// the bound.
///
/// Used for `IP_ADAPTER_ADDRESSES_LH.AdapterName`, which Windows
/// documents as a `PSTR` (single-byte) GUID-shaped string. The
/// adapter GUID is pure ASCII (`{`, hex digits, `-`, `}`), so
/// byte-to-char conversion via `String::from_utf8_lossy` is exact
/// for any well-formed value.
unsafe fn ansi_ptr_to_string_bounded(ptr: *const u8, max_units: usize) -> Option<String> {
    if ptr.is_null() || max_units == 0 {
        return None;
    }

    let mut len = 0usize;
    while len < max_units {
        // SAFETY: the caller guarantees ptr points to a foreign single-byte
        // string. The scan is bounded by max_units and stops before reading
        // past it.
        if unsafe { *ptr.add(len) } == 0 {
            // SAFETY: len was discovered by a bounded scan from ptr, so this
            // slice covers only initialized bytes before the terminator.
            let slice = unsafe { slice::from_raw_parts(ptr, len) };
            return Some(String::from_utf8_lossy(slice).into_owned());
        }
        len += 1;
    }

    None
}

unsafe fn socket_address_to_ip(
    address: &windows::Win32::Networking::WinSock::SOCKET_ADDRESS,
) -> Option<FormattedIp> {
    use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, SOCKADDR_IN, SOCKADDR_IN6};

    if address.lpSockaddr.is_null() {
        return None;
    }
    let len = usize::try_from(address.iSockaddrLength).ok()?;
    if len < size_of::<windows::Win32::Networking::WinSock::SOCKADDR>() {
        return None;
    }

    // SAFETY: lpSockaddr is non-null and the caller owns the adapter-address
    // buffer while this function runs. The family field fits in SOCKADDR.
    let family = unsafe { (*address.lpSockaddr).sa_family };
    match family {
        AF_INET if len >= size_of::<SOCKADDR_IN>() => {
            // SAFETY: family and iSockaddrLength prove the buffer is large
            // enough for SOCKADDR_IN before casting.
            let sa4 = unsafe { &*(address.lpSockaddr as *const SOCKADDR_IN) };
            // SAFETY: SOCKADDR_IN for AF_INET initializes the S_addr variant.
            let addr = unsafe { sa4.sin_addr.S_un.S_addr };
            Some(FormattedIp::V4(format_ipv4_from_network_order(
                addr.to_be(),
            )))
        }
        AF_INET6 if len >= size_of::<SOCKADDR_IN6>() => {
            // SAFETY: family and iSockaddrLength prove the buffer is large
            // enough for SOCKADDR_IN6 before casting.
            let sa6 = unsafe { &*(address.lpSockaddr as *const SOCKADDR_IN6) };
            // SAFETY: SOCKADDR_IN6 initializes the Byte view of the IPv6 addr.
            let bytes = unsafe { sa6.sin6_addr.u.Byte };
            Some(FormattedIp::V6(format_ipv6_from_bytes(bytes)))
        }
        _ => None,
    }
}

fn format_ipv4_from_network_order(ip: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        (ip >> 24) & 0xFF,
        (ip >> 16) & 0xFF,
        (ip >> 8) & 0xFF,
        ip & 0xFF
    )
}

fn format_ipv6_from_bytes(bytes: [u8; 16]) -> String {
    format!(
        "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
        u16::from_be_bytes([bytes[0], bytes[1]]),
        u16::from_be_bytes([bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        u16::from_be_bytes([bytes[6], bytes[7]]),
        u16::from_be_bytes([bytes[8], bytes[9]]),
        u16::from_be_bytes([bytes[10], bytes[11]]),
        u16::from_be_bytes([bytes[12], bytes[13]]),
        u16::from_be_bytes([bytes[14], bytes[15]]),
    )
}

/// Returns (rx_bytes, tx_bytes, link_speed_bps).
fn get_if_stats(if_index: u32) -> (u64, u64, u64) {
    use windows::Win32::NetworkManagement::IpHelper::*;

    let mut row = MIB_IF_ROW2 {
        InterfaceIndex: if_index,
        ..Default::default()
    };

    // SAFETY: row is a properly initialized MIB_IF_ROW2 with InterfaceIndex
    // set. GetIfEntry2 writes to the struct and the return value is checked.
    unsafe {
        if GetIfEntry2(&mut row) == windows::Win32::Foundation::WIN32_ERROR(0) {
            (row.InOctets, row.OutOctets, row.TransmitLinkSpeed)
        } else {
            (0, 0, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ipv4_from_network_order_formats_octets() {
        assert_eq!(format_ipv4_from_network_order(0xC0A80101), "192.168.1.1");
    }

    #[test]
    fn format_ipv6_from_bytes_formats_segments() {
        assert_eq!(
            format_ipv6_from_bytes([0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
            "2001:db8:0:0:0:0:0:1"
        );
    }

    #[test]
    fn wide_ptr_to_string_bounded_stops_at_nul() {
        let name = ['L' as u16, 'A' as u16, 'N' as u16, 0, 'x' as u16];
        // SAFETY: name is a local null-terminated UTF-16 buffer.
        let parsed = unsafe { wide_ptr_to_string_bounded(name.as_ptr(), name.len()) };
        assert_eq!(parsed.as_deref(), Some("LAN"));
    }

    #[test]
    fn ansi_ptr_to_string_bounded_stops_at_nul() {
        let buf = b"{12345678-1234-1234-1234-123456789012}\0extra";
        // SAFETY: buf is a local null-terminated single-byte buffer.
        let parsed = unsafe { ansi_ptr_to_string_bounded(buf.as_ptr(), buf.len()) };
        assert_eq!(
            parsed.as_deref(),
            Some("{12345678-1234-1234-1234-123456789012}")
        );
    }

    #[test]
    fn ansi_ptr_to_string_bounded_returns_none_when_no_nul() {
        let buf = b"NoTerminator";
        // SAFETY: buf is a local single-byte buffer; passing buf.len() bounds
        // the scan to its initialized range.
        let parsed = unsafe { ansi_ptr_to_string_bounded(buf.as_ptr(), buf.len()) };
        assert_eq!(parsed, None);
    }

    #[test]
    fn collect_returns_at_least_one_interface() {
        let mut collector = NetCollector::new();
        collector.collect();
        assert!(!collector.nets.is_empty());
    }
}
