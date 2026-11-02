//! **What an address is, for whether an origin may reach it.**
//!
//! A public origin reaches ordinary global unicast addresses alone: none in
//! a block of IANA's IPv4 or IPv6 Special-Purpose Address Registry (read
//! 2026-10-25, the registries' CSV), whether or not the registry marks it
//! globally reachable, as no provider is reached at an anycast service's
//! address; no multicast; and, in IPv6, nothing outside 2000::/3, the global
//! unicast space. A loopback address is told apart, as a development origin
//! reaches it and nothing else. An IPv4-mapped IPv6 address is the IPv4
//! address it maps, which a dual-stack socket reaches.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// **An address's class.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Ordinary global unicast: a public origin's.
    Global,
    /// This machine: `127.0.0.0/8`, `::1`.
    Loopback,
    /// A special-purpose block, named as IANA's registry names it.
    Special(&'static str),
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::Global => "global",
            Class::Loopback => "loopback",
            Class::Special(name) => name,
        }
    }
}

/// IANA's IPv4 special-purpose blocks, beside multicast and the reserved
/// `240.0.0.0/4` (which holds the limited broadcast address): each a prefix,
/// its length, and its name.
const V4: [(Ipv4Addr, u8, &str); 15] = [
    (Ipv4Addr::new(0, 0, 0, 0), 8, "\"this network\""),
    (Ipv4Addr::new(10, 0, 0, 0), 8, "private-use"),
    (Ipv4Addr::new(100, 64, 0, 0), 10, "shared address space"),
    (Ipv4Addr::new(169, 254, 0, 0), 16, "link-local"),
    (Ipv4Addr::new(172, 16, 0, 0), 12, "private-use"),
    (Ipv4Addr::new(192, 0, 0, 0), 24, "IETF protocol assignments"),
    (
        Ipv4Addr::new(192, 0, 2, 0),
        24,
        "documentation (TEST-NET-1)",
    ),
    (Ipv4Addr::new(192, 31, 196, 0), 24, "AS112-v4"),
    (Ipv4Addr::new(192, 52, 193, 0), 24, "AMT"),
    (
        Ipv4Addr::new(192, 88, 99, 0),
        24,
        "6to4 relay anycast (deprecated)",
    ),
    (Ipv4Addr::new(192, 168, 0, 0), 16, "private-use"),
    (
        Ipv4Addr::new(192, 175, 48, 0),
        24,
        "direct delegation AS112 service",
    ),
    (Ipv4Addr::new(198, 18, 0, 0), 15, "benchmarking"),
    (
        Ipv4Addr::new(198, 51, 100, 0),
        24,
        "documentation (TEST-NET-2)",
    ),
    (
        Ipv4Addr::new(203, 0, 113, 0),
        24,
        "documentation (TEST-NET-3)",
    ),
];

/// IANA's IPv6 special-purpose blocks inside 2000::/3; everything outside it
/// is no global unicast address.
const V6: [(Ipv6Addr, u8, &str); 5] = [
    (
        Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0),
        23,
        "IETF protocol assignments",
    ),
    (
        Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0),
        32,
        "documentation",
    ),
    (Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16, "6to4"),
    (
        Ipv6Addr::new(0x2620, 0x4f, 0x8000, 0, 0, 0, 0, 0),
        48,
        "direct delegation AS112 service",
    ),
    (
        Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0),
        20,
        "documentation",
    ),
];

fn in_v4(address: Ipv4Addr, prefix: Ipv4Addr, length: u8) -> bool {
    let mask = u32::MAX.checked_shl(32 - u32::from(length)).unwrap_or(0);
    u32::from(address) & mask == u32::from(prefix) & mask
}

fn in_v6(address: Ipv6Addr, prefix: Ipv6Addr, length: u8) -> bool {
    let mask = u128::MAX.checked_shl(128 - u32::from(length)).unwrap_or(0);
    u128::from(address) & mask == u128::from(prefix) & mask
}

/// **The class of `address`.**
pub fn class(address: IpAddr) -> Class {
    match address {
        IpAddr::V4(v4) => class_v4(v4),
        IpAddr::V6(v6) => class_v6(v6),
    }
}

fn class_v4(address: Ipv4Addr) -> Class {
    if in_v4(address, Ipv4Addr::new(127, 0, 0, 0), 8) {
        return Class::Loopback;
    }
    if in_v4(address, Ipv4Addr::new(224, 0, 0, 0), 4) {
        return Class::Special("multicast");
    }
    if in_v4(address, Ipv4Addr::new(240, 0, 0, 0), 4) {
        return Class::Special("reserved (or the limited broadcast)");
    }
    for (prefix, length, name) in V4 {
        if in_v4(address, prefix, length) {
            return Class::Special(name);
        }
    }
    Class::Global
}

fn class_v6(address: Ipv6Addr) -> Class {
    if let Some(v4) = address.to_ipv4_mapped() {
        return class_v4(v4);
    }
    if address == Ipv6Addr::LOCALHOST {
        return Class::Loopback;
    }
    if !in_v6(address, Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 3) {
        let name = if address == Ipv6Addr::UNSPECIFIED {
            "unspecified"
        } else if in_v6(address, Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7) {
            "unique-local"
        } else if in_v6(address, Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10) {
            "link-local"
        } else if in_v6(address, Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0), 8) {
            "multicast"
        } else if in_v6(address, Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0), 32) {
            "IPv4-IPv6 translation"
        } else {
            "outside global unicast (2000::/3)"
        };
        return Class::Special(name);
    }
    for (prefix, length, name) in V6 {
        if in_v6(address, prefix, length) {
            return Class::Special(name);
        }
    }
    Class::Global
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(text: &str) -> Class {
        class(text.parse().unwrap())
    }

    /// **Each special-purpose block is refused at both of its ends, and the
    /// addresses just outside it are global**: the registry's blocks, read
    /// 2026-10-25, multicast, and the reserved block.
    #[test]
    fn each_special_block_is_told_at_its_ends() {
        let special = [
            ("0.0.0.0", "\"this network\""),
            ("0.255.255.255", "\"this network\""),
            ("10.0.0.0", "private-use"),
            ("10.255.255.255", "private-use"),
            ("100.64.0.0", "shared address space"),
            ("100.127.255.255", "shared address space"),
            ("169.254.0.0", "link-local"),
            ("169.254.169.254", "link-local"),
            ("169.254.255.255", "link-local"),
            ("172.16.0.0", "private-use"),
            ("172.31.255.255", "private-use"),
            ("192.0.0.0", "IETF protocol assignments"),
            ("192.0.0.255", "IETF protocol assignments"),
            ("192.0.2.0", "documentation (TEST-NET-1)"),
            ("192.0.2.255", "documentation (TEST-NET-1)"),
            ("192.31.196.1", "AS112-v4"),
            ("192.52.193.1", "AMT"),
            ("192.88.99.2", "6to4 relay anycast (deprecated)"),
            ("192.168.0.0", "private-use"),
            ("192.168.255.255", "private-use"),
            ("192.175.48.1", "direct delegation AS112 service"),
            ("198.18.0.0", "benchmarking"),
            ("198.19.255.255", "benchmarking"),
            ("198.51.100.7", "documentation (TEST-NET-2)"),
            ("203.0.113.7", "documentation (TEST-NET-3)"),
            ("224.0.0.1", "multicast"),
            ("239.255.255.255", "multicast"),
            ("240.0.0.0", "reserved (or the limited broadcast)"),
            ("255.255.255.255", "reserved (or the limited broadcast)"),
            ("::", "unspecified"),
            ("::ffff:10.1.2.3", "private-use"),
            ("::ffff:169.254.169.254", "link-local"),
            ("fc00::1", "unique-local"),
            ("fdff:ffff::1", "unique-local"),
            ("fe80::1", "link-local"),
            ("febf:ffff::1", "link-local"),
            ("ff02::1", "multicast"),
            ("64:ff9b::a00:1", "IPv4-IPv6 translation"),
            ("64:ff9b:1::1", "IPv4-IPv6 translation"),
            ("100::1", "outside global unicast (2000::/3)"),
            ("::a00:1", "outside global unicast (2000::/3)"),
            ("5f00::1", "outside global unicast (2000::/3)"),
            ("2001::1", "IETF protocol assignments"),
            ("2001:1ff:ffff::1", "IETF protocol assignments"),
            ("2001:db8::1", "documentation"),
            ("2002:a00:1::1", "6to4"),
            ("2620:4f:8000::1", "direct delegation AS112 service"),
            ("3fff:fff::1", "documentation"),
        ];
        for (address, name) in special {
            assert_eq!(of(address), Class::Special(name), "{address}");
        }
        for loopback in ["127.0.0.1", "127.255.255.254", "::1", "::ffff:127.0.0.1"] {
            assert_eq!(of(loopback), Class::Loopback, "{loopback}");
        }
        let global = [
            "1.1.1.1",
            "9.255.255.255",
            "11.0.0.0",
            "100.63.255.255",
            "100.128.0.0",
            "126.255.255.255",
            "128.0.0.0",
            "169.253.255.255",
            "169.255.0.0",
            "172.15.255.255",
            "172.32.0.0",
            "192.0.1.0",
            "192.0.3.0",
            "192.167.255.255",
            "192.169.0.0",
            "198.17.255.255",
            "198.20.0.0",
            "223.255.255.255",
            "::ffff:8.8.8.8",
            "2001:200::1",
            "2001:4860:4860::8888",
            "2600::1",
            "2620:4f:7fff::1",
            "2a00:1450:4001::1",
            "3fff:1000::1",
        ];
        for address in global {
            assert_eq!(of(address), Class::Global, "{address}");
        }
    }
}
