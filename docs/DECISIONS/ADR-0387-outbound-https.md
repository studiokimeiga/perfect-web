# ADR-0387: the host reaches another origin through one client, and only an origin it declares

Status: accepted by the integrator under the owner's delegation of
2026-10-02; proposed by track `outbound-https` (2026-10-25) under the
integrator's ruling of 2026-10-25, held to two independent reviews (the
second's findings and their fixes: "Independent reviews (2026-10-25)") and
a third before it lands, numbered ADR-0387; it lands by pull request.
Milestone: E14. Builds on ADR-0006 and ADR-0026
(a capability authorizes an operation the host provides), ADR-0332 (a
payment is the provider's word; step eight's wait and read by length),
ADR-0333 (what a page may run) and the dating-app note's ruling 6 (one
`outbound-https` platform track for NEXT 6d, H7, H11 and the production
track's OIDC).

## Context

- **One outbound call existed, over plain HTTP, with an HTTP of its own.**
  `payments.rs` reached its provider with `provider_stream` and `answer_of`
  over a raw `TcpStream`: `http://` only. Nothing else in the host calls
  another origin; the recommender is simulated in process, and the identity
  providers are development ones (ADR-0258).
- **It was unbounded where it read** (found by this track, reading it):
  `answer_of` allocated what the provider's `Content-Length` said
  (`body.resize(length, 0)`), read to the connection's close with no bound
  where no length was stated, and read a head line with no bound; and
  `PROVIDER_WAIT` bounded each read, not the answer, so a provider dripping
  its answer, a byte inside each wait, held the reader's command, and the
  host's layer with it, without end. A provider, or anything answering at
  its address, could exhaust the host's memory or hold it.
- **What waits on it**: NEXT 6d (a Stripe adapter in test mode), H7 (Web
  Push), H11 (verification providers), the production track's Google OIDC,
  and webhooks out: each a platform service calling a provider over TLS.
- **The integrator's ruling** (2026-10-25): one outbound HTTPS client in the
  host, behind the capability a program declares; the host holds TLS, keys
  and secrets, and a program never holds a secret; rustls with the *ring*
  provider and platform or vetted roots; no HTTP framework unless the host's
  own HTTP/1.1 over a rustls stream is shown worse.
- **Practice**, each read 2026-10-25:
  - RFC 9112 §6.3: a message with both `Transfer-Encoding` and
    `Content-Length` "might indicate an attempt to perform request smuggling
    ... and ought to be handled as an error"; a body cut short of its
    `Content-Length` is incomplete; and "there is no way to distinguish a
    successfully completed, close-delimited response message from a
    partially received message interrupted by network failure". §2.2: a
    bare CR is invalid. §5.2: a user agent receiving an obs-fold "MUST
    replace each received obs-fold with one or more SP octets".
  - Fetch: "A redirect status is a status that is 301, 302, 303, 307, or
    308."
  - IANA's IPv4 and IPv6 Special-Purpose Address Registries (their CSV): the
    blocks, and which are globally reachable.
  - rustls 0.23.46's release notes: four advisories fixed
    (GHSA-h2v8-394m-p7vv, GHSA-c8m5-c952-6cm4, GHSA-w44r-93j7-qv28,
    GHSA-vp88-798v-h67f, the last in 0.23.44-45); rustls-webpki 0.103.16's:
    "Distrust a small set of intermediate certificate subject public keys".
    The *ring* provider's key exchanges are X25519, P-256 and P-384
    (`crypto/ring/mod.rs`, `ALL_KX_GROUPS`).
  - rustls-native-certs 0.8.4: the operating system's store, or the files
    `SSL_CERT_FILE` and `SSL_CERT_DIR` name; `CertificateResult` holds what
    it found and what failed.
  - httparse, hyper's parser, refuses a folded field line in a response
    unless a client sets `allow_obsolete_multiline_headers_in_responses`
    (docs.rs).

## Decision

1. **One client, `runtime/pw-outbound`.** Every platform service that calls
   another origin calls it through a `pw_outbound::Client`; the payments do
   now (decision 10), and NEXT 6d, H7, H11 and OIDC are to. No other host
   code opens a connection to another origin (the development provider's
   own webhook to the host is the provider's, not the host's).
2. **A grant is the capability, and a client holds one.** A `Client` is made
   from a `Grant`: the origins the deployment declares for one service, each
   a scheme, a host and a port, compared whole (`https://API.example.com:443`
   is `https://api.example.com`; another port, scheme or host is another
   origin). A request to an origin its grant does not declare is refused
   (`Error::Undeclared`) before anything is resolved, connected or written.
   A program's capability authorizes the host's operation (ADR-0026): the
   store's `payments.begin` authorizes `pw:host/payments#begin`, which the
   host performs with a client granted the provider's origin alone, that the
   deployment names; the program names no URL and holds no key.
   - **A public origin** (`Grant::public`) is `https`, and every address it
     names is global unicast.
   - **A development origin** (`Grant::development`), which only a
     development deployment names, may be `http`, and every address it names
     is a loopback one: the development provider's, on this machine.
3. **A name is resolved once, every address it names is checked, and a
   checked one is connected to.** An address in any block of IANA's
   special-purpose registries (globally reachable or not, as no provider is
   reached at an anycast service's address), multicast, or, in IPv6, outside
   2000::/3, is refused for a public origin; an IPv4-mapped IPv6 address is
   the IPv4 address it maps. One refused address refuses the name, whatever
   the others are. So a name rebound to loopback, a private network or
   a cloud's metadata service (169.254.169.254) reaches nothing, and nothing
   is resolved again between the check and the connection: a name whose
   answer changes after its first lookup is looked up once, and its first,
   checked answer is the one reached. The addresses
   are tried in the resolver's order, each given its share of what is left
   of the connection's bound, so one that never answers (an unrouted IPv6
   address before a working IPv4 one) does not take the others' time. The
   system's resolver runs each lookup on a thread of its own, so its wait is
   bounded too, and at most 32 run at once across all system resolvers in
   this process, the next
   refused: a resolver that never answers leaves no thread per request.
4. **TLS is rustls 0.23.46 with the *ring* provider**: its safe default
   versions (TLS 1.3 and 1.2), ALPN `http/1.1`, SNI for a name (a server
   holding several sites at one address presents the origin's), an address
   verified as an IP SAN, no client certificate, no early data, no key log.
   A certificate is verified by rustls-webpki against the client's
   `Roots`, and a refusal says what it is (`Refusal::Expired`,
   `NotValidYet`, `WrongName`, `UnknownIssuer`).
   - **The roots are the platform's** (`Roots::platform`,
     rustls-native-certs): the deployment's operator keeps them, a
     distrusted root is removed and a private authority added by the
     operating system's updates and configuration, without a rebuild of the
     host; and they bring no licence beyond ISC (decision 11). A store
     holding no usable certificate is refused, so a host that has none
     fails at its start, not at its first call. A test gives its own
     authority's root (`Roots::only`); a client of plain-HTTP development
     origins alone needs none (`Roots::none`, which trusts nothing).
5. **No redirect is followed.** Fetch's redirect statuses (301, 302, 303,
   307, 308) are `Error::Redirected`, with the origin they point to (never
   the path or query, which may echo the request's); following one would be
   a new request, held to the grant like any other. So no answer
   takes the client, or a secret it sent, to another origin, and a 307 or
   308 never resends a body anywhere.
6. **One deadline bounds the whole exchange** (`Limits::total`, ten seconds
   by default): resolving, connecting (`Limits::connect`, five), the
   handshake (`Limits::handshake`, five), writing and reading are each
   within it, and every read and write on the socket waits only for what is
   left of it. A peer that drips its answer is given up at the total, and so
   is one that takes the connection and never reads a request larger than
   the sockets' buffers. No
   wait is longer than a day (`MOST_WAIT`): a longer one is held to it, so
   no deadline overflows the clock.
7. **An answer is bounded**: its head (64 KiB, its interim answers among
   it, 128 fields at most) and its body (1 MiB, a chunked body's framing to
   as many bytes again). A stated length past the bound is refused before a
   byte of it is read or allocated; one within it is the peer's word, so at
   most 64 KiB is set aside before its bytes come, and the buffer grows as
   they do (under a bound set high, a lying `Content-Length` would otherwise
   panic or abort the process). A chunk's size is held to what is
   left of the bound, never summed with what is held (a size near 2^64
   would wrap the sum).
8. **HTTP/1.1 is the host's own** (RFC 9112), strictly: one request per
   connection, `Connection: close`; an answer is read by its stated length
   or its chunks, never to the connection's close, and as soon as it is
   whole (ADR-0332, step eight). Refused: both lengths at once, a length
   stated twice or as a list, a transfer coding other than `chunked` (or
   any in HTTP/1.0), a body cut short, bytes past the answer's end, a chunk
   whose data does not end, with its CRLF, where its size says (longer or
   shorter than its size, said as such), a trailer that is no field line, a
   bare CR or LF, a folded field line, whitespace
   before a `:`, a control in a value, a content coding the client never
   asked for, a switch of protocol, more than eight interim answers, and a
   status line of another protocol or out of 100 to 599. Every request
   asks for no content coding (`accept-encoding: identity`; RFC 9110 §12.5.3
   lets a server choose any where the field is absent). A request's field,
   a secret's as any other,
   is a token and a value of visible ASCII, spaces and tabs (no CR or LF, so
   no field ends its line), never one of the client's own (`Host`,
   `Accept-Encoding`, the framing, the connection's); its target is a path
   and a query of URI characters. Three refusals are stricter than RFC 9112 allows a user
   agent: an obs-fold, which §5.2 has it unfold (as httparse refuses one by
   default); a close-delimited answer, which §6.3 reads to the close, and a
   transfer coding whose last is not `chunked`, also read to the close
   there; none is in an answer a provider this host calls sends.
9. **A secret is written to the connection and nowhere else.** A `Secret`
   (a key, a bearer token, a client secret) has no `Display`, its `Debug`
   says only that it is one, and no code outside the crate can read it
   back, so no host code can put it in a program's `Val`, an error or a log
   line. The client logs nothing. No error holds a request's header value,
   body, path or query, and a URL's error never quotes the URL. An
   answer's status line is never quoted (a server echoing the request would
   make it the request line, its path and query among it); an error quotes
   at most 32 characters of an answer's field value (a coding, a length)
   or of a request's field name that is no token, each control, format or
   combining character escaped (`char::escape_debug`), so no peer writes a
   line, or a terminal's command, of its own into a log. A
   `Request`'s `Debug` shows its URL's origin alone (a webhook's path may be
   a secret), its headers, its secrets' names and its body's length; an
   `Answer`'s, its status, its fields' names and its body's length (a
   provider's answer may hold a payment's client secret or a token).
10. **The payments reach their provider through it.** `payments.rs`'s own
    HTTP is gone: `Payments` holds a client granted the provider's origin
    alone (`development_client`: a development origin, plain HTTP, no
    roots, within `PROVIDER_WAIT`, now the whole exchange's bound), and
    `begin`, `cancel` and the hearing's read of an intent all go through
    it. A request the client refuses before sending it (a field or a target
    that would split it, an origin not granted) traps the command: it says
    the host's or the program's mistake, and `TransportFailed` would say the
    provider could not be reached. A payment's id, and an intent's a signed
    event names, are each one segment of the provider's path (letters,
    digits, `_`, as Stripe's are), so no id makes another request of the one
    meant. The development provider's client is made before the provider
    starts, so a port no origin can name (0) is refused at start. A
    provider at an https origin is reached over TLS by the same code
    (tested with the development provider behind a TLS front); a
    deployment's provider at a public origin differs in its grant
    (`Grant::public`) and its roots (`Roots::platform`) alone: NEXT 6d's
    configuration.
11. **Dependencies** (crates.io, 2026-10-25; `cargo deny check` admits each,
    ISC added to `deny.toml` with its reason, and to the technology matrix):

    | Crate | Version | Published | Licence | Bytes |
    | --- | --- | --- | --- | --- |
    | `rustls` | 0.23.46 | 2026-10-14 | Apache-2.0 OR ISC OR MIT | 386,227 |
    | `ring` | 0.17.14 | 2025-03-11 | Apache-2.0 AND ISC | 1,502,610 |
    | `rustls-webpki` | 0.103.16 | 2026-10-20 | ISC | 89,984 |
    | `rustls-pki-types` | 1.15.1 | 2026-07-23 | MIT OR Apache-2.0 | 39,322 |
    | `rustls-native-certs` | 0.8.4 | 2026-06-01 | Apache-2.0 OR ISC OR MIT | 22,129 |
    | `openssl-probe` | 0.2.1 | 2026-01-20 | MIT OR Apache-2.0 | 9,155 |
    | `untrusted` | 0.9.0 | 2021-07-13 | ISC | 14,447 |
    | `subtle` | 2.6.1 | 2024-06-24 | BSD-3-Clause | 14,562 |
    | `zeroize` | 1.9.1 | 2026-10-06 | Apache-2.0 OR MIT | 25,043 |

    2,103,479 bytes on Linux. In the lock for other targets, never built
    here: macOS's `security-framework` 3.7.0, `security-framework-sys`
    2.17.0, `core-foundation` 0.10.1, `core-foundation-sys` 0.8.7 (MIT OR
    Apache-2.0); Windows's `schannel` 0.1.29 (MIT); and, for *ring* on
    aarch64 Windows alone, `windows-sys` 0.52.0 with `windows-targets`
    0.52.6 and its eight target crates (MIT OR Apache-2.0). A version under
    two weeks old was taken only for a security fix (`rustls`,
    `rustls-webpki`); otherwise the newest older than that
    (`rustls-native-certs` 0.8.4, not 0.8.5; `schannel` 0.1.29, not
    0.1.30). The tests add no crate from crates.io: their certificate
    authority and servers are a workspace crate of their own,
    `runtime/pw-outbound-testkit` (DER written over *ring*, servers over
    rustls), which each user takes as a dev-dependency alone (the
    integrator's ruling, 2026-10-25), so no crate the host ships holds a
    test authority; and the connection's bound uses `libc`, already in the
    graph.

## Alternatives

- **reqwest or hyper**: an async runtime (tokio) and an HTTP framework for
  a host whose callers are synchronous host functions; dozens of crates;
  redirects followed and proxies read from the environment by default. The
  host's needs are one request per connection to a few providers, read
  strictly: about 450 lines of code here (`http1.rs`; the whole client
  about 1,500), every rule tested and each guarded by a mutant.
- **ureq** (blocking, smaller): still its own redirect following and proxy
  handling to turn off, and the address checked between resolving and
  connecting would need its resolver and connector replaced: most of this
  crate again, beside its graph.
- **`webpki-roots`** (Mozilla's roots, compiled in): the same roots
  everywhere, but a distrusted root stays trusted until the host is rebuilt
  and redeployed, an operator cannot add a private authority, and its
  licence, CDLA-Permissive-2.0, is one more to admit. The platform's store
  keeps trust the operator's; a machine without one fails at start.
- **aws-lc-rs**, rustls's default provider: post-quantum key exchange, but a
  C and assembly build; the ruling named *ring*.
- **Following redirects to declared origins**: each would need its own
  checks (a method changed, a body resent, a secret header dropped across
  origins), for providers that never redirect; refusing them is one rule.
- **Happy Eyeballs** (RFC 8305, addresses raced): a thread or a poller per
  attempt for a host that calls a few providers; each address's share of
  the bound keeps one dead address from taking the rest's time.
- **Kept-alive connections**: a pool, a stale connection, and the retry of
  a request that may have been applied, for providers called a few times a
  minute; TLS sessions are resumed instead (rustls's in-memory cache, one
  per client). H7's volume may want one, measured first.
- **`rcgen` for the tests' certificates**: tried; its weak dependency
  features put 25 crates into `Cargo.lock` (the `x509-parser` family and
  `time`'s), 7 of them built. The DER a test certificate needs is a page of
  the testkit's, over *ring*, and every negative test has a
  positive neighbour from the same writer, so a writer's mistake fails a
  test rather than passing one.
- **Test certificates committed**: private key material in the repository
  (charter §3.7); the authority is made for each run instead.
- **The test authority as a `test-ca` feature of `pw-outbound`** (the
  track's first form): a dependent's tests turned the feature on, so the
  production crate held a module of test servers and an authority that any
  build could enable by a feature, in the same graph as the host. The
  integrator's ruling (2026-10-25): a dev-only crate instead.
- **Checking addresses at connect time only, by the connector**: the same
  check, but a resolver's second answer could differ from the first; one
  resolution, checked whole, is what is connected to.

## Structural guarantees and their evidence

Each is a test, and a mutant the test kills (`scripts/outbound_https_mutations.py`).

| Guarantee | Tests | Mutants |
| --- | --- | --- |
| Only a declared origin, compared whole | `an_undeclared_origin_is_never_reached`, `a_grant_holds_its_origins_to_their_reach`, `origin::tests` | any origin; no port; no scheme; public over http; public or development at a special address; host case; default port; port 0; another IPv4 form; credentials, fragment |
| Addresses checked, every one, before any is reached, and a name looked up once | `a_public_origin_rebound_to_loopback_reaches_nothing`, `a_name_is_looked_up_once_and_its_checked_answer_reached` (Linux), `a_public_origin_reaches_no_special_address`, `every_address_is_checked_before_any_is_reached`, `a_development_origin_reaches_this_machine_alone`, `an_address_that_never_answers_leaves_the_next_its_share` (Linux), `address::tests` | not checked; first only; the name resolved again before connecting; the first taking the whole bound; development reaching global; loopback global; mapped read as IPv6; 10/8, 172.16/12, 192.168/16, 169.254/16, 100.64/10, 0/8, multicast, reserved, 6to4 narrowed; all of IPv6 global |
| Certificates verified | `an_expired_certificate_is_refused`, `a_certificate_not_yet_valid_is_refused`, `a_certificate_for_another_host_is_refused`, `a_certificate_of_no_given_root_is_refused`, `the_platforms_roots_do_not_trust_a_tests_authority`, `an_address_origin_is_verified_by_its_address`, `a_name_is_sent_by_sni`, `tls::tests` | https in the clear; roots ignored; empty store taken; no SNI; refusals misnamed |
| No redirect followed | `a_redirect_is_never_followed`, `http1::tests::a_redirect_is_refused` | none refused; 307 and 308 answers; the error holding the whole location |
| One deadline | `a_handshake_never_begun_is_given_up`, `an_answer_never_sent_is_given_up`, `an_answer_dripped_is_given_up_at_the_total`, `the_exchange_has_the_rest_of_the_total`, `a_connection_never_accepted_is_given_up` (Linux), `a_write_to_a_peer_that_never_reads_is_given_up`, `no_wait_is_longer_than_a_day`, `resolve::tests`, payments' `a_provider_that_does_not_answer_in_time_is_not_reached` | each read bounded, not the answer; a write's wait unbounded; a read waiting a minute; the deadline unchecked; connect and handshake by the total alone; the exchange by the handshake's bound; the resolver past its wait, or lookups unbounded; a wait not held to a day; the payments' wait six times longer |
| Bounded sizes | `an_answer_is_bounded`, `a_stated_length_is_not_allocated_before_its_bytes`, `a_length_past_the_bound_is_refused_unread`, `a_head_past_the_bound_is_refused` | a length one past; a stated length allocated before its bytes; chunks bounded singly, or summed so the sum wraps; chunk framing unbounded; head at four times |
| HTTP/1.1 framing | `http1::tests` (lengths, chunks and their trailers, each refusal's reason, codings, grammar, interim), `the_framing_is_held_over_tls`, `an_answer_held_open_is_read_by_its_length` | a mutant per rule, `accept-encoding: identity` among them; a chunk's end not read, any two bytes taken for it, or a cut there taken; a trailer unchecked |
| A request's fields cannot split it | `no_field_can_split_a_request`, `a_field_that_would_split_the_request_is_never_sent`, `a_requests_head_is_written_whole` | a line break written; a secret's value, or its name, unchecked; reserved fields in another case, or `accept-encoding` not reserved; a non-token name; no length; a target with a line break |
| Secrets said nowhere | `a_secret_is_sent_and_said_nowhere`, `an_echoed_request_is_quoted_in_no_error`, `an_error_quotes_an_answer_short_and_escaped`, `a_urls_error_never_quotes_it`, `no_field_can_split_a_request` | a secret's `Debug`; a request's body, or its path and query, in its `Debug`; an answer's body, or its fields' values, in its; a secret in its refusal; a status line quoted; a quoted value unescaped, or not cut short |
| The payments through it | payments' `https_tests` (an https provider; an untrusted one; a split interaction; an id that is not one; the grant) and `answer_tests`, the store's payments in memory | granted nothing; waiting longer; the hearing through another client; a refusal taken for an unreachable provider; a payment's id or an intent's unchecked |

## Acceptance

**Historical evidence, predating the review fixes:** recorded by
`just e14-outbound-https` in `docs/evidence/E14/outbound-https.txt`:
`runtime/pw-outbound`'s 46 tests; the development server's 36 payments
tests (the store's among them, in memory; seven of them of the payments
at an https origin and of the provider's answer); and the mutation controls
(`scripts/outbound_https_mutations.py`, 88 mutants: 82 of the client, 6 of
the payments; each kill named by its tests): 88 of 88 killed, recorded at
the track's merge of master (90810781). The scripts the change reaches
(`ci_plan.py --changed`), each run whole on this track: `payments` (39),
`invoked_by` (14), `each_heads` (12; reached only because the planner reads
`resolve` as pw-core's module of that name). `cargo deny check` (licences
and advisories) passed in the `ci` workflow at the track's first push.

The current working source has 55 client tests and 11 focused provider
answer/HTTPS tests, and 111 mutation controls. Focused baseline tests pass;
no new whole-campaign result is claimed until the final source commit
finishes hosted verification and its eligible evidence is fetched.

## Not claimed

- **Refusing this host's own public addresses**: a globally routable address
  assigned to this machine passes the public-address policy. A permitted
  provider name resolving there can connect if TLS also authenticates it;
  the address classifier is not a local-interface or deployment topology
  policy. Loopback/private/special addresses are refused as stated above.

- **A program's own `network.fetch` naming its origins** (the ruling's
  "the capability a program declares"): no program fetches a real origin
  (the recommender is simulated in process), and the effect takes no origin
  today. The client is ready for one (a grant is a list of origins). The
  integrator's ruling (below): a program will name an origin by an `origin`
  declaration carried in its effect row (`network.fetch<Stripe>`), on a
  track of its own.
- **A provider at a public origin, configured**: no deployment names one
  yet. NEXT 6d's `PW_PAYMENTS=stripe` is the first (below).
- **Post-quantum key exchange**: *ring* has none (X25519, P-256, P-384), so
  a recorded session's key exchange is classical. aws-lc-rs has it; a
  provider change is a ruling of its own.
- **Revocation**: no CRL or OCSP is checked; a revoked certificate is taken
  until it expires (rustls-webpki checks revocation only against CRLs it is
  given).
- **Proxies**: `HTTPS_PROXY` and its kin are not read; a deployment behind
  an egress proxy cannot use the client yet.
- **HTTP/2, kept-alive connections, addresses raced** (they are tried in
  the resolver's order, each with its share), internationalized names (an
  A-label is taken, a U-label refused), `100-continue`.
- **A secret's bytes wiped from memory** once sent.
- **Every byte past an answer's end**: bytes that arrive with the answer
  are refused; any sent later are never read, as the answer is taken once
  it is whole (ADR-0332, step eight).
- **The platform's roots, read at start**: a change to the store reaches
  the host at its next start, and `SSL_CERT_FILE` / `SSL_CERT_DIR` change
  what is trusted (the operator's to set).
- **The refusal of an empty platform store** is tested with an empty
  `CertificateResult`, not on a machine without a store; the connection's
  bound, each address's share of it and a name's one lookup are tested on
  Linux alone (a listener whose queue is full; a second loopback address).
- **An operator's own NAT64 prefix**: the well-known prefix (`64:ff9b::/96`,
  and `64:ff9b:1::/48`) is an IANA special-purpose block, refused for a
  public origin; a network-specific prefix an operator's translator uses
  is in 2000::/3 and looks public, so a name answering in it reaches what
  the translator reaches, a private IPv4 address among it. Naming that
  prefix as a deployment's option, refused as its IPv4 address would be, is
  future work.
- **An IPv6-only host reaching an IPv4-only provider**: through DNS64 and
  NAT64 a provider with no IPv6 address answers in the well-known prefix,
  which is refused, so an IPv6-only host cannot reach it.

## What next work needs

- **NEXT 6d, a Stripe adapter in test mode**: `PW_PAYMENTS=stripe`,
  `Grant::public("https://api.stripe.com")` and `Roots::platform()` at
  start; the test key (`sk_test_…`) as a `Secret` in `Authorization:
  Bearer`, and the webhook secret (`whsec_…`), both the deployment's,
  never committed, where nothing yet manages a deployment's secrets (the
  owner's key, the owner's step); Stripe's form-encoded API mapped to
  `begin`, `cancel` and the intent read again; the hosted fields from
  Stripe's own origin, which ADR-0333's policy then names. **Live keys**:
  a deployment's, not a test's; the owner's decision.
- **Google OIDC** (the production track): the discovery document, the JWKS
  and the token exchange through a client granted Google's origins as its
  discovery document names them (`accounts.google.com`,
  `oauth2.googleapis.com`, `www.googleapis.com`, read from Google's own
  documentation when built); the client secret as a `Secret`; the JWKS kept
  by its `Cache-Control` (the client returns an answer's fields). RS256 is
  *ring*'s (`RSA_PKCS1_2048_8192_SHA256`). Needs the owner's OAuth client.
- **H7, Web Push**: VAPID (ES256) and RFC 8291's encryption (ECDH P-256,
  HKDF, AES-128-GCM) are *ring*'s. Push services' origins differ by browser
  (`fcm.googleapis.com`, `updates.push.services.mozilla.com`,
  `web.push.apple.com`, Windows's `*.notify.windows.com`, a host per
  region): a grant of exact origins cannot name the last. The integrator's
  ruling (below): a push grant holds the platform's list of push-service
  suffixes (FCM, Mozilla, Apple, WNS) and each subscription's endpoint is
  checked against it; never a general wildcard. A payload is the user's and
  never logged.
- **H11, verification**: its provider's origin granted as the payment
  provider's is.
- **Webhooks out**: each destination a public grant, never redirected, its
  body signed (as ADR-0332's HMAC), delivered by a queue.

## What the capability matrix and KNOWN_LIMITATIONS say (applied at the landing)

- **Third-party APIs** (`docs/vision/capabilities.md`): "The host has one
  outbound client (`pw-outbound`, ADR-0387): only the origins a service's
  grant declares, every address checked against IANA's special-purpose
  blocks before any is reached, TLS 1.3/1.2 (rustls, *ring*) against the
  platform's roots, no redirect followed, one deadline, bounded answers,
  strict HTTP/1.1, secrets written to the connection alone. The payments
  reach their (development) provider through it." Gaps: "No provider at a
  public origin is configured yet (a Stripe test-mode adapter is NEXT 6d);
  a program cannot yet name the origins of its own `network.fetch`; the
  recommender is simulated in process; nothing sends a webhook; no
  post-quantum key exchange, no revocation checking, no proxy."
- **Production**: "There is no TLS" becomes "The host serves plain HTTP;
  outbound, it speaks TLS (ADR-0387)."
- **KNOWN_LIMITATIONS**: the not-claimed list above.

## The integrator's rulings (2026-10-25)

The track's three questions, ruled:

1. **A program names an origin by an `origin` declaration carried in its
   effect row** (`network.fetch<Stripe>`, `Stripe` a declared origin), not
   by a clause on a host binding: the origin is part of what the program's
   type says it does. Built on a track of its own, with the first program
   that fetches a real origin.
2. **A push grant uses the platform's push-service suffix list** (FCM,
   Mozilla, Apple, WNS), each subscription's endpoint checked against it;
   never a general wildcard grant. Built with H7.
3. **The test authority and servers are a dev-only crate** of their own
   (`runtime/pw-outbound-testkit`, a dev-dependency of each user), not a
   `test-ca` feature of the production crate. Done (decision 11).

## Found

- **(correction, found by the track's review) A chunk's size wrapped the
  bound.** The first version summed a chunk's size with the body held; a
  size near 2^64 after a first chunk wrapped the sum past the bound in a
  release build (the server's `release` profile has no overflow checks),
  and the body was read without bound until the deadline. Held to what is
  left now; the regression is in `an_answer_is_bounded`, and its mutant
  ("the sum wraps") in the script.
- **(correction, found by the review) A payment's id made another
  request**: `cancel` and the hearing pasted an id into the provider's
  path; `pi_1?x=` read the intent `pi_1` (its owner the reader's), then
  posted to `/v1/payment_intents/pi_1?x=/cancel`, Stripe's update, not its
  cancel. Before the one client a CR or LF in an id reached the request
  line itself. Each id is one segment now.
- **(flake, ci run 40217665465, ARM64)** the first resolver test waited
  1 ns for `localhost` and raced the answer: `recv_timeout` takes a message
  already sent before it reads the clock. Rewritten over a lookup that
  sleeps; the review had named it.
- **(correction) The payments' own HTTP was unbounded** (Context): a
  provider's stated length allocated whole, a body read to the close, a
  head line unbounded, and each read bounded rather than the answer. Fixed
  by decision 10; the regressions are `an_answer_is_bounded`,
  `an_answer_dripped_is_given_up_at_the_total` and their mutants.
- **(correction) ADR-0332's "ten seconds at most, to connect, to send and
  to be answered"** held for each read, not for the answer: true now.
- **(correction) A reader's interaction reached the provider's request
  head unchecked**: `post_json` wrote `idempotency-key: {order}:{interaction}`
  as it came, and the interaction is the reader's runtime's to name
  (`pw-interaction`, a line of the command's head, trimmed), so a bare CR,
  a control or a character outside ASCII in it went to the provider in a
  field of its own making. The client refuses such a field: the command
  traps, and the provider is asked nothing
  (`an_interaction_that_would_split_the_providers_request_is_never_sent`).


## Independent reviews (2026-10-25)

The recovered second review was addressed in `7aa99873`: a two-server
rebinding probe; a blocked writer's deadline; escaped, bounded quoted
fields and no request-line echo; bounded initial allocation for a stated
length; secret-name/answer-Debug/trailer/SNI controls and exact chunk ends.
Its 13-control smoke is historical and is not a final whole campaign.

The recovered third review identified credential-bearing origin diagnostics,
unbounded TLS certificate details, provider-body diagnostics, per-client DNS
budgets, and the own-public-address overclaim. New failing-first regressions
reproduce those leaks and two independently constructed resolvers admitting
64 blocked lookups under the old budget. Invalid configured origins now
show only a static context and reason. TLS details are cut and escaped.
Payment begin/cancel errors never include provider bodies; unknown payment
statuses never enter diagnostics or typed variants. Known neighboring
statuses still map to their declared variants. System resolvers share one
process-wide 32-lookup budget. The tests pin the literal limit, release
held lookups before assertions, and keep a test-serialization mutex usable
after a deliberately failing mutation. Eleven new controls undo these
repairs independently; build/crash/inventory failures cannot earn kills.
The whole 111-control campaign and independent review of this delta remain
required before landing.


Hosted verify 40248500320 at `828e4269` completed the campaign but refused
one control: 110 of 111 earned assertion kills. The one-byte overrun mutant
added one to an unrestricted `usize::MAX` test limit and overflowed in debug
mode, so its second failure was a product-site panic rather than a valid
assertion. Its bound is now expressed with saturating addition; this keeps
the one-byte overrun observable without inventing an impossible limit above
u64::MAX. The valid huge-limit allocation test stays. The revised whole
campaign remains required; the failed run is not acceptance evidence.

### Verified source evidence after the review fixes

At source `b83044313de06055e5bf67187f708d8622b2e458`, hosted
verify 40250992644 succeeds: 55 client tests, 40 payment tests (11 focused
answer/HTTPS tests in each control baseline), and 111 of 111 controls killed
by completed native assertions, with no process stopped by the memory bound.
All three full-browser jobs succeed. Exact-source CI 40250931540 and census
40250931477 succeed. Independent review accepted the source delta and the
corrected overrun witness. The canonical `just evidence-fetch 40250992644`
imports the eligible source record; earlier failed or pre-review records
above remain historical. The resolved TLS inventory in that record is
rustls 0.23.46, ring 0.17.14, webpki 0.103.16 and native-certs 0.8.4; that is
the built version, distinct from the earlier proposal's 0.8.5 inventory.
The evidence-only head must still earn its own CI, and planned PR checks
must pass before integration. The Not claimed obligations remain open.

### PR diagnostic gate: test assertions also keep secrets private

PR57's exact-head CodeQL result at `1ed64596` reported six sensitive-log
and ten log-injection alerts. Each sink was a test assertion's failure
message, including the very strings whose redaction it tested. Test
fixtures do not justify printing these values: a failing privacy control
should not copy them to its logs. Those assertions now use fixed messages,
with their conditions and comparisons unchanged. No alert is suppressed,
no production behavior changes, and the earlier 111/111 campaign remains
historical proof at its recorded source. The affected native tests, a fresh
whole strict campaign and the final exact-head CodeQL/quality gates must
pass before integration.


### Final diagnostic-source evidence

[Push verify40263257525](https://github.com/studiokimeiga/perfect-web/actions/runs/40263257525)
at `2a99a51a70b0ccd7ab5e2089fa8b1bbcbd7cf7e4` passed all four planned
recipes and all three browser jobs. Canonical `just evidence-fetch
40263257525` records outbound-https, payments, invoked-by and each-heads.
The strict outbound catalog again completes **111/111 valid assertion
kills**. PR CodeQL40263260443, quality40263260419, Fallow40263260441,
CI40263260473 and census40263260481 also pass on that exact source head;
no scanning alert is suppressed.

The separate dispatch40260714455 completed its111-control recipe, then
failed because a WebKit page crashed in an unrelated soft-navigation test.
Only that browser job was rerun at the same source; attempt2 completed
successfully. The first crash remains a failed attempt and receives no
mutation kill credit. The later successful whole push run above supplies
the canonical final record. Evidence/document-only successors still need
exact-head CI and PR gates, plus current-master/index reconciliation after
PR54/55/56. No app origin capability, configured real provider or complete
application claim follows from these results.
