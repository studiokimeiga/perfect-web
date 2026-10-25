#!/usr/bin/env python3
"""Mutation controls for track outbound-https (ADR-XXXX): the host's one
outbound client, and the payments reaching their provider through it.

Each mutant undoes one piece:
- the grant: any origin reached, or one compared without its port or its
  scheme; a public origin over plain HTTP, or at a special address; a
  development origin at any address;
- the addresses: a resolved address not checked, or only the first; the
  first address taking the whole connection's bound; a development origin
  reaching global addresses; loopback, an IPv4-mapped
  address, 10/8, 172.16/12, 192.168/16, 169.254/16 (a cloud's metadata),
  100.64/10, 0/8, multicast, the reserved block, 6to4, or everything
  outside 2000::/3 taken for global;
- the certificates: https sent in the clear; the roots given ignored; an
  empty platform store taken; an expired, or wrongly named, certificate
  said as another refusal;
- redirects: none refused, or a 307 or 308 (which keep the method and the
  body) taken for an answer;
- the deadline: each read bounded, not the answer (the development
  provider's wait); a read's wait not cut to what is left; the deadline not
  checked before a read; connecting or the handshake bounded by the total
  alone; the exchange bounded by the handshake's bound; the resolver waited
  for past its wait, or running lookups without bound; a wait not held to
  a day;
- the bounds: a length one past the bound read; chunks bounded one at a
  time, or summed with what is held so that the sum wraps; their framing
  unbounded; the head bounded at four times its limit;
- the framing: an answer stating no length taken as empty; both lengths, or
  a length stated twice, taken; a coding other than `chunked` read as it; a
  coding in HTTP/1.0 taken; a cut body taken whole; bytes past the end
  taken; a chunk longer than its size taken; a bare LF or CR taken; a folded
  line, whitespace before a `:`, or a control in a value taken; interim
  answers unbounded; a switch of protocol passed over; a body read for 304;
  a content coding taken, or any asked for; a status out of range, or
  another protocol's status line, read; a redirect's error holding its whole
  location;
- the request: a value with a line break written; a secret's value
  unchecked or shown in its refusal; the client's own fields settable, or
  `accept-encoding` among them; a
  field name not a token; a body's length not stated; a target holding a
  line break written; credentials or a fragment in a URL refused as
  something else; an IPv4 address in another form read as a name; a host
  in any case; https's default port another; port 0 taken;
- the secret: a `Secret`'s `Debug` showing it; a request's `Debug` showing
  its body, or its path and query; an answer's `Debug` showing its body;
- the payments: their client granted nothing; their wait six times
  longer; their hearing reading through another client; a request the
  client refused taken for an unreachable provider; a payment's id, or an
  intent's, not checked as one segment of the provider's path.

A client mutant must fail pw-outbound's tests (`cargo test -p pw-outbound
--lib`); a payments mutant, the development server's tests of the payments
at an https origin and of the provider's answer (`payments::https_tests::`,
`payments::answer_tests::`). Each kill names the tests that failed. Only complete native results earn a kill: a
build that fails, a producer that times out, an inventory that differs from
the baseline's, or a failure anywhere but an assertion of a test source is
REFUSED, never a kill.

Run from the repository root; `just e14-outbound-https` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "runtime/pw-outbound/src"
LIB = SRC / "lib.rs"
ORIGIN = SRC / "origin.rs"
ADDRESS = SRC / "address.rs"
HTTP1 = SRC / "http1.rs"
TLS = SRC / "tls.rs"
RESOLVE = SRC / "resolve.rs"
SECRET = SRC / "secret.rs"
PAYMENTS = ROOT / "spikes/own-renderer/server/src/payments.rs"

CLIENT, SERVER = "client", "server"

# (what is undone, the suite that kills it, file, anchor, replacement)
MUTANTS = [
    # --- the grant ---------------------------------------------------------
    (
        "any origin is reached",
        CLIENT,
        LIB,
        "            .find(|(declared, _)| declared == origin)\n",
        "            .find(|_| true)\n",
    ),
    (
        "an origin is compared without its port",
        CLIENT,
        LIB,
        "            .find(|(declared, _)| declared == origin)\n",
        "            .find(|(declared, _)| {\n"
        "                declared.scheme() == origin.scheme() && declared.host() == origin.host()\n"
        "            })\n",
    ),
    (
        "an origin is compared without its scheme",
        CLIENT,
        LIB,
        "            .find(|(declared, _)| declared == origin)\n",
        "            .find(|(declared, _)| {\n"
        "                declared.host() == origin.host() && declared.port() == origin.port()\n"
        "            })\n",
    ),
    (
        "a public origin may be plain HTTP",
        CLIENT,
        LIB,
        "        if origin.scheme() != Scheme::Https {\n",
        "        if origin.scheme() != Scheme::Https && false {\n",
    ),
    (
        "a public origin may be a special address",
        CLIENT,
        LIB,
        "            Reach::Public.admits(*address).map_err(|class| {\n",
        "            Reach::Public\n"
        "                .admits(*address)\n"
        "                .or_else(|_| Ok::<(), &'static str>(()))\n"
        "                .map_err(|class| {\n",
    ),
    (
        "a development origin may be any address",
        CLIENT,
        LIB,
        "            Reach::Loopback.admits(*address).map_err(|class| {\n",
        "            Reach::Loopback\n"
        "                .admits(*address)\n"
        "                .or_else(|_| Ok::<(), &'static str>(()))\n"
        "                .map_err(|class| {\n",
    ),
    # --- the addresses -----------------------------------------------------
    (
        "a resolved address is not checked",
        CLIENT,
        LIB,
        "            reach.admits(*address).map_err(|class| Error::Address {\n",
        "            reach\n"
        "                .admits(*address)\n"
        "                .or_else(|_| Ok::<(), &'static str>(()))\n"
        "                .map_err(|class| Error::Address {\n",
    ),
    (
        "only the first resolved address is checked",
        CLIENT,
        LIB,
        "        for address in &named {\n",
        "        for address in named.iter().take(1) {\n",
    ),
    (
        "the first address takes the whole connection's bound",
        CLIENT,
        LIB,
        "        let share = left / (addresses.len() - tried) as u32;\n",
        "        let share = left / (addresses.len() - tried).min(1) as u32;\n",
    ),
    (
        "a development origin reaches global addresses",
        CLIENT,
        LIB,
        "            (Reach::Loopback, Class::Global) => {\n"
        "                Err(\"global (a development origin is this machine's)\")\n"
        "            }\n",
        "            (Reach::Loopback, Class::Global) => Ok(()),\n",
    ),
    (
        "loopback is global",
        CLIENT,
        ADDRESS,
        "        return Class::Loopback;\n    }\n    if in_v4(address, Ipv4Addr::new(224, 0, 0, 0), 4) {\n",
        "        return Class::Global;\n    }\n    if in_v4(address, Ipv4Addr::new(224, 0, 0, 0), 4) {\n",
    ),
    (
        "an IPv4-mapped address is read as IPv6",
        CLIENT,
        ADDRESS,
        "    if let Some(v4) = address.to_ipv4_mapped() {\n        return class_v4(v4);\n    }\n",
        "    let _ = address.to_ipv4_mapped();\n",
    ),
    (
        "10.0.0.0/8 is one address",
        CLIENT,
        ADDRESS,
        '    (Ipv4Addr::new(10, 0, 0, 0), 8, "private-use"),\n',
        '    (Ipv4Addr::new(10, 0, 0, 0), 32, "private-use"),\n',
    ),
    (
        "172.16.0.0/12 is a /16",
        CLIENT,
        ADDRESS,
        '    (Ipv4Addr::new(172, 16, 0, 0), 12, "private-use"),\n',
        '    (Ipv4Addr::new(172, 16, 0, 0), 16, "private-use"),\n',
    ),
    (
        "192.168.0.0/16 is a /24",
        CLIENT,
        ADDRESS,
        '    (Ipv4Addr::new(192, 168, 0, 0), 16, "private-use"),\n',
        '    (Ipv4Addr::new(192, 168, 0, 0), 24, "private-use"),\n',
    ),
    (
        "169.254.0.0/16, a cloud's metadata among it, is a /24",
        CLIENT,
        ADDRESS,
        '    (Ipv4Addr::new(169, 254, 0, 0), 16, "link-local"),\n',
        '    (Ipv4Addr::new(169, 254, 0, 0), 24, "link-local"),\n',
    ),
    (
        "100.64.0.0/10 is a /16",
        CLIENT,
        ADDRESS,
        '    (Ipv4Addr::new(100, 64, 0, 0), 10, "shared address space"),\n',
        '    (Ipv4Addr::new(100, 64, 0, 0), 16, "shared address space"),\n',
    ),
    (
        "0.0.0.0/8 is one address",
        CLIENT,
        ADDRESS,
        '    (Ipv4Addr::new(0, 0, 0, 0), 8, "\\"this network\\""),\n',
        '    (Ipv4Addr::new(0, 0, 0, 0), 32, "\\"this network\\""),\n',
    ),
    (
        "multicast is a /8",
        CLIENT,
        ADDRESS,
        "    if in_v4(address, Ipv4Addr::new(224, 0, 0, 0), 4) {\n",
        "    if in_v4(address, Ipv4Addr::new(224, 0, 0, 0), 8) {\n",
    ),
    (
        "the reserved block is a /8",
        CLIENT,
        ADDRESS,
        "    if in_v4(address, Ipv4Addr::new(240, 0, 0, 0), 4) {\n",
        "    if in_v4(address, Ipv4Addr::new(240, 0, 0, 0), 8) {\n",
    ),
    (
        "6to4, which embeds an IPv4 address, is a /32",
        CLIENT,
        ADDRESS,
        '    (Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16, "6to4"),\n',
        '    (Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 32, "6to4"),\n',
    ),
    (
        "every IPv6 address is in 2000::/3",
        CLIENT,
        ADDRESS,
        "    if !in_v6(address, Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 3) {\n",
        "    if !in_v6(address, Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 0) {\n",
    ),
    # --- the certificates --------------------------------------------------
    (
        "https is sent in the clear",
        CLIENT,
        LIB,
        "        match origin.scheme() {\n            Scheme::Http => exchange(&mut sock, &out, &self.limits),\n",
        "        match Scheme::Http {\n            Scheme::Http => exchange(&mut sock, &out, &self.limits),\n",
    ),
    (
        "the roots given are ignored",
        CLIENT,
        TLS,
        "            .with_root_certificates(roots.store.clone())\n",
        "            .with_root_certificates(RootCertStore::empty())\n",
    ),
    (
        "an empty platform store is taken",
        CLIENT,
        TLS,
        "        if taken == 0 {\n",
        "        if taken == usize::MAX {\n",
    ),
    (
        "an expired certificate is said as another refusal",
        CLIENT,
        TLS,
        "            CertificateError::Expired | CertificateError::ExpiredContext { .. } => Refusal::Expired,\n",
        "            CertificateError::Expired | CertificateError::ExpiredContext { .. } => {\n"
        "                Refusal::Other(\"expired\".into())\n"
        "            }\n",
    ),
    (
        "a certificate for another name is said as of an unknown issuer",
        CLIENT,
        TLS,
        "                Refusal::WrongName\n",
        "                Refusal::UnknownIssuer\n",
    ),
    # --- redirects ---------------------------------------------------------
    (
        "no redirect is refused",
        CLIENT,
        HTTP1,
        "    if matches!(status, 301 | 302 | 303 | 307 | 308) {\n",
        "    if matches!(status, 301 | 302 | 303 | 307 | 308) && false {\n",
    ),
    (
        "a 307 or 308, which keep the method and the body, is an answer",
        CLIENT,
        HTTP1,
        "    if matches!(status, 301 | 302 | 303 | 307 | 308) {\n",
        "    if matches!(status, 301..=303) {\n",
    ),
    # --- the deadline ------------------------------------------------------
    (
        "each read is bounded, not the answer",
        CLIENT,
        LIB,
        "    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {\n        let left = self.left()?;\n",
        "    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {\n"
        "        self.deadline = Instant::now() + Duration::from_secs(1);\n"
        "        let left = self.left()?;\n",
    ),
    (
        "a read waits as long as a minute, past the deadline",
        CLIENT,
        LIB,
        "        self.tcp.set_read_timeout(Some(left))?;\n",
        "        self.tcp\n            .set_read_timeout(Some(left.max(Duration::from_secs(60))))?;\n",
    ),
    (
        "the deadline is not checked before a read",
        CLIENT,
        LIB,
        "        let left = self.deadline.saturating_duration_since(Instant::now());\n"
        "        if left.is_zero() {\n"
        "            return Err(io::ErrorKind::TimedOut.into());\n",
        "        let left = self.deadline.saturating_duration_since(Instant::now());\n"
        "        if left.is_zero() && false {\n"
        "            return Err(io::ErrorKind::TimedOut.into());\n",
    ),
    (
        "connecting is bounded by the total alone",
        CLIENT,
        LIB,
        "            deadline.min(Instant::now() + self.limits.connect),\n",
        "            deadline.max(Instant::now() + self.limits.connect),\n",
    ),
    (
        "the handshake is bounded by the total alone",
        CLIENT,
        LIB,
        "                sock.deadline = deadline.min(Instant::now() + self.limits.handshake);\n",
        "                sock.deadline = deadline.max(Instant::now() + self.limits.handshake);\n",
    ),
    (
        "the exchange is bounded by the handshake's bound",
        CLIENT,
        LIB,
        "                sock.deadline = deadline;\n",
        "                sock.deadline = sock.deadline.min(deadline);\n",
    ),
    (
        "the resolver is waited for past its wait",
        CLIENT,
        RESOLVE,
        "        match answer.recv_timeout(within) {\n",
        "        match answer.recv_timeout(within.max(Duration::from_secs(5))) {\n",
    ),
    (
        "lookups running at once are not bounded",
        CLIENT,
        RESOLVE,
        "        if self.running.fetch_add(1, Ordering::SeqCst) >= MOST_LOOKUPS {\n",
        "        if self.running.fetch_add(1, Ordering::SeqCst) >= usize::MAX {\n",
    ),
    (
        "a wait is not held to a day",
        CLIENT,
        LIB,
        "            total: self.total.min(MOST_WAIT),\n",
        "            total: self.total,\n",
    ),
    # --- the bounds --------------------------------------------------------
    (
        "a length one past the bound is read",
        CLIENT,
        HTTP1,
        "    if length > limits.body as u64 {\n",
        "    if length > limits.body as u64 + 1 {\n",
    ),
    (
        "chunks are bounded one at a time, not together",
        CLIENT,
        HTTP1,
        "        if size > (limits.body - body.len()) as u64 {\n",
        "        if size > limits.body as u64 {\n",
    ),
    (
        "a chunk's size is summed with what is held, and the sum wraps",
        CLIENT,
        HTTP1,
        "        if size > (limits.body - body.len()) as u64 {\n",
        "        if (body.len() as u64).wrapping_add(size) > limits.body as u64 {\n",
    ),
    (
        "a chunked body's framing is unbounded",
        CLIENT,
        HTTP1,
        "    let mut framing = Budget::new(limits.body, \"body's chunk framing\");\n",
        "    let mut framing = Budget::new(usize::MAX, \"body's chunk framing\");\n",
    ),
    (
        "the head is bounded at four times its limit",
        CLIENT,
        HTTP1,
        "    let mut budget = Budget::new(limits.head, \"head\");\n",
        "    let mut budget = Budget::new(limits.head * 4, \"head\");\n",
    ),
    # --- the framing -------------------------------------------------------
    (
        "an answer stating no length is taken as empty",
        CLIENT,
        HTTP1,
        "    let length = match lengths.as_slice() {\n        [] => {\n",
        "    let length = match lengths.as_slice() {\n"
        "        [] => \"0\",\n"
        "        #[allow(unreachable_patterns)]\n"
        "        [] => {\n",
    ),
    (
        "an answer stating both lengths is read by its chunks",
        CLIENT,
        HTTP1,
        "        if !lengths.is_empty() {\n",
        "        if !lengths.is_empty() && false {\n",
    ),
    (
        "a length stated twice is taken",
        CLIENT,
        HTTP1,
        "        _ => {\n"
        "            return Err(Error::Framing(\n"
        "                \"the answer states its length more than once\".into(),\n"
        "            ));\n"
        "        }\n",
        "        _ => lengths[0],\n",
    ),
    (
        "a coding other than chunked is read as chunked",
        CLIENT,
        HTTP1,
        "        if codings.len() != 1 || !codings[0].eq_ignore_ascii_case(\"chunked\") {\n",
        "        if codings.len() != 1 {\n",
    ),
    (
        "a transfer coding in HTTP/1.0 is taken",
        CLIENT,
        HTTP1,
        "        if version == Version::Http10 {\n",
        "        if version == Version::Http10 && false {\n",
    ),
    (
        "a body cut short is taken whole",
        CLIENT,
        HTTP1,
        "                return Err(Error::Framing(format!(\n"
        "                    \"the connection closed {} bytes into a body of {n}\",\n"
        "                    n - want\n"
        "                )));\n",
        "                return Ok(());\n",
    ),
    (
        "bytes past the answer's end are taken",
        CLIENT,
        HTTP1,
        "    if input.buffered() > 0 {\n",
        "    if input.buffered() > 0 && false {\n",
    ),
    (
        "a chunk longer than its size is taken",
        CLIENT,
        HTTP1,
        "        if !input.line(&mut framing)?.is_empty() {\n",
        "        if !input.line(&mut framing)?.is_empty() && false {\n",
    ),
    (
        "a bare LF ends a line",
        CLIENT,
        HTTP1,
        "                if line.pop() != Some(b'\\r') {\n"
        "                    return Err(Error::Framing(\"a line ended by a bare LF\".into()));\n"
        "                }\n",
        "                if line.last() == Some(&b'\\r') {\n"
        "                    line.pop();\n"
        "                }\n",
    ),
    (
        "a bare CR inside a line is taken",
        CLIENT,
        HTTP1,
        "                if line.contains(&b'\\r') {\n",
        "                if line.contains(&b'\\r') && false {\n",
    ),
    (
        "a folded line is refused as another error",
        CLIENT,
        HTTP1,
        "    if matches!(line.first(), Some(b' ' | b'\\t')) {\n",
        "    if matches!(line.first(), Some(b' ' | b'\\t')) && false {\n",
    ),
    (
        "whitespace before a field's `:` is taken",
        CLIENT,
        HTTP1,
        "    if !is_token(name) {\n",
        "    if !is_token(name.trim_ascii_end()) {\n",
    ),
    (
        "a control in a field's value is taken",
        CLIENT,
        HTTP1,
        "        .any(|b| (*b < 0x20 && *b != b'\\t') || *b == 0x7f)\n",
        "        .any(|b| *b == 0x7f)\n",
    ),
    (
        "interim answers are taken without end",
        CLIENT,
        HTTP1,
        "                if interim > MOST_INTERIM {\n",
        "                if interim > MOST_INTERIM * 2 {\n",
    ),
    (
        "a switch of protocol is passed over as an interim answer",
        CLIENT,
        HTTP1,
        "            101 => {\n",
        "            101 if false => {\n",
    ),
    (
        "a body is read for a 304",
        CLIENT,
        HTTP1,
        "    let body = if status == 204 || status == 304 {\n",
        "    let body = if status == 204 {\n",
    ),
    (
        "a content coding is taken",
        CLIENT,
        HTTP1,
        "        .find(|c| !c.eq_ignore_ascii_case(\"identity\"))\n",
        "        .find(|c| c.is_empty())\n",
    ),
    (
        "any content coding is asked for",
        CLIENT,
        HTTP1,
        "    head.extend_from_slice(b\"accept-encoding: identity\\r\\nconnection: close\\r\\n\\r\\n\");\n",
        "    head.extend_from_slice(b\"connection: close\\r\\n\\r\\n\");\n",
    ),
    (
        "a redirect's error holds its whole location",
        CLIENT,
        HTTP1,
        "                .and_then(|l| crate::origin::origin_of(l)),\n",
        "                .map(|l| l.to_string()),\n",
    ),
    (
        "a status out of range is read",
        CLIENT,
        HTTP1,
        "    if !(100..=599).contains(&status) {\n",
        "    if !(0..=999).contains(&status) {\n",
    ),
    (
        "another protocol's status line is read as HTTP/1.1",
        CLIENT,
        HTTP1,
        "        Some(b\"HTTP/1.0 \") => Version::Http10,\n        _ => return Err(bad()),\n",
        "        Some(b\"HTTP/1.0 \") => Version::Http10,\n        _ => Version::Http11,\n",
    ),
    # --- the request -------------------------------------------------------
    (
        "a value with a line break is written",
        CLIENT,
        HTTP1,
        "            .all(|b| b == b'\\t' || (0x20..0x7f).contains(&b))\n",
        "            .all(|b| b != 0)\n",
    ),
    (
        "a secret's value is not checked",
        CLIENT,
        HTTP1,
        "        if !is_value(secret.expose()) {\n",
        "        if !is_value(secret.expose()) && false {\n",
    ),
    (
        "a secret's refusal shows its value",
        CLIENT,
        HTTP1,
        "                 outside ASCII, or a space at an end (not shown)\"\n",
        "                 outside ASCII, or a space at an end: {}\",\n"
        "                secret.expose()\n",
    ),
    (
        "the client's own fields are settable in another case",
        CLIENT,
        HTTP1,
        "    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name)) {\n",
        "    if RESERVED.iter().any(|r| *r == name) {\n",
    ),
    (
        "accept-encoding is the request's to set",
        CLIENT,
        HTTP1,
        "    \"accept-encoding\",\n",
        "    \"x-accept-encoding\",\n",
    ),
    (
        "a field's name need not be a token",
        CLIENT,
        HTTP1,
        "    if !is_token(name.as_bytes()) {\n",
        "    if name.is_empty() {\n",
    ),
    (
        "a body's length is not stated",
        CLIENT,
        HTTP1,
        "        field(\"content-length\", &body.len().to_string());\n",
        "        field(\"x-length\", &body.len().to_string());\n",
    ),
    (
        "a target holding a line break is written",
        CLIENT,
        ORIGIN,
        "                return Err(Error::Request(\n"
        "                    \"the path or query holds a character a URI does not: a space, a control, \\\n",
        "                let _ = Err::<(), Error>(Error::Request(\n"
        "                    \"the path or query holds a character a URI does not: a space, a control, \\\n",
    ),
    (
        "credentials in a URL are refused as another error",
        CLIENT,
        ORIGIN,
        "    if authority.contains('@') {\n",
        "    if authority.contains('@') && false {\n",
    ),
    (
        "a fragment in a URL is refused as another error",
        CLIENT,
        ORIGIN,
        "    if after.contains('#') {\n",
        "    if after.contains('#') && false {\n",
    ),
    (
        "an IPv4 address in another form is read as a name",
        CLIENT,
        ORIGIN,
        "    if numeric {\n",
        "    if numeric && false {\n",
    ),
    (
        "a host is compared in its case",
        CLIENT,
        ORIGIN,
        "    let host = text.to_ascii_lowercase();\n",
        "    let host = text.to_string();\n",
    ),
    (
        "https's default port is another",
        CLIENT,
        ORIGIN,
        "            Scheme::Https => 443,\n",
        "            Scheme::Https => 8443,\n",
    ),
    (
        "port 0 is taken",
        CLIENT,
        ORIGIN,
        "        Ok(port) if port > 0 => Ok(port),\n",
        "        Ok(port) => Ok(port),\n",
    ),
    # --- the secret --------------------------------------------------------
    (
        "a secret's Debug shows it",
        CLIENT,
        SECRET,
        "        f.write_str(\"Secret(..)\")\n",
        "        write!(f, \"Secret({})\", self.0)\n",
    ),
    (
        "a request's Debug shows its body",
        CLIENT,
        LIB,
        "            .field(\"body\", &self.body.as_ref().map(Vec::len))\n",
        "            .field(\n"
        "                \"body\",\n"
        "                &self.body.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()),\n"
        "            )\n",
    ),
    (
        "a request's Debug shows its path and query",
        CLIENT,
        LIB,
        "        let origin = origin::origin_of(&self.url);\n",
        "        let origin = Some(self.url.clone());\n",
    ),
    (
        "an answer's Debug shows its body",
        CLIENT,
        LIB,
        "            .field(\"body\", &self.body.len())\n",
        "            .field(\"body\", &String::from_utf8_lossy(&self.body))\n",
    ),
    # --- the payments ------------------------------------------------------
    (
        "the payments' client is granted nothing",
        SERVER,
        PAYMENTS,
        "    let grant = outbound::Grant::none()\n"
        "        .development(provider)\n"
        "        .map_err(|e| e.to_string())?;\n",
        "    let grant = outbound::Grant::none();\n    let _ = provider;\n",
    ),
    (
        "the payments wait for their provider six times longer",
        SERVER,
        PAYMENTS,
        "        total: PROVIDER_WAIT,\n        ..outbound::Limits::default()\n",
        "        total: PROVIDER_WAIT * 6,\n        ..outbound::Limits::default()\n",
    ),
    (
        "a request the client refused is taken for an unreachable provider",
        SERVER,
        PAYMENTS,
        "            Err(e) if unsent(&e) => Err(format!(\"the payment was not begun: {e}\")),\n",
        "",
    ),
    (
        "a payment's id is not checked as one segment of the provider's path",
        SERVER,
        PAYMENTS,
        "        let payment = segment(payment)?;\n",
        "        let payment = payment.as_str();\n",
    ),
    (
        "an intent's id is not checked as one segment of the provider's path",
        SERVER,
        PAYMENTS,
        "        let intent = segment(intent).map_err(NotActed::Unreadable)?;\n",
        "        let intent: &str = intent;\n",
    ),
    (
        "the payments' hearing reads through another client",
        SERVER,
        PAYMENTS,
        "            hearing: Hearing::new(secret, provider, client.clone()),\n",
        "            hearing: Hearing::new(\n"
        "                secret,\n"
        "                provider,\n"
        "                Client::new(outbound::Grant::none(), outbound::Roots::none())\n"
        "                    .expect(\"a client\"),\n"
        "            ),\n",
    ),
]

CLIENT_TESTS = [SRC / name for name in ("address.rs", "http1.rs", "origin.rs", "resolve.rs", "tls.rs", "tests.rs")]

# Each suite: its command, and the sources whose assertions may kill.
SUITES = {
    CLIENT: (
        ["cargo", "test", "--quiet", "--locked", "-p", "pw-outbound", "--lib"],
        CLIENT_TESTS,
    ),
    SERVER: (
        ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server",
         "--", "payments::https_tests::", "payments::answer_tests::"],
        [PAYMENTS],
    ),
}
INVENTORY = {}


def run_tests(suite):
    """(valid, passed, failed, the failing tests): a complete named
    inventory, failures only at a test's assertions."""
    cmd, sources = SUITES[suite]
    full = [*cmd, "--format=pretty"] if "--" in cmd else [*cmd, "--", "--format=pretty"]
    try:
        child = producer_process.run(full, cwd=ROOT, timeout=900)
    except subprocess.TimeoutExpired as expired:
        print(expired.output, flush=True)
        return False, 0, 0, ()
    result = cargo_assertion_results.validate(
        child.returncode, child.stdout, root=ROOT, sources=sources,
        expected=INVENTORY.get(suite))
    if not result.valid:
        print(child.stdout[-4000:], flush=True)
        return False, 0, 0, ()
    INVENTORY.setdefault(suite, cargo_assertion_results.names(child.stdout))
    return True, result.passed, result.failed, result.failure_names


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    watch = mutation_baseline.mutation_bound.start()
    for suite in (CLIENT, SERVER):
        built, passed, failed, _ = run_tests(suite)
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed or watch.stopped:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, suite, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            built, passed, failed, killers = run_tests(suite)
        if not built or watch.stopped:
            verdict = "REFUSED (invalid native assertion producer)"
            survivors += 1
        elif failed:
            verdict = f"KILLED by {', '.join(killers)} ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
