# `musicdb-rs`

[![Crates.io](https://img.shields.io/crates/v/musicdb-rs.svg)](https://crates.io/crates/musicdb-rs)

Pure-Rust parser, domain model, SQLite/JSON exporter, and reverse-engineering CLI for Apple Music `.musiclibrary` bundles (`Library.musicdb`, `Library Preferences.musicdb`, and `preferences/*.plist`).

Verified with **100% byte-identical lossless round-tripping** across Apple Music versions **1.4 (`format 20.2`, 2024)**, **1.5 (`format 20.20` & `20.40`, 2025)**, **1.6 (`format 21.40`, 2026)**, and **1.7 (`format 23.1`, 2026)**.

---

## Architecture

`musicdb-rs` is structured into three cleanly isolated layers so downstream consumers can query Apple Music data through meaningful domain primitives without coupling to binary layout details:

1. **Layer 1 — Binary Envelope & Lossless Section AST (`musicdb::crypto`, `musicdb::sections`)**:
   - Decrypts the AES-128-ECB payload (`DEFAULT_MUSICDB_KEY = b"BHUILuilfghuila3"`) after the uncompressed 160-byte outer `hfma` header and inflates the zlib stream.
   - Parses every `hsma` section (`InnerHfma`, `plma` LibraryMaster, `lama`/`iama` Albums, `lAma`/`iAma` Artists, `Lsma`/`isma` SocialProfiles, `ltma`/`itma` Tracks, `lPma`/`lpma` Playlists, `LPma` Padding) and every `boma` subtype into strongly-typed `binrw` structs while preserving unknown byte slices for 100% byte-for-byte re-serialization (`RawLibraryFile::to_raw_bytes`).
2. **Layer 2 — Meaningful Domain Model (`musicdb::model`)**:
   - Exposes `Library`, `LibraryMetadata`, `Track`, `Album`, `Artist`, `Playlist`, and `SocialProfile`.
   - Translates binary bitmasks, magic strings, and big-endian rule trees into self-explanatory domain types:
     - `SmartPlaylistConfig`, `SmartRuleGroup`, `SmartRuleNode`, `SmartCondition`, `SmartField`, `SmartOperator`, `SmartRuleValue`, `SmartPlaylistLimit`, `SmartLimitUnit`, `SmartSelectionMethod`, `RelativeTimeUnit`
     - `EqualizerPreset` (translates `#!#100#!#`..`#!#121#!#` built-in preset codes into `"Acoustic"`, `"Rock"`, etc.)
     - `TrackSortModes` & `SortMode` (`Auto`, `ManualOverride`, `DefaultCleared`)
     - `SuggestionFlag` (`Neutral`, `Favorite`, `SuggestLess`, `Removed` + `is_locally_modified`)
     - `CloudStatus` (`Matched`, `Uploaded`, `AppleMusic`, `Error`, `NoLongerAvailable`)
     - `DownloadedStatus`, `ContentRating`, `MediaKind`, `PlaylistKind`, `DistinguishedPlaylistKind`, `PlaylistShareMode`, and `PlaylistViewPreferences`
   - Automatically merges sidecar settings from `Library Preferences.musicdb` and `.musiclibrary/preferences/*.plist` (`Songs.plist`, `Playlist_<ID>.plist`) when opened from a `.musiclibrary` bundle or `Library.musicdb` path.
3. **Layer 3 — Normalized SQLite & Rich JSON Exporters (`musicdb::sqlite`, `Library::to_json_pretty`)**:
   - **SQLite Exporter (`Library::export_sqlite`)**: Creates a self-contained, indexed SQLite database (using bundled `rusqlite`) with 8 normalized tables and 2 convenience SQL views.
   - **JSON Exporter (`Library::to_json_pretty`)**: Serializes the complete `Library` hierarchy (including nested `SmartRuleGroup` trees).

---

## Rust Crate Usage

```rust
use musicdb::Library;

fn main() -> musicdb::Result<()> {
    // Accepts either a `.musiclibrary` directory or a `Library.musicdb` file path
    let lib = Library::open("path/to/Apple Music Library.musiclibrary")?;

    println!(
        "Library {} (App {}): {} tracks, {} albums, {} artists, {} playlists",
        lib.metadata.persistent_id,
        lib.metadata.app_version,
        lib.tracks.len(),
        lib.albums.len(),
        lib.artists.len(),
        lib.playlists.len(),
    );

    // Query a playlist by name or Persistent ID
    if let Some(pl) = lib.playlist_by_name("Most Played") {
        println!("Playlist {:?} ({:?}):", pl.name, pl.kind);
        if let Some(smart) = &pl.smart_playlist {
            println!("  Smart Config: {:?}", smart);
        }
        for track in lib.playlist_tracks(&pl.persistent_id) {
            println!("  - {:?} by {:?}", track.title, track.artist);
        }
    }

    // Export to a normalized, indexed SQLite database
    lib.export_sqlite("/tmp/apple_music.sqlite")?;

    Ok(())
}
```

---

## CLI Usage

Build the release binary:

```bash
cargo build --release
```

### 1. Exporting to SQLite or JSON

```bash
# Export the entire library (tracks, albums, artists, playlists, items, smart rules, views) to SQLite
musicdb-rs export-sqlite "path/to/Apple Music Library.musiclibrary" apple_music.sqlite

# Export the entire library domain model to pretty-printed JSON
musicdb-rs export-json "path/to/Apple Music Library.musiclibrary" -o apple_music.json
```

### 2. Targeted High-Level Domain Queries

```bash
# Inspect a playlist by name or 16-char hex Persistent ID (decodes Smart Playlist rules, view preferences, and tracks)
musicdb-rs query-playlist "path/to/Apple Music Library.musiclibrary" "Most Played"
musicdb-rs query-playlist "path/to/Apple Music Library.musiclibrary" BA3165933D277022 --json

# Search tracks by 16-char hex Persistent ID or case-insensitive title/artist/album substring
musicdb-rs query-track "path/to/Apple Music Library.musiclibrary" "Working Man"
musicdb-rs query-track "path/to/Apple Music Library.musiclibrary" 47A081DF30DA6FB2 --json
```

### 3. Forensics & Reverse-Engineering Commands

```bash
# Summary of header metadata, hsma section layout, and 100% byte-identical round-trip verification
musicdb-rs info "path/to/Apple Music Library.musiclibrary"

# Diff two .musicdb snapshots across header, plma, tracks, albums, artists, and playlists
musicdb-rs diff path/to/before/Library.musicdb path/to/after/Library.musicdb

# Scan unknown byte regions, boma subtype frequencies, and structural invariants
musicdb-rs unknowns "path/to/Apple Music Library.musiclibrary"

# Cross-check a .musicdb file against an iTunes Music Library.xml file by Persistent ID
musicdb-rs xml-check path/to/Library.musicdb "path/to/iTunes Music Library.xml"

# Decrypt and decompress a .musicdb file into raw binary bytes
musicdb-rs decrypt path/to/Library.musicdb /tmp/raw_library.bin

# Inspect a binary or XML .plist file (e.g. Preferences.plist or preferences/Songs.plist)
musicdb-rs plist "path/to/Apple Music Library.musiclibrary/preferences/Songs.plist"
```

---

## SQLite Database Schema

`Library::export_sqlite` (`musicdb-rs export-sqlite`) generates the following tables and views:

| Table / View | Description |
| :--- | :--- |
| `library_metadata` | Single-row library metadata (`persistent_id`, `itunes_persistent_id`, `revision_hash`, `app_version`, `format_major_version`, `format_minor_version`, `timezone_offset_seconds`, `apple_store_account_id`, `show_song_list_checkboxes`, `keep_media_folder_organized`, `copy_files_to_media_folder`, `media_folder_path`, `songs_view_preferences_json`, etc.). |
| `tracks` | All tracks (113 columns: `persistent_id`, `title`, `album`, `artist`, `album_artist`, `composer`, `genre`, `work_name`, `movement_name`, `movement_number`, `movement_total`, `sort_*`, `sort_mode_*`, `track_number`, `disc_number`, `year`, `bpm`, `duration_ms`, `file_size`, `bit_rate`, `is_vbr`, `sample_rate`, `file_type_fourcc`, `codec_fourcc`, `volume_adjustment`, `normalization`, `gapless_*`, `equalizer_preset_name`, `star_rating`, `stars`, `suggestion_flag`, `disabled`, `skip_when_shuffling`, `remember_playback_position`, `show_composer_in_all_views`, `use_work_and_movement`, `album_is_compilation`, `part_of_gapless_album`, `has_custom_lyrics`, `has_local_custom_lyrics`, `has_unsynced_local_changes`, `is_itunes_matched`, `is_apple_music_catalog`, `cloud_saga_id`, `store_track_adam_id`, `downloaded_state`, `cloud_status`, `content_advisory`, `media_type`, `play_count`, `true_play_count`, `skip_count`, `true_skip_count`, `date_*`, `file_url`, `legacy_file_path`, `album_persistent_id`, `artist_persistent_id`, `artwork_uuid`, `has_embedded_artwork`, `isrc`, `copyright`, etc.). |
| `albums` | All album entities (`persistent_id`, `title`, `artist`, `album_artist`, `cloud_album_id`, `editorial_notes`, `is_compilation`, `star_rating`, `stars`, `star_rating_is_computed`, `suggestion_flag`, `suggestion_locally_modified`, `apple_music_store_album_id`, `date_*`). |
| `artists` | All artist entities (`persistent_id`, `name`, `sort_name`, `cloud_artist_id`, `suggestion_flag`, `suggestion_locally_modified`, `apple_music_store_artist_id`, `artwork_uuid`, `artwork_url_plist`, `date_*`). |
| `playlists` | All folders, standard playlists, smart playlists, subscribed Apple Music catalog playlists, and system playlists (`persistent_id`, `parent_persistent_id`, `name`, `description`, `kind`, `is_master`, `is_folder`, `is_smart`, `show_in_profile_and_search`, `system_kind`, `suggestion_flag`, `share_mode`, `universal_library_id`, `cloud_uuid`, `subscribed_container_url`, `owner_name`, `owner_id`, `author_social_profile_persistent_id`, `smart_live_updating`, `smart_match_rules`, `smart_match_only_checked_items`, `smart_limit_count`, `smart_limit_unit`, `smart_limit_selection`, `smart_rule_group_json`, `view_as`, `view_sort_field`, `view_sort_ascending`, `view_visible_columns_json`, etc.). |
| `playlist_items` | Ordered junction table (`playlist_persistent_id`, `position`, `track_persistent_id`). |
| `smart_playlist_rules` | Flattened representation of every rule in `SLst` (`playlist_persistent_id`, `rule_index`, `parent_group_index`, `conjunction`, `field_id`, `field`, `operator`, `comparison_raw`, `value_type`, `value_json`, `description`). |
| `social_profiles` | Apple Music social profiles (`persistent_id`, `profile_id`, `name`, `handle`, `avatar_id`, `avatar_url`). |
| `v_playlist_tracks` | Convenience SQL view joining `playlists`, `playlist_items`, and `tracks` in playlist order. |
| `v_tracks_enriched` | Convenience SQL view joining `tracks` with `albums` and `artists` metadata. |
