//! Phone access: serve the workspace to a phone on the same network.
//!
//! Phone access is off until `rmd web --phone` turns it on. While it is on,
//! the config root holds `.phone.json` (`{"token", "port"}`, owner-only): a
//! long-lived token that paired phones keep in a cookie, and the port their
//! bookmarks name. The server then listens on every interface instead of
//! only `127.0.0.1`, and accepts requests addressed to an IPv4 address
//! (never a host name, which a DNS-rebinding page could control) carrying
//! either token. `rmd web --no-phone` deletes the file, which unpairs every
//! phone.

use std::io;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::path::{Path, PathBuf};

use qrcode::{Color, EcLevel, QrCode};
use serde_json::{Value, json};

use crate::py;
use crate::web::workspace;

/// The phone access file in a config root.
pub const FILE: &str = ".phone.json";

/// How long a paired phone stays signed in: 400 days, the longest cookie
/// lifetime browsers keep.
pub const COOKIE_MAX_AGE: u64 = 400 * 24 * 60 * 60;

/// Phone access while it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhoneAccess {
    /// What paired phones authenticate with.
    pub token: String,
    /// The port paired phones use; 0 until a server has run with phone access.
    pub port: u16,
}

impl PhoneAccess {
    /// The link that pairs a phone reaching this computer at `address`.
    pub fn pairing_url(&self, address: Ipv4Addr) -> String {
        format!("{}#token={}", self.url(address), self.token)
    }

    /// The workspace address for a phone that is paired already.
    pub fn url(&self, address: Ipv4Addr) -> String {
        format!("http://{address}:{}/", self.port)
    }
}

fn path(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// Phone access for config root `root`, when it is on.
pub fn load(root: &Path) -> Option<PhoneAccess> {
    let state: Value = serde_json::from_str(&std::fs::read_to_string(path(root)).ok()?).ok()?;
    let token = state.get("token")?.as_str()?;
    if token.is_empty() {
        return None;
    }
    let port = state.get("port").and_then(Value::as_u64).and_then(|port| u16::try_from(port).ok()).unwrap_or(0);
    Some(PhoneAccess { token: token.to_string(), port })
}

pub fn save(root: &Path, access: &PhoneAccess) -> io::Result<()> {
    super::write_private(&path(root), &py::dumps(&json!({"token": access.token, "port": access.port})))
}

/// Turn phone access on, keeping the token of phones that are paired already.
pub fn enable(root: &Path) -> io::Result<PhoneAccess> {
    if let Some(access) = load(root) {
        return Ok(access);
    }
    let access = PhoneAccess { token: workspace::token_urlsafe(32), port: 0 };
    save(root, &access)?;
    Ok(access)
}

/// Turn phone access off, which unpairs every phone.
pub fn disable(root: &Path) -> io::Result<()> {
    match std::fs::remove_file(path(root)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Whether a `Host` header names this server on `port` by an IPv4 address.
pub fn is_address_host(host: &[u8], port: u16) -> bool {
    std::str::from_utf8(host)
        .ok()
        .and_then(|host| host.rsplit_once(':'))
        .is_some_and(|(address, host_port)| host_port == port.to_string() && address.parse::<Ipv4Addr>().is_ok())
}

/// Tailscale gives each device an address in 100.64.0.0/10.
pub fn is_tailscale(address: Ipv4Addr) -> bool {
    let [first, second, ..] = address.octets();
    first == 100 && (64..128).contains(&second)
}

/// An interface's IPv4 address.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Interface {
    name: String,
    address: Ipv4Addr,
    /// A tunnel to one peer, as VPNs use.
    point_to_point: bool,
}

/// Name prefixes of virtual interfaces a phone on the local network cannot
/// reach: VPN tunnels, and bridges to virtual machines and containers.
const VIRTUAL_INTERFACES: [&str; 14] = [
    "utun", "tun", "tap", "wg", "ipsec", "ppp", "feth", "bridge", "vmnet", "vboxnet", "docker", "br-", "veth", "virbr",
];

/// Addresses a phone can reach this computer at: Wi-Fi and Ethernet first
/// (the one on the default route ahead of the others), then Tailscale.
/// Without any, the default route's address stands in.
pub fn network_addresses() -> Vec<Ipv4Addr> {
    choose_addresses(default_route_address(), &interface_addresses())
}

fn choose_addresses(primary: Option<Ipv4Addr>, interfaces: &[Interface]) -> Vec<Ipv4Addr> {
    let usable = |address: &Ipv4Addr| {
        !(address.is_loopback()
            || address.is_link_local()
            || address.is_unspecified()
            || address.is_broadcast()
            || address.is_multicast())
    };
    let mut local: Vec<&Interface> = interfaces
        .iter()
        .filter(|interface| {
            usable(&interface.address)
                && !interface.point_to_point
                && !is_tailscale(interface.address)
                && !VIRTUAL_INTERFACES.iter().any(|prefix| interface.name.starts_with(prefix))
        })
        .collect();
    // A VPN can own the default route, so it only orders the local addresses.
    let physical = |name: &str| ["en", "eth", "wl"].iter().any(|prefix| name.starts_with(prefix));
    local.sort_by_key(|interface| (Some(interface.address) != primary, !physical(&interface.name)));
    let tailscale = interfaces.iter().filter(|interface| is_tailscale(interface.address));
    let mut chosen: Vec<Ipv4Addr> = Vec::new();
    for interface in local.into_iter().chain(tailscale) {
        if !chosen.contains(&interface.address) {
            chosen.push(interface.address);
        }
    }
    if chosen.is_empty() {
        chosen.extend(primary.filter(usable));
    }
    chosen
}

/// This computer's address on the default route.
fn default_route_address() -> Option<Ipv4Addr> {
    // TEST-NET-1 (RFC 5737): reached through the default route, never contacted.
    route_source(Ipv4Addr::new(192, 0, 2, 1))
}

/// The address this computer sends from to reach `destination`, as its
/// routing table says. Connecting a UDP socket only picks a route; nothing
/// is sent.
fn route_source(destination: Ipv4Addr) -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((destination, 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(address) => Some(address),
        IpAddr::V6(_) => None,
    }
}

/// Whether traffic to other Tailscale devices leaves through another VPN
/// instead of Tailscale, which happens when a VPN that takes over all traffic
/// (Cisco Secure Client, for one) removes Tailscale's route. A phone's request
/// then arrives over Tailscale, but the reply goes into the other VPN and is lost.
pub fn tailscale_captured(tailscale: Ipv4Addr) -> bool {
    // Any address in Tailscale's range shows where its traffic is routed.
    route_source(Ipv4Addr::new(100, 64, 0, 1)).is_some_and(|source| source != tailscale)
}

/// The IPv4 addresses of this computer's interfaces that are up.
#[cfg(unix)]
fn interface_addresses() -> Vec<Interface> {
    let mut found = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs(3) fills `list` with a linked list that stays valid
    // until freeifaddrs(3); every node and address is read before that.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return found;
        }
        let mut node = list;
        while let Some(entry) = node.as_ref() {
            let up = entry.ifa_flags & (libc::IFF_UP as libc::c_uint) != 0;
            if up
                && let Some(address) = entry.ifa_addr.as_ref()
                && i32::from(address.sa_family) == libc::AF_INET
            {
                let address = &*(entry.ifa_addr as *const libc::sockaddr_in);
                found.push(Interface {
                    name: std::ffi::CStr::from_ptr(entry.ifa_name).to_string_lossy().into_owned(),
                    address: Ipv4Addr::from(u32::from_be(address.sin_addr.s_addr)),
                    point_to_point: entry.ifa_flags & (libc::IFF_POINTOPOINT as libc::c_uint) != 0,
                });
            }
            node = entry.ifa_next;
        }
        libc::freeifaddrs(list);
    }
    found
}

#[cfg(not(unix))]
fn interface_addresses() -> Vec<Interface> {
    Vec::new()
}

/// `text` as a QR code for a terminal, two modules per character. With
/// `color`, the code prints black on white whatever the terminal's colors;
/// without, dark modules print as blocks, which suits a light background.
pub fn qr_lines(text: &str, color: bool) -> Vec<String> {
    const QUIET: usize = 2;
    let Ok(code) = QrCode::with_error_correction_level(text, EcLevel::L) else {
        return Vec::new();
    };
    let width = code.width();
    let modules = code.to_colors();
    let size = width + 2 * QUIET;
    let dark = |x: usize, y: usize| {
        (QUIET..width + QUIET).contains(&x)
            && (QUIET..width + QUIET).contains(&y)
            && modules[(y - QUIET) * width + x - QUIET] == Color::Dark
    };
    (0..size)
        .step_by(2)
        .map(|y| {
            let row: String = (0..size)
                .map(|x| match (dark(x, y), dark(x, y + 1)) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                })
                .collect();
            if color { format!("\x1b[30;107m{row}\x1b[0m") } else { row }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    #[test]
    fn access_is_off_until_enabled_and_keeps_its_token() {
        let env = TestEnv::new();
        assert_eq!(load(env.root()), None);
        let access = enable(env.root()).unwrap();
        assert_eq!(access.token.len(), 43);
        assert_eq!(access.port, 0);
        save(env.root(), &PhoneAccess { port: 8765, ..access.clone() }).unwrap();
        let again = enable(env.root()).unwrap();
        assert_eq!((&again.token, again.port), (&access.token, 8765), "paired phones stay paired");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(env.root().join(FILE)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        disable(env.root()).unwrap();
        assert_eq!(load(env.root()), None);
        disable(env.root()).unwrap();
        assert_ne!(enable(env.root()).unwrap().token, again.token, "turning it on again pairs anew");
    }

    #[test]
    fn malformed_access_files_count_as_off() {
        let env = TestEnv::new();
        for content in ["", "[]", r#"{"port": 1}"#, r#"{"token": ""}"#, r#"{"token": 5}"#] {
            env.write(FILE, content);
            assert_eq!(load(env.root()), None, "{content}");
        }
        env.write(FILE, r#"{"token": "t", "port": 70000}"#);
        assert_eq!(load(env.root()), Some(PhoneAccess { token: "t".into(), port: 0 }));
    }

    #[test]
    fn only_ipv4_hosts_on_the_server_port_are_addresses() {
        assert!(is_address_host(b"192.168.1.20:8765", 8765));
        assert!(is_address_host(b"100.101.102.103:8765", 8765));
        assert!(is_address_host(b"127.0.0.1:8765", 8765));
        for host in ["192.168.1.20:8766", "192.168.1.20", "evil.example:8765", "[::1]:8765", "1.2.3:8765", ":8765"] {
            assert!(!is_address_host(host.as_bytes(), 8765), "{host}");
        }
        assert!(!is_address_host(b"\xff:8765", 8765));
    }

    fn interface(name: &str, address: [u8; 4], point_to_point: bool) -> Interface {
        Interface { name: name.into(), address: Ipv4Addr::from(address), point_to_point }
    }

    #[test]
    fn addresses_prefer_wifi_and_ethernet_then_add_tailscale() {
        let wifi = Ipv4Addr::new(172, 20, 20, 20);
        let vpn = Ipv4Addr::new(10, 64, 191, 214);
        let tailscale = Ipv4Addr::new(100, 101, 102, 103);
        let interfaces = [
            interface("lo0", [127, 0, 0, 1], false),
            interface("bridge100", [192, 168, 64, 1], false),
            interface("feth1828", [10, 111, 101, 213], false),
            interface("en0", wifi.octets(), false),
            interface("utun6", vpn.octets(), true),
            interface("utun7", tailscale.octets(), true),
            interface("en7", [169, 254, 3, 4], false),
        ];
        // A VPN that owns the default route is not where a phone on the Wi-Fi connects.
        assert_eq!(choose_addresses(Some(vpn), &interfaces), vec![wifi, tailscale]);
        assert_eq!(choose_addresses(Some(tailscale), &interfaces), vec![wifi, tailscale]);
        assert_eq!(choose_addresses(None, &interfaces), vec![wifi, tailscale]);

        // With Wi-Fi and Ethernet both up, the default route's goes first.
        let ethernet = interface("en5", [192, 168, 1, 30], false);
        let both = [interface("en0", wifi.octets(), false), ethernet.clone()];
        assert_eq!(choose_addresses(Some(ethernet.address), &both), vec![ethernet.address, wifi]);
        // An unfamiliar interface name still counts, after the familiar ones.
        let other = interface("igb0", [192, 168, 2, 2], false);
        assert_eq!(choose_addresses(None, &[other.clone(), ethernet.clone()]), vec![ethernet.address, other.address]);

        // Nothing local: the default route's address is the best guess.
        assert_eq!(choose_addresses(Some(vpn), &interfaces[4..5]), vec![vpn]);
        assert_eq!(choose_addresses(Some(Ipv4Addr::LOCALHOST), &[]), Vec::<Ipv4Addr>::new());
        assert!(!is_tailscale(Ipv4Addr::new(100, 128, 0, 1)) && !is_tailscale(Ipv4Addr::new(100, 63, 0, 1)));
    }

    #[test]
    fn routes_are_read_from_this_computer() {
        assert_eq!(route_source(Ipv4Addr::LOCALHOST), Some(Ipv4Addr::LOCALHOST));
        // Loopback never carries Tailscale's traffic, so it counts as captured
        // wherever a route to Tailscale's range exists.
        let routed = route_source(Ipv4Addr::new(100, 64, 0, 1)).is_some();
        assert_eq!(tailscale_captured(Ipv4Addr::LOCALHOST), routed);
    }

    #[test]
    fn links_name_the_address_port_and_token() {
        let access = PhoneAccess { token: "abc".into(), port: 8765 };
        let address = Ipv4Addr::new(192, 168, 1, 20);
        assert_eq!(access.url(address), "http://192.168.1.20:8765/");
        assert_eq!(access.pairing_url(address), "http://192.168.1.20:8765/#token=abc");
    }

    #[test]
    fn qr_codes_render_two_rows_per_line_with_a_quiet_zone() {
        let lines = qr_lines("http://192.168.1.20:8765/#token=abc", false);
        // Version 3 is 29 modules; a quiet zone of 2 on each side makes 33.
        assert_eq!(lines.len(), 17);
        assert!(lines.iter().all(|line| line.chars().count() == 33));
        assert!(lines[0].trim().is_empty() && lines[16].trim().is_empty());
        // The finder pattern's first two rows: a dark edge over a dark frame.
        assert!(lines[1].starts_with("  █▀▀▀▀▀█"), "{:?}", lines[1]);
        let colored = qr_lines("x", true);
        assert!(colored.iter().all(|line| line.starts_with("\x1b[30;107m") && line.ends_with("\x1b[0m")));
    }
}
