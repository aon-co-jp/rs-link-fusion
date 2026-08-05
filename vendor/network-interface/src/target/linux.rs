use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::slice::from_raw_parts;

use libc::{
    AF_INET, AF_INET6, AF_PACKET, IFF_LOOPBACK, if_nametoindex, sockaddr_in, sockaddr_in6,
    sockaddr_ll, strlen,
};

#[allow(unused_imports)]
#[cfg(not(target_os = "android"))]
use crate::target::getifaddrs;
use crate::{Error, NetworkInterface, NetworkInterfaceConfig, Result};
#[allow(unused_imports)]
use crate::utils::{ipv4_from_in_addr, ipv6_from_in6_addr, make_ipv4_netmask, make_ipv6_netmask};

/// Android(Bionic libc)向けフォールバック実装。
///
/// `getifaddrs(3)`/`freeifaddrs(3)`はBionic libcにリンクできない
/// (2026-08-05、`rs-link-fusion`側で実機ビルド時のリンクエラーとして
/// 発見。RUNOエコシステムの`android`ブランチとしてこのcrateへローカル
/// パッチ)ため、Androidターゲットでは`getifaddrs`を一切使わず、
/// `/proc/net/dev`によるインターフェース名列挙+`SIOCGIFADDR`系ioctl
/// (Bionic libcでも利用可能)によるIPv4アドレス取得に置き換える。
/// **既知の制約(正直な開示)**: IPv6アドレス・MACアドレス・ブロード
/// キャストアドレスは取得しない(IPv4アドレスのみ)。WiFi/USB-Ethernet
/// のボンディング用途ではIPv4のみで実用上十分なため許容している。
#[cfg(target_os = "android")]
mod android {
    use std::collections::HashMap;
    use std::ffi::CString;
    use std::fs;
    use std::net::Ipv4Addr;
    use std::os::unix::io::RawFd;

    use libc::{c_char, c_int, c_short, c_ulong, if_nametoindex, sockaddr, sockaddr_in, AF_INET, IFF_LOOPBACK, IFF_UP};

    use crate::{NetworkInterface, Result};

    const IFNAMSIZ: usize = 16;
    // Linux/Bionkでの標準的なioctl番号(<linux/sockios.h>由来、値は
    // アーキテクチャ非依存)。
    const SIOCGIFADDR: c_ulong = 0x8915;
    const SIOCGIFFLAGS: c_ulong = 0x8913;

    // `libc`crateはlinux_like全般で`ifreq`構造体を公開していないため、
    // Linuxカーネルのabiに合わせて必要最小限を自前定義する。
    #[repr(C)]
    struct IfReq {
        ifr_name: [c_char; IFNAMSIZ],
        ifr_union: IfReqUnion,
    }

    #[repr(C)]
    union IfReqUnion {
        ifr_addr: sockaddr,
        ifr_flags: c_short,
        _pad: [u8; 24],
    }

    fn ioctl_raw(fd: RawFd, request: c_ulong, req: *mut IfReq) -> c_int {
        extern "C" {
            fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
        }
        unsafe { ioctl(fd, request, req) }
    }

    fn make_ifreq(name: &str) -> IfReq {
        let mut ifr_name = [0 as c_char; IFNAMSIZ];
        let c_name = CString::new(name).unwrap_or_default();
        let bytes = c_name.as_bytes();
        let len = bytes.len().min(IFNAMSIZ - 1);
        for i in 0..len {
            ifr_name[i] = bytes[i] as c_char;
        }
        IfReq { ifr_name, ifr_union: IfReqUnion { _pad: [0; 24] } }
    }

    /// `/proc/net/dev`からインターフェース名を列挙する(loopback含む)。
    fn list_interface_names() -> Result<Vec<String>> {
        let contents = fs::read_to_string("/proc/net/dev").unwrap_or_default();
        let mut names = Vec::new();
        for line in contents.lines().skip(2) {
            if let Some((name, _)) = line.split_once(':') {
                names.push(name.trim().to_string());
            }
        }
        Ok(names)
    }

    pub fn show() -> Result<Vec<NetworkInterface>> {
        let mut out: HashMap<String, NetworkInterface> = HashMap::new();

        let fd = unsafe { libc::socket(AF_INET, libc::SOCK_DGRAM, 0) };
        if fd < 0 {
            // ソケット作成に失敗した場合でも、名前だけは返す
            // (呼び出し元がpanicしないよう、空アドレスのインターフェース
            // 一覧を返す)。
            for name in list_interface_names()? {
                let index = if_nametoindex_safe(&name);
                out.entry(name.clone()).or_insert(NetworkInterface {
                    name,
                    addr: Vec::new(),
                    mac_addr: None,
                    index,
                    internal: false,
                });
            }
            return Ok(out.into_values().collect());
        }

        for name in list_interface_names()? {
            let index = if_nametoindex_safe(&name);

            let mut flags_req = make_ifreq(&name);
            let internal = if ioctl_raw(fd, SIOCGIFFLAGS, &mut flags_req) == 0 {
                let flags = unsafe { flags_req.ifr_union.ifr_flags } as c_int;
                flags & IFF_LOOPBACK != 0
            } else {
                name == "lo"
            };
            let _up = if ioctl_raw(fd, SIOCGIFFLAGS, &mut flags_req) == 0 {
                (unsafe { flags_req.ifr_union.ifr_flags } as c_int) & IFF_UP != 0
            } else {
                false
            };

            let mut addr_req = make_ifreq(&name);
            let mut addrs = Vec::new();
            if ioctl_raw(fd, SIOCGIFADDR, &mut addr_req) == 0 {
                let sockaddr_in_ptr = unsafe { &addr_req.ifr_union.ifr_addr as *const sockaddr as *const sockaddr_in };
                let s_addr = unsafe { (*sockaddr_in_ptr).sin_addr.s_addr };
                let ip = Ipv4Addr::from(u32::from_be(s_addr));
                addrs.push(crate::Addr::V4(crate::V4IfAddr {
                    ip,
                    broadcast: None,
                    netmask: None,
                }));
            }

            out.insert(
                name.clone(),
                NetworkInterface { name, addr: addrs, mac_addr: None, index, internal },
            );
        }

        unsafe { libc::close(fd) };

        Ok(out.into_values().collect())
    }

    fn if_nametoindex_safe(name: &str) -> u32 {
        match CString::new(name) {
            Ok(c_name) => unsafe { if_nametoindex(c_name.as_ptr()) },
            Err(_) => 0,
        }
    }
}

impl NetworkInterfaceConfig for NetworkInterface {
    #[cfg(target_os = "android")]
    fn show() -> Result<Vec<NetworkInterface>> {
        android::show()
    }

    #[cfg(not(target_os = "android"))]
    fn show() -> Result<Vec<NetworkInterface>> {
        let mut network_interfaces: HashMap<String, NetworkInterface> = HashMap::new();

        for netifa in getifaddrs()? {
            let netifa_addr = netifa.ifa_addr;
            let netifa_family = if netifa_addr.is_null() {
                continue;
            } else {
                unsafe { (*netifa_addr).sa_family as i32 }
            };

            let internal = netifa.ifa_flags & IFF_LOOPBACK as u32 != 0;

            let mut network_interface = match netifa_family {
                AF_PACKET => {
                    let name = make_netifa_name(&netifa)?;
                    let mac = make_mac_addrs(&netifa);
                    let index = netifa_index(&netifa);
                    NetworkInterface {
                        name,
                        addr: Vec::new(),
                        mac_addr: Some(mac),
                        index,
                        internal,
                    }
                }
                AF_INET => {
                    let socket_addr = netifa_addr as *mut sockaddr_in;
                    let internet_address = unsafe { (*socket_addr).sin_addr };
                    let name = make_netifa_name(&netifa)?;
                    let index = netifa_index(&netifa);
                    let netmask = make_ipv4_netmask(&netifa);
                    let addr = ipv4_from_in_addr(&internet_address)?;
                    let broadcast = make_ipv4_broadcast_addr(&netifa)?;
                    NetworkInterface::new_afinet(
                        name.as_str(),
                        addr,
                        netmask,
                        broadcast,
                        index,
                        internal,
                    )
                }
                AF_INET6 => {
                    let socket_addr = netifa_addr as *mut sockaddr_in6;
                    let internet_address = unsafe { (*socket_addr).sin6_addr };
                    let name = make_netifa_name(&netifa)?;
                    let index = netifa_index(&netifa);
                    let netmask = make_ipv6_netmask(&netifa);
                    let addr = ipv6_from_in6_addr(&internet_address)?;
                    let broadcast = make_ipv6_broadcast_addr(&netifa)?;
                    NetworkInterface::new_afinet6(
                        name.as_str(),
                        addr,
                        netmask,
                        broadcast,
                        index,
                        internal,
                    )
                }
                _ => continue,
            };

            network_interfaces
                .entry(network_interface.name.clone())
                .and_modify(|old| old.addr.append(&mut network_interface.addr))
                .or_insert(network_interface);
        }

        Ok(network_interfaces.into_values().collect())
    }
}

/// Retrieves the network interface name
fn make_netifa_name(netifa: &libc::ifaddrs) -> Result<String> {
    let data = netifa.ifa_name as *const libc::c_char;
    let len = unsafe { strlen(data) };
    let bytes_slice = unsafe { from_raw_parts(data as *const u8, len) };

    match String::from_utf8(bytes_slice.to_vec()) {
        Ok(s) => Ok(s),
        Err(e) => Err(Error::ParseUtf8Error(e)),
    }
}

/// Retrieves the broadcast address for the network interface provided of the
/// AF_INET family.
///
/// ## References
///
/// https://man7.org/linux/man-pages/man3/getifaddrs.3.html
fn make_ipv4_broadcast_addr(netifa: &libc::ifaddrs) -> Result<Option<Ipv4Addr>> {
    let ifa_dstaddr = netifa.ifa_ifu;

    if ifa_dstaddr.is_null() {
        return Ok(None);
    }

    let socket_addr = ifa_dstaddr as *mut sockaddr_in;
    let internet_address = unsafe { (*socket_addr).sin_addr };
    let addr = ipv4_from_in_addr(&internet_address)?;

    Ok(Some(addr))
}

/// Retrieves the broadcast address for the network interface provided of the
/// AF_INET6 family.
///
/// ## References
///
/// https://man7.org/linux/man-pages/man3/getifaddrs.3.html
fn make_ipv6_broadcast_addr(netifa: &libc::ifaddrs) -> Result<Option<Ipv6Addr>> {
    let ifa_dstaddr = netifa.ifa_ifu;

    if ifa_dstaddr.is_null() {
        return Ok(None);
    }

    let socket_addr = ifa_dstaddr as *mut sockaddr_in6;
    let internet_address = unsafe { (*socket_addr).sin6_addr };
    let addr = ipv6_from_in6_addr(&internet_address)?;

    Ok(Some(addr))
}

fn make_mac_addrs(netifa: &libc::ifaddrs) -> String {
    let netifa_addr = netifa.ifa_addr;
    let socket_addr = netifa_addr as *mut sockaddr_ll;
    let mac_array = unsafe { (*socket_addr).sll_addr };
    let addr_len = unsafe { (*socket_addr).sll_halen };
    let real_addr_len = std::cmp::min(addr_len as usize, mac_array.len());
    let mac_slice = unsafe { std::slice::from_raw_parts(mac_array.as_ptr(), real_addr_len) };

    mac_slice
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Retreives the name for the the network interface provided
///
/// ## References
///
/// https://man7.org/linux/man-pages/man3/if_nametoindex.3.html
fn netifa_index(netifa: &libc::ifaddrs) -> u32 {
    let name = netifa.ifa_name as *const libc::c_char;

    unsafe { if_nametoindex(name) }
}
