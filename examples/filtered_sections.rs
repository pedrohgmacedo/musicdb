//! Example demonstrating selective `hsma` section parsing with [`RawLibraryFile::open_filtered`].

use std::process::ExitCode;

use musicdb::sections::HsmaChild;
use musicdb::{RawLibraryFile, SectionKind};

fn run() -> musicdb::Result<ExitCode> {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("usage: filtered_sections <path/to/Library.musicdb|.musiclibrary>");
        return Ok(ExitCode::from(2));
    };

    let raw = RawLibraryFile::open_filtered(&path, |kind| {
        !matches!(kind, SectionKind::TrackList | SectionKind::PlaylistList)
    })?;

    println!(
        "Opened {} with {} hsma sections:",
        path.to_string_lossy(),
        raw.sections.len()
    );
    for (idx, sec) in raw.sections.iter().enumerate() {
        let kind = sec.header.kind();
        let total_size = sec.header.effective_total_size();
        match &sec.child {
            HsmaChild::Skipped { kind, raw_bytes } => {
                println!(
                    "  [{idx}] {kind} (total_size={total_size}): skipped ({} raw bytes preserved)",
                    raw_bytes.len()
                );
            }
            HsmaChild::Unknown {
                kind, raw_bytes, ..
            } => {
                println!(
                    "  [{idx}] {kind} (total_size={total_size}): unparsed ({} raw bytes preserved)",
                    raw_bytes.len()
                );
            }
            _ => {
                println!("  [{idx}] {kind} (total_size={total_size}): parsed");
            }
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
