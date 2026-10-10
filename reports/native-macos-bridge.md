# Native macOS C ABI broker — 2026-10-10

## Implemented and exercised

The Mac product UI uses SwiftUI/AppKit and `siliq-native-bridge`. It calls the existing canonical Rust host through synchronous plain helpers. It initializes no Tauri window, WebView, HTTP server or plugin. Retained Tauri host dependencies and a WebKit framework link declaration remain in the library inventory; this is disclosed rather than treated as absence of an OS framework. The tested optimized dylib has no direct WKWebView, URLSession or socket/connect imports.

The [C header](../crates/native-bridge/include/siliq_native.h) defines opaque `create`/`close`, bounded `request`, owned `{data,len}` response buffers, `last_error` and `free`. Each response is `{ok:true,value:...}` or `{ok:false,issues:[...]}`. Requests are UTF-8 JSON with a 20 MiB bound, response JSON has a 64 MiB bound, paths are absolute UTF-8 and at most 4095 bytes. Duplicate JSON keys are rejected before values are parsed; typed argument structs deny unknown fields, and a finite command allowlist rejects unknown commands. Container/OS picker access is still checked through Darwin metadata and `O_NOFOLLOW_ANY`, retained descriptors, type/link/size/local-mount checks, checksums and atomic publication.

Registry tokens never dereference foreign handles. Pending requests retain their broker across `close`. Closing a broker cancels workers and holds the host/workspace lock until the workers reach terminal status. Swift owns the selected URL's security scope; access is active during path calls, and batch scopes remain active while plans/jobs need them. User output paths are not persisted as automatic grants.

Canonical preview PNGs are encoded into bounded Base64 JSON, then displayed as images by AppKit. The metadata carries request/revision/dimensions/issues and the canonical PNG checksum. Swift drawing is used only for selection controls and guides. Candidate previews use the same frozen dataset/document/resources and approved native raster, reduced to at most 512 pixels. They remain inspection images before derivative format conversion.

## Actual commands and results

```sh
scripts/cargo-verified.sh test -p siliq-native-bridge -p siliq-desktop --lib
scripts/cargo-verified.sh clippy -p siliq-native-bridge -p siliq-desktop --all-targets -- -D warnings
scripts/cargo-verified.sh build --release -p siliq-native-bridge
scripts/offline-desktop-audit.sh
```

The final C ABI suite passed **3 tests** in **5.50 seconds**; the host suite passed **17 tests** in **2.74 seconds**, including the closing-worker workspace-lock lifetime. Clippy passed with warnings denied. A previous incremental optimized dylib build completed in **31.61 seconds**; packaging must be repeated after the final import-result change. These warm build timings are not performance acceptance benchmarks. `npm run macos:typecheck` also completed successfully against the Mac 13/Swift 5 target.

The integration directly calls the exported C functions and frees their returned buffers. It performs real fixture load, typed text edit, idempotent retry, canonical PNG preview, explicit variant approval, `.siliqcg` save/open with exact document equality, real PNG/JPEG/lossless WebP/SVG/PDF output, JSON selection and mapping, five-format all-item preflight, a candidate PNG, and a job completing five files plus a manifest. It also checks duplicate/unknown request fields, an unknown command, null buffers, closed handles and error envelopes. Existing host tests cover revisions, Undo/Redo, exact font identity, JSON/CSV equivalence and limits, recovery, input/output grants and protected cleanup.

Native `import_asset_path` adds `importedResourceId` to its session read model. A regression imports an image, adds another SVG, reimports the same image and verifies both the returned ID and unchanged catalog order. It then deletes the selected source and retries the same transaction, obtaining the exact cached response. The ID metadata shares the existing bounded transaction lifetime, and legacy Tauri response fields remain unchanged. A canvas insertion must select this returned ID rather than the last catalog entry.

The updated static audit passed with **253 target-specific normal packages, 27 broker commands and 9 Swift source files**. The graph root is `siliq-native-bridge`; build/dev edges are excluded and proc-macro roles are marked. Swift WebView/network/process APIs and native direct network calls are rejected. Old Tauri CSP/capability checks are separately labeled reference evidence. Bundle mode additionally verifies the actual `siliq-studio` signature and strict entitlements, the native broker dylib's signature, imports and SHA-256, and records retained framework links.

The real packaged SwiftUI `.app` also passed the updated bundle audit: `siliq-studio` is arm64, the signature verifies, App Sandbox and user-selected read/write are enabled, and network/temporary-exception entitlements are absent. The broker dylib signature verifies with no entitlements or direct network-symbol imports. Its packaged SHA-256 was `6a9e4fea0f3e14d7eb45ee7baa7829aee40aa4f2699704a36092fb0a94868736`; the WebKit framework link declaration remains recorded. Evidence is in `out/native-release-audit` and must be regenerated after another packaged build. Signature checks do not replace actual launch and process observation.

## Signed Sandbox persistence probe

`scripts/test-macos-sandbox.sh` builds and signs a separate Foundation-only app with identifier `jp.siliq.creative-generator.sandbox-tests`. It copies the current product bundle's broker and public-safe fixtures, loads the broker through `@rpath`, and applies exactly the product's two entitlements: App Sandbox and user-selected read/write. Network entitlements and exceptions are absent; strict signature verification succeeds. The probe refuses to run unless `NSHomeDirectory` is its own container and Application Support is inside that container. It reads no user projects, uses a unique private run directory, and uses no UI, subprocess or network API.

The actual signed run passed on 2026-10-10 in **1.82 seconds**. It loaded the bundled fixture, edited text and committed recovery, saved/opened a project with exact document equality, requested the native PNG preview, wrote actual PNG/PDF files, mapped a string-only JSON row, inspected a batch thumbnail, and completed PNG/JPEG/lossless WebP/SVG/PDF output plus the terminal manifest. Every output's signature is checked and row ID `001` retains its leading zero. This exercises the container file-access path that the earlier unsandboxed tests did not cover. In particular, project-store now opens the granted directory with Darwin `O_NOFOLLOW_ANY` rather than reading every ancestor from `/`; the previous approach failed during the real signed app's recovery checkpoint.

Evidence is emitted to `out/sandbox-probe/result.json`, `signature-audit.json`, `entitlements.plist`, `probe.log` and `probe.stderr.log`. The packaged source broker SHA-256 was `a91c4b5279a959d489b2341f5c2cc74fa7cc8bdf5c38a1e30af6052e699942ea`, unchanged in the signed probe copy. The CI workflow now runs this after the native composition checks and retains the evidence even after a later failure; the remote CI run has not been observed here. This is a container persistence/render/job test; OS picker grants and visible product interaction still require separate UI evidence.

## Release boundaries

This report establishes the C ABI/native engine path. It does not declare signed SwiftUI first-launch, GUI interaction, IME/reconversion, worker stress, observed zero traffic, p50/p95/RSS targets, Developer ID distribution, notarization or all G4 gates complete. Actual signed-bundle interaction and process observations must be recorded separately. The machine used here is Apple M1 Max/32 GiB/macOS 26.3.1; the proposed M1/16 GiB acceptance machine is different.

The retained Tauri release failed with network entitlements omitted: WebKit's sandbox bootstrap rejected the client. Its [upstream entitlement check](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/Shared/EntryPointUtilities/Cocoa/XPCService/XPCServiceEntryPoint.mm) explains that gate. [ADR 0010](../docs/adr/0010-native-macos-interface.md) records the decision to keep the network-denied Sandbox and replace the product UI. Network entitlements, broad sandbox exceptions and disabling Sandbox were not adopted.
