# Native editor engine verification

Date: 2026-10-10. Scope: `design-model`, `design-edit`, `design-svg`, `design-render`, `design-export`, document schema, and the independent path editor component. This report does not declare the desktop release or all G4 gates complete.

## Results

`cargo test -p design-model -p design-edit -p design-svg -p design-render -p design-export` completed with **136 passed, 0 failed** on the current Mac environment: edit 15, export 3, model 42, render unit 1, editor features 14, existing render integration 36, SVG integration 25. Empty unit/doc-test suites are excluded from this count.

`cargo clippy -p design-model -p design-edit -p design-svg -p design-render -p design-export --all-targets -- -D warnings` completed successfully. The independent UI checks completed with **7 path editor tests** (5 parser/handle tests and 2 component tests) and TypeScript type checking.

After final App integration, the complete UI suite completed with **28 passed, 0 failed** in 6 test files, and `npm run build` completed successfully. Six duplication helper tests cover same-parent subtree placement, duplicate ancestor/child selection, retained locks/styles/variant fit choices, affine/clip/mask offsets and collision rejection. Five App tests use a mocked native bridge to verify damaged recovery display, failure resynchronization, exact-font and Save As routing, shortcut guards, duplication and active-layout nudge. These bridge tests verify frontend operation boundaries; the real OS picker/native application checks are recorded separately by the desktop host and root verification.

A subsequent final-review correction extended `SetImageAsset` to authoring logo nodes. The updated `design-edit` suite completed with **16 passed, 0 failed** and clippy completed without warnings. Its new regression checks production refusal, missing assets, image/vector typing, brand logo restrictions and unchanged logo geometry. The earlier full-engine count above records the measured 136-test run before this additional regression; final workspace verification records the updated combined total.

## Verified behavior

| Area | Evidence |
| --- | --- |
| Document contract | Old core versions read; unknown/version-incompatible fields, bad references and UTF-8 run ranges are rejected. Named brand colors/styles and allowed logos survive round trip. |
| Transactions | One-command and batch Undo, stale revision rejection, approval invalidation and explicit final approval, production-only slot history, stable imported catalog order, locked group descendants. |
| Paths | Native cubic/quadratic curves, compound even-odd hole, actual stroke pixels; finite expanded relative coordinates; unsupported UI commands preserve source data and display an error. |
| Fills/effects | Radial center/radius, multiply/screen pixels, ordered shape shadow/blur, clip and inverted/normal alpha mask preserve the previous background. |
| Geometry | Base and explicit variant group affine affect real pixels while child layout remains unchanged; group/ungroup retain IDs and paint order. |
| Text | Explicit Japanese policy keeps source UTF-8 offsets; declared font/color runs and feature tags reach native shaping. Missing resources/glyphs and overflow retain stop behavior. |
| Images/logos | Explicit image adjustment/crop protection; WebP orientation applied once and malformed ICC rejected; logo clear space, transformed minimum width and parent aspect distortion rejected. |
| Resources | Many individually legal images and heavy output/offscreen combinations exceed aggregate predicted capacity before decode; verified cache is capped. |
| Exports | Genuine PNG/JPEG/WebP/SVG/PDF payloads; lossless WebP straight-alpha round trip; JPEG background and destructive SVG/PDF rasterization require explicit settings; report records conversions. |

## Deliberate supported subset

- WebP export is lossless only. JPEG uses an explicit opaque background.
- Compatible SVG and RGB proof PDF contain a whole-artboard raster and report that fact. Editable vector/text, font embedding, CMYK and PDF/X are unsupported.
- Text runs change declared font and color, inheriting frame size/line spacing. The strict Japanese policy is explicit; vertical/ruby layout is unsupported.
- Group creation requires adjacent siblings. Ungroup of a transformed or affected group is rejected instead of changing the artwork.
- Clear-space overlap uses author-declared targets and conservative frame boxes. Image protected regions are authored normalized regions, without content inference.
- Memory limits use conservative estimates plus cache/decode checks. They do not claim a measured hard OS RSS limit; process isolation and release stress measurements remain separate gates.

Public command, schema, rendering and export contracts are documented in [ADR 0009](../docs/adr/0009-editor-core-and-derivative-exports.md). The renderer build fingerprint includes the source of output/resume algorithms so resumed jobs can reject incompatible builds.

## Native SwiftUI editor checks

`npm run macos:test` runs [EditorChecks.swift](../apps/macos/Tests/EditorChecks.swift) through [test-macos-editor.sh](../scripts/test-macos-editor.sh). It type checks the complete native view/model sources except the application scene entry point, then links the test executable to the release native bridge library. The measured run completed with **16 checks passed**: 10 geometry/context/path cases and 6 direct AppKit composition cases.

Staged Inspector inputs capture their original session, artboard, node and document snapshot. Text/numeric commands retain the original node after layer selection, and a context check inside the serialized edit queue rejects callbacks from a different session or artboard. Regression checks cover original target routing, cross-session/artboard rejection, and direct AppKit confirmation after simulated layer selection. Layer-list selection also commits confirmed input before changing selection.

Geometry cases cover explicit variant text overrides without canonical mutation, a rotated/scaled parent's inverse movement, omission of a group's own affine from its frame, explicit variant group affine updates, same-parent subtree duplication without double-copying selected descendants, relative/smooth Bezier round trip, unsupported arc rejection and finite expanded coordinates.

AppKit cases exercise the actual `ConfirmedTextView` class with `setMarkedText`/`insertText`: marked text adds no transaction, confirmation commits once, pending focus-loss confirmation commits once, cancellation adds no transaction, and mixed/astral plus canonically equivalent composed/decomposed text retain their authored UTF-8 bytes. The executable creates no window, injects no keyboard events, instantiates no broker and makes no network requests. A development build may acquire missing dependencies before the executable runs.

These are native unit checks. They do **not** establish hardware Japanese keyboard input, the selected OS input method, reconversion, VoiceOver operation, live selection/drag workflows or a release gate. Actual hardware IME and Sandbox application workflows remain separate release evidence. The macOS CI job runs these checks after the native release build.
