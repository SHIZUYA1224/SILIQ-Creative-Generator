# Signed native Mac UI verification — 2026-10-10

The locally ad-hoc signed SwiftUI app launched with App Sandbox and user-selected read/write only, without network or temporary-exception entitlements. The tested machine is Apple M1 Max / 32 GiB / macOS 26.3.1. This is a local product verification, not a notarized distribution or all G4 acceptance result.

## Observed through the real application

- Opened the article summary template: one 1200 × 800 artboard, 17 layers, canonical Skia preview and no inspection errors.
- Replaced the headline with two Japanese lines. Switching to the description layer committed only the headline; the description retained its original text. Revision advanced from 1 to 2.
- Undo restored the original headline at revision 3; Redo restored the edited headline at revision 4. Explicit approval advanced the document to revision 5.
- Restarting the app recovered the confirmed revision 5 and its edited content. The GUI displayed the recovery notice and the approved artboard.
- The native Save panel and PNG output panel displayed the selected local destination and editable filename. The automation bridge stopped responding when confirming them. No output file was observed from those automated attempts. A manual Save-panel confirmation remains pending; external user-selected file/folder workflows are not declared passed from the container-only probe below.

The initial native checkpoint failure was traced to the project store's ancestor-directory read traversal, which App Sandbox refused. Darwin now uses metadata-only checks and a single exact-target `O_NOFOLLOW_ANY` directory open, preserving the directory descriptor for atomic operations. Linux retains its component traversal. No extra entitlement was added. The new signed Sandbox probe exercises this previously missed boundary in CI.

Inspector commits capture their original node/session/artboard context. Delayed callbacks from a previous session or artboard are rejected inside the serialized edit queue. File panels now use completion handlers and keep the editor busy while a choice is pending. Cancellation returns without claiming a save. The project save call requests overwrite only when the selected destination already exists; reimported media uses the returned resource ID rather than the catalog's last entry.

## Automated evidence

- Rust workspace: **216 tests passed, 0 failed** (`out/final-workspace-tests.log`).
- Retained React reference UI: **31 tests passed**, plus TypeScript checking.
- Native geometry, commit context, path editing and direct AppKit composition: **16 checks passed**. Whole-app Swift 5 / macOS 13 typechecking succeeded.
- Signed Foundation-only App Sandbox probe: real checkpoint, project save/open equality, native preview, PNG/PDF files, JSON mapping, five-format batch and completed manifest passed. It uses its own test container and no visible UI; see [native broker evidence](native-macos-bridge.md).
- Bundle signature, strict entitlements, bundled `@rpath` broker loading, absence of non-system absolute executable dependencies, and static offline audit passed. Source/license inventory is regenerated for the native product.

Evidence under `out/` is local and not committed as user data or public test artifacts. CI uploads bounded signature, audit and probe results. Hosted CI has not yet been observed passing at the time of this record.

## Outstanding release verification

External OS-picker Save/Open/Export and batch-folder grants, physical Japanese IME/reconversion, attributed runtime traffic observation, isolated first launch, all GUI performance/stress conditions, Developer ID distribution and notarization remain separate checks. The CLI 300-PNG performance run produced all files correctly but exceeded its 180-second target; [the performance report](performance.md) records the unmet target and measurement limits.
