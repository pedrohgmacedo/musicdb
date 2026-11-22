//! CLI inspector, exporter, and reverse-engineering toolkit for Apple Music `.musicdb` libraries.

mod analysis;
mod donate;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};

use crate::analysis::{
    LibraryDiffReport, QueryUnknownsOptions, SanityCheckOptions, UnknownsReport,
    XmlComparisonReport, analyze_sanity, analyze_unknowns, compare_with_itunes_xml, diff_libraries,
    query_unknowns,
};
use crate::donate::{
    DEFAULT_DONATED_LIBRARY_ARCHIVE, DEFAULT_DONATED_PREVIOUS_LIBRARIES_ARCHIVE,
    discover_default_previous_libraries_path, find_musiclibrary_bundle,
    find_previous_libraries_dir, find_previous_libraries_for_bundle, list_previous_library_bundles,
    package_donated_library, package_donated_previous_libraries,
};
use musicdb::sections::{HsmaChild, HsmaSection};
use musicdb::{Library, Playlist, RawLibraryFile, discover_default_library_path, load_raw_musicdb};

fn write_stdout_or_exit(args: std::fmt::Arguments<'_>) {
    if let Err(err) = io::stdout().write_fmt(args) {
        if err.kind() == io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        // Best-effort diagnostic to stderr before exiting on non-BrokenPipe stdout error.
        let _ = writeln!(io::stderr(), "error: failed printing to stdout: {err}");
        std::process::exit(1);
    }
}

macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::write_stdout_or_exit(format_args!($($arg)*));
    }};
}

macro_rules! println {
    () => {{
        $crate::write_stdout_or_exit(format_args!("\n"));
    }};
    ($($arg:tt)*) => {{
        $crate::write_stdout_or_exit(format_args!("{}\n", format_args!($($arg)*)));
    }};
}

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

    /// Audit raw section fields for enum fallthroughs, unknown bitflags, invalid timestamps, and corrupt payloads
    SanityCheck {
        /// Path to Library.musicdb or .musiclibrary directory (defaults to user Music library if omitted)
        path: Option<PathBuf>,
        /// Include informational observations (e.g. expected future lease/release dates, known sentinel 2)
        #[arg(long)]
        include_info: bool,
        /// Emit structured JSON report
        #[arg(long)]
        json: bool,
    },

    /// Query raw unknown binary fields and unhandled boma subtypes with entity context
    QueryUnknowns {
        /// Path to Library.musicdb or .musiclibrary directory (defaults to user Music library if omitted)
        path: Option<PathBuf>,
        /// Field or section selector in code notation (e.g. `iama.unk_28`, `AlbumItemHeader.unk_28`, `TrackNumerics.unk_200_shared_val`, `iama.*`, `boma:0x0042`)
        #[arg(short, long, required_unless_present_any = ["entity", "list_fields"])]
        field: Option<String>,
        /// Filter by entity: `[album|artist|track|playlist|profile|library:]<persistent_id|substring>`
        #[arg(short, long)]
        entity: Option<String>,
        /// Filter to instances matching a value (decimal/signed int, 0x... hex, byte-hex, or exact formatted value)
        #[arg(short, long)]
        value: Option<String>,
        /// Exclude zero / all-zero values
        #[arg(long, conflicts_with = "include_zero")]
        nonzero: bool,
        /// Include zero values even in multi-field or entity-only queries
        #[arg(long, conflicts_with = "nonzero")]
        include_zero: bool,
        /// Maximum examples per distinct value bucket (default: 5 grouped, 50 --flat)
        #[arg(long)]
        examples: Option<usize>,
        /// Show all matching entity examples and remove sample-track cap
        #[arg(long)]
        all: bool,
        /// Maximum sample tracks included in Album / Artist / Playlist entity context
        #[arg(long, default_value_t = 5)]
        tracks: usize,
        /// Print matching occurrences flat without grouping by distinct value
        #[arg(long)]
        flat: bool,
        /// List all unknown field selectors and unhandled boma subtypes present in the file, then exit
        #[arg(long)]
        list_fields: bool,
        /// Emit structured JSON
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

    /// Export the high-level parsed Library model to JSON.
    /// Note: The exported JSON includes personal metadata from Library.musicdb (such as account IDs and purchaser emails).
    ExportJson {
        /// Path to Library.musicdb or .musiclibrary directory
        input: PathBuf,
        /// Optional output JSON file path (prints to stdout if omitted)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Export the entire Apple Music Library into a normalized, indexed SQLite database (.sqlite / .db).
    /// Note: The exported database includes personal metadata from Library.musicdb (such as account IDs and purchaser emails).
    ExportSqlite {
        /// Path to Library.musicdb or .musiclibrary directory
        input: PathBuf,
        /// Output SQLite file path
        output: PathBuf,
    },

    /// Query a specific playlist by name or Persistent ID, or list all playlists when query is omitted
    QueryPlaylist {
        /// Path to Library.musicdb or .musiclibrary directory
        input: PathBuf,
        /// Playlist name or 16-char hex Persistent ID (omit to list all playlists)
        query: Option<String>,
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

    /// Package Apple Music Library metadata files (excluding media and artwork images) into a .tar archive for format research.
    /// Note: Library.musicdb contains personal metadata such as purchaser names, Apple ID emails, and account IDs.
    DonateLibrary {
        /// Optional path to Apple Music folder or .musiclibrary bundle (auto-discovers default library if omitted)
        path: Option<PathBuf>,
        /// Output archive path (defaults to donated-music-library.tar; use .tar.gz for gzip compression)
        #[arg(short, long, default_value = DEFAULT_DONATED_LIBRARY_ARCHIVE)]
        output: PathBuf,
        /// Skip interactive confirmation prompts
        #[arg(short, long)]
        yes: bool,
    },

    /// Package all Previous Libraries metadata bundles (excluding media and artwork images) into a .tar archive for format research.
    /// Note: Library.musicdb contains personal metadata such as purchaser names, Apple ID emails, and account IDs.
    DonatePreviousLibraries {
        /// Optional path to Previous Libraries or Apple Music folder (auto-discovers default location if omitted)
        path: Option<PathBuf>,
        /// Output archive path (defaults to donated-previous-libraries.tar; use .tar.gz for gzip compression)
        #[arg(short, long, default_value = DEFAULT_DONATED_PREVIOUS_LIBRARIES_ARCHIVE)]
        output: PathBuf,
        /// Skip interactive confirmation prompts
        #[arg(short, long)]
        yes: bool,
    },

    /// Generate shell completions for bash, zsh, fish, elvish, or powershell
    Completions {
        /// Target shell
        shell: Shell,
    },

    /// Run optional ad-hoc reverse-engineering probes (requires `--features adhoc`)
    #[cfg(feature = "adhoc")]
    Adhoc {
        /// Specific ad-hoc probe to run
        #[command(subcommand)]
        command: crate::analysis::adhoc::AdhocCommand,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Err(err) = dispatch_command(cli.command) {
        if err
            .downcast_ref::<io::Error>()
            .is_some_and(|io_err| io_err.kind() == io::ErrorKind::BrokenPipe)
        {
            return Ok(());
        }
        return Err(err);
    }
    Ok(())
}

fn dispatch_command(command: Commands) -> Result<()> {
    match command {
        Commands::Info { path } => run_info(path),
        Commands::Unknowns { path, json } => run_unknowns(path, json),
        Commands::SanityCheck {
            path,
            include_info,
            json,
        } => run_sanity_check(path, include_info, json),
        Commands::QueryUnknowns {
            path,
            field,
            entity,
            value,
            nonzero,
            include_zero,
            examples,
            all,
            tracks,
            flat,
            list_fields,
            json,
        } => run_query_unknowns(
            path,
            &QueryUnknownsOptions {
                field,
                entity,
                value,
                nonzero,
                include_zero,
                examples,
                all,
                tracks_limit: tracks,
                flat,
                list_fields,
            },
            json,
        ),
        Commands::Diff {
            path_a,
            path_b,
            json,
        } => run_diff(&path_a, &path_b, json),
        Commands::XmlCheck {
            musicdb_path,
            xml_path,
            json,
        } => run_xml_check(&musicdb_path, &xml_path, json),
        Commands::Decrypt { input, output } => run_decrypt(&input, &output),
        Commands::ExportJson { input, output } => run_export_json(&input, output.as_deref()),
        Commands::ExportSqlite { input, output } => run_export_sqlite(&input, &output),
        Commands::QueryPlaylist { input, query, json } => {
            run_query_playlist(&input, query.as_deref(), json)
        }
        Commands::QueryTrack { input, query, json } => run_query_track(&input, &query, json),
        Commands::Plist { path } => run_plist(&path),
        Commands::DonateLibrary { path, output, yes } => {
            run_donate_library(path.as_deref(), &output, yes)
        }
        Commands::DonatePreviousLibraries { path, output, yes } => {
            run_donate_previous_libraries(path.as_deref(), &output, yes)
        }
        Commands::Completions { shell } => {
            run_completions(shell);
            Ok(())
        }
        #[cfg(feature = "adhoc")]
        Commands::Adhoc { command } => crate::analysis::adhoc::run(command),
    }
}

fn prompt_yes_no(question: &str) -> Result<bool> {
    print!("{question} [y/N]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_ascii_lowercase();
    Ok(trimmed == "y" || trimmed == "yes")
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

fn write_file_atomic(output_path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = output_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("musicdb.out");
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    if let Err(err) = fs::write(&temp_path, bytes) {
        let _ = fs::remove_file(&temp_path);
        return Err(err.into());
    }
    if let Err(err) = fs::rename(&temp_path, output_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(err.into());
    }
    Ok(())
}

fn run_info(path: Option<PathBuf>) -> Result<()> {
    let path = resolve_path(path)?;
    let raw_bytes = load_raw_musicdb(&path)?;
    let raw = RawLibraryFile::from_raw_bytes(&raw_bytes)?;
    let reserialized = raw.to_raw_bytes()?;
    let roundtrip_exact = raw_bytes == reserialized;
    let lib = Library::from(&raw);
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
    println!("HSMA Sections         : {}", m.hsma_count);
    println!("Library Persistent ID : {}", m.persistent_id);
    println!(
        "iTunes Persistent ID  : {}",
        m.itunes_persistent_id
            .map_or_else(|| "<none>".to_string(), |id| id.to_string())
    );
    println!(
        "Date Modified         : {}",
        m.date_modified
            .map_or_else(|| "<none>".to_string(), |d| d.to_string())
    );
    let tz_hours = f64::from(m.timezone_offset_seconds) / 3600.0;
    println!(
        "Timezone Offset (s)   : {} ({tz_hours:+.1}h)",
        m.timezone_offset_seconds
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
        println!("Media Folder URI      : {uri}");
    }
    if let Some(p) = &m.media_folder_path {
        println!("Media Folder Path     : {p}");
    }
    if let Some(itl) = &m.imported_itl_path {
        println!("Imported .itl Path    : {itl}");
    }
    print_hsma_sections(&raw.sections, &raw.trailing_bytes);
    Ok(())
}

fn print_hsma_sections(sections: &[HsmaSection], trailing_bytes: &[u8]) {
    println!("\n=== hsma Major Sections ({}) ===", sections.len());
    for (i, sec) in sections.iter().enumerate() {
        let desc = describe_hsma_child(&sec.child);
        println!(
            "  [{i}] subtype={:<2} ({}) total_size={:<10} -> {desc}",
            sec.header.subtype,
            sec.header.kind(),
            sec.header.effective_total_size(),
        );
    }
    if !trailing_bytes.is_empty() {
        println!("Trailing bytes after last hsma: {}", trailing_bytes.len());
    }
}

fn describe_hsma_child(child: &HsmaChild) -> String {
    match child {
        HsmaChild::InnerHfma(h) => format!("InnerHfma (header_size={})", h.header_size),
        HsmaChild::LibraryMaster(p) => format!(
            "plma LibraryMaster (header_size={}, bomas={})",
            p.header.header_size,
            p.bomas.len()
        ),
        HsmaChild::AlbumList(l) => format!(
            "lama AlbumList (header_size={}, albums={})",
            l.header.header_size,
            l.albums.len()
        ),
        HsmaChild::ArtistList(l) => format!(
            "lAma ArtistList (header_size={}, artists={})",
            l.header.header_size,
            l.artists.len()
        ),
        HsmaChild::TrackList(l) => format!(
            "ltma TrackList (header_size={}, tracks={})",
            l.header.header_size,
            l.tracks.len()
        ),
        HsmaChild::PlaylistList(l) => format!(
            "lPma PlaylistList (header_size={}, playlists={})",
            l.header.header_size,
            l.playlists.len()
        ),
        HsmaChild::PendingCloudDeletions(d) => format!(
            "DCma PendingCloudDeletions (header_size={}, items={})",
            d.header.header_size,
            d.items.len()
        ),
        HsmaChild::SocialProfileList(s) => format!(
            "Lsma SocialProfileList (header_size={}, profiles={})",
            s.header.header_size,
            s.profiles.len()
        ),
        HsmaChild::Padding(p) => format!("LPma Padding (section_size={})", p.section_size),
        HsmaChild::Unknown {
            kind,
            signature,
            raw_bytes,
        } => format!(
            "Unknown(kind={kind}, sig={:?}, len={})",
            String::from_utf8_lossy(signature),
            raw_bytes.len()
        ),
        HsmaChild::Skipped { kind, raw_bytes } => {
            format!("Skipped(kind={kind}, len={})", raw_bytes.len())
        }
        _ => "Other".to_string(),
    }
}

fn run_unknowns(path: Option<PathBuf>, json: bool) -> Result<()> {
    let path = resolve_path(path)?;
    let raw = RawLibraryFile::open(&path)?;
    let report = analyze_unknowns(&raw);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_unknowns_report(&path, &report);
    }
    Ok(())
}

fn run_sanity_check(path: Option<PathBuf>, include_info: bool, json: bool) -> Result<()> {
    let path = resolve_path(path)?;
    let raw = RawLibraryFile::open(&path)?;
    let opts = SanityCheckOptions {
        include_info,
        ..SanityCheckOptions::default()
    };
    let report = analyze_sanity(&raw, &opts);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{report}");
    }
    Ok(())
}

fn run_query_unknowns(
    path: Option<PathBuf>,
    options: &QueryUnknownsOptions,
    json: bool,
) -> Result<()> {
    let path = resolve_path(path)?;
    let raw = RawLibraryFile::open(&path)?;
    let report = query_unknowns(&raw, options)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{report}");
    }
    Ok(())
}

fn print_unknowns_report(path: &Path, report: &UnknownsReport) {
    println!("=== Unknowns & Invariant Analysis: {} ===", path.display());
    if !report.unknown_hsma_sections.is_empty() {
        println!(
            "\n0. Unhandled hsma sections: {}",
            report.unknown_hsma_sections.len()
        );
        for (subtype, sig, len) in &report.unknown_hsma_sections {
            println!(
                "   - subtype={subtype} ({}) sig={sig:?} ({len} bytes)",
                musicdb::SectionKind::from(*subtype)
            );
        }
    }
    println!(
        "\n1. Unhandled boma subtypes: {}",
        report.unknown_boma_subtypes.len()
    );
    for (k, v) in &report.unknown_boma_subtypes {
        println!("   - {k}: {v} occurrences");
    }
    println!("\n2. All boma subtypes encountered:");
    for (k, v) in &report.boma_subtype_counts {
        println!("   - {k:<42} : {v}");
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
        println!("     ! {ex}");
    }

    print_unknown_field_summaries(report);
}

fn print_unknown_field_summaries(report: &UnknownsReport) {
    println!(
        "\n5. Unknown Fields with Non-Zero Data ({} fields):",
        report.nonzero_unknown_fields.len()
    );
    for f in &report.nonzero_unknown_fields {
        let top: Vec<_> = f
            .sample_values
            .iter()
            .take(3)
            .map(|(val, cnt)| format!("{val} (x{cnt})"))
            .collect();
        println!(
            "   - {:<14} {:<28} (off {:<9}): {:>5}/{:<5} non-zero ({} distinct) | top: {:?}",
            f.section,
            f.field_name,
            f.offset_info,
            f.nonzero_instances,
            f.total_instances,
            f.distinct_nonzero_count,
            top
        );
    }
}

fn run_diff(path_a: &Path, path_b: &Path, json: bool) -> Result<()> {
    let raw_a = RawLibraryFile::open(path_a)?;
    let raw_b = RawLibraryFile::open(path_b)?;
    let report = diff_libraries(&raw_a, &raw_b);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_diff_header_and_counts(path_a, path_b, &report);
        print_diff_entities(&report);
    }
    Ok(())
}

fn print_diff_header_and_counts(path_a: &Path, path_b: &Path, report: &LibraryDiffReport) {
    println!(
        "=== Library Diff: {} vs {} ===",
        path_a.display(),
        path_b.display()
    );
    if !report.hfma_changed_4byte_offsets.is_empty() {
        println!("\nhfma (outer header) 4-byte changes:");
        for (off, va, vb) in &report.hfma_changed_4byte_offsets {
            println!(
                "  offset {:>3}..{:>3}: 0x{va:08X} ({va}) -> 0x{vb:08X} ({vb})",
                off,
                off + 4
            );
        }
    }
    if report.hsma_10_hex_a == report.hsma_10_hex_b {
        println!("\nhsma subtype 10 UNCHANGED ({:?})", report.hsma_10_hex_a);
    } else {
        println!(
            "\nhsma subtype 10 CHANGED:\n  A: {:?}\n  B: {:?}",
            report.hsma_10_hex_a, report.hsma_10_hex_b
        );
    }
    if !report.plma_changed_4byte_offsets.is_empty() || !report.plma_boma_changes.is_empty() {
        println!("\nplma (LibraryMaster) changes:");
        for (off, va, vb) in &report.plma_changed_4byte_offsets {
            println!(
                "  offset {:>3}..{:>3}: 0x{va:08X} ({va}) -> 0x{vb:08X} ({vb})",
                off,
                off + 4
            );
        }
        for bc in &report.plma_boma_changes {
            println!("  * {bc}");
        }
    }
    println!(
        "\nTracks in A: {}, Tracks in B: {}, Common: {}",
        report.tracks_in_a, report.tracks_in_b, report.common_tracks
    );
    if !report.added_tracks.is_empty() {
        println!("Added tracks: {}", report.added_tracks.len());
        for t in report.added_tracks.iter().take(10) {
            println!("  + {t}");
        }
    }
    if !report.removed_tracks.is_empty() {
        println!("Removed tracks: {}", report.removed_tracks.len());
        for t in report.removed_tracks.iter().take(10) {
            println!("  - {t}");
        }
    }
}

fn print_diff_entities(report: &LibraryDiffReport) {
    println!("\nitma 4-byte offsets that changed across common tracks:");
    for (off, cnt) in &report.itma_offset_change_frequencies {
        println!(
            "  offset {:>3}..{:>3}: changed in {cnt} tracks",
            off,
            off + 4
        );
    }
    println!("\nTrackNumerics (boma 0x01) 4-byte offsets that changed across common tracks:");
    for (off, cnt) in &report.track_numerics_offset_change_frequencies {
        println!(
            "  offset {:>3}..{:>3}: changed in {cnt} tracks",
            off,
            off + 4
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
                println!("    hl: {hl}");
            }
            for (off, va, vb) in &td.itma_changed_4byte_offsets {
                println!(
                    "    itma {:>3}..{:>3}: 0x{va:08X} ({va}) -> 0x{vb:08X} ({vb})",
                    off,
                    off + 4
                );
            }
            for (off, va, vb) in &td.numerics_changed_4byte_offsets {
                println!(
                    "    num  {:>3}..{:>3}: 0x{va:08X} ({va}) -> 0x{vb:08X} ({vb})",
                    off,
                    off + 4
                );
            }
            for (off, va, vb) in &td.plays_skips_changed_4byte_offsets {
                println!(
                    "    ps   {:>3}..{:>3}: 0x{va:08X} ({va}) -> 0x{vb:08X} ({vb})",
                    off,
                    off + 4
                );
            }
        }
    }
    if !report.album_diffs.is_empty() {
        println!("\nAlbum (iama) Diffs ({}):", report.album_diffs.len());
        for ad in &report.album_diffs {
            println!("  {ad}");
        }
    }
    if !report.artist_diffs.is_empty() {
        println!("\nArtist (iAma) Diffs ({}):", report.artist_diffs.len());
        for rd in &report.artist_diffs {
            println!("  {rd}");
        }
    }
    if !report.playlist_diffs.is_empty() {
        println!("\nPlaylist (lpma) Diffs ({}):", report.playlist_diffs.len());
        for pd in &report.playlist_diffs {
            println!("  {pd}");
        }
    }
}

fn run_xml_check(musicdb_path: &Path, xml_path: &Path, json: bool) -> Result<()> {
    let raw = RawLibraryFile::open(musicdb_path)?;
    let report = compare_with_itunes_xml(&raw, xml_path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_xml_check_report(musicdb_path, xml_path, &report);
    }
    Ok(())
}

#[expect(
    clippy::cast_precision_loss,
    reason = "match percentage display only requires approximate f64 precision"
)]
fn print_xml_check_report(musicdb_path: &Path, xml_path: &Path, report: &XmlComparisonReport) {
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
        println!("  - {field:<28}: {matched:>5}/{total:<5} ({pct:>6.2}%)");
    }
}

fn run_decrypt(input: &Path, output: &Path) -> Result<()> {
    let raw_bytes = load_raw_musicdb(input)?;
    write_file_atomic(output, &raw_bytes)?;
    println!(
        "Wrote {} raw decrypted/decompressed bytes to {}",
        raw_bytes.len(),
        output.display()
    );
    Ok(())
}

fn run_export_json(input: &Path, output: Option<&Path>) -> Result<()> {
    let lib = Library::open(input)?;
    let json_str = lib.to_json_pretty()?;
    if let Some(out_path) = output {
        eprintln!(
            "Note: Exporting library data to {} (includes account IDs and purchaser metadata from Library.musicdb).",
            out_path.display()
        );
        write_file_atomic(out_path, json_str.as_bytes())?;
        println!("Exported library JSON to {}", out_path.display());
    } else {
        println!("{json_str}");
    }
    Ok(())
}

fn run_export_sqlite(input: &Path, output: &Path) -> Result<()> {
    let lib = Library::open(input)?;
    eprintln!(
        "Note: Exporting library data to {} (includes account IDs and purchaser metadata from Library.musicdb).",
        output.display()
    );
    lib.export_sqlite(output)?;
    println!(
        "Exported SQLite database ({} tracks, {} albums, {} artists, {} playlists, {} social profiles) to {}",
        lib.tracks.len(),
        lib.albums.len(),
        lib.artists.len(),
        lib.playlists.len(),
        lib.social_profiles.len(),
        output.display()
    );
    Ok(())
}

fn run_query_playlist(input: &Path, query: Option<&str>, json: bool) -> Result<()> {
    let lib = Library::open(input)?;
    let Some(query) = query else {
        if json {
            println!("{}", serde_json::to_string_pretty(&lib.playlists)?);
        } else {
            println!("=== Playlists ({}) ===", lib.playlists.len());
            for pl in &lib.playlists {
                println!(
                    "  [{}] kind={:<13?} sys={:<18?} dist_raw={:<3} folder={:<5} smart={:<5} master={:<5} parent={:<16} tracks={:<5} name={:?}",
                    pl.persistent_id,
                    pl.kind,
                    pl.system_kind,
                    pl.system_kind.raw(),
                    pl.is_folder,
                    pl.is_smart,
                    pl.is_master,
                    pl.parent_persistent_id
                        .map_or_else(|| "-".to_string(), |id| id.to_string()),
                    pl.declared_track_count,
                    pl.name.as_deref().unwrap_or("<unnamed>")
                );
            }
        }
        return Ok(());
    };
    let pl = lib
        .playlist_by_id(query)
        .or_else(|| lib.playlist_by_name(query))
        .ok_or_else(|| anyhow::anyhow!("No playlist matching {query:?} found"))?;
    if json {
        println!("{}", serde_json::to_string_pretty(pl)?);
    } else {
        print_playlist_details(&lib, pl);
    }
    Ok(())
}

fn print_playlist_details(lib: &Library, pl: &Playlist) {
    println!(
        "=== Playlist: {:?} ({}) ===",
        pl.name.as_deref().unwrap_or("<unnamed>"),
        pl.persistent_id
    );
    println!("Kind                  : {:?}", pl.kind);
    println!(
        "Parent Persistent ID  : {}",
        pl.parent_persistent_id
            .map_or_else(|| "<root>".to_string(), |id| id.to_string())
    );
    println!("Share Mode            : {:?}", pl.share_mode);
    println!("Show in Profile/Search: {}", pl.show_in_profile_and_search);
    println!("Suggestion Flag       : {:?}", pl.suggestion_flag);
    if let Some(desc) = &pl.description {
        println!("Description           : {desc}");
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
    let tracks: Vec<_> = lib.playlist_tracks(pl).collect();
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

fn run_query_track(input: &Path, query: &str, json: bool) -> Result<()> {
    let lib = Library::open(input)?;
    let matches: Vec<_> = if let Some(exact) = lib.track_by_id(query) {
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
        println!("Found {} matching track(s) for {query:?}:", matches.len());
        for t in matches.iter().take(25) {
            println!(
                "  [{}] {:?} - {:?} (Album: {:?}, Stars: {}, Cloud: {:?}, UnsyncedLocal: {})",
                t.persistent_id,
                t.artist.as_deref().unwrap_or(""),
                t.title.as_deref().unwrap_or(""),
                t.album.as_deref().unwrap_or(""),
                t.stars(),
                t.cloud_status,
                t.has_unsynced_local_changes
            );
        }
    }
    Ok(())
}

fn run_plist(path: &Path) -> Result<()> {
    let val = plist::Value::from_file(path)?;
    println!("{val:#?}");
    Ok(())
}

fn run_donate_library(path: Option<&Path>, output: &Path, yes: bool) -> Result<()> {
    let bundle_dir = if let Some(user_path) = path {
        find_musiclibrary_bundle(user_path).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not find an Apple Music .musiclibrary bundle inside {}",
                user_path.display()
            )
        })?
    } else {
        discover_default_library_path().ok_or_else(|| {
            anyhow::anyhow!(
                "Could not auto-discover a default Apple Music Library in your Music directory. Please specify a path."
            )
        })?
    };

    if !yes
        && !prompt_yes_no(&format!(
            "Found Apple Music Library at {:?}.\nNote: Library.musicdb contains purchaser names/emails and Apple account IDs.\nPackage metadata into {:?}?",
            bundle_dir.display(),
            output.display()
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }
    if yes {
        eprintln!(
            "Note: Packaging library metadata from {} into {} (includes purchaser names/emails and Apple account IDs).",
            bundle_dir.display(),
            output.display()
        );
    }

    let report = package_donated_library(&bundle_dir, output)?;
    println!(
        "Created {} ({} bytes, {} files):",
        report.output_path.display(),
        report.archive_size_bytes,
        report.archived_paths.len()
    );
    for p in &report.archived_paths {
        println!("  + {p}");
    }
    offer_previous_libraries_donation(&bundle_dir, output, yes)
}

fn offer_previous_libraries_donation(bundle_dir: &Path, output: &Path, yes: bool) -> Result<()> {
    let Some(prev_dir) = find_previous_libraries_for_bundle(bundle_dir) else {
        return Ok(());
    };
    let prev_bundles = list_previous_library_bundles(&prev_dir);
    if prev_bundles.is_empty() {
        return Ok(());
    }
    let prev_output = output
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(DEFAULT_DONATED_PREVIOUS_LIBRARIES_ARCHIVE);
    let should_donate_prev = if yes {
        true
    } else {
        prompt_yes_no(&format!(
            "\nFound {} previous library snapshot(s) in {:?}. Donate them into {:?} as well?",
            prev_bundles.len(),
            prev_dir.display(),
            prev_output.display()
        ))?
    };

    if should_donate_prev {
        let prev_report = package_donated_previous_libraries(&prev_dir, &prev_output)?;
        println!(
            "Created {} ({} bytes, {} bundles, {} files):",
            prev_report.output_path.display(),
            prev_report.archive_size_bytes,
            prev_report.bundle_count,
            prev_report.archived_paths.len()
        );
        for p in &prev_report.archived_paths {
            println!("  + {p}");
        }
    }
    Ok(())
}

fn run_donate_previous_libraries(path: Option<&Path>, output: &Path, yes: bool) -> Result<()> {
    let prev_dir = if let Some(user_path) = path {
        find_previous_libraries_dir(user_path).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not find a 'Previous Libraries' folder with .musiclibrary bundles inside {}",
                user_path.display()
            )
        })?
    } else {
        discover_default_previous_libraries_path().ok_or_else(|| {
            anyhow::anyhow!(
                "Could not auto-discover a 'Previous Libraries' folder in your Music directory. Please specify a path."
            )
        })?
    };

    let count = list_previous_library_bundles(&prev_dir).len();
    if !yes
        && !prompt_yes_no(&format!(
            "Found {count} previous library snapshot(s) in {:?}.\nNote: Library.musicdb contains purchaser names/emails and Apple account IDs.\nPackage metadata into {:?}?",
            prev_dir.display(),
            output.display()
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }
    if yes {
        eprintln!(
            "Note: Packaging {count} previous library snapshot(s) from {} into {} (includes purchaser names/emails and Apple account IDs).",
            prev_dir.display(),
            output.display()
        );
    }

    let report = package_donated_previous_libraries(&prev_dir, output)?;
    println!(
        "Created {} ({} bytes, {} bundles, {} files):",
        report.output_path.display(),
        report.archive_size_bytes,
        report.bundle_count,
        report.archived_paths.len()
    );
    for p in &report.archived_paths {
        println!("  + {p}");
    }
    Ok(())
}

fn run_completions(shell: Shell) {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    generate(shell, &mut cmd, bin_name, &mut io::stdout());
}

fn print_smart_rule_group(group: &musicdb::SmartRuleGroup, indent: usize) {
    let pad = "  ".repeat(indent);
    for node in &group.rules {
        match node {
            musicdb::SmartRuleNode::Group(sub) => {
                println!("{pad}[Nested Group: Match {:?}]", sub.conjunction);
                print_smart_rule_group(sub, indent + 1);
            }
            musicdb::SmartRuleNode::Condition(cond) => {
                println!("{pad}- {}", cond.description);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_query_unknowns_parsing() {
        let cli = Cli::try_parse_from([
            "musicdb",
            "query-unknowns",
            "-f",
            "iama.unk_28",
            "-v",
            "2",
            "--nonzero",
            "--examples",
            "10",
            "--tracks",
            "3",
            "--json",
        ])
        .unwrap();
        match cli.command {
            Commands::QueryUnknowns {
                path,
                field,
                entity,
                value,
                nonzero,
                include_zero,
                examples,
                all,
                tracks,
                flat,
                list_fields,
                json,
            } => {
                assert!(path.is_none());
                assert_eq!(field.as_deref(), Some("iama.unk_28"));
                assert!(entity.is_none());
                assert_eq!(value.as_deref(), Some("2"));
                assert!(nonzero);
                assert!(!include_zero);
                assert_eq!(examples, Some(10));
                assert!(!all);
                assert_eq!(tracks, 3);
                assert!(!flat);
                assert!(!list_fields);
                assert!(json);
            }
            _ => panic!("expected QueryUnknowns"),
        }
    }

    #[test]
    fn cli_query_unknowns_validation() {
        let cli_entity = Cli::try_parse_from([
            "musicdb",
            "query-unknowns",
            "lib.musicdb",
            "-e",
            "album:Moving",
        ])
        .unwrap();
        assert!(matches!(
            cli_entity.command,
            Commands::QueryUnknowns {
                path: Some(_),
                entity: Some(_),
                ..
            }
        ));

        let cli_list = Cli::try_parse_from(["musicdb", "query-unknowns", "--list-fields"]).unwrap();
        assert!(matches!(
            cli_list.command,
            Commands::QueryUnknowns {
                list_fields: true,
                ..
            }
        ));

        assert!(
            Cli::try_parse_from(["musicdb", "query-unknowns"]).is_err(),
            "bare query-unknowns without -f/-e/--list-fields must fail"
        );
        assert!(
            Cli::try_parse_from([
                "musicdb",
                "query-unknowns",
                "-f",
                "iama.unk_28",
                "--nonzero",
                "--include-zero"
            ])
            .is_err(),
            "--nonzero and --include-zero must conflict"
        );
    }
}
