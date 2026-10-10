# Desktop native host — 2026-10-10

## Scope and evidence

The host in `apps/desktop/src-tauri` is a functioning Tauri 2 native application. Rust owns canonical edit sessions, resources, recovery and output grants. The React UI consumes native read models and native CPU render frames. This report records host tests and static audit results; desktop interaction, signed-bundle first launch and attributed traffic observations belong to the release acceptance report.

Environment: macOS 26.3.1 (25D2128), Apple M1 Max, 32 GiB RAM, `arm64`, Rust 1.97.0, target `aarch64-apple-darwin`. This differs from the proposed M1/16 GiB benchmark machine. The Skia archive was verified with the pinned SHA-256 `75e65a6e374581d59b00519719db1ff4064c256aeac9c4df977366fbcf1ec047` by `scripts/cargo-verified.sh`. The public-safe `basic.json` used for native equality tests had SHA-256 `0367a00d69519d64c30a537cf450a33d250f3e4e8d5d7acef11755d55e74a9c6`.

Commands actually run:

```sh
scripts/cargo-verified.sh test -p siliq-desktop --lib
scripts/cargo-verified.sh clippy -p siliq-desktop --all-targets -- -D warnings
scripts/offline-desktop-audit.sh
```

Latest completed host test run: **16 passed, 0 failed**, 1.15 seconds test execution; 2.39 seconds warm compilation. The tests include strict JSON datasets, candidate inspection thumbnails, recovery-pointer error classification and the Darwin grant path. Clippy passed with warnings denied. The static audit passed with 252 target-specific normal packages and 29 explicitly permitted native commands. Counts include normal proc-macro dependencies, marked as compile-time in the inventory. They are not a count of libraries dynamically loaded by the application.

## Implemented contracts

- `open_fixture` uses a finite bundled-document allowlist. `new_document` creates a blank draft with the bundled licensed font catalog available for explicit authoring.
- `apply_edit`, `undo_edit`, `redo_edit` and `set_mode` use opaque session IDs, expected revisions and transaction IDs. A successful retry with the same descriptor is replayed once. Reusing an ID for different input or submitting a stale revision is rejected. The replay window holds 512 successful transactions; older retries still carry their obsolete revision and cannot silently reapply.
- `request_preview` returns a raw binary `ArrayBuffer`, **RGBA8, sRGB, straight alpha**, without Base64. `preview_metadata` carries revision, request ID, dimensions, `stride = width × 4`, payload length, frame digest, canonical PNG digest and issues. Revision/request supersession is checked before accepting results and metadata. The shared renderer permits draft previews; production exports continue to require approval.
- OS pickers originate in Rust. No frontend command accepts arbitrary input or output paths. Checked file descriptors reject symlinks in path components, hard-linked inputs, unsupported types and size excess. macOS uses `getfsstat(MNT_NOWAIT)` to reject existing nonlocal mounts, then metadata-only `getattrlist(FSOPT_NOFOLLOW_ANY)` for provider flags and a direct granted-target `open(O_NOFOLLOW_ANY)`. This kernel flag refuses links anywhere in the path without requiring read-open access to ungranted ancestor directories. Descriptor mount/flags/type/link count and `F_GETPATH` are checked afterward. Output operations remain relative to retained directory descriptors. Actual signed App Sandbox startup is a separate release test.
- `open_project`/`save_project` use project-store descriptor APIs. Save As is a native picker operation. Recovery commits after canonical transactions and uses an atomic app-owned pointer. Resuming recovery adopts a direct child workspace to avoid progressively nested paths.
- `import_asset` validates content rather than extension and registers hash-addressed bytes. `resolve_font` requires the exact declared hash and keeps the declared face, name and embedding permission. A different font must be imported and explicitly selected; no substitute is inferred.
- `export_design` supports PNG, opaque-background JPEG, lossless WebP and explicitly approved whole-artboard rasterized SVG/RGB proof PDF. Reports disclose rasterization, text editability, alpha/background conversion and output digest. The host creates new exports without overwriting existing user files.
- CSV/JSON selection returns an opaque dataset handle. JSON is a non-empty array of objects with unique non-empty keys, identical columns and string values only; numbers/null/boolean/nested values are rejected. Explicit mappings produce canonical rowId-first CSV with sorted slot names, preserving leading zeroes and literal formula text. Unrepresentable carriage-return text is refused instead of normalized; LF text is preserved. Input and mapped CSV are limited to 20 MiB and 10,000 rows. Native all-row preflight selects a local output-directory grant. Jobs use fixed document/data/format recipes and resources, retained descriptors, progress, cancellation and checksum-verified resume. App-owned snapshots support resume after restart by asking the user to select the original output folder again; output permission paths are not persisted.
- Preflight exposes every candidate's row, variant, filename, format and issues. `request_batch_preview` accepts only an opaque frozen plan ID and bounded item index, renders its bound row with the common approved renderer and returns a raw PNG inspection thumbnail with maximum side 512 pixels and maximum payload 2 MiB. One thumbnail raster is allocated at a time; resources are protected by operation leases. The UI must label these as inspection images: they show the common raster before derivative JPEG background/codec conversion. Blocking-error plans can be inspected but cannot start publication.
- Storage inspection and explicit cleanup report the app-owned location and logical file sizes. Cleanup uses descriptor-relative enumeration/deletion, does not follow symlinks, and preserves open sessions, operations in flight, pending/unfinished jobs and the latest recovery. Completed job snapshots can be removed; user project packages and exported outputs are not cleanup targets. Closing a session releases memory references without deleting its workspace. A retained native file lock prevents two host processes from sharing one workspace's cleanup state.

## Host test coverage

The tests actually exercised:

1. Idempotent mutations, stale/reused transaction rejection and increasing Undo/Redo revisions.
2. Native raw frame equality with the common export renderer, PNG digest correspondence, payload dimensions and stale preview requests.
3. A transparent pixel and a 50%-opacity red pixel yielding straight RGBA `[255, 0, 0, 128]`, preventing double premultiplication.
4. Symbolic/hard-link input rejection and no-clobber/path-traversal output rejection.
5. Atomic app metadata replacement without writing through a planted symlink.
6. Same-revision recovery retry without duplicate journal failure.
7. CSV mapping with `001` and `0009` preserved, and duplicate target rejection.
8. Saved input/resource snapshots reopening with byte-identical native PNG output, a 512 × 512 deterministic candidate PNG from frozen mapped data, invalid-index/item-error rejection, blocking-plan start refusal, a real native fixture job completing, and a persisted completion marker becoming eligible for history cleanup.
9. Cleanup preserving live sessions, operations in flight, latest recovery and an unselected symlink target.
10. Workspace lock exclusivity and release on host drop.
11. Synthetic remote-mount classification with component boundaries, without accessing any actual remote share.
12. Exact-font hash acceptance and different-font rejection with `E_FONT_MISSING` and the declared font location.
13. Only a genuinely missing recovery-pointer entry is classified as an initial unsaved state; a dangling symlink, oversized input, directory entry and missing parent retain their rejection issues.
14. Equivalent CSV/JSON data with reordered columns/keys mapping to identical canonical bytes and SHA-256, including `001`, `0009`, literal `=SUM(1,2)` and LF text; unrepresentable carriage-return text is refused.
15. JSON duplicate keys, Unicode-escaped duplicate keys, non-string values, non-array inputs, missing/extra columns, empty input and invalid UTF-8 rejection.
16. Exact 10,000-row JSON acceptance, 10,001-row refusal and input byte-limit refusal.

## Audit boundaries

The release configuration is default-deny CSP with local IPC, a local main-window capability, an explicit command manifest and external-navigation rejection. No HTTP, WebSocket, updater, shell or opener plugin is initialized. `tauri-plugin-fs` is a transitive native-dialog `FilePath` dependency; it is not initialized and receives no frontend capability. Tauri's Tokio runtime is used for IPC/CPU work. Its runtime normal feature tree contains filesystem, I/O, runtime and synchronization features; networking/process features are absent. Cargo metadata's combined build-context feature set must not be mistaken for the runtime feature tree.

Release compilation without `custom-protocol` is refused so that a release cannot accidentally depend on the development server. Configured macOS entitlements enable App Sandbox and user-selected files, with no network entitlement or broad filesystem exception.

`scripts/offline-desktop-audit.sh <signed .app> [output-directory]` additionally verifies the actual bundle signature and entitlements, and records its executable digest, architecture and direct network-symbol imports. It deliberately does **not** claim that configuration, package names or symbols prove an observed absence of traffic. Its artifacts are local under `out/desktop-audit` by default.

Not established by these host tests: network-disconnected clean release first launch, process-attributed connection/DNS attempts, real SMB/WebDAV/provider behavior, Japanese IME/reconversion, complete native GUI workflows, standardized p50/p95/memory targets, Developer ID distribution, notarization or Gatekeeper acceptance. Do not turn those unperformed checks into a G4 pass based on this report.

Primary implementation references: [Tauri capabilities](https://v2.tauri.app/security/capabilities/), [native dialogs](https://v2.tauri.app/plugin/dialog/), [macOS bundles and entitlements](https://v2.tauri.app/distribute/macos-application-bundle/), [Apple user-selected file entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.files.user-selected.read-write), [Apple XNU open flags](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/open.2), [Apple XNU metadata lookup](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/getattrlist.2).
