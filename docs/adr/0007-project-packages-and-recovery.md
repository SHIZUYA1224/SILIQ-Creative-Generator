# ADR 0007 — Project packages and recovery

Decision date: 2026-10-10.

`project-store` owns `.siliqcg` persistence. A portable project contains
`manifest.json`, `design.json`, `fonts/manifest.json` and exactly the referenced
resolved asset files and explicitly embed-confirmed font files. The manifest
records the package format, core schema, minimum engine version, document ID,
revision, and every other entry's SHA-256 and size. Machine paths are excluded.
Optional brand snapshots and thumbnails await a versioned model contract; an
unknown entry is rejected instead of silently discarded on a later save.

Package version `1.0.0` uses Stored ZIP entries. Assets and fonts frequently
already contain compressed data; this profile avoids adding runtime decompression
and encryption code. Deflate, ZIP64, multi-volume containers, encrypted entries,
extra fields, links and other entry types are explicitly unsupported. Save/open
works offline. The raw ZIP directory is checked before the library is allowed
to allocate entries, including duplicates, ASCII content-addressed paths,
central/local header consistency, sizes, overlapping ranges and container limits.
The CRC, SHA-256, JSON syntax, schema and resource permission checks still run.
Hashes detect corruption; they do not establish a trusted author.

Saving writes a new sibling temporary file, flushes it, fully verifies it, then
commits it through the retained directory handle. Ordinary saves use atomic
no-clobber linking; overwrites require explicit host intent and replace the
directory entry atomically. Open uses the host's retained regular-file handle.
All path components are opened without following symlinks, and multiply-linked
project files are refused. Archive extraction only creates known regular files
inside a new child of the host's private app workspace.

Recovery writes complete package checkpoints before committing length-framed,
checksum-protected sequential journal records. It recovers the last verifiable
checkpoint, reports a damaged suffix, and leaves the journal and the user's
saved original untouched. Eight durable package generations are retained;
garbage collection happens after the newest journal commit is flushed. Same
machine reference-only fonts are retained as verified private workspace files,
never included in a portable or checkpoint ZIP. Missing exact fonts remain
blocking rendering issues; opening and editing the document remains possible.

New dependencies are exact-pinned: [zip 8.6.0](https://docs.rs/zip/8.6.0/zip/)
(MIT, all default features disabled) and
[rustix 1.1.5](https://docs.rs/rustix/1.1.5/rustix/)
(Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT, `std` and `fs` only).
The crate adds no network, async, AI or cryptography dependency. Windows support
is not established by this POSIX directory-handle implementation.

Integration tests exercise exact native round trips and bytes, permissions,
deterministic packages, explicit overwrite, failed-save preservation, grants
surviving a directory rename, traversal/case/NUL, duplicate entries, symlink and
hardlink refusal, huge size headers, compression/encryption refusal, corrupt
CRC/hash, unknown versions, torn journals, corrupt latest checkpoints and bounded
retention. Real disk exhaustion and power-loss testing remain release checks.
