use std::fs;
use std::io;
use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};

use musicdb::analysis::{analyze_unknowns, compare_with_itunes_xml, diff_libraries};
use musicdb::sections::HsmaChild;
use musicdb::{Library, RawLibraryFile, discover_default_library_path, load_raw_musicdb};

#[derive(Parser)]
#[command(
    name = "musicdb",
    about = "Apple Music .musicdb parser, inspector, and format analysis toolkit"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Display library header metadata, hsma section layout, and verify 100% lossless byte round-trip
    Info {
        /// Path to Library.musicdb or .musiclibrary directory (defaults to user Music library if omitted)
        path: Option<PathBuf>,
    },

    /// Scan all unknown byte regions, list boma subtypes, and verify structural hypotheses
    Unknowns {
        /// Path to Library.musicdb or .musiclibrary directory (defaults to user Music library if omitted)
        path: Option<PathBuf>,
        /// Emit full report as JSON
        #[arg(long)]
        json: bool,
    },

    /// Diff two .musicdb snapshots (e.g. 2024 vs 2025 vs 2026) by Persistent ID and raw byte offsets
    Diff {
        /// Older .musicdb path
        path_a: PathBuf,
        /// Newer .musicdb path
        path_b: PathBuf,
        /// Emit full report as JSON
        #[arg(long)]
        json: bool,
    },

    /// Cross-reference a .musicdb file against an iTunes Music Library.xml file by Persistent ID
    XmlCheck {
        /// Path to Library.musicdb
        musicdb_path: PathBuf,
        /// Path to iTunes Music Library.xml
        xml_path: PathBuf,
        /// Emit full report as JSON
        #[arg(long)]
        json: bool,
    },

    /// Decrypt and decompress a .musicdb file into raw binary bytes
    Decrypt {
        /// Input .musicdb file
        input: PathBuf,
        /// Output raw binary file
        output: PathBuf,
    },

    /// Export the high-level parsed Library model to JSON
    ExportJson {
        /// Path to Library.musicdb or .musiclibrary directory
        input: PathBuf,
        /// Optional output JSON file path (prints to stdout if omitted)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Export the entire Apple Music Library into a normalized, indexed SQLite database (.sqlite / .db)
    ExportSqlite {
        /// Path to Library.musicdb or .musiclibrary directory
        input: PathBuf,
        /// Output SQLite file path
        output: PathBuf,
    },

    /// Query a specific playlist by name or Persistent ID (including decoded Smart Playlist rules, view preferences, and tracks)
    QueryPlaylist {
        /// Path to Library.musicdb or .musiclibrary directory
        input: PathBuf,
        /// Playlist name or 16-char hex Persistent ID
        query: String,
        /// Emit as JSON
        #[arg(long)]
        json: bool,
    },

    /// Query tracks by Persistent ID or case-insensitive title/artist/album substring
    QueryTrack {
        /// Path to Library.musicdb or .musiclibrary directory
        input: PathBuf,
        /// 16-char hex Persistent ID or search string
        query: String,
        /// Emit as JSON
        #[arg(long)]
        json: bool,
    },

    /// Inspect a binary or XML .plist file (e.g. Preferences.plist or preferences/Playlist_*.plist)
    Plist {
        /// Path to .plist file
        path: PathBuf,
    },

    /// Generate shell completions for bash, zsh, fish, elvish, or powershell
    Completions {
        /// Target shell
        shell: Shell,
    },
}

fn resolve_path(path: Option<PathBuf>) -> Result<PathBuf> {
    match path {
        Some(p) => Ok(p),
        None => discover_default_library_path().ok_or_else(|| {
            anyhow::anyhow!(
                "No path provided and no default Apple Music Library found in the user's Music directory"
            )
        }),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Info { path } => {
            let path = resolve_path(path)?;
            let raw_bytes = load_raw_musicdb(&path)?;
            let raw = RawLibraryFile::from_raw_bytes(&raw_bytes)?;
            let reserialized = raw.to_raw_bytes()?;
            let roundtrip_exact = raw_bytes == reserialized;

            let lib = Library::from_raw(&raw);
            let m = &lib.metadata;

            println!("=== MusicDB File Summary: {} ===", path.display());
            println!("Raw Decompressed Size : {} bytes", raw_bytes.len());
            println!(
                "Lossless Round-Trip   : {} ({} bytes -> {} bytes)",
                if roundtrip_exact {
                    "PASS (100% byte-identical)"
                } else {
                    "MISMATCH"
                },
                raw_bytes.len(),
                reserialized.len()
            );
            println!(
                "App Version           : {} (format {}.{})",
                m.app_version, m.format_major_version, m.format_minor_version
            );
            println!("File Type             : {}", m.file_type);
            println!("Library Persistent ID : {}", m.persistent_id);
            println!(
                "iTunes Persistent ID  : {}",
                m.itunes_persistent_id.as_deref().unwrap_or("<none>")
            );
            println!(
                "Date Modified         : {}",
                m.date_modified
                    .map(|d| d.to_rfc3339())
                    .unwrap_or_else(|| "<none>".to_string())
            );
            println!(
                "Timezone Offset (s)   : {} ({:+.1}h)",
                m.timezone_offset_seconds,
                (m.timezone_offset_seconds as f64) / 3600.0
            );
            println!(
                "Header Counts         : tracks={}, albums={}, artists={}, playlists={}",
                raw.outer_header.track_count,
                raw.outer_header.album_count,
                raw.outer_header.artist_count,
                raw.outer_header.playlist_count
            );
            println!(
                "Parsed Counts         : tracks={}, albums={}, artists={}, playlists={}, social_profiles={}",
                lib.tracks.len(),
                lib.albums.len(),
                lib.artists.len(),
                lib.playlists.len(),
                lib.social_profiles.len()
            );
            if let Some(uri) = &m.media_folder_uri {
                println!("Media Folder URI      : {}", uri);
            }
            if let Some(p) = &m.media_folder_path {
                println!("Media Folder Path     : {}", p);
            }
            if let Some(itl) = &m.imported_itl_path {
                println!("Imported .itl Path    : {}", itl);
            }
            println!("\n=== hsma Major Sections ({}) ===", raw.sections.len());
            for (i, sec) in raw.sections.iter().enumerate() {
                let desc = match &sec.child {
                    HsmaChild::InnerHfma(h) => {
                        format!("InnerHfma (header_size={})", h.header_size)
                    }
                    HsmaChild::LibraryMaster(p) => {
                        format!(
                            "plma LibraryMaster (header_size={}, bomas={})",
                            p.header.header_size,
                            p.bomas.len()
                        )
                    }
                    HsmaChild::AlbumList(l) => {
                        format!(
                            "lama AlbumList (header_size={}, albums={})",
                            l.header.header_size,
                            l.albums.len()
                        )
                    }
                    HsmaChild::ArtistList(l) => {
                        format!(
                            "lAma ArtistList (header_size={}, artists={})",
                            l.header.header_size,
                            l.artists.len()
                        )
                    }
                    HsmaChild::TrackList(l) => {
                        format!(
                            "ltma TrackList (header_size={}, tracks={})",
                            l.header.header_size,
                            l.tracks.len()
                        )
                    }
                    HsmaChild::PlaylistList(l) => {
                        format!(
                            "lPma PlaylistList (header_size={}, playlists={})",
                            l.header.header_size,
                            l.playlists.len()
                        )
                    }
                    HsmaChild::SocialProfileList(s) => {
                        format!(
                            "Lsma SocialProfileList (header_size={}, profiles={})",
                            s.header.header_size,
                            s.profiles.len()
                        )
                    }
                    HsmaChild::Padding(p) => {
                        format!("LPma Padding (section_size={})", p.section_size)
                    }
                    HsmaChild::Unknown {
                        subtype,
                        signature,
                        raw_bytes,
                    } => {
                        format!(
                            "Unknown(subtype={}, sig={:?}, len={})",
                            subtype,
                            String::from_utf8_lossy(signature),
                            raw_bytes.len()
                        )
                    }
                };
                println!(
                    "  [{}] subtype={:<2} total_size={:<10} -> {}",
                    i,
                    sec.header.subtype,
                    sec.header.effective_total_size(),
                    desc
                );
            }
            if !raw.trailing_bytes.is_empty() {
                println!(
                    "Trailing bytes after last hsma: {}",
                    raw.trailing_bytes.len()
                );
            }
        }

        Commands::Unknowns { path, json } => {
            let path = resolve_path(path)?;
            let raw = RawLibraryFile::open(&path)?;
            let report = analyze_unknowns(&raw);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("=== Unknowns & Invariant Analysis: {} ===", path.display());
                println!(
                    "\n1. Unhandled boma subtypes: {}",
                    report.unknown_boma_subtypes.len()
                );
                for (k, v) in &report.unknown_boma_subtypes {
                    println!("   - {}: {} occurrences", k, v);
                }
                println!("\n2. All boma subtypes encountered:");
                for (k, v) in &report.boma_subtype_counts {
                    println!("   - {:<42} : {}", k, v);
                }
                let gc = &report.global_counter_analysis;
                println!("\n3. Global Counter / Runtime ID Analysis:");
                println!(
                    "   - Albums: {}, Artists: {} (sum = {})",
                    gc.album_count,
                    gc.artist_count,
                    gc.album_count + gc.artist_count
                );
                println!(
                    "   - First itma global_counter : {:?}",
                    gc.first_itma_global_counter
                );
                println!(
                    "   - Last itma global_counter  : {:?}",
                    gc.last_itma_global_counter
                );
                println!(
                    "   - First lpma global_counter : {:?}",
                    gc.first_lpma_global_counter
                );
                println!(
                    "   - Last ipfa global_counter  : {:?}",
                    gc.last_ipfa_global_counter
                );

                let ic = &report.ipfa_short_id_verification;
                println!("\n4. ipfa Offset 40 == Target Track (itma) Global Counter Check:");
                println!("   - Total ipfa items          : {}", ic.total_ipfa_items);
                println!(
                    "   - Matched Track PersistentID: {}",
                    ic.matched_track_persistent_id
                );
                println!(
                    "   - Matched itma.global_counter: {}",
                    ic.matched_itma_global_counter
                );
                for ex in &ic.mismatched_examples {
                    println!("     ! {}", ex);
                }

                println!(
                    "\n5. Unknown Fields with Non-Zero Data ({} fields):",
                    report.nonzero_unknown_fields.len()
                );
                for f in &report.nonzero_unknown_fields {
                    println!(
                        "   - {:<14} {:<28} (off {:<9}): {:>5}/{:<5} non-zero ({} distinct) | top: {:?}",
                        f.section,
                        f.field_name,
                        f.offset_info,
                        f.nonzero_instances,
                        f.total_instances,
                        f.distinct_nonzero_count,
                        f.sample_values
                            .iter()
                            .take(3)
                            .map(|(val, cnt)| format!("{} (x{})", val, cnt))
                            .collect::<Vec<_>>()
                    );
                }
            }
        }

        Commands::Diff {
            path_a,
            path_b,
            json,
        } => {
            let raw_a = RawLibraryFile::open(&path_a)?;
            let raw_b = RawLibraryFile::open(&path_b)?;
            let report = diff_libraries(&raw_a, &raw_b);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "=== Library Diff: {} vs {} ===",
                    path_a.display(),
                    path_b.display()
                );
                if !report.hfma_changed_4byte_offsets.is_empty() {
                    println!("\nhfma (outer header) 4-byte changes:");
                    for (off, va, vb) in &report.hfma_changed_4byte_offsets {
                        println!(
                            "  offset {:>3}..{:>3}: 0x{:08X} ({}) -> 0x{:08X} ({})",
                            off,
                            off + 4,
                            va,
                            va,
                            vb,
                            vb
                        );
                    }
                }
                if report.hsma_10_hex_a != report.hsma_10_hex_b {
                    println!(
                        "\nhsma subtype 10 CHANGED:\n  A: {:?}\n  B: {:?}",
                        report.hsma_10_hex_a, report.hsma_10_hex_b
                    );
                } else {
                    println!("\nhsma subtype 10 UNCHANGED ({:?})", report.hsma_10_hex_a);
                }
                if !report.plma_changed_4byte_offsets.is_empty()
                    || !report.plma_boma_changes.is_empty()
                {
                    println!("\nplma (LibraryMaster) changes:");
                    for (off, va, vb) in &report.plma_changed_4byte_offsets {
                        println!(
                            "  offset {:>3}..{:>3}: 0x{:08X} ({}) -> 0x{:08X} ({})",
                            off,
                            off + 4,
                            va,
                            va,
                            vb,
                            vb
                        );
                    }
                    for bc in &report.plma_boma_changes {
                        println!("  * {}", bc);
                    }
                }
                println!(
                    "\nTracks in A: {}, Tracks in B: {}, Common: {}",
                    report.tracks_in_a, report.tracks_in_b, report.common_tracks
                );
                if !report.added_tracks.is_empty() {
                    println!("Added tracks: {}", report.added_tracks.len());
                    for t in report.added_tracks.iter().take(10) {
                        println!("  + {}", t);
                    }
                }
                if !report.removed_tracks.is_empty() {
                    println!("Removed tracks: {}", report.removed_tracks.len());
                    for t in report.removed_tracks.iter().take(10) {
                        println!("  - {}", t);
                    }
                }
                println!("\nitma 4-byte offsets that changed across common tracks:");
                for (off, cnt) in &report.itma_offset_change_frequencies {
                    println!(
                        "  offset {:>3}..{:>3}: changed in {} tracks",
                        off,
                        off + 4,
                        cnt
                    );
                }
                println!(
                    "\nTrackNumerics (boma 0x01) 4-byte offsets that changed across common tracks:"
                );
                for (off, cnt) in &report.track_numerics_offset_change_frequencies {
                    println!(
                        "  offset {:>3}..{:>3}: changed in {} tracks",
                        off,
                        off + 4,
                        cnt
                    );
                }
                if !report.sample_track_diffs.is_empty() {
                    println!(
                        "\nDetailed Track Diffs ({} tracks):",
                        report.sample_track_diffs.len()
                    );
                    for td in &report.sample_track_diffs {
                        println!(
                            "  [Track {}] {:?} - {:?}",
                            td.persistent_id, td.artist, td.title
                        );
                        for hl in &td.high_level_changes {
                            println!("    hl: {}", hl);
                        }
                        for (off, va, vb) in &td.itma_changed_4byte_offsets {
                            println!(
                                "    itma {:>3}..{:>3}: 0x{:08X} ({}) -> 0x{:08X} ({})",
                                off,
                                off + 4,
                                va,
                                va,
                                vb,
                                vb
                            );
                        }
                        for (off, va, vb) in &td.numerics_changed_4byte_offsets {
                            println!(
                                "    num  {:>3}..{:>3}: 0x{:08X} ({}) -> 0x{:08X} ({})",
                                off,
                                off + 4,
                                va,
                                va,
                                vb,
                                vb
                            );
                        }
                        for (off, va, vb) in &td.plays_skips_changed_4byte_offsets {
                            println!(
                                "    ps   {:>3}..{:>3}: 0x{:08X} ({}) -> 0x{:08X} ({})",
                                off,
                                off + 4,
                                va,
                                va,
                                vb,
                                vb
                            );
                        }
                    }
                }
                if !report.album_diffs.is_empty() {
                    println!("\nAlbum (iama) Diffs ({}):", report.album_diffs.len());
                    for ad in &report.album_diffs {
                        println!("  {}", ad);
                    }
                }
                if !report.artist_diffs.is_empty() {
                    println!("\nArtist (iAma) Diffs ({}):", report.artist_diffs.len());
                    for rd in &report.artist_diffs {
                        println!("  {}", rd);
                    }
                }
                if !report.playlist_diffs.is_empty() {
                    println!("\nPlaylist (lpma) Diffs ({}):", report.playlist_diffs.len());
                    for pd in &report.playlist_diffs {
                        println!("  {}", pd);
                    }
                }
            }
        }

        Commands::XmlCheck {
            musicdb_path,
            xml_path,
            json,
        } => {
            let raw = RawLibraryFile::open(&musicdb_path)?;
            let report = compare_with_itunes_xml(&raw, &xml_path)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "=== iTunes XML Cross-Check: {} vs {} ===",
                    musicdb_path.display(),
                    xml_path.display()
                );
                println!("Tracks in XML             : {}", report.xml_track_count);
                println!("Tracks in MusicDB         : {}", report.musicdb_track_count);
                println!(
                    "Matched by Persistent ID  : {}",
                    report.matched_by_persistent_id
                );
                println!("\nField Match Rates (exact equality across matched tracks):");
                for (field, (matched, total)) in &report.field_match_counts {
                    let pct = if *total > 0 {
                        (*matched as f64) * 100.0 / (*total as f64)
                    } else {
                        0.0
                    };
                    println!(
                        "  - {:<28}: {:>5}/{:<5} ({:>6.2}%)",
                        field, matched, total, pct
                    );
                }
            }
        }

        Commands::Decrypt { input, output } => {
            let raw_bytes = load_raw_musicdb(&input)?;
            fs::write(&output, &raw_bytes)?;
            println!(
                "Wrote {} raw decrypted/decompressed bytes to {}",
                raw_bytes.len(),
                output.display()
            );
        }

        Commands::ExportJson { input, output } => {
            let lib = Library::open(&input)?;
            let json_str = lib.to_json_pretty()?;
            if let Some(out_path) = output {
                fs::write(&out_path, json_str)?;
                println!("Exported library JSON to {}", out_path.display());
            } else {
                println!("{}", json_str);
            }
        }

        Commands::ExportSqlite { input, output } => {
            let lib = Library::open(&input)?;
            lib.export_sqlite(&output)?;
            println!(
                "Exported SQLite database ({} tracks, {} albums, {} artists, {} playlists, {} social profiles) to {}",
                lib.tracks.len(),
                lib.albums.len(),
                lib.artists.len(),
                lib.playlists.len(),
                lib.social_profiles.len(),
                output.display()
            );
        }

        Commands::QueryPlaylist { input, query, json } => {
            let lib = Library::open(&input)?;
            let pl = lib
                .playlist_by_id(&query)
                .or_else(|| lib.playlist_by_name(&query))
                .ok_or_else(|| anyhow::anyhow!("No playlist matching {:?} found", query))?;
            if json {
                println!("{}", serde_json::to_string_pretty(pl)?);
            } else {
                println!(
                    "=== Playlist: {:?} ({}) ===",
                    pl.name.as_deref().unwrap_or("<unnamed>"),
                    pl.persistent_id
                );
                println!("Kind                  : {:?}", pl.kind);
                println!(
                    "Parent Persistent ID  : {}",
                    pl.parent_persistent_id.as_deref().unwrap_or("<root>")
                );
                println!("Share Mode            : {:?}", pl.share_mode);
                println!("Show in Profile/Search: {}", pl.show_in_profile_and_search);
                println!("Suggestion Flag       : {:?}", pl.suggestion_flag);
                if let Some(desc) = &pl.description {
                    println!("Description           : {}", desc);
                }
                if let Some(vp) = &pl.view_preferences {
                    println!(
                        "View Preferences      : view_as={:?}, sort_field={:?}, ascending={:?}",
                        vp.view_as, vp.sort_field, vp.sort_ascending
                    );
                }
                if !pl.child_playlist_persistent_ids.is_empty() {
                    println!(
                        "Child Playlists ({})  : {:?}",
                        pl.child_playlist_persistent_ids.len(),
                        pl.child_playlist_persistent_ids
                    );
                }
                if let Some(sp) = &pl.smart_playlist {
                    println!(
                        "\nSmart Playlist Config : live_updating={}, match_rules={}, only_checked={}",
                        sp.live_updating, sp.match_rules, sp.match_only_checked_items
                    );
                    if let Some(lim) = &sp.limit {
                        println!(
                            "Limit                 : {} {:?} selected by {:?}",
                            lim.count, lim.unit, lim.selection
                        );
                    }
                    if let Some(group) = &sp.rule_group {
                        println!("Rule Tree (Match {:?}):", group.conjunction);
                        print_smart_rule_group(group, 1);
                    }
                }
                let tracks = lib.playlist_tracks(pl);
                println!("\nTracks ({}):", tracks.len());
                for (i, t) in tracks.iter().take(50).enumerate() {
                    println!(
                        "  {:>3}. [{}] {:?} - {:?} ({:?})",
                        i + 1,
                        t.persistent_id,
                        t.artist.as_deref().unwrap_or(""),
                        t.title.as_deref().unwrap_or(""),
                        t.album.as_deref().unwrap_or("")
                    );
                }
                if tracks.len() > 50 {
                    println!("  ... ({} more tracks)", tracks.len() - 50);
                }
            }
        }

        Commands::QueryTrack { input, query, json } => {
            let lib = Library::open(&input)?;
            let matches: Vec<_> = if let Some(exact) = lib.track_by_id(&query) {
                vec![exact]
            } else {
                let q = query.to_lowercase();
                lib.tracks
                    .iter()
                    .filter(|t| {
                        t.title
                            .as_deref()
                            .is_some_and(|s| s.to_lowercase().contains(&q))
                            || t.artist
                                .as_deref()
                                .is_some_and(|s| s.to_lowercase().contains(&q))
                            || t.album
                                .as_deref()
                                .is_some_and(|s| s.to_lowercase().contains(&q))
                    })
                    .collect()
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&matches)?);
            } else {
                println!("Found {} matching track(s) for {:?}:", matches.len(), query);
                for t in matches.iter().take(25) {
                    println!(
                        "  [{}] {:?} - {:?} (Album: {:?}, Stars: {}, Cloud: {:?}, UnsyncedLocal: {})",
                        t.persistent_id,
                        t.artist.as_deref().unwrap_or(""),
                        t.title.as_deref().unwrap_or(""),
                        t.album.as_deref().unwrap_or(""),
                        t.stars,
                        t.cloud_status,
                        t.has_unsynced_local_changes
                    );
                }
            }
        }

        Commands::Plist { path } => {
            let val = plist::Value::from_file(&path)?;
            println!("{:#?}", val);
        }

        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            generate(shell, &mut cmd, bin_name, &mut io::stdout());
        }
    }

    Ok(())
}

fn print_smart_rule_group(group: &musicdb::SmartRuleGroup, indent: usize) {
    let pad = "  ".repeat(indent);
    for node in &group.rules {
        match node {
            musicdb::SmartRuleNode::Group(sub) => {
                println!("{}[Nested Group: Match {:?}]", pad, sub.conjunction);
                print_smart_rule_group(sub, indent + 1);
            }
            musicdb::SmartRuleNode::Condition(cond) => {
                println!("{}- {}", pad, cond.description);
            }
        }
    }
}
