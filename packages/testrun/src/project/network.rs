//! Native host-interface inspection; candidates do not establish a device route.
use super::*;
use std::net::IpAddr;

fn candidates(addresses: Vec<(String, IpAddr)>, port: u16, scheme: &str) -> Value {
    let mut unique = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for (interface, address) in addresses {
        if address.is_loopback()
            || address.is_unspecified()
            || address.is_multicast()
            || !unique.insert((interface.clone(), address))
        {
            continue;
        }
        let host = match address {
            IpAddr::V4(v) => v.to_string(),
            IpAddr::V6(v) => format!("[{v}]"),
        };
        result.push(json!({"interface":interface,"address":address.to_string(),"serverUrl":format!("{scheme}://{host}:{port}"),"evidence":"host_interface_only","deviceReachable":"unknown","serverListening":"unknown"}));
        if result.len() == 32 {
            break;
        }
    }
    json!({"state":"candidates","ambiguous":result.len()>1,"candidates":result,"nextAction":"Choose a reachable interface and configure Expo's LAN advertisement or an explicit connection_url; interface presence does not establish server binding or phone connectivity"})
}

pub(super) fn inspect(endpoint: &str) -> Value {
    let Ok(endpoint) = url::Url::parse(endpoint) else {
        return json!({"state":"unknown"});
    };
    match interfaces() {
        Ok(addresses) => candidates(
            addresses,
            endpoint.port_or_known_default().unwrap_or(8081),
            endpoint.scheme(),
        ),
        Err(error) => json!({"state":"unknown","error":error}),
    }
}

#[cfg(unix)]
fn interfaces() -> Result<Vec<(String, IpAddr)>> {
    struct Addresses(*mut libc::ifaddrs);
    impl Drop for Addresses {
        fn drop(&mut self) {
            // SAFETY: getifaddrs returned this list; this guard frees it once.
            unsafe { libc::freeifaddrs(self.0) };
        }
    }
    let mut pointer = std::ptr::null_mut();
    // SAFETY: pointer is an initialized out parameter owned by this call.
    if unsafe { libc::getifaddrs(&mut pointer) } != 0 {
        return Err(Error::new(
            "NETWORK_INSPECTION",
            "Cannot enumerate host interfaces",
        ));
    }
    let _addresses = Addresses(pointer);
    let mut result = Vec::new();
    let mut visited = 0;
    while !pointer.is_null() && visited < 256 {
        // SAFETY: each entry belongs to the live getifaddrs list above.
        let entry = unsafe { &*pointer };
        pointer = entry.ifa_next;
        visited += 1;
        if entry.ifa_addr.is_null()
            || entry.ifa_name.is_null()
            || entry.ifa_flags & libc::IFF_UP as u32 == 0
        {
            continue;
        }
        // SAFETY: getifaddrs provides a NUL-terminated interface name.
        let name = unsafe { std::ffi::CStr::from_ptr(entry.ifa_name) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: a non-null ifa_addr has a sockaddr header; family determines its layout.
        let family = unsafe { (*entry.ifa_addr).sa_family } as i32;
        let address = match family {
            libc::AF_INET => {
                // SAFETY: AF_INET identifies a sockaddr_in in this list.
                let address = unsafe { &*entry.ifa_addr.cast::<libc::sockaddr_in>() };
                Some(IpAddr::V4(std::net::Ipv4Addr::from(
                    address.sin_addr.s_addr.to_ne_bytes(),
                )))
            }
            libc::AF_INET6 => {
                // SAFETY: AF_INET6 identifies a sockaddr_in6 in this list.
                let address = unsafe { &*entry.ifa_addr.cast::<libc::sockaddr_in6>() };
                let ip = std::net::Ipv6Addr::from(address.sin6_addr.s6_addr);
                // Scope IDs are required for link-local IPv6 routes; do not publish an unusable URL.
                if ip.is_unicast_link_local() {
                    None
                } else {
                    Some(IpAddr::V6(ip))
                }
            }
            _ => None,
        };
        if let Some(address) = address {
            result.push((name, address));
        }
    }
    Ok(result)
}
#[cfg(not(unix))]
fn interfaces() -> Result<Vec<(String, IpAddr)>> {
    Err(Error::new(
        "CAPABILITY_UNAVAILABLE",
        "Host interface candidates are unavailable on this platform; configure connection_url explicitly",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_network_candidates_keep_interface_ambiguity_and_unknown_reachability() {
        let result = candidates(
            vec![
                ("wifi".into(), "192.0.2.1".parse().unwrap()),
                ("vpn".into(), "198.51.100.1".parse().unwrap()),
                ("loopback".into(), "127.0.0.1".parse().unwrap()),
            ],
            8081,
            "http",
        );
        assert_eq!(result["ambiguous"], true);
        assert_eq!(result["candidates"].as_array().unwrap().len(), 2);
        assert_eq!(
            result["candidates"][0]["serverUrl"],
            "http://192.0.2.1:8081"
        );
        assert_eq!(result["candidates"][0]["deviceReachable"], "unknown");
        assert_eq!(result["candidates"][0]["serverListening"], "unknown");
    }
}
