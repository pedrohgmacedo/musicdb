# musicdb

[![Crates.io](https://img.shields.io/crates/v/musicdb.svg)](https://crates.io/crates/musicdb)
[![docs.rs](https://docs.rs/musicdb/badge.svg)](https://docs.rs/musicdb/latest/musicdb/)
[![CI](https://img.shields.io/github/actions/workflow/status/pedrohgmacedo/musicdb/ci.yml?branch=master&label=CI)](https://github.com/pedrohgmacedo/musicdb/actions/workflows/ci.yml)

`musicdb` is a Rust library and CLI for reading Apple Music `.musiclibrary` bundles (`Library.musicdb` and its sidecar `.plist` files) on macOS and Windows.

Apple doesn't document the `.musicdb` format, and modern Apple Music no longer keeps an `iTunes Music Library.xml` file updated on disk. This crate decrypts and decompresses `Library.musicdb`, parses the binary sections, and builds a typed library model you can query from Rust or export to SQLite and JSON.

> **Status:** The binary format is reverse-engineered and still changing as new Apple Music versions ship, so this crate stays on `0.x` on purpose. It has been tested against libraries from Apple Music 1.4 through 1.7 (format `20.2` through `23.1`) on macOS and Windows.

## Installation

To install the `musicdb` command-line tool:

```sh
cargo install musicdb
```

To use `musicdb` as a library in a Rust project without pulling in CLI dependencies or bundled SQLite:

```sh
cargo add --no-default-features musicdb
```

### Cargo features

Default features are `["serde", "sqlite", "cli"]` so `cargo install musicdb` works out of the box.

- ***(none)***: Core decryption, raw section parser (`musicdb::sections`), and domain model (`Library`, `Track`, `Album`, `Artist`, `Playlist`, `SocialProfile`).
- **`serde`**: `Serialize` / `Deserialize` implementations across the domain model and raw AST.
- **`sqlite`**: `Library::export_sqlite` (uses bundled `rusqlite`) and `Library::to_json_pretty` (implies `serde`).
- **`cli`**: Builds the `musicdb` binary (implies `serde` and `sqlite`).

## Library usage

`Library::open` accepts a path to `Library.musicdb`, a `.musiclibrary` bundle directory, or a parent directory containing one (or use `Library::open_default()` to look in your OS Music folder):

```rust,no_run
use musicdb::Library;

fn main() -> musicdb::Result<()> {
    let lib = Library::open("path/to/Apple Music Library.musiclibrary")?;

    println!(
        "{} tracks, {} albums, {} artists, {} playlists (app {})",
        lib.tracks.len(),
        lib.albums.len(),
        lib.artists.len(),
        lib.playlists.len(),
        lib.metadata.app_version,
    );

    if let Some(pl) = lib.playlist_by_name("Rush: Studio") {
        for track in lib.playlist_tracks(pl) {
            println!(
                "{} - {} ({} stars)",
                track.artist.as_deref().unwrap_or("?"),
                track.title.as_deref().unwrap_or("?"),
                track.stars(),
            );
        }
    }

    #[cfg(feature = "sqlite")]
    lib.export_sqlite("music.sqlite")?;

    Ok(())
}
```

When opened from a `.musiclibrary` bundle, `Library::open` also reads `Preferences.plist` and `preferences/*.plist` (`Songs.plist`, `Playlist_<ID>.plist`) for view and sorting settings. Any non-fatal `.plist` read issues are recorded in `Library::sidecar_warnings()`.

If you need direct access to the binary layout, `musicdb::sections::RawLibraryFile` exposes the raw `hfma` / `hsma` / `boma` AST (keeping undecoded bytes intact so parsed files round-trip back to identical bytes) and `RawLibraryFile::open_filtered` lets you skip sections you don't need (see `examples/filtered_sections.rs`).

## CLI usage

Most inspection commands accept an optional path and fall back to your default Apple Music library when omitted.

### Exporting to SQLite or JSON

```sh
# Export to a normalized SQLite database
musicdb export-sqlite "path/to/Apple Music Library.musiclibrary" music.sqlite

# Export the parsed Library model as pretty-printed JSON (writes to stdout if -o is omitted)
musicdb export-json "path/to/Apple Music Library.musiclibrary" -o music.json
```

### Inspecting a library

```sh
# Header summary, hsma section layout, and round-trip check
musicdb info "path/to/Apple Music Library.musiclibrary"

# Search tracks by 16-char hex Persistent ID or title/artist/album substring
musicdb query-track "path/to/Apple Music Library.musiclibrary" "Tom Sawyer"
musicdb query-track "path/to/Apple Music Library.musiclibrary" 47A081DF30DA6FB2 --json

# List all playlists, or inspect one playlist (including smart playlist rules)
musicdb query-playlist "path/to/Apple Music Library.musiclibrary"
musicdb query-playlist "path/to/Apple Music Library.musiclibrary" "Rush: Studio"

# Dump a binary or XML sidecar plist
musicdb plist "path/to/Apple Music Library.musiclibrary/preferences/Songs.plist"
```

### Format research & reverse engineering

```sh
# Summarize undecoded header fields and boma subtype counts
musicdb unknowns "path/to/Apple Music Library.musiclibrary"

# Query specific unknown fields by struct/section selector, entity, or value
musicdb query-unknowns "path/to/Apple Music Library.musiclibrary" --list-fields
musicdb query-unknowns "path/to/Apple Music Library.musiclibrary" -f iama.unk_28 --nonzero

# Diff two .musicdb snapshots by Persistent ID and 4-byte offsets
musicdb diff before/Library.musicdb after/Library.musicdb

# Cross-check parsed fields against an exported iTunes Music Library.xml
musicdb xml-check path/to/Library.musicdb "path/to/iTunes Music Library.xml"

# Decrypt and decompress Library.musicdb to raw bytes
musicdb decrypt path/to/Library.musicdb raw_library.bin

# Package .musiclibrary metadata (without audio or artwork files) for format research
musicdb donate-library
```

## SQLite schema

`Library::export_sqlite` (`musicdb export-sqlite`) writes eight tables and two convenience views:

| Table / View | Contents |
| :--- | :--- |
| `library_metadata` | Single row with library IDs, app/format version, timezone offset, media folder paths, and global preferences. |
| `tracks` | Track tags, sort strings and modes, audio/codec properties, gapless metadata, ratings, play/skip counts, UTC timestamps, file URLs, and iCloud/Store IDs. |
| `albums` | Album records (`iama`), including ratings, favorites, Apple Music catalog genre, motion artwork URLs, editorial notes, and Store/cloud IDs. |
| `artists` | Artist records (`iAma`), sort names, favorites, Store/cloud IDs, and artwork references. |
| `playlists` | Folders, user playlists, smart playlists, subscribed catalog playlists, and system playlists, plus smart-playlist options and view preferences. |
| `playlist_items` | Ordered `(playlist_persistent_id, position, track_persistent_id)` rows. |
| `smart_playlist_rules` | Flattened `SLst` rule conditions with nesting `depth`, `conjunction`, field/operator names, human-readable `description`, and `value_json`. |
| `social_profiles` | Apple Music social profiles (`isma`) referenced by shared playlists. |
| `v_playlist_tracks` | View joining `playlists`, `playlist_items`, and `tracks` in playlist order (excluding the internal master playlist, folder unions, and hidden cloud-only container). |
| `v_tracks_enriched` | View joining `tracks` with linked `albums` and `artists` fields. |

## Privacy note

`Library.musicdb` stores personal metadata alongside your music catalog, including your Apple Store account ID and purchaser names/emails on iTunes Store tracks. Any JSON/SQLite export or archive created by `donate-library` / `donate-previous-libraries` will include those fields.

## License

Licensed under the Apache License, Version 2.0 (see `LICENSE` and `NOTICE`).
