# AGENTS.md — Engineering Guide for `musicdb`

Welcome to `musicdb`! These rules apply to every change in the repository, whether written by a human or an AI teammate. If a rule ever conflicts with an explicit instruction from the user, follow the user and state which rule you overrode.

## What we're building

A Rust library and CLI for decoding Apple Music `.musiclibrary` bundles (`Library.musicdb` plus `.plist` sidecars):

- **Library (`musicdb`)**: decrypts and parses the binary format into a lossless raw AST, then projects it into a clean domain model (`Library`, `Track`, `Album`, `Artist`, `Playlist`, …).
- **CLI (`musicdb` binary, `src/main.rs`)**: inspection, JSON/SQLite export, and reverse-engineering tools.

Apple Music's binary format is reverse-engineered and still evolving, so the crate lives intentionally at **0.x**. That gives us the freedom to keep refining the API cleanly while staying honest about what is proven versus still being decoded.

## Our two north stars

Every design choice in this repo flows from two invariants:

1. **Bulletproof against untrusted bytes.** Any length, count, offset, or size read from a file could be corrupt or adversarial. Malformed input always produces a clean `Err` — never a panic, abort, hang, or unbounded allocation.
2. **Byte-for-byte lossless round-trip.** `RawLibraryFile::from_raw_bytes(b)?.to_raw_bytes()? == b` holds for every file that parses. We preserve every undecoded byte (`unk_*` fields, `Unknown { raw_bytes }` variants) so future breakthroughs still have the exact raw data. Whenever a change could affect parsing or serialization, prove round-trip fidelity with a test.

## Working in this codebase

Parts of the repo predate this guide, so apply the Boy Scout rule:
- Hold everything you **write or touch** to these standards, leaving every edited function cleaner than you found it.
- Keep each change focused on one clear purpose (one topic, one commit). When you spot a worthwhile large cleanup elsewhere, propose it separately rather than bundling unrelated refactors.

## Definition of done

Checks are tiered by what they catch so day-to-day work stays fast while heavy real-corpus verification runs right when it matters — and automatically across release boundaries. Only call a task done once every applicable gate has actually run and passed; if something blocks you, report it plainly.

### 1. Fast gate (every change)

```sh
cargo fmt --all
cargo clippy --all-targets --no-default-features -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings
cargo test --no-default-features
cargo test --no-default-features --features serde
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --no-default-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

Locally, `tests/corpus.rs` runs fast targeted real-corpus regressions. CI runs without the private corpus and acts as our multi-OS, lint, doc, feature-matrix, and synthetic-test canary. The exhaustive multi-file corpus round-trip test stays `#[ignore]` so debug runs don't repeat it 3× on every commit.

### 2. Wire-format round-trip sweep (parser, crypto, or corpus changes)

Run **once in `--release`** whenever you touch `src/sections/`, `src/crypto.rs`, parsing/crypto dependencies (`binrw`, `aes`, `cipher`, `flate2`), or add `.musicdb` files to `corpus/`:

```sh
cargo test --release --test corpus -- --ignored
```

*Why:* proves byte-for-byte lossless round-tripping across every verified `.musicdb` in `corpus/library-data/` and `corpus/snapshots/` at full speed.

### 3. Semantic calibration (`musicdb sanity-check`)

Run the release binary against 2–3 representative libraries spanning Apple Music versions (e.g., `2024`, `2025`, and latest `2026` in `corpus/library-data/`) whenever you:
- promote an `Unknown<T>` field or refine enum, bitflag, or timestamp semantics (`src/sections/` or `src/model.rs`),
- add or update rules in `src/analysis/sanity.rs`, or
- ingest a new Apple Music version or mutation campaign snapshot.

```sh
cargo run --release -- sanity-check <path/to/Library.musicdb>
```

*Why:* catches semantic surprises (undocumented flags/enums, local vs. UTC timestamps, mirror divergences) that lossless round-tripping alone can't spot.

### 4. Dependency audit (`cargo deny check`)

Run `cargo deny check` whenever you add, remove, or update a dependency (see [Tooling policy](#tooling-policy-clippytoml-and-denytoml)).

### 5. Release boundary (automatic full sweep)

Run **all four** gates (1–4) once whenever:
- the latest commit before your work (`git log -1`) is a bare `x.y.z` release commit (e.g., `0.14.0`) cut by the user, **or**
- you are preparing a release for `cargo publish`.

*Why:* the user cuts `x.y.z` releases manually, so treating a release commit at `HEAD` as an automatic full-sweep trigger catches anything missed across the release boundary without adding a manual checklist.

## Library craft

### Safe parsing & numeric hygiene
- Keep `#![forbid(unsafe_code)]`.
- On any path touching input data: **no** `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!`, and **no** raw slice indexing (`buf[a..b]`) on input-derived bounds. Use `get(..)`, `split_at_checked`, `binrw`, or a checked helper instead.
- **Validate before allocating:** verify every length or count against the bytes remaining in the stream *before* `vec![0; n]`, `Vec::with_capacity(n)`, or `#[br(count = n)]`.
- Use `checked_add` / `checked_sub` for offset math, and `unsigned_abs()` / `checked_*` instead of unchecked negation (`-x`) on signed untrusted `i32`/`i64` values.
- Make integer conversions explicit: `u64::from(x)` to widen, `usize::try_from(x)?` / `u32::try_from(x)?` to narrow. Use bare `as` only when obviously lossless, with a brief comment explaining why.
- Reject `header_size` values below a struct's minimum instead of masking them with `saturating_sub`.
- Cap decompression output sizes.

### Clear, actionable errors
- Return `musicdb::Error` / `MusicDbError` from library code (`anyhow` belongs strictly in the CLI).
- Write error messages in lowercase with no trailing period (e.g., `"failed to read {path}"`), and include helpful context (file path, section name, byte offset) wherever you have it.
- Keep `#[source]` / `#[from]` fields out of the message template (no `{0}` duplication).
- Keep the error enum `#[non_exhaustive]` and trim any variant that is never constructed.
- **Never swallow errors silently** (no `.ok()`, `let _ =`, `.flatten()` on `read_dir`, or `unwrap_or_default()` on I/O): either propagate the error, or record a caller-visible warning for best-effort data like sidecar plists. If something truly needs to be ignored, leave a short comment explaining why.

### Public API design
- **Future-proof types:** mark every public struct and enum `#[non_exhaustive]`, except single-field newtypes (`PersistentId`, `ParsePersistentIdError`, `Unknown<T>`, `AppleTimestamp`). Read-only domain records (`Track`, `Album`, `Artist`, `Playlist`, `LibraryMetadata`) may expose `pub` data fields for ergonomic access and pattern matching, while container internals (such as `Library` indexes) stay private behind methods. Derived values (`stars()`, `flags()`, `raw()`) belong as methods, not stored fields.
- **Format types & unknowns in `src/sections/`:**
  - Wrap every undecoded binary field in `Unknown<T>` (never bare `u8`, `u32`, `[u8; N]`, or `Vec<u8>`) and register it in `VisitUnknowns` via `visit_unknowns!`. Use `Unknown<TailBytes>` for hot per-item variable-length header tails and `Unknown<Vec<u8>>` for rare once-per-section tails.
  - Split partially-decoded byte arrays cleanly into named known fields plus `Unknown<T>`.
  - Promote fields out of `Unknown<T>` once verified rather than dereferencing `*h.unk_*` in `model.rs` or `sqlite.rs`.
  - Use `AppleTimestamp` for 32-bit Apple epoch timestamps in `src/sections/`, and `SectionKind` for `hsma` section filtering and dispatch. Convert timestamps via `.and_then(AppleTimestamp::to_utc)` before chaining fallbacks — chaining `Option<AppleTimestamp>` with `.or()` first lets `Some(AppleTimestamp(0))` silently shadow the fallback.
  - Keep `VisitUnknowns` / `UnknownVisitor` format introspection in `src/sections/` and `analysis` reports in the CLI.
- **Clean public surface:** keep internal helpers, serde `with` modules, research utilities, and test-only helpers `pub(crate)` for API clarity (not out of fear of `0.x` breaks) — never make something `pub` when only tests or the CLI need it.
- **Minimal dependency bleed:** ask before putting third-party dependency types (like `plist::Value`) in public domain signatures or adding new public dependency error wrappers. Existing `#[from]`/`#[source]` wrappers in `MusicDbError` (`binrw::Error`, `plist::Error`, and `#[cfg(feature = "sqlite")] rusqlite::Error`) are fine while at `0.x`.
- **Strong, idiomatic Rust types:**
  - Model persistent IDs with `PersistentId(u64)` (`Display`/`FromStr` as 16-digit uppercase hex), not paired `String` + `u64` fields.
  - Name types after their meaning, putting the 4-byte file magic in the doc comment (`` /// Album record (`iama`). ``). Keep all type names distinct beyond letter casing so case-insensitive filesystems (macOS/Windows) and rustdoc stay happy.
  - Prefer standard traits (`From`, `TryFrom`, `FromStr`, `Display`) over ad-hoc `parse` / `from_raw` / `to_string` methods — reserve `from_raw` for raw pointers.
  - Derive common traits (`Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`, `Default`) where sensible and implement `Display` for user-facing enums. Never persist `{:?}` output as stored data (SQLite, JSON keys, etc.).
  - Return iterators (`impl Iterator<Item = &Track> + '_`) instead of `Vec<&Track>`, use small named structs instead of unnamed public tuples (`Vec<(u32, String, usize)>`), and take `impl AsRef<Path>` for path parameters.
  - Store one canonical typed enum (with an `Unknown(raw)` variant + `.raw()` method) instead of duplicating raw and decoded fields.
  - Build an index once for repeated ID lookups rather than scanning linearly in loops.

### Documentation & our reverse-engineering journal
- Give every `pub` item a doc comment, with `# Errors` on fallible functions and `# Panics` on anything that can panic. Use intra-doc links (`` [`Track`] ``) for crate items.
- Document raw-format fields with their byte offset and meaning (`/// Offset 24..28: …`), marking hypotheses clearly as guesses until proven.
- **Capture every discovery permanently:** reverse-engineering wins include negative results! Any evidence that supports, sharpens, promotes, weakens, or **falsifies** a hypothesis about `src/sections/` — along with narrowed unknowns and mutation ideas — belongs in the repo right away: update `src/sections/` doc comments, record the evidence (and correct superseded claims) in workspace `history.md`, and add proposed mutations to `next-steps.md`. Do this even when no code changes or a proposal is declined so discoveries never stay trapped in chat.
- `README.md` is compiled as crate docs and doctested: keep every Rust block compiling and in sync with API changes.
- Pair new user-facing features with an example (a doctest or a file in `examples/`).

### Dependencies, features & licensing
- Keep the library and CLI in a **single crate (`musicdb`)** so `cargo add musicdb` and `cargo install musicdb` share one package name (stay single-crate unless asked to split).
- Prefer the standard library; every new dependency should have a clear reason and pass `cargo deny check`.
- Keep CLI and research-only dependencies (`clap`, `clap_complete`, `anyhow`, `tar`, `directories`, `serde_json`, …) optional for library users behind plain Cargo feature names (`serde`, `sqlite`, `cli` — not `use-serde` or `with-sqlite`).
- Keep the license as standalone **`Apache-2.0`** with [`NOTICE`](NOTICE) so Section 4(d) attribution notices stay mandatory (never change it to `MIT OR Apache-2.0`).
- Stay within the MSRV declared by `rust-version` in `Cargo.toml`.

### Code shape & safe file writes
- Aim for procedural functions under ~80 lines by extracting named helpers for complex branches and mapping logic. Flat 1-to-1 lookup tables and full-row SQL column bindings may use a local `#[expect(clippy::too_many_lines, reason = "...")]` when splitting hurts clarity.
- Prefer `#[derive(Default)]` and `..Default::default()` over hand-initializing large structs.
- Always list explicit column names in SQL `INSERT` statements.
- Keep format lookup tables (e.g., smart playlist selection methods or field names) single-sourced or guarded by parity tests so `src/sections/` and `src/model.rs` never drift.
- Make output file writes atomic: write to a sibling temporary file with both PID and an atomic sequence counter (`.{name}.{pid}.{seq}.tmp`), finish encoders explicitly (`GzEncoder::finish()` rather than relying on `Drop`), and `rename` into place on success.

## CLI & reverse-engineering tools (`src/main.rs` and CLI-only modules)

- Keep `main` lean (parse arguments and dispatch) with one focused function per subcommand (`fn run_info(args) -> anyhow::Result<()>`).
- Format human-readable output via `Display` impls on report types instead of chains of `println!`.
- `anyhow` with `.context("…")` — and `unwrap`/`expect` with a clear explanation — are welcome on the CLI side.
- Keep reverse-engineering and research tooling (analysis reports, offset diffing, library donation) CLI-only rather than in the library's public API.
- Put experimental or one-off RE probes in `src/analysis/adhoc/` behind `--features adhoc`: **one file per probe**, shared corpus helpers in `src/analysis/adhoc/helpers.rs`, wired under `musicdb adhoc <probe>` in `src/analysis/adhoc/mod.rs`. That keeps default CLI builds crisp and lets probes run individually.

## Testing & fixtures

- **Synthetic fixtures powered by Rush:** never commit real Apple Music libraries or anything derived from them (they carry personal data like purchaser names, emails, and account IDs). Build synthetic fixtures in code (see the synthetic round-trip test in `src/lib.rs`) using **Rush** albums, tracks, and metadata wherever applicable!
- **Private corpus ergonomics:** tests gated on `MUSICDB_CORPUS_DIR` gracefully return early when the corpus isn't present — preserve that behavior so contributors and CI stay green.
- **Verified corpus provenance only:** restrict real-corpus testing and reverse-engineering strictly to captured libraries and controlled mutation snapshots in `corpus/library-data/` and `corpus/snapshots/`. Leave `corpus/libraries-music-samples/` completely untouched (unproven provenance and unverifiable corruptions — never test against it, investigate it, file issues from it, or work around it).
- **Red-green regression tests:** pair every bug fix with a regression test (for parser bugs, a minimal malformed byte sequence). Run the new test against the old code first to watch it fail for the exact right reason, then land the fix and watch it turn green.
- Use `tempfile::tempdir()` for filesystem tests rather than fixed paths in `/tmp` or `std::env::temp_dir()`.
- Place unit tests next to the code (`#[cfg(test)] mod tests`) and public API / CLI tests under `tests/`.
- `proptest` is a great fit for round-trip invariants (`parse(serialize(x)) == x`).

## Privacy

- Only log, print, or include personal data in errors when the user explicitly asked for it.
- Keep real user library contents out of code, tests, docs, commit messages, and issues.
- Before writing any file that exports or packages library data, tell the user clearly what will be written.

## Versioning & commits

- Because the crate is **0.x**, minor releases (`0.x`) are free to improve and break the API while patch releases (`0.a.x`) stay semver-safe. Build the cleanest API rather than contorting code to dodge a `0.x` break, and highlight any breaking changes (and why they're worth it) in your summary.
- When to cut a `0.x` vs. `0.a.x` release — and bumping `version` in `Cargo.toml` — is strictly the user's call (assume the user knows this; only hand-hold if evidence contradicts it). Never bump `version` in `Cargo.toml` unless asked.
- Use **Conventional Commits** (`feat:`, `fix:`, `perf:`, `refactor:`, `docs:`, `test:`, `chore:`), adding `!` for breaking changes (`feat!:`, `fix!:`, `perf!:`, `refactor!:`). Keep subjects imperative, specific, and under ~72 characters (not vague messages like `doc` or `update`).

## Tooling policy: `clippy.toml` and `deny.toml`

Both files live at the repository root and set project policy — treat edits to them with the same care as this guide.

### `clippy.toml`
- `Cargo.toml` (`[lints.clippy]` / `[lints.rust]` and `rust-version`) controls lint levels and MSRV; `clippy.toml` only tunes lint parameters (**never** set `msrv` in `clippy.toml` — Clippy reads `rust-version` from `Cargo.toml`), e.g.:
  ```toml
  allow-unwrap-in-tests = true
  allow-expect-in-tests = true
  allow-panic-in-tests = true
  allow-indexing-slicing-in-tests = true
  too-many-lines-threshold = 80
  ```
- Fix lint warnings in code rather than loosening `clippy.toml` or `[lints]`.
- When a lint genuinely misfires in a single spot, silence it locally with `#[expect(..., reason = "...")]` (never `#[allow]` or crate-wide `#![allow(...)]`), so the compiler flags it once the suppression is no longer needed:
  ```rust
  #[expect(clippy::cast_possible_truncation, reason = "value is masked to 16 bits above")]
  ```

### `deny.toml`
[`cargo-deny`](https://embarkstudios.github.io/cargo-deny/) guards the dependency graph across four areas (run `cargo deny check` after any dependency change; advisories require network access for RustSec):
- **advisories:** known vulnerabilities and unmaintained crates. Prefer upgrading or replacing affected crates; adding an `ignore` entry requires the advisory ID, a reason, and user approval.
- **licenses:** permissive allow list (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0). If a dependency fails the license check, stop and ask the user instead of expanding the allow list unilaterally.
- **bans:** no `*` wildcard versions; duplicate versions warn (try unifying versions rather than reaching for `skip`/`skip-tree`).
- **sources:** crates.io only (no unknown registries or git dependencies).

## Hard lines

- Never make parsing lossy to "simplify" a structure.
- Never make something `pub` when only tests or the CLI need it.
- Never catch-all-and-ignore errors to make a feature "work".
- Never commit corpus files or break the corpus-test skip behavior when `MUSICDB_CORPUS_DIR` is unset.
- Never run tests against `corpus/libraries-music-samples/` or chase/report issues that only appear there.
- Never loosen lint, `clippy.toml`, or `deny.toml` rules just to get a green build.
- Never report something as verified unless you actually ran the command and saw it pass.
