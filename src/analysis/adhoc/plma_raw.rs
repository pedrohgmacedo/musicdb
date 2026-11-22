//! Ad-hoc probe: `plma` (`LibraryMasterHeader`) raw 194-byte layout & alignment across libraries.

use std::path::Path;

use anyhow::Result;

use super::helpers::{load_raw, plma_of};

pub(crate) fn run(corpus_dir: &Path) -> Result<()> {
    println!("=== plma (LibraryMasterHeader) RAW 194-BYTE HEX DUMP & ALIGNMENT ===");
    for (label, rel) in [
        (
            "2026-11-19",
            "snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb",
        ),
        (
            "Monterey",
            "libraries-music-samples/Monterey/SmallerLibrary.musicdb",
        ),
        (
            "Sequoia",
            "libraries-music-samples/Sequoia/MiniLibraryV7.musicdb",
        ),
    ] {
        let p = corpus_dir.join(rel);
        if !p.exists() {
            continue;
        }
        let raw = load_raw(&p)?;
        let Some(h) = plma_of(&raw) else {
            continue;
        };
        let u46 = h.apple_store_account_id_1;
        let u54 = u32::from_le_bytes(*h.unk_54_58);
        let u84 = h.apple_store_account_id_2;
        let u112 = u64::from_le_bytes(*h.unk_112_120);
        println!("  --- {label} (plma header_size={}) ---", h.header_size);
        println!("    46..54  (u64): 0x{u46:016X} ({u46})");
        println!(
            "    54..58  (u32): 0x{u54:08X} ({u54})         | bytes={:02X?}",
            *h.unk_54_58
        );
        println!(
            "    58..66  (u64): 0x{:016X} (library_persistent_id)",
            h.library_persistent_id
        );
        println!("    84..92  (u64): 0x{u84:016X} ({u84})");
        println!(
            "    92..100 (u64): 0x{:016X} (library_persistent_id_2)",
            h.library_persistent_id_2
        );
        println!(
            "    104..112(u64): 0x{:016X} ({}) (apple_store_account_id)",
            h.apple_store_account_id, h.apple_store_account_id
        );
        println!(
            "    112..120(u64): 0x{u112:016X} ({u112}) | bytes={:02X?}",
            *h.unk_112_120
        );
    }
    Ok(())
}
