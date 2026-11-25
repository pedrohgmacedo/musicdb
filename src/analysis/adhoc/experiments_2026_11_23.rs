//! Ad-hoc probe: `2026-11-23` offline-local-probes campaign
//! (`City Life` A/B playback + Sound Check, `itma[54]` tag tests, local video + ALAC/WAV/AIFF, cleanup).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use musicdb::sections::{BomaPayload, TrackItem, TrackNumerics, VideoInfo};

use super::helpers::{boma_str, load_raw, tracks_of};

const SNAP_ROOT: &str = "snapshots/2026-11-23-offline-local-probes";

fn dump_numerics(idx: usize, n: &TrackNumerics) {
    let u = *n.unk_42_54;
    println!(
        "    boma[{idx}] TrackNumerics sub={}: dur={} size={} br={} sr={} vbr={} fcc={:?}(0x{:08X}) u68=0x{:08X} fmt={} cfcc={:?}(0x{:08X}) cloud_codec={}",
        n.subrecord_kind,
        n.duration_ms,
        n.file_size,
        n.bit_rate,
        n.sample_rate,
        n.is_vbr,
        n.file_type_fourcc_str(),
        n.file_type_fourcc,
        *n.unk_68_file_type,
        n.container_format_kind,
        n.codec_fourcc_str(),
        n.codec_fourcc,
        n.cloud_codec_kind,
    );
    println!(
        "      gapless/norm: norm={} delay={} pad={} flags=0x{:X} tot_s={} last8={} art=(cnt={},emb={},loc={},bytes={}) dl_traits=0x{:X}",
        n.normalization,
        n.gapless_encoder_delay,
        n.gapless_encoder_padding,
        n.gapless_playback_flags,
        n.gapless_total_samples,
        n.gapless_last_eight_frames_offset,
        n.artwork_count,
        n.has_embedded_artwork,
        n.local_artwork_state,
        n.artwork_total_size_bytes,
        n.downloaded_audio_traits,
    );
    println!(
        "      bytes: u16={} u21_24={:02X?} u24_27={:02X?} is_mp3={} is_vid={} u42_54={u:02X?} off54={} u55_59={:02X?} off59={} u152=0x{:X} u300=0x{:X} u360=0x{:X}",
        *n.unk_16,
        *n.unk_21_24,
        *n.unk_24_27,
        n.is_local_mp3,
        n.is_local_video,
        n.asset_container_kind,
        *n.unk_55_59,
        n.catalog_lyrics_state,
        *n.unk_152,
        *n.unk_300,
        *n.unk_360,
    );
    let [b42, b43, b44, b45, b46, b47, b48, b49, b50, b51, b52, b53] = u;
    println!(
        "      u41_54 breakdown: b41(is_vid)={} b42={b42} b43={b43} b44={b44} b45={b45} b46={b46} b47={b47} b48={b48} b49_53={:02X?}",
        n.is_local_video,
        [b49, b50, b51, b52, b53],
    );
}

fn dump_video_info(idx: usize, v: &VideoInfo) {
    let codec_bytes = v.codec_fourcc.to_be_bytes();
    let codec_str = String::from_utf8_lossy(&codec_bytes);
    let u56_words: Vec<u32> = v
        .unk_56_84
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| u32::from_le_bytes(*chunk))
        .collect();
    println!(
        "    boma[{idx}] VideoInfo: w={} h={} unk_8={} codec={codec_str:?}(0x{:08X}) unk_16={} vbr={} unk_24={} unk_28={} prof={} lvl={} unk_40={} fps={} depth={} unk_52={} u56_words={u56_words:?}",
        v.width,
        v.height,
        *v.unk_8,
        v.codec_fourcc,
        *v.unk_16,
        v.video_bit_rate_kbps,
        *v.unk_24,
        *v.unk_28,
        v.avc_profile_idc,
        v.avc_level_idc,
        *v.unk_40,
        v.frame_rate_milli,
        v.color_bit_depth,
        *v.unk_52,
    );
}

fn dump_bomas(bomas: &[musicdb::sections::BomaSection]) {
    for (idx, b) in bomas.iter().enumerate() {
        match &b.payload {
            BomaPayload::TrackNumerics(n) => dump_numerics(idx, n),
            BomaPayload::TrackPlaysSkips(ps) => {
                println!(
                    "    boma[{idx}] TrackPlaysSkips: plays={}/{} last_play={:?} first_play_utc={:?} u24={} skips={}/{} last_skip={:?} u40={} u44_52={:02X?}",
                    ps.play_count,
                    ps.true_play_count,
                    ps.date_last_played.to_utc(),
                    ps.date_first_played_utc.to_utc(),
                    *ps.unk_24,
                    ps.skip_count,
                    ps.true_skip_count,
                    ps.date_last_skipped.to_utc(),
                    *ps.unk_40,
                    *ps.unk_44_52,
                );
            }
            BomaPayload::VideoInfo(v) => dump_video_info(idx, v),
            _ => {
                if let Some(s) = b.as_str() {
                    let preview = if s.chars().count() > 140 {
                        let prefix: String = s.chars().take(140).collect();
                        format!("{prefix}... (len={})", s.len())
                    } else {
                        s.to_string()
                    };
                    println!(
                        "    boma[{idx}] 0x{:04X} ({}) str: {preview:?}",
                        b.subtype,
                        b.subtype_name()
                    );
                } else {
                    println!(
                        "    boma[{idx}] 0x{:04X} ({}) total_size={}",
                        b.subtype,
                        b.subtype_name(),
                        b.total_size
                    );
                }
            }
        }
    }
}

fn dump_track_full(label: &str, t: &TrackItem) {
    let h = &t.header;
    let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
    let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
    let album = boma_str(&t.bomas, 0x0003).unwrap_or("");
    let kind = boma_str(&t.bomas, 0x0006).unwrap_or("");
    println!(
        "  [{label}] [{:016X}] {artist:?} / {album:?} / {title:?} | kind={kind:?}",
        h.track_persistent_id,
    );
    println!(
        "    itma flags: dl={} cloud={} sync_state={} unsynced_local={} pl_only={} ever_played={} playback_or_bounds={} gapless_cb={} rem_pos={} media_kind=0x{:X} u64={} audio_traits=0x{:02X}",
        h.downloaded_status,
        h.cloud_status,
        h.cloud_sync_state,
        h.has_unsynced_local_changes,
        h.playlist_only,
        h.has_ever_been_played,
        h.has_playback_or_bounds_activity,
        h.checkbox_part_of_gapless_album,
        h.checkbox_remember_playback_position,
        h.media_kind,
        h.store_media_kind,
        h.audio_traits,
    );
    println!(
        "    itma unk: u28_30={:02X?} u32={} is_cloud_synced={} u34_38={:02X?} u40_42={:02X?} u43={} u45={} u47_49={:02X?} u53={} has_asset_info={} u56={} u60_62={:02X?} u112={} u156={} u329_331={:02X?} u332={} u340={}",
        *h.unk_28_30,
        *h.unk_32,
        h.is_cloud_synced,
        *h.unk_34_38,
        *h.unk_40_42,
        *h.unk_43,
        *h.unk_45,
        *h.unk_47_49,
        h.has_extended_text_metadata,
        h.has_asset_info_plist,
        *h.unk_56,
        *h.unk_60_62,
        *h.unk_112,
        *h.unk_156,
        *h.unk_329_331,
        *h.unk_332,
        *h.unk_340,
    );
    println!(
        "    itma art: src={} state={} sz_kind={} img_fmt={} uuid={} auto_sort={:02X?} sync_utc={:?}",
        h.artwork_source_kind,
        h.artwork_cache_state,
        h.artwork_size_kind,
        h.artwork_image_format,
        hex::encode(h.artwork_uuid),
        h.auto_sort_flags,
        h.date_cloud_synced.to_utc(),
    );
    dump_bomas(&t.bomas);
}

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc inspection of all four 2026-11-23 offline-local-probes campaign steps"
)]
pub(crate) fn run(corpus_dir: &Path) -> Result<()> {
    println!("=== 2026-11-23 OFFLINE-LOCAL-PROBES CAMPAIGN ANALYSIS ===");

    let snaps = [
        ("00_baseline", "00_baseline"),
        ("01_city_life", "01_city_life_offline_playback"),
        ("02_itma54", "02_itma54_tag_tests"),
        ("03_local_fmt", "03_local_video_and_lossless_audio"),
        ("04_undo", "04_undo_and_restore_library"),
    ];
    let mut loaded = Vec::new();
    for (short, dir) in snaps {
        let p = corpus_dir
            .join(SNAP_ROOT)
            .join(dir)
            .join("Apple Music Library.musiclibrary/Library.musicdb");
        loaded.push((short, load_raw(&p)?));
    }

    // 1. Experiment 2A: City Life (orig, A, B) & Camel - Nude across 00, 01, 02, 04
    println!("\n--- PART 1: Experiment 2A (City Life orig / A / B across snapshots) ---");
    for (short, raw) in &loaded {
        for t in tracks_of(raw) {
            let pid = t.header.track_persistent_id;
            let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
            let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
            let interesting = matches!(
                pid,
                0xB3C1_FFDC_0A45_DD68
                    | 0x0F62_E4F9_A143_8516
                    | 0x9F57_8D9F_D288_32EB
                    | 0xFFAD_DE17_DB88_42D8
            ) || (artist == "Camel" && title.starts_with("City Life"));
            if interesting
                && (*short == "00_baseline"
                    || *short == "01_city_life"
                    || ((*short == "02_itma54" || *short == "04_undo")
                        && matches!(pid, 0x0F62_E4F9_A143_8516 | 0x9F57_8D9F_D288_32EB)))
            {
                dump_track_full(short, t);
            }
        }
    }

    // 2. Experiment 2B: itma[54] 5 fixtures in 02_itma54 and 03_local_fmt (after background scan)
    println!("\n--- PART 2: Experiment 2B (itma[54] 5 fixtures in 02 and 03) ---");
    for (short, raw) in loaded
        .iter()
        .filter(|(s, _)| matches!(*s, "02_itma54" | "03_local_fmt"))
    {
        println!("  >> Snapshot {short}:");
        let mut tracks: Vec<&TrackItem> = tracks_of(raw)
            .iter()
            .filter(|t| {
                boma_str(&t.bomas, 0x0003).is_some_and(|alb| alb == "ZZZ Probe Signals (itma54)")
            })
            .collect();
        tracks.sort_by_key(|t| t.header.track_number);
        for t in tracks {
            dump_track_full(short, t);
        }
    }

    // 3. Experiment 2E: local video (.mp4), ALAC (.m4a), WAV, AIFF in 03_local_fmt
    if let Some((short3, raw3)) = loaded.iter().find(|(s, _)| *s == "03_local_fmt") {
        println!(
            "\n--- PART 3: Experiment 2E (Local Video + ALAC + WAV + AIFF in 03_local_fmt) ---"
        );
        for t in tracks_of(raw3) {
            if matches!(
                t.header.track_persistent_id,
                0xAC4A_A560_6D9B_880C
                    | 0xD98A_C926_FB1A_C81B
                    | 0x8B60_5B7E_8111_8CE6
                    | 0x989C_F90D_6A53_71D8
            ) {
                dump_track_full(short3, t);
            }
        }

        // 4. Compare Stylo VideoInfo side-by-side
        println!(
            "\n--- PART 4: Existing Stylo VideoInfo in 03_local_fmt for direct comparison ---"
        );
        if let Some(stylo) = tracks_of(raw3)
            .iter()
            .find(|t| t.header.track_persistent_id == 0x022A_2731_DE16_355B)
        {
            dump_track_full("03_stylo", stylo);
        }

        // 5. Snapshot-wide distributions for newly observed flags in 03_local_fmt
        println!(
            "\n--- PART 5: 03_local_fmt distributions (itma[54], itma[47], itma[32], unk_21_24) ---"
        );
        let mut i54_vs_1d: BTreeMap<(u8, bool), usize> = BTreeMap::new();
        let mut i47_vs_tags: BTreeMap<(u8, bool, bool), usize> = BTreeMap::new();
        let mut u21_24_local: BTreeMap<([u8; 3], u8, u16, u8), usize> = BTreeMap::new();
        for t in tracks_of(raw3) {
            let i54 = t.header.has_asset_info_plist;
            let [i47, _] = *t.header.unk_47_49;
            let has_1d = t.bomas.iter().any(|b| b.subtype == 0x001D);
            let has_artist = boma_str(&t.bomas, 0x0004).is_some();
            let has_album = boma_str(&t.bomas, 0x0003).is_some();
            *i54_vs_1d.entry((i54, has_1d)).or_default() += 1;
            if i47 != 0 || (!has_artist && !has_album) {
                *i47_vs_tags.entry((i47, has_artist, has_album)).or_default() += 1;
            }
            if t.header.cloud_status == 0 {
                for b in &t.bomas {
                    if let BomaPayload::TrackNumerics(n) = &b.payload {
                        *u21_24_local
                            .entry((
                                *n.unk_21_24,
                                t.header.has_ever_been_played,
                                n.container_format_kind,
                                n.is_local_mp3,
                            ))
                            .or_default() += 1;
                    }
                }
            }
        }
        println!("  itma[54] vs boma 0x001D (i54, has_0x1D): {i54_vs_1d:?}");
        println!("  itma[47] vs artist/album (i47, has_artist, has_album): {i47_vs_tags:?}");
        println!("  LocalOnly unk_21_24 (u21_24, ever_played, fmt, is_mp3): {u21_24_local:?}");
    }

    Ok(())
}
