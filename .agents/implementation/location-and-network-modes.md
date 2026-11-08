# Location sources and VM network attachment

Date: 2026-11-08. Status: implemented in the development source; real Mac location and successful bridged LAN delivery remain separate acceptance gates.

## Product behavior

Settings offers Off, Follow Mac and a fixed virtual latitude/longitude. The default is Off. Changes apply to a running VM without restarting it. Android applications still need Android location permission. Follow Mac additionally needs macOS Location Services permission for the packaged runtime owner. Mac coordinates are held in memory and sent over the instance-owned control socket; they are not stored in configuration, diagnostic status or logs. Only a user-entered virtual point is saved.

The runtime owns one Core Location session while Follow Mac is selected and Android is running. Switching away releases it. Missing permission, an unavailable source or a fix older than 60 seconds disables Android location instead of substituting the virtual point. Off also disables Android's global location setting: the pinned Cuttlefish GNSS HAL has a synthetic fallback location if no host fix is supplied. This integration controls GPS and the global location setting; it is not a guarantee about an application's cached locations or location inferred independently from IP addresses or Google services.

Shared with Mac keeps the existing IPv4 host-socket backend. It follows applicable host routes. It does not apply macOS HTTP/SOCKS/PAC settings, promise VPN capture, or add IPv6 to that backend. Bridged selects a specific interface returned by `vmnet_copy_shared_interface_list`, attaches Android's Ethernet NIC to macOS vmnet and leaves the shared Wi-Fi/modem NICs disconnected. The LAN must provide the applicable address/routing services. A bridge error fails startup; it never silently selects Shared.

Network changes apply on the next Android start. Saved configuration and effective runtime attachment are reported separately. Internal GMS/image-maintenance candidates use Shared and do not start a location session. Existing configurations migrate to Off and Shared.

Shared startup explicitly enables the guest's synthetic Wi-Fi and joins `VirtWifi`. The test image had Wi-Fi disabled, so merely attaching the host backend left Android without an address or default route. A bounded worker now restores the selected connection without blocking supervisor requests. It publishes address-acquisition results separately from attachment mode; an address is not proof of Internet access. Bridged startup disables guest Wi-Fi and waits for Ethernet addressing. These commands never change macOS network preferences.

## Implementation and limits

- Core Location is a thread-owned Objective-C session with a pumped run loop. Polling and guest transport run outside the UI and vCPU threads. Disabled location sends one clear command on transition rather than repeated control traffic.
- The GNSS control path validates finite coordinate, altitude, accuracy, speed and bearing ranges. Engine fixes expire after 15 seconds without a supervisor refresh. A fresh virtual or host fix is refreshed every two seconds.
- Interface names are validated and checked against the current eligible inventory before launch. The external engine rejects an unsupported bridge argument. Frame sizes and the outgoing packet queue are bounded; a vmnet read failure terminates the bridged engine rather than enabling host sockets.
- The existing vmnet API requires macOS authorization. The current ad-hoc development engine has the Hypervisor entitlement only. On the test Mac, interfaces `en0` and `en1` are listed but `vmnet_start_interface` returns status 1001. This establishes an explicit startup failure, not a successful bridge. No privileged helper is installed and the VM is not run as root.
- The external engine remains independently pinned and evaluated. This change does not resolve its distribution/licensing boundary or bundle Android/GMS images.

Apple references: [Core Location manager](https://developer.apple.com/documentation/corelocation/cllocationmanager), [macOS Location Services](https://support.apple.com/guide/mac-help/allow-apps-to-see-the-location-of-your-mac-mh35873/mac), [vmnet operating modes](https://developer.apple.com/documentation/vmnet/operating_modes_t), and [restricted virtual-network entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.vm.networking).

## Reproduce

Use a disposable registered Developer VM; keep the user's runtime registration unchanged.

```sh
rtk proxy cargo build --workspace --locked
rtk proxy python3 tools/build-android.py
rtk proxy python3 tools/build-memory-engine.py --host-services --data-dir .cache/TEST_STORE
rtk proxy target/debug/glidepodctl --data-dir .cache/TEST_STORE location virtual --latitude 1.3 --longitude 103.8
rtk proxy target/debug/glidepodctl --data-dir .cache/TEST_STORE network-mode shared
rtk proxy python3 tools/check-location-network.py --data-dir .cache/TEST_STORE
```

`location off` and `location host` select the other sources. `network-mode bridged --interface en0` selects a bridge for the next boot. Bare `location` reports delivery state without coordinates; bare `network-mode` reports desired/effective attachment and eligible interfaces. The builder combines the existing memory patch and `engine/patches/avo-host-services.patch`, verifies the exact source delta and emits binary/patch provenance next to the engine.

## Acceptance record

`evidence/location-network/report.json` records the disposable VM checks. The original unprivileged Android fixture requests GPS with ordinary permissions and remains visibly active while testing continuous delivery; Android's normal background location throttling is preserved. The harness checks permission denial, first coordinates, live coordinate changes, invalid-input rejection, Off, unavailable host source, Shared TCP delivery to a controlled host endpoint, reconnection after disabled guest Wi-Fi, and explicit bridge failure without Shared fallback. It restores Off/Shared and shuts down its VM afterward.

Workspace tests: 100 passed, 13 opt-in integration tests ignored. Locked all-target Clippy passed with warnings denied. The engine's three existing GNSS tests passed. Android helpers and the isolated signed development bundle built successfully. The native Settings page and its location section rendered and opened with Command-comma; coordinate clicks/scrolling did not produce a visible response in this automation session, so complete native control interaction is not accepted. The packaged runtime returned macOS location permission denied and kept Android location disabled (`packaged-host-status.json`). Actual authorized Mac-to-Android delivery and successful bridge DHCP/LAN connectivity remain unverified.

The final VM harness passed all nine checks. During reruns, this reused test image reached 96% data usage (7.4 GiB of 7.9 GiB; `/data/gsi` accounted for roughly 6.6 GiB), and Android refused a redundant fixture installation. The harness now reuses an installed fixture only when its APK SHA-256 exactly matches the local artifact; no storage threshold was weakened or app data cleared. This is a disposable-image capacity limit, not an accepted production storage policy.

Rebuilding the host-services engine a second time verified source-delta checks and reproduced SHA-256 `38bbd7ad058115b5f325c1a573bb9146cb7069e5cc72b1c9a3ceac63d0e1023c`. Combined patch SHA-256: `efe90c759bf1f85f62492dc176a5f737211e068a86385e05fefa0219750ae972`. The user's original VM registration was preserved; only the disposable `.cache/adaptive-memory-test` store used this candidate.

Required bridge qualification still includes DHCP, DNS, TCP/UDP, a second LAN peer, Wi-Fi and Ethernet interfaces, interface disappearance, reconnect, VM shutdown and simultaneous app traffic. Required host-location qualification includes permission grant/revocation, stationary and moving fixes, suspend/resume, source switching, helper-only residency and GMS fused-provider behavior. Neither capability is a production compatibility claim.
