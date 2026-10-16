//! Who may ask: only addresses on your own network.

use std::net::IpAddr;

/// Private ranges, link-local and loopback. Everything else, including the shared range that
/// VPNs like Tailscale use (100.64.0.0/10), is refused: the server is for your home Wi-Fi.
pub fn is_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_lan(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                // Unique local (fc00::/7) and link-local (fe80::/10).
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_lan;

    #[test]
    fn only_your_own_network_may_ask() {
        for ok in [
            "192.168.1.20",
            "10.0.0.5",
            "172.20.3.4",
            "127.0.0.1",
            "169.254.9.9",
            "::1",
            "fe80::1",
            "fd12::7",
            "::ffff:192.168.1.20",
        ] {
            assert!(is_lan(ok.parse().unwrap()), "{ok}");
        }
        for no in [
            "8.8.8.8",
            "100.101.102.103",
            "172.32.0.1",
            "2001:4860::8888",
            "::ffff:8.8.8.8",
        ] {
            assert!(!is_lan(no.parse().unwrap()), "{no}");
        }
    }
}
