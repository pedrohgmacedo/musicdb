# AGENTS.md — rules for working on `musicdb`

These rules apply to every change in this repository, whether a human or an AI agent writes it.
If a rule conflicts with an explicit instruction from the user, follow the user and say which rule you broke.

## What this project is

A Rust library and CLI for reading Apple Music `.musiclibrary` bundles (`Library.musicdb` plus the `.plist` sidecars).

- **Library** (`musicdb`): parses and decrypts the file, keeps a lossless raw AST, and builds a domain model (`Library`, `Track`, `Album`, `Artist`, `Playlist`, …).
- **CLI** (`musicdb` binary, `src/main.rs`): inspection, export (JSON/SQLite), and reverse-engineering tools.

The format is reverse-engineered and still changing. The crate is at **0.x** on purpose. Don't make anything look more stable than it is.

## Definition of done

Before you say a change is finished, run these and make sure they pass:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

If you add or update a dependency, also run `cargo deny check` (see [Tooling configuration](#tooling-configuration-clippytoml-and-denytoml)).

Don't report success if any of these fail. If you can't fix a failure, say so plainly.

## Working with existing code

Much of the existing code predates these rules.

- Apply the rules to code you **write or touch**. Leave every function you edit in better shape than you found it.
- **No drive-by mass refactors.** Don't reformat, rename or restructure unrelated code in the same change. Propose large cleanups separately.
- Keep each change to one topic: one commit, one purpose.

## Two rules that come before everything else

1. **Input files are untrusted.** Every length, count, offset or size read from a file can be wrong or malicious. A corrupt file must produce an `Err`, never a panic, an abort, a hang or an unbounded allocation.
2. **Lossless round-trip.** `RawLibraryFile::from_raw_bytes(b)?.to_raw_bytes()? == b` must hold for every file that parses. Unknown bytes are kept (`unk_*` fields, `Unknown { raw_bytes }` variants), never dropped. If a change can break this, add a test that proves it doesn't.

## Library code rules

### Parsing and safety
- No `unwrap`, `expect`, `panic!`, `unreachable!` or `todo!` on any path that touches input data. No slice indexing (`buf[a..b]`) on input-derived bounds. Use `get(..)`, `split_at_checked`, `binrw` or a checked helper.
- **Check the length before allocating.** Check every length or count against the bytes actually left in the stream *before* `vec![0; n]`, `Vec::with_capacity(n)` or `#[br(count = n)]`. Use `checked_add`/`checked_sub` for arithmetic on offsets.
- No silent narrowing casts. Use `u64::from(x)` to widen and `usize::try_from(x)?` / `u32::try_from(x)?` to narrow. A bare `as` is only acceptable where the conversion is obviously lossless and a comment says so.
- Reject `header_size` values below a structure's minimum. Don't hide them with `saturating_sub`.
- Decompression must have an output size limit.
- `#![forbid(unsafe_code)]` stays.

### Errors
- Library functions return the crate's own error type (`musicdb::Error` / `MusicDbError`). **Never `anyhow` in the library.** `anyhow` is only for the binary.
- Error messages are lowercase with no trailing period: `"failed to read {path}"`.
- **Don't print the source twice.** If a field is the `#[source]`/`#[from]`, don't also put it in the message with `{0}`.
- Add context so the error can be acted on: the file path, section name and byte offset where you know them.
- The error enum is `#[non_exhaustive]`. Don't add variants that are never constructed.
- **Don't ignore errors silently** (`.ok()`, `let _ =`, `.flatten()` on `read_dir`, `unwrap_or_default()` on I/O). Either propagate the error, or, for best-effort data such as sidecar plists, record a warning the caller can see. Write a comment wherever you ignore something on purpose.

### Public API design (Rust API Guidelines)
- **Every new public struct and enum is `#[non_exhaustive]`.** Prefer private fields with accessor methods for domain types. Ask before adding a new `pub` field to an existing domain struct.
- **Be careful with what you make `pub`.** Helpers, serde `with` modules and research utilities are `pub(crate)`. Everything `pub` falls under semver.
- **Don't put dependency types in public signatures** (e.g. `binrw::Error`, `rusqlite::Error`, `plist::Value`) without asking. Each one makes that dependency's major updates breaking changes for us. Wrap them as opaque `#[source]` errors instead.
- **Use newtypes for identifiers:** persistent IDs are a `PersistentId(u64)` with `Display`/`FromStr` as 16-digit uppercase hex, not a `String` and a `u64` side by side.
- **Name types after what they mean.** Put the 4-character file magic in the doc comment (`/// Album record (`iama`).`). **Never** add type names that differ from another type only by letter case. They collide on case-insensitive file systems (macOS/Windows), and rustdoc output breaks.
- **Use standard conversion traits:** `From`/`TryFrom`/`FromStr`/`Display` instead of ad-hoc `parse`/`from_raw`/`to_string` methods. Don't name safe constructors `from_raw`; in Rust that name means raw pointers.
- **Implement the common traits** on public types where they make sense: `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`, `Default`. Enums shown to users implement `Display`. **Never use `{:?}` output as data** (in SQLite, JSON keys, or anything stored).
- **Return iterators, not `Vec`s:** `impl Iterator<Item = &Track> + '_` instead of `Vec<&Track>`.
- **No public tuples with unnamed meaning** (`Vec<(u32, String, usize)>`). Define a small struct with named fields.
- **Raw and decoded duplicates:** store the typed enum (with an `Unknown(raw)` variant) and expose `raw()` if needed. Don't add a second field for the same value. Derived values (e.g. stars from rating) are methods, not fields.
- **Path parameters** take `impl AsRef<Path>`.
- **Lookups by ID** should not scan linearly on every call when called in loops. Build an index once.

### Documentation
- Every `pub` item has a doc comment. Fallible functions have an `# Errors` section, and anything that can panic has `# Panics`.
- Raw-format fields document their **byte offset and meaning** (existing style: `/// Offset 24..28: …`). Mark guesses as guesses.
- Use intra-doc links (`` [`Track`] ``), not plain backticks, for items in this crate.
- `README.md` is included as crate docs and runs as a doctest. **Every Rust code block in it must compile.** If you change an API that the README uses, update the README in the same change.
- New user-facing features need an example, either a doctest or a file in `examples/`.

### Dependencies and features
- Don't add a dependency without a reason you can name. Prefer the standard library.
- **Dependencies that only the CLI or the research tools need** (`clap`, `anyhow`, `tar`, …) must never become required for library users. Gate optional functionality behind a Cargo feature with a plain name (`serde`, `sqlite`, `cli`), not `use-serde` or `with-sqlite`.
- Any new dependency must pass `cargo deny check`.
- Don't use Rust features newer than the MSRV (`rust-version` in `Cargo.toml`).

### Code shape
- Aim for functions under ~80 lines. Split large `match` arms and long mapping code into named helpers.
- Use `#[derive(Default)]` and `..Default::default()` instead of initializing dozens of fields by hand.
- SQL `INSERT` statements always list their column names.
- When writing output files, write to a temporary file in the same directory and `rename` on success. Finish encoders explicitly (`GzEncoder::finish()`); never rely on `Drop` to flush.

## CLI rules (`src/main.rs` and CLI-only modules)
- One function per subcommand (`fn run_info(args) -> anyhow::Result<()>`). `main` only parses arguments and dispatches.
- Human-readable output comes from `Display` impls on the report types, not from long chains of `println!`.
- `anyhow` with `.context("…")` is fine here, and so are `unwrap`/`expect` where an explanation is given.
- Reverse-engineering and research code (analysis reports, offset diffing, library donation) belongs to the CLI side, not the library's public API.

## Tests
- **Never commit real Apple Music libraries or anything derived from them.** They contain personal data (purchaser names and emails, account IDs). Build fixtures **synthetically** in code (see the synthetic round-trip test in `src/lib.rs`).
- Tests against the private corpus (`MUSICDB_CORPUS_DIR`) intentionally return early when it isn't present. **Keep that behavior.** Don't remove these tests or make them fail without the corpus.
- **Every bug fix comes with a regression test.** For parser bugs, add the minimal malformed byte sequence as a test.
- Use `tempfile::tempdir()` for filesystem tests. Never use fixed paths in `/tmp` or `std::env::temp_dir()`.
- Put unit tests next to the code they test (`#[cfg(test)] mod tests`). Put tests of the public API and the CLI under `tests/`.
- Round-trip properties (`parse(serialize(x)) == x`) are good candidates for `proptest`.

## Privacy
- Don't log, print or include in error messages more personal data than the user asked for.
- Never paste contents of a user's library into code, tests, docs, commit messages or issues.
- Anything that exports or packages library data must say so clearly to the user before writing files.

## Versioning and commits
- The crate is **0.x**. Under Cargo's semver rules, a breaking change bumps `0.MINOR` and a compatible change bumps `0.x.PATCH`.
- **Don't change `version` in `Cargo.toml` unless asked.** Mention breaking changes explicitly in your summary.
- Commit messages follow **Conventional Commits**: `feat: …`, `fix: …`, `refactor: …`, `docs: …`, `test: …`, `chore: …`, and `feat!:` / `fix!:` (or a `BREAKING CHANGE:` footer) for breaking changes. The subject line is imperative and under ~72 characters. Don't write messages like `doc` or `update`.

## Tooling configuration: `clippy.toml` and `deny.toml`

Both files live at the repository root and set **project policy**. Treat changes to them like changes to this file.

### `clippy.toml`
- **Lint levels** (which lints warn or deny) are set in `Cargo.toml` under `[lints.clippy]` / `[lints.rust]`, **not** in `clippy.toml`. `clippy.toml` only sets **parameters** for lints, for example:
  ```toml
  allow-unwrap-in-tests = true
  allow-expect-in-tests = true
  allow-panic-in-tests = true
  allow-indexing-slicing-in-tests = true
  too-many-lines-threshold = 80
  ```
- Don't set `msrv` there. Clippy reads `rust-version` from `Cargo.toml`.
- **Don't fix a warning by loosening `clippy.toml` or the `[lints]` table.** Fix the code.
- If a lint really is wrong in one place, suppress it **locally and with a reason**:
  ```rust
  #[expect(clippy::cast_possible_truncation, reason = "value is masked to 16 bits above")]
  ```
  Use `#[expect]`, not `#[allow]`: `#[expect]` warns once the suppression is no longer needed. Never add crate-wide `#![allow(...)]`.

### `deny.toml`
[`cargo-deny`](https://embarkstudios.github.io/cargo-deny/) checks the dependency tree in four areas:
- **advisories:** known vulnerabilities and unmaintained crates (RustSec).
- **licenses:** dependency licenses must be on the allow list (permissive licenses such as MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0).
- **bans:** no wildcard (`*`) version requirements. Duplicate versions of the same crate produce a warning.
- **sources:** crates come from crates.io only, with no unknown registries or git dependencies.

Run `cargo deny check` after adding, removing or updating any dependency. The advisory check needs network access to fetch the RustSec database.

- If a new dependency fails the **license** check, **stop and ask the user**. Don't extend the allow list on your own.
- If an **advisory** fires, prefer upgrading or replacing the dependency. An `ignore` entry needs the advisory ID, a `reason`, and the user's approval.
- Don't add `skip`/`skip-tree` entries for duplicates just to get a clean run. Try to unify the versions first.

## Don't
- Don't make parsing lossy to "simplify" a structure.
- Don't make `pub` things that only tests or the CLI need.
- Don't catch-all-and-ignore errors to make a feature "work".
- Don't change the corpus-test skip behavior or commit corpus files.
- Don't loosen lint, clippy or deny configuration to get a green build.
- Don't claim something is verified unless you actually ran the command and saw it pass.
