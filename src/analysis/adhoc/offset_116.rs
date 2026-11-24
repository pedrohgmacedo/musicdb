//! Ad-hoc probe: `TrackNumerics[116..120]` (`unk_116_cloud_timestamp`) value breakdown & corpus scan.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
use musicdb::sections::{AppleTimestamp, BomaPayload, RawLibraryFile};

use super::helpers::{boma_str, collect_mdb, load_1119, tracks_of};

pub(crate) fn run(corpus_dir: &Path) -> Result<()> {
    let raw = load_1119(corpus_dir)?;
    println!("=== TrackNumerics[116..120] BREAKDOWN & SONG EXAMPLES ===");
    check_offset_116(&raw);
    println!(
        "\n=== TrackNumerics[116..120] ACROSS ALL CORPUS FILES & COMPARISON WITH date_released/date_added ==="
    );
    check_116_across_all_files(&raw, corpus_dir)?;
    Ok(())
}

fn check_offset_116(raw: &RawLibraryFile) {
    type Offset116Key = (u32, u8, u8, String);
    let mut cats: BTreeMap<Offset116Key, (usize, Vec<String>)> = BTreeMap::new();
    for t in tracks_of(raw) {
        let title = boma_str(&t.bomas, 0x0002).unwrap_or("?");
        let artist = boma_str(&t.bomas, 0x0004).unwrap_or("?");
        let album = boma_str(&t.bomas, 0x0003).unwrap_or("?");
        for b in &t.bomas {
            if let BomaPayload::TrackNumerics(n) = &b.payload {
                let v = *n.unk_116_cloud_timestamp;
                let off45 = n.unk_42_54.get(3).copied().unwrap_or(0);
                let ts = AppleTimestamp::from(v).to_utc();
                let cat = if v == 0 {
                    String::from("0 (zero)")
                } else if let Some(dt) = ts {
                    let s = dt.to_string();
                    let y = s.get(..4).unwrap_or("?");
                    if matches!(
                        y,
                        "2014"
                            | "2015"
                            | "2016"
                            | "2017"
                            | "2018"
                            | "2019"
                            | "2020"
                            | "2021"
                            | "2022"
                            | "2023"
                            | "2024"
                            | "2025"
                            | "2026"
                            | "2027"
                    ) {
                        format!("AppleTimestamp year={y}")
                    } else {
                        format!("non-2014..2027 ts: 0x{v:08X} ({s})")
                    }
                } else {
                    format!("raw non-ts: 0x{v:08X}")
                };

                let entry = cats
                    .entry((n.subrecord_kind, t.header.cloud_status, off45, cat))
                    .or_default();
                entry.0 += 1;
                if entry.1.len() < 4 {
                    entry.1.push(format!(
                        "[{:016X}] {} - {:?} ({:?}) | v116=0x{v:08X} ({:?}) delay={} pad={} flags=0x{:08X} tot_samples={} last8={}",
                        t.header.track_persistent_id,
                        artist,
                        title,
                        album,
                        ts.map(|d| d.to_string()),
                        n.gapless_encoder_delay,
                        n.gapless_encoder_padding,
                        n.gapless_playback_flags,
                        n.gapless_total_samples,
                        n.gapless_last_eight_frames_offset
                    ));
                }
            }
        }
    }

    for ((sk, cs, off45, cat), (count, exs)) in &cats {
        println!("  sub={sk} cloud={cs} off45={off45} [{cat}] -> {count} records");
        for ex in exs {
            println!("    - {ex}");
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc corpus-wide TrackNumerics[116..120] scan"
)]
fn check_116_across_all_files(raw_1119: &RawLibraryFile, corpus_dir: &Path) -> Result<()> {
    let mut rel_cmp: BTreeMap<&str, usize> = BTreeMap::new();
    let mut eq_rel_samples = Vec::new();
    let mut ne_rel_samples = Vec::new();
    for t in tracks_of(raw_1119) {
        let title = boma_str(&t.bomas, 0x0002).unwrap_or("?");
        let artist = boma_str(&t.bomas, 0x0004).unwrap_or("?");
        for b in &t.bomas {
            if let BomaPayload::TrackNumerics(n) = &b.payload
                && *n.unk_116_cloud_timestamp != 0
            {
                let v = *n.unk_116_cloud_timestamp;
                let rel = n.date_released.raw();
                let add = n.date_added.raw();
                let sync = n.date_cloud_synced.raw();
                if v == rel {
                    *rel_cmp.entry("v116 == date_released").or_default() += 1;
                    if eq_rel_samples.len() < 5 {
                        eq_rel_samples.push(format!(
                            "sub={} cloud={} {} - {:?}: v116==date_released={:?}",
                            n.subrecord_kind,
                            t.header.cloud_status,
                            artist,
                            title,
                            AppleTimestamp::from(v).to_utc()
                        ));
                    }
                } else if v == add {
                    *rel_cmp.entry("v116 == date_added").or_default() += 1;
                } else if v == sync {
                    *rel_cmp.entry("v116 == date_cloud_synced").or_default() += 1;
                } else {
                    *rel_cmp.entry("v116 != released/added/synced").or_default() += 1;
                    if ne_rel_samples.len() < 8 {
                        ne_rel_samples.push(format!(
                            "sub={} cloud={} {} - {:?}: v116={:?} vs released={:?} added={:?} synced={:?}",
                            n.subrecord_kind,
                            t.header.cloud_status,
                            artist,
                            title,
                            AppleTimestamp::from(v).to_utc(),
                            n.date_released.to_utc(),
                            n.date_added.to_utc(),
                            n.date_cloud_synced.to_utc()
                        ));
                    }
                }
            }
        }
    }
    println!("  2026-11-19 v116 relation to other timestamps: {rel_cmp:?}");
    for s in &eq_rel_samples {
        println!("    [eq] {s}");
    }
    for s in &ne_rel_samples {
        println!("    [ne] {s}");
    }

    let mut files = Vec::new();
    collect_mdb(corpus_dir, &mut files)?;
    files.sort();
    for p in &files {
        let Ok(disk) = fs::read(p) else {
            continue;
        };
        let Ok(bytes) = musicdb::decrypt_and_decompress_bytes(&disk, musicdb::DEFAULT_MUSICDB_KEY)
        else {
            continue;
        };
        let Ok(raw) = RawLibraryFile::from_raw_bytes(&bytes) else {
            continue;
        };
        let mut non_ts = 0usize;
        let mut ts_cnt = 0usize;
        let mut sample_non_ts = None;
        for t in tracks_of(&raw) {
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    let v = *n.unk_116_cloud_timestamp;
                    if v == 0 {
                        continue;
                    }
                    if (0xC000_0000..=0xE900_0000).contains(&v) {
                        ts_cnt += 1;
                    } else {
                        non_ts += 1;
                        if sample_non_ts.is_none() {
                            sample_non_ts = Some(format!(
                                "sub={} 0x{v:08X} ({:?})",
                                n.subrecord_kind,
                                boma_str(&t.bomas, 0x0002)
                            ));
                        }
                    }
                }
            }
        }
        let rel = p.strip_prefix(corpus_dir).unwrap_or(p);
        if non_ts > 0
            || rel.to_string_lossy().contains("other-libraries")
            || rel.to_string_lossy().contains("2025-09-28")
            || rel.to_string_lossy().contains("2026-10-28")
        {
            println!(
                "  file {}: ts_cnt={ts_cnt}, non_ts={non_ts}, sample_non_ts={sample_non_ts:?}",
                rel.display()
            );
        }
    }
    Ok(())
}
