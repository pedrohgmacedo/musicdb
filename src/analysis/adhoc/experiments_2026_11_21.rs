//! Ad-hoc probe: `2026-11-21` experiments (`City Life` A/B offline `APEv2` + `Stylo` `.m4v` download).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use musicdb::sections::BomaPayload;

use super::helpers::{boma_str, collect_mdb, load_raw, tracks_of};

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc analysis of 2026-11-21 City Life A/B and Stylo video snapshots"
)]
pub(crate) fn run(corpus_dir: &Path) -> Result<()> {
    println!("=== 2026-11-21 EXPERIMENTS (CITY LIFE A/B + STYLO VIDEO + CORPUS VIDEOINFO) ===");

    let base_p = corpus_dir.join(
        "snapshots/2026-11-21-city-life-and-video/baseline/Apple Music Library.musiclibrary/Library.musicdb",
    );
    let mut_p = corpus_dir.join(
        "snapshots/2026-11-21-city-life-and-video/mutated/Apple Music Library.musiclibrary/Library.musicdb",
    );
    let vid_p = corpus_dir.join(
        "snapshots/2026-11-21-city-life-and-video/video/Apple Music Library.musiclibrary/Library.musicdb",
    );

    let raw_base = load_raw(&base_p)?;
    let raw_mut = load_raw(&mut_p)?;
    let raw_vid = load_raw(&vid_p)?;

    // 9.1 City Life original vs A vs B + Nude, plus has_unsynced_local_changes distribution
    println!("--- 9.1 City Life (orig / A / B) & Nude raw fields in 2026-11-21-mutated ---");
    for t in tracks_of(&raw_mut) {
        let pid = t.header.track_persistent_id;
        let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
        let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
        if matches!(
            pid,
            0xB3C1_FFDC_0A45_DD68 | 0x0CB6_197D_EA26_44FA | 0x7758_8155_0568_70D3
        ) || (artist == "Camel" && title == "Nude")
        {
            let h = &t.header;
            println!(
                "  [{pid:016X}] {artist} - {title:?} | cloud={} sync_state={} unsynced_local={} pl_only={} dl={} itma_sync={:?}",
                h.cloud_status,
                h.cloud_sync_state,
                h.has_unsynced_local_changes,
                h.playlist_only,
                h.downloaded_status,
                h.date_cloud_synced.to_utc(),
            );
            println!(
                "    itma: u32={} is_cloud_synced={} u34_38={:02X?} u53_55={:02X?} u64={} art=(src={},state={},sz={},fmt={}) saga1=0x{:X} saga=0x{:X}",
                *h.unk_32,
                h.is_cloud_synced,
                *h.unk_34_38,
                *h.unk_53_55,
                *h.unk_64_store_flag,
                h.artwork_source_kind,
                h.artwork_cache_state,
                h.artwork_size_kind,
                h.artwork_image_format,
                h.cloud_saga_id_1,
                h.cloud_saga_id,
            );
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    println!(
                        "    num sub={}: dur={} size={} br={} sr={} fcc={:?} cfcc={:?} norm={} delay={} pad={} tot_s={} last8={} emb={} art_sz={} lyr_h={}",
                        n.subrecord_kind,
                        n.duration_ms,
                        n.file_size,
                        n.bit_rate,
                        n.sample_rate,
                        n.file_type_fourcc_str(),
                        n.codec_fourcc_str(),
                        n.normalization,
                        n.gapless_encoder_delay,
                        n.gapless_encoder_padding,
                        n.gapless_total_samples,
                        n.gapless_last_eight_frames_offset,
                        n.has_embedded_artwork,
                        n.artwork_total_size_bytes,
                        n.custom_lyrics_hash,
                    );
                    println!(
                        "      u16={} u21_24={:02X?} is_mp3={} u41_54={:02X?} off54={} u55_59={:02X?} off59={} dadd={:?} dmod={:?} drel={:?} dsync={:?}",
                        *n.unk_16,
                        *n.unk_21_24,
                        n.is_local_mp3,
                        *n.unk_41_54,
                        n.asset_container_kind,
                        *n.unk_55_59,
                        n.catalog_lyrics_state,
                        n.date_added.to_utc(),
                        n.date_modified.to_utc(),
                        n.date_released.to_utc(),
                        n.date_cloud_synced.to_utc(),
                    );
                }
            }
        }
    }

    let mut sync_dist: BTreeMap<(u8, u8, u8, u8), usize> = BTreeMap::new();
    for t in tracks_of(&raw_mut) {
        let h = &t.header;
        *sync_dist
            .entry((
                h.cloud_status,
                h.cloud_sync_state,
                h.has_unsynced_local_changes,
                h.playlist_only,
            ))
            .or_default() += 1;
    }
    println!(
        "  2026-11-21-mutated (cloud_status, cloud_sync_state, has_unsynced_local_changes, playlist_only):"
    );
    for ((cs, css, ulc, po), cnt) in &sync_dist {
        println!("    cloud={cs} sync_state={css} unsynced_local={ulc} pl_only={po} -> {cnt}");
    }

    // 9.2 Stylo (022A2731DE16355B) before vs after download
    println!("\n--- 9.2 Stylo (022A2731DE16355B) in baseline vs video ---");
    for (label, raw) in [("baseline", &raw_base), ("video", &raw_vid)] {
        if let Some(t) = tracks_of(raw)
            .iter()
            .find(|t| t.header.track_persistent_id == 0x022A_2731_DE16_355B)
        {
            println!(
                "  [{label}] itma: dl={} cloud={} sync_state={} media_kind=0x{:X} u53_55={:02X?} u64={} audio_traits=0x{:02X} bomas={}",
                t.header.downloaded_status,
                t.header.cloud_status,
                t.header.cloud_sync_state,
                t.header.media_kind,
                *t.header.unk_53_55,
                *t.header.unk_64_store_flag,
                t.header.audio_traits,
                t.bomas.len(),
            );
            for (idx, b) in t.bomas.iter().enumerate() {
                match &b.payload {
                    BomaPayload::TrackNumerics(n) => {
                        println!(
                            "    boma[{idx}] TrackNumerics sub={}: dur={} size={} br={} sr={} fcc={:?} fmt={} cfcc={:?} drm=(plat={},vers=0x{:08X},acct={}) dl_traits=0x{:X}",
                            n.subrecord_kind,
                            n.duration_ms,
                            n.file_size,
                            n.bit_rate,
                            n.sample_rate,
                            n.file_type_fourcc_str(),
                            n.container_format_kind,
                            n.codec_fourcc_str(),
                            n.drm_platform,
                            n.drm_version_restrictions,
                            n.drm_account_id,
                            n.downloaded_audio_traits,
                        );
                        println!(
                            "      u16={} u21_24={:02X?} is_mp3={} u41_54={:02X?} off54={} u55_59={:02X?} off59={} u216_248={}",
                            *n.unk_16,
                            *n.unk_21_24,
                            n.is_local_mp3,
                            *n.unk_41_54,
                            n.asset_container_kind,
                            *n.unk_55_59,
                            n.catalog_lyrics_state,
                            hex::encode(*n.unk_216_248),
                        );
                    }
                    BomaPayload::VideoInfo(v) => {
                        let codec_bytes = v.codec_fourcc.to_be_bytes();
                        let codec_str = String::from_utf8_lossy(&codec_bytes);
                        let u56_w0 = v
                            .unk_56_84
                            .get(..4)
                            .and_then(|s| s.try_into().ok())
                            .map_or(0, u32::from_le_bytes);
                        let rest = v.unk_56_84.get(4..).unwrap_or_default();
                        println!(
                            "    boma[{idx}] VideoInfo: w={} h={} unk_8={} codec={codec_str:?}(0x{:08X}) unk_16={} vbr={} unk_24={} unk_28={} prof={} lvl={} unk_40={} fps={} depth={} unk_52={} unk_56_w0={} (0x{u56_w0:08X}) rest={}",
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
                            u56_w0,
                            hex::encode(rest),
                        );
                    }
                    _ => {
                        if let Some(s) = b.as_str() {
                            let preview = if s.chars().count() > 80 {
                                let prefix: String = s.chars().take(80).collect();
                                format!("{prefix}... (len={})", s.len())
                            } else {
                                s.to_string()
                            };
                            println!("    boma[{idx}] 0x{:04X} str: {preview:?}", b.subtype);
                        } else {
                            println!(
                                "    boma[{idx}] 0x{:04X} (total_size={})",
                                b.subtype, b.total_size
                            );
                        }
                    }
                }
            }
        }
    }

    // 9.3 Corpus-wide VideoInfo dump + TrackNumerics[41, 46, 58, 21..24] + 4-term b45 formula
    println!(
        "\n--- 9.3 Corpus-wide VideoInfo (0x0024), TrackNumerics[41/46/58/21..24], and 4-term b45 ---"
    );
    let mut files = Vec::new();
    collect_mdb(corpus_dir, &mut files)?;
    files.sort();
    let mut seen_video_infos: BTreeMap<String, String> = BTreeMap::new();
    let mut b41_dist: BTreeMap<(u32, u8, u32, u16), usize> = BTreeMap::new();
    let mut b46_dist: BTreeMap<(u32, u8, u32, u16), usize> = BTreeMap::new();
    let mut b58_dist: BTreeMap<(u32, u8, u32, u16), usize> = BTreeMap::new();
    let mut u21_24_dist: BTreeMap<(u32, [u8; 3], u8, u8), usize> = BTreeMap::new();
    let mut b45_4term_mismatches: BTreeMap<(String, u32, u8), usize> = BTreeMap::new();
    let mut b45_4term_total = 0usize;

    for path in &files {
        let Ok(raw) = load_raw(path) else { continue };
        let rel = path
            .strip_prefix(corpus_dir)
            .unwrap_or(path)
            .display()
            .to_string();
        let is_vid_snap = rel.contains("2026-11-21-city-life-and-video/video");
        for t in tracks_of(&raw) {
            let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
            let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
            for b in &t.bomas {
                match &b.payload {
                    BomaPayload::VideoInfo(v) => {
                        let codec_bytes = v.codec_fourcc.to_be_bytes();
                        let codec_str = String::from_utf8_lossy(&codec_bytes);
                        let u56_w0 = v
                            .unk_56_84
                            .get(..4)
                            .and_then(|s| s.try_into().ok())
                            .map_or(0, u32::from_le_bytes);
                        let rest = v.unk_56_84.get(4..).unwrap_or_default();
                        let key = format!(
                            "{:016X} {} - {:?} ({}x{} {codec_str})",
                            t.header.track_persistent_id, artist, title, v.width, v.height
                        );
                        seen_video_infos.entry(key).or_insert_with(|| {
                            format!(
                                "file={rel} | u8={} u16={} vbr={} u24={} u28={} prof={} lvl={} u40={} fps={} depth={} u52={} u56w0={} rest={}",
                                *v.unk_8,
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
                                u56_w0,
                                hex::encode(rest),
                            )
                        });
                    }
                    BomaPayload::TrackNumerics(n) => {
                        let b41 = n.unk_41_54.first().copied().unwrap_or(0);
                        let b45 = n.unk_41_54.get(4).copied().unwrap_or(0);
                        let b46 = n.unk_41_54.get(5).copied().unwrap_or(0);
                        let b58 = n.unk_55_59.get(3).copied().unwrap_or(0);
                        b45_4term_total += 1;
                        let expected_4term = n.subrecord_kind == 3
                            || n.date_released.raw() != 0
                            || *n.unk_116_cloud_timestamp != 0
                            || n.store_track_adam_id != 0;
                        if (b45 == 1) != expected_4term {
                            *b45_4term_mismatches
                                .entry((rel.clone(), n.subrecord_kind, b45))
                                .or_default() += 1;
                        }
                        if is_vid_snap {
                            *b41_dist
                                .entry((
                                    n.subrecord_kind,
                                    b41,
                                    t.header.media_kind,
                                    n.container_format_kind,
                                ))
                                .or_default() += 1;
                            *b46_dist
                                .entry((
                                    n.subrecord_kind,
                                    b46,
                                    t.header.media_kind,
                                    n.container_format_kind,
                                ))
                                .or_default() += 1;
                            *b58_dist
                                .entry((
                                    n.subrecord_kind,
                                    b58,
                                    t.header.media_kind,
                                    n.container_format_kind,
                                ))
                                .or_default() += 1;
                            *u21_24_dist
                                .entry((
                                    n.subrecord_kind,
                                    *n.unk_21_24,
                                    t.header.downloaded_status,
                                    t.header.cloud_status,
                                ))
                                .or_default() += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    println!(
        "  Distinct VideoInfo (0x0024) records across ALL corpus files ({}):",
        seen_video_infos.len()
    );
    for (k, v) in &seen_video_infos {
        println!("    {k}\n      {v}");
    }
    println!("  2026-11-21-video TrackNumerics[41] (sub, b41, media_kind, container_fmt):");
    for ((sub, b41, mk, fmt), cnt) in &b41_dist {
        if *b41 != 0 || *mk != 1 {
            println!("    sub={sub} b41={b41} media_kind=0x{mk:X} fmt={fmt} -> {cnt}");
        }
    }
    println!("  2026-11-21-video TrackNumerics[46] (sub, b46, media_kind, container_fmt):");
    for ((sub, b46, mk, fmt), cnt) in &b46_dist {
        if *b46 != 0 || *mk != 1 {
            println!("    sub={sub} b46={b46} media_kind=0x{mk:X} fmt={fmt} -> {cnt}");
        }
    }
    println!("  2026-11-21-video TrackNumerics[58] (sub, b58, media_kind, container_fmt):");
    for ((sub, b58, mk, fmt), cnt) in &b58_dist {
        if *b58 != 0 || *mk != 1 {
            println!("    sub={sub} b58={b58} media_kind=0x{mk:X} fmt={fmt} -> {cnt}");
        }
    }
    println!("  2026-11-21-video TrackNumerics[21..24] (sub, u21_24, dl_status, cloud_status):");
    for ((sub, u, dl, cs), cnt) in &u21_24_dist {
        println!("    sub={sub} u21_24={u:02X?} dl={dl} cloud={cs} -> {cnt}");
    }
    println!("  Corpus-wide 4-term b45 mismatches (out of {b45_4term_total}):");
    for ((rel, sub, b45), cnt) in &b45_4term_mismatches {
        println!("    {rel} sub={sub} b45={b45} -> {cnt}");
    }

    // 9.4 boma 0x0034 (itunes_store_flavor) vs TrackNumerics[54]
    println!("\n--- 9.4 boma 0x0034 (itunes_store_flavor) vs TrackNumerics[54] ---");
    let mut flvr_dist: BTreeMap<(Option<String>, u8, u8, u8), usize> = BTreeMap::new();
    for t in tracks_of(&raw_vid) {
        let flvr = boma_str(&t.bomas, 0x0034).map(str::to_string);
        let s1_54 = t
            .bomas
            .iter()
            .find_map(|b| match &b.payload {
                BomaPayload::TrackNumerics(n) if n.subrecord_kind == 1 => {
                    Some(n.asset_container_kind)
                }
                _ => None,
            })
            .unwrap_or(255);
        let s3_54 = t
            .bomas
            .iter()
            .find_map(|b| match &b.payload {
                BomaPayload::TrackNumerics(n) if n.subrecord_kind == 3 => {
                    Some(n.asset_container_kind)
                }
                _ => None,
            })
            .unwrap_or(255);
        if flvr.is_some()
            || (s3_54 != 1 && s3_54 != 2 && s3_54 != 255)
            || (s1_54 != 0 && s1_54 != 255)
        {
            *flvr_dist
                .entry((flvr, t.header.cloud_status, s1_54, s3_54))
                .or_default() += 1;
        }
    }
    for ((flvr, cs, s1, s3), cnt) in &flvr_dist {
        println!("  flvr={flvr:?} cloud={cs} sub1.off54={s1} sub3.off54={s3} -> {cnt}");
    }

    // 9.5 The 12 tracks in 2025 where 4-term b45 mismatched
    let mdb_2025 =
        corpus_dir.join("library-data/2025/Apple Music Library.musiclibrary/Library.musicdb");
    if mdb_2025.exists() {
        let raw_2025 = load_raw(&mdb_2025)?;
        println!("\n--- 9.5 The 12 4-term b45 mismatches in 2025 ---");
        for t in tracks_of(&raw_2025) {
            let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
            let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    let b45 = n.unk_41_54.get(4).copied().unwrap_or(0);
                    let expected = n.subrecord_kind == 3
                        || n.date_released.raw() != 0
                        || *n.unk_116_cloud_timestamp != 0
                        || n.store_track_adam_id != 0;
                    if (b45 == 1) != expected {
                        println!(
                            "  [{:016X}] {artist} - {title:?} | sub={} b45={b45} cloud={} drel={} dpurch={} adam={} album_id={} artist_id={} sf={}",
                            t.header.track_persistent_id,
                            n.subrecord_kind,
                            t.header.cloud_status,
                            n.date_released.raw(),
                            n.date_purchased.raw(),
                            n.store_track_adam_id,
                            n.apple_music_store_album_id,
                            n.apple_music_store_artist_id,
                            n.storefront_id,
                        );
                    }
                }
            }
        }
    }

    Ok(())
}
