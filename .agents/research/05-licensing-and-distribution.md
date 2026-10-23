# Open-source licensing and direct distribution

## Project license recommendation

Use **Apache License 2.0** for new GlidePod code, including Rust services, UI, protocols and guest integration. It allows free distribution and modification, supplies an express patent grant, and aligns with GPUI and much of AOSP. Retain upstream notices and mark modified upstream files. The license does not require forks to remain open, prohibit commercial use, or grant trademark rights. [Apache-2.0 terms](https://www.apache.org/licenses/LICENSE-2.0).

If reciprocal source publication becomes a business requirement, evaluate MPL-2.0 separately; it is not necessary for the stated free/open distribution goal. GPL would be a deliberate project-wide policy choice, not an automatic consequence of using Android. This document recommends a license; it does not relicense upstream components or assert legal clearance for a future binary.

## Component obligations

| Component | Observed license / treatment | Release action |
| --- | --- | --- |
| New GlidePod code | Proposed Apache-2.0 | Add LICENSE, NOTICE if needed, SPDX headers and contributor policy before code release |
| GPUI / gpui_macos | Manifests inspected: Apache-2.0 | Pin revision and audit full dependency graph; the Zed editor's licensing is a separate question |
| Avo candidate VMM | Root: Apache-2.0; engine workspace and crates: BSD-3-Clause | Reconcile component scope and missing engine license text before reuse; no engine code is linked or distributed by this foundation |
| AOSP / Cuttlefish | Predominantly Apache-2.0 with exceptions | Inventory every built image component; generate notices from the exact manifest |
| Linux kernel | GPLv2 with applicable exceptions | Supply corresponding source, exact config, patches and build instructions for shipped binaries |
| gfxstream | Apache-2.0 plus bundled third-party notices | Audit the actual build closure, including decoder/compiler dependencies |
| ANGLE | BSD-style project license and dependency licenses | Include matching license inventory for the chosen guest build |
| MoltenVK / Vulkan loader | Apache-2.0-based projects with dependencies | Include relevant notices and audit source/binary provenance |
| QEMU contingency | GPLv2 and component-specific terms | Ship as a separate executable with matching source obligations; assess any linked integration separately |
| Optional Magisk/root tools | Separate copyleft project, not included in initial scope | Review exact version and redistribution obligations before integration |
| Apple frameworks | OS-provided proprietary frameworks | Link through documented APIs under applicable Apple terms; do not redistribute framework binaries |
| Third-party APKs and icons | Rights belong to their authors | No app catalog bundled without rights; local extraction of user-installed app assets only |

[AOSP licensing documentation](https://source.android.com/docs/setup/contribute/licenses) explicitly identifies the Apache preference and Linux GPL exception. The resulting application-plus-guest distribution has multiple licenses. A single top-level Apache file cannot cover it all. Process separation is an architectural choice, not a universal legal exemption; audit combined/derived code, especially if using a GPL-only implementation.

Google Play, Play Services, GMS images, Widevine keys and proprietary native-translation libraries are not included in v0.1. Do not package binaries copied from an installed Android SDK merely because upstream source exists. The [Android SDK agreement](https://developer.android.com/studio/terms) distinguishes SDK terms from components governed by open-source licenses. Build and package redistributable components from known sources with their own terms. Optional future user imports must not be described as an automatic solution to redistribution or certification rights.

## Direct-distribution shape

Ship a Developer ID signed and notarized `GlidePod.app` in a signed/notarized DMG, with a small signed runtime manifest for separately downloaded large Android images. Provide public source, exact source manifests, source archives for copyleft components, checksums, signature verification instructions, SBOM and notices alongside each release. No App Store submission is planned.

Sign nested executables/libraries correctly and staple applicable tickets. The VMM needs `com.apple.security.hypervisor`; other executables receive only the entitlements they actually use. Do not add unsigned-executable-memory or disable-library-validation broadly without a concrete tested dependency. Guest ART JIT is not host JIT and does not by itself justify host JIT entitlement. [Apple Hypervisor entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.hypervisor).

The same-user helper uses a normal background-service registration, with no permanent privileged system daemon. Main-app freedom from App Sandbox does not bypass camera/microphone/notifications TCC or the widget extension's requirements. Validate [SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice) and [notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) on a clean Mac account.

## Release gates

1. Dependency/source/license lock is reproducible and every distributed file has provenance.
2. A clean checkout builds the host and the image on documented Mac/Linux builders.
3. Signature, notarization, quarantine launch and downloaded-image verification pass on a fresh user account without disabling Gatekeeper.
4. Locally generated app entries launch, repair and survive host updates; no signing key is embedded. Any identity compromise is documented before release.
5. Widget extension is discovered and updates under the actual distribution signing setup, or is explicitly excluded from the release feature list.
6. Update rollback preserves or restores matching userdata. Uninstall stops helpers, offers data retention and removes only owned application entries.
7. No real user APK, credential, signing key, app data, clipboard content or diagnostic personal data is accidentally included in artifacts.

Free distribution still has build, hosting, signing-account and maintenance costs. Developer ID distribution requires the relevant Apple membership; contributors can build locally with development/ad-hoc signing, but that is not equivalent to a notarized public download.
