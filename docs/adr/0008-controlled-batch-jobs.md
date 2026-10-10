# ADR 0008 — Controlled batch jobs

Decision date: 2026-10-10.

`job-runner` provides native all-item preflight, progress, cancellation, and
checksum-verified resumption. CSV bytes and document bytes remain fixed for a
job; the host persists the approved snapshot in its app-owned workspace for
restart recovery and requires a fresh OS output-directory grant. No frontend
path command is added. ResourceStore's verified-byte cache fixes font and asset
bytes for native preflight and subsequent rendering.

The default `block-all` policy checks every row and approved variant through the
same CPU renderer before creating any output staging directory. Explicit
`valid-only` is the sole partial-output option and returns
`completed_with_errors`, expected total, successful item records, and all
row/variant/node issues. Dataset shape, approval, path and naming failures always
block the whole job. Filename collisions include portable ASCII case aliases.
The initial per-job guard is 20,000 output items and 20 MiB journal JSON.

The private `.siliq-job-<input digest>` directory lies inside the selected output
directory so publication remains on the same filesystem. Complete PNGs are
flushed and hash-verified, then the journal is atomically replaced and flushed.
Each record includes row, variant, approved fit values, warning reports, output
hash and input digest. The job digest covers exact document/data bytes, variants,
filename recipe, error policy, font backend and native render profile. Cancel
checks happen between native items and publication steps; Skia calls are not
interrupted mid-render. Cancelled or failed jobs retain their verified work.

Publication uses directory-handle-relative no-clobber links. A path renamed or
replaced after OS selection cannot redirect writes. The manifest is committed
last; successful completion removes only known staging files. Interrupted
publication may leave a prefix of verified outputs and a retained job journal;
it is reported as incomplete, and resume verifies all existing bytes before
adopting any item. Different snapshots, malformed state, unexpected files and
checksum mismatches are refused, with no automatic overwrite.

Resume also runs native preflight and compares newly computed output hashes to
saved item hashes. Completed jobs can be rechecked from their final manifest.
No matching journal/manifest means resume fails; selecting an empty directory
never silently starts another job. Cancellation before the first native item
still persists a queued snapshot journal so the host can resume that same job.

The legacy PNG request remains compatible. Explicit export recipe APIs extend
the same plan to row × approved variant × PNG/JPEG/WebP/SVG/PDF. JPEG background
and quality, WebP lossless mode and whole-artboard SVG/PDF rasterization approval
are frozen into the digest. Per-item manifests record the derivative conversion,
loss of editing, alpha handling, RGB proof DPI and resulting bytes' checksum.
The `{format}` placeholder expands to the chosen extension, and existing `.png`
patterns derive the selected formats' extensions in their preflight name list.
Current jobs run one item at a time, within the required maximum of two native workers. Real
process-kill, disk exhaustion and offline release monitoring remain system-level
checks; integration tests verify callback termination, interrupted publication,
same-snapshot resume, corruption refusal, competing output creation, retained
directory grants and explicit partial policy against safe fixtures.
