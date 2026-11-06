# VM networking, system proxies and bounded egress

Date: 2026-11-05. Status: **system proxy inventory implemented; VM proxy forwarding planned, not implemented**. This extends the [dependency and performance work](12-upstream-and-frame-performance.md). A detected proxy is not an active VM route.

## Current implementation

The pinned Avo VMM (`99153258763a9ccdf5d4c6de36bd631e4dd0dded`) implements IPv4 user-mode networking in `engine/hvf/vmm/src/net.rs`. Guest TCP terminates in smoltcp and continues over host TCP sockets; UDP uses host datagram sockets, and ICMP echo uses an unprivileged host ICMP socket. These sockets use the host's applicable routing/firewall policies, including VPN/TUN routes that actually cover the VMM process and destination. This is not evidence that a particular VPN captures the VM's traffic.

The VMM does not read CFNetwork proxy preferences or perform HTTP CONNECT/SOCKS handshakes. Setting macOS's HTTP, HTTPS or SOCKS proxy therefore does not automatically proxy the guest. `HTTP_PROXY`/`HTTPS_PROXY` environment variables do not change the VMM's raw socket behavior either. Setting Android's HTTP proxy would cover cooperating HTTP clients; it is insufficient for arbitrary native sockets, games, UDP or QUIC.

The guest gateway `10.0.2.2` maps to host loopback. A proxy listening on the Mac's `127.0.0.1` must be reached from the host-side connector, not copied into the guest as its own `127.0.0.1`. The current guest DNS service at `10.0.2.3` forwards to an address parsed from `/etc/resolv.conf`. That file does not model every macOS scoped/split-DNS rule; corporate VPN DNS and private domains require separate qualification. IPv6 is not currently provided by this backend.

## Delivered diagnostic

```sh
cargo run -p glidepodctl --locked -- network
cargo run -p glidepodctl --locked -- --json network
```

`glidepod-macos::network::system_proxy_snapshot` calls `CFNetworkCopySystemProxySettings` on demand. It reports HTTP/HTTPS/SOCKS endpoints separately, whether PAC or automatic discovery is enabled, whether simple names are excluded, and the number of bypass rules. An enabled malformed endpoint remains enabled and is marked invalid; it is not mistaken for disabled/direct configuration. The native bridge allowlists report fields, omitting passwords, usernames, PAC URLs/scripts and the contents of bypass rules. PAC URLs may contain credentials or tokens.

The command also reports the audited pinned backend's source-level behavior. It does not inspect an arbitrary running engine or prove a route. It explicitly sets `system_proxy_applied`, `proxy_reachability_tested`, `vpn_routing_verified` and `pac_evaluated` to false. It does not boot a VM, create a runtime store, fetch a PAC script, connect to the network or change host/guest settings. Default-system inventory is not per-destination or per-interface policy evaluation.

No proxy configuration switch is exposed until the VMM can enforce and report the selected policy.

## Product contract

“Follow system rules” and “require all external traffic to use a proxy” have different semantics. Expose both explicitly:

| Proposed mode | Behavior | Failure behavior |
| --- | --- | --- |
| Direct / host routing | Use the host's applicable routes. Useful when an existing host VPN/TUN already handles egress. | Report connectivity failures. Do not claim the presence of a VPN from proxy settings. |
| Follow macOS proxy rules | Resolve supported destination traffic using system proxy settings, bypass rules and PAC; explicitly selected `DIRECT` rules remain direct. | Config/PAC errors are errors, not an implicit direct result. Unsupported traffic needs an explicit default connector or a blocked state. |
| Require proxy | Use a user-selected system endpoint or a manual HTTP CONNECT/SOCKS5 endpoint. Every supported external guest flow must use that connector. Ignore direct bypasses unless the user changes mode. | Proxy unavailable, authentication rejected, unsupported protocol or no endpoint: block affected egress and report why. Never silently fall back to direct. |

Retain existing users' current routing during configuration migration. Offer system-following as the fresh-install default only after its compatibility gates pass. Store desired policy separately from effective policy. Display active connector, TCP/UDP/DNS coverage, policy generation and blocked reason; an enabled checkbox must not imply full coverage.

Manual proxy configuration should accept a host, port and protocol. Keep credentials in Keychain, referenced by an opaque identifier; exclude them from settings JSON, command-line arguments, logs and process environment. A system configuration snapshot does not establish whether authentication is required.

## Enforcement architecture

```text
Guest apps (including raw sockets)
    -> virtio-net / existing IPv4 stack
    -> bounded flow table + policy generation
    -> selected host connector
       -> direct host socket
       -> HTTP CONNECT tunnel
       -> SOCKS5 CONNECT / UDP ASSOCIATE
    -> existing host route / VPN

macOS proxy + interface change notifications
    -> supervisor policy snapshot / bounded PAC resolver
    -> atomic policy update at the VMM egress gate
```

Integrate at `open_tcp`, `guest_udp`, DNS and ICMP handling in a separately pinned VMM candidate. The supervisor owns system policy and secrets; the VMM owns per-flow enforcement. Use an authenticated, instance-owned control channel and a versioned capability handshake. The current unpatched engine must reject proxy modes rather than accept an ignored environment variable. Fail startup before exposing guest networking when the requested enforcement capability is absent.

Keep lookup, PAC execution, proxy negotiation and DNS off vCPU threads. Use the existing readiness/event loop or a bounded connector worker pool; no thread per connection, packet or lookup. Stage a new policy fully, then publish a generation. When switching to required-proxy mode, revoke existing direct flows before acknowledging success. During an unresolved policy transition, block new external flows. A lost supervisor/connector must close affected tunnels without restoring direct access. Preserve ordering and lifetime rules on guest reset and VM shutdown.

The external engine and renderer locks remain independent. Build and validate the network candidate without changing the current graphics candidate or user runtime registration. Record source, original adaptation/patch and binary hashes; preserve dependency notices and the external engine's unresolved distribution-license boundary.

## Protocol and resolver coverage

| Traffic/configuration | Implementation requirement |
| --- | --- |
| TCP, including non-HTTP applications | SOCKS5 CONNECT or an HTTP CONNECT tunnel where the proxy permits the target port. Return proxy rejection to the guest; do not substitute a direct socket. Preserve half-close, cancellation and backpressure. |
| UDP, including game traffic and QUIC | SOCKS5 UDP ASSOCIATE with a live control connection, verified relay source, bounded datagram sizes/queues and explicit fragmentation handling. An HTTP-only proxy has no general UDP relay. In required-proxy mode block unsupported UDP, or use an explicitly selected compatible tunnel. Do not promise QUIC fallback for every application. |
| Google push / FCM | Firebase documents that ordinary network proxies are unsupported for its device push connection. Qualify any transparent VMM connector or host tunnel with a real FCM client, long-lived connections and network-change tests. Required-proxy mode must not silently exempt Google traffic. See the [GMS and notification gates](14-google-services-and-notifications.md). |
| DNS in required-proxy mode | Route guest DNS through the selected connector: supported SOCKS UDP or DNS over a proxied TCP connection. Cover the advertised guest resolver and hard-coded external DNS addresses. No direct host UDP/53 fallback. DoH/DoT follow the normal TCP/UDP egress policy. |
| Proxy/PAC bootstrap resolution | Resolve only the configured proxy/PAC service on an explicitly defined host control path. Do not turn that exception into a guest destination resolver. Document this bootstrap separately from guest DNS enforcement. |
| DNS in direct/system modes | Use macOS-aware scoped resolution for hostname requests and validate VPN split-DNS behavior. Preserve guest wire-DNS semantics rather than treating `/etc/resolv.conf` as the entire resolver policy. Resolve the distinction between hostname API queries and arbitrary DNS record types explicitly. |
| PAC and automatic discovery | Use Apple's destination-aware APIs and bounded asynchronous evaluation. Cache by network/policy generation, destination and expiry; react to proxy/interface changes. A PAC URL string is not a route decision. Limit fetch size, redirects, execution and cache memory. |
| Opaque TCP/UDP under PAC | The VMM initially sees destination IP/port, not necessarily the original URL/hostname. Do not invent a URL from port 443 or inspect/decrypt TLS to claim exact PAC semantics. Preserve DNS-derived names where reliable, and require an explicit default connector for traffic that cannot be evaluated faithfully. Report unsupported configurations clearly. |
| Different HTTP and HTTPS endpoints | Preserve protocol-specific decisions for recognized HTTP traffic. Do not arbitrarily reuse the HTTP endpoint for every protocol. Non-web traffic needs a declared default SOCKS/tunnel connector. |
| Host loopback and LAN | Keep gateway/local-service access separate from Internet egress. Required-proxy mode must not let arbitrary connections to `10.0.2.2` bypass policy. Keep control/input on owned vsock/Unix channels and define any allowed local services narrowly. |
| ICMP / other IP protocols | Ordinary HTTP/SOCKS does not offer general IP tunneling. Block unsupported external protocols in required-proxy mode and label coverage. A separately qualified VPN/TUN path is required for broader IP support. |
| IPv6 | Current backend drops IPv6. Future IPv6 must pass through the same enforcement point before being enabled; no parallel direct path. |

`Require proxy` means unsupported egress is blocked, not that an HTTP endpoint can transport every IP protocol. This distinction belongs in the product's coverage description.

## Performance and memory acceptance

The current source has an additional memory risk worth addressing with the network work: up to 2,048 TCP flows **per network stack**, with 128 KiB receive and 256 KiB transmit buffers per flow. That is up to 768 MiB of TCP buffer capacity per stack before host socket overhead. Cuttlefish creates stacks for both mobile data and Wi-Fi, so the combined theoretical capacity is 1.5 GiB if both fill. This is a source-derived capacity bound, not measured resident usage. The uplink inbox also appends incoming packets to a `Vec` without a byte/packet bound.

Give all NICs one VM-level budget. An initial candidate budget is 64 MiB for network-owned payload buffers, including flow buffers, pending handshakes, DNS/PAC cache and packet queues; account host socket memory separately. Use demand-based allocation and explicit credits rather than eagerly allocating 384 KiB for every new SYN. Cap pending handshakes, total flows, per-flow buffers, packet sizes and work per event-loop tick. Apply TCP backpressure and protocol-appropriate UDP drops when saturated. Expose counters for refused flows, dropped datagrams and timeouts without retaining destination histories or payloads.

Benchmark direct, HTTP CONNECT and SOCKS5 paths with identical payloads and successful-delivery counts. Measure throughput, p50/p95/p99 connection and request latency, VMM/connector CPU, physical footprint, buffer high-water marks and teardown. Include high connection churn, stalled peers, DNS timeouts, proxy restarts and simultaneous two-app GPU work. Do not count dropped work as a performance gain or let network processing starve input/rendering.

## Delivery sequence and gates

1. **N0 — discovery (delivered).** Read-only system inventory and truthful CLI coverage. Test malformed/disabled endpoints, protocol separation, redaction and the real native bridge. No live preference mutation is needed.
2. **N1 — required TCP proxy candidate.** Add versioned VMM policy capability, bounded HTTP/SOCKS negotiation, proxied DNS and explicit denial of unsupported external UDP/ICMP. Prove no direct SYN or DNS escape in isolated tests before exposing settings. This is TCP/DNS coverage, not full UDP support.
3. **N2 — UDP and system-policy migration.** Add SOCKS UDP lifecycle, network-change handling, scoped resolver integration and bounded PAC support with accurate opaque-flow limitations. Validate required-proxy behavior when PAC contains `DIRECT` and when system proxy settings disappear.
4. **N3 — product integration and resource gates.** Add per-profile UI/CLI configuration backed by effective runtime status, credentials, rollback and diagnostics. Qualify proxy-only, host-TUN and enterprise-PAC scenarios separately. Apply the shared network memory budget and run graphics/network contention tests.

Acceptance fixtures must include a controlled destination, a recording proxy, a DNS responder and a deliberately unavailable proxy. Run original Android HTTP/TLS, raw TCP, UDP and hard-coded-DNS clients in an isolated guest. In required-proxy mode, use packet/endpoint evidence to prove the destination is reached only through the proxy, and receives nothing when the proxy is killed or rejects authentication. Check loopback addressing, bypass rules, network/PAC changes, captive portals, proxy authentication, IPv6 rejection, two NICs, reboot/reset and shutdown. Check host preferences before/after and preserve the user's system network configuration throughout.

No proxy forwarding, tunnel, PAC execution, VPN qualification or leak test is claimed by N0.

## N0 validation

- Workspace formatting, all-target Clippy with `-D warnings`, build and tests pass: 53 tests passed, 5 explicitly ignored. The three new tests cover malformed endpoints, protocol/PAC separation and redaction, and decoding the real Objective-C bridge. Existing ignored guest/performance tests were not repeated for this read-only diagnostic.
- Rebuilt the development app and verified its ad-hoc signature. Executed both text and JSON diagnostics from the packaged `glidepodctl`; the reported enable flags match `scutil --proxy` on this Mac. HTTP, HTTPS, SOCKS, PAC and auto-discovery were disabled on the live host; enabled and malformed endpoints were exercised with fixtures, not by changing host preferences.
- The `scutil --proxy` output is identical before and after the packaged diagnostic, and an explicitly selected nonexistent runtime-store path remains absent. The command reports all proxy application/reachability, VPN qualification and PAC execution flags as false.
- Local logs, the redacted inventory, packaged CLI hash and validation results are in the ignored `evidence/network-2026-11-05/` directory. These establish discovery and packaging only; they do not establish guest forwarding or absence of network leaks.

## Primary references

- [Pinned Avo network implementation](https://github.com/nizwar/avo/blob/99153258763a9ccdf5d4c6de36bd631e4dd0dded/engine/hvf/vmm/src/net.rs) and [NIC setup](https://github.com/nizwar/avo/blob/99153258763a9ccdf5d4c6de36bd631e4dd0dded/engine/hvf/vmm/src/cuttlefish.rs).
- Apple: [system proxy snapshot](https://developer.apple.com/documentation/cfnetwork/cfnetworkcopysystemproxysettings()), [per-URL selection](https://developer.apple.com/documentation/cfnetwork/cfnetworkcopyproxiesforurl(_:_:)), [asynchronous PAC evaluation](https://developer.apple.com/documentation/cfnetwork/cfnetworkexecuteproxyautoconfigurationurl(_:_:_:_:)).
- Android: [default HTTP proxy](https://developer.android.com/reference/android/net/ConnectivityManager#getDefaultProxy()) and [ProxyInfo](https://developer.android.com/reference/android/net/ProxyInfo).
- [SOCKS5, RFC 1928](https://www.rfc-editor.org/rfc/rfc1928), especially UDP ASSOCIATE and the lifetime of its TCP control connection.
