//! Ad-hoc probe for the 5 specific verification checks:
//! 1. Lyrics (`TrackNumerics[59]` & `boma 0x0036` plist keys)
//! 2. Matched/Uploaded codec (`TrackNumerics[54]`, `cloud_codec_kind`, `codec_fourcc` vs local `sub=1` file format)
//! 3. `TrackNumerics[116..120]` (`gapless_heuristic_info`) value breakdown across all 45,242 subrecords + song examples
//! 4. Artwork cache state (`0`, `3`, `5`, `8`, `12` in `itma[328]`, `iAma[100]`, `lpma[279]`, `isma[52]`) vs `artwork.sqlite`
//! 5. `plma` raw 194-byte layout across `2026-11-19`, `Monterey`, `Sequoia`, and `Catalina`

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;

use anyhow::Result;

use musicdb::sections::{
    AlbumItem, AppleTimestamp, ArtistItem, BomaPayload, BomaSection, HsmaChild,
    LibraryMasterHeader, PlaylistItem, RawLibraryFile, SocialProfileItem, TrackItem,
};

pub(crate) fn run_unknowns_probe(corpus_dir: &Path) -> Result<()> {
    let mdb_1119 =
        corpus_dir.join("snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb");
    let raw_bytes =
        musicdb::decrypt_and_decompress_bytes(&fs::read(&mdb_1119)?, musicdb::DEFAULT_MUSICDB_KEY)?;
    let raw = RawLibraryFile::from_raw_bytes(&raw_bytes)?;

    println!("=== 1. LYRICS: TrackNumerics[59] vs boma 0x0036 ON TARGET TRACKS & FULL LIBRARY ===");
    check_lyrics(&raw);

    println!(
        "\n=== 2. CODEC: subrecord_kind=1 (local file) vs subrecord_kind=3 (off54 / cloud_codec_kind / codec_fourcc) ==="
    );
    check_codecs(&raw);

    println!(
        "\n=== 3. TrackNumerics[116..120] (gapless_heuristic_info) BREAKDOWN & SONG EXAMPLES ==="
    );
    check_offset_116(&raw);

    println!(
        "\n=== 4. ARTWORK STATE BYTE (0, 3, 5, 8, 12) ACROSS itma[328], iAma[100], lpma[279], isma[52] vs artwork.sqlite ==="
    );
    let art_db =
        corpus_dir.join("snapshots/2026-11-19/Apple Music Library.musiclibrary/artwork.sqlite");
    check_artwork_states(&raw, &art_db)?;

    println!("\n=== 5. plma (LibraryMasterHeader) RAW 194-BYTE HEX DUMP & ALIGNMENT ===");
    check_plma_raw(&raw_bytes, corpus_dir)?;

    println!(
        "\n=== 6. TrackNumerics[116..120] ACROSS ALL 43 CORPUS FILES & COMPARISON WITH date_released/date_added ==="
    );
    check_116_across_all_files(&raw, corpus_dir)?;

    println!("\n=== 7. HISTORY.MD SECTION 5 SHAKY FIELDS & BITFIELD VERIFICATION ===");
    check_history_shaky_fields(&raw, corpus_dir)?;

    println!("\n=== 8. FFPROBE / EXIFTOOL / MOVPKG DISCOVERY VERIFICATION ===");
    check_probe_verifications(&raw, corpus_dir)?;

    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc lyrics diagnostic cross-tabulation"
)]
fn check_lyrics(raw: &RawLibraryFile) {
    let targets = [
        "Orion",
        "The Call of Ktulu",
        "(Anesthesia)—Pulling Teeth",
        "To Live Is to Die",
        "To Live is to Die (Remastered)",
        "One (Remastered)",
        "The Ides of March",
        "Losfer Words (Big 'Orra)",
        "Transylvania",
        "Genghis Khan",
        "Red Barchetta",
        "Tom Sawyer",
        "YYZ",
    ];

    for t in tracks_of(raw) {
        let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
        let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
        let album = boma_str(&t.bomas, 0x0003).unwrap_or("");
        if targets.contains(&title)
            && (artist.contains("Metallica")
                || artist.contains("Iron Maiden")
                || artist.contains("Rush"))
        {
            let mut sub_str = Vec::new();
            let mut b36_desc = String::from("none");
            for b in &t.bomas {
                match &b.payload {
                    BomaPayload::TrackNumerics(n) => {
                        sub_str.push(format!(
                            "sub{}:off59={}",
                            n.subrecord_kind, n.catalog_lyrics_state
                        ));
                    }
                    BomaPayload::RawStringUtf8 { raw_bytes, text } if b.subtype == 0x0036 => {
                        if let Ok(val) = plist::Value::from_reader(Cursor::new(raw_bytes)) {
                            b36_desc = format!("{val:?}");
                        } else {
                            b36_desc = format!("utf8:{text:?}");
                        }
                    }
                    _ => {}
                }
            }
            println!(
                "  [{:016X}] {} - {:?} (album={:?}, cloud={}, playlist_only={}, itma.has_custom_lyrics={}, {:?})\n    0x0036: {}",
                t.header.track_persistent_id,
                artist,
                title,
                album,
                t.header.cloud_status,
                t.header.playlist_only,
                t.header.has_custom_lyrics,
                sub_str,
                b36_desc
            );
        }
    }

    let mut dist: BTreeMap<(u8, Option<u8>, u8, String), usize> = BTreeMap::new();
    for t in tracks_of(raw) {
        let mut off59_3 = None;
        let mut b36_keys = String::from("no_0x0036");
        for b in &t.bomas {
            if let BomaPayload::TrackNumerics(n) = &b.payload
                && n.subrecord_kind == 3
            {
                off59_3 = Some(n.catalog_lyrics_state);
            } else if b.subtype == 0x0036
                && let BomaPayload::RawStringUtf8 { raw_bytes, .. } = &b.payload
            {
                if let Ok(plist::Value::Dictionary(dict)) =
                    plist::Value::from_reader(Cursor::new(raw_bytes))
                {
                    let mut parts = Vec::new();
                    for (k, v) in &dict {
                        match v {
                            plist::Value::Boolean(b) => parts.push(format!("{k}={b}")),
                            plist::Value::Integer(i) => parts.push(format!("{k}={i:?}")),
                            plist::Value::String(_) => parts.push(format!("{k}=<str>")),
                            _ => parts.push(format!("{k}=<?>")),
                        }
                    }
                    b36_keys = parts.join(",");
                } else {
                    b36_keys = String::from("non_plist");
                }
            }
        }
        *dist
            .entry((
                t.header.cloud_status,
                off59_3,
                t.header.has_custom_lyrics,
                b36_keys,
            ))
            .or_default() += 1;
    }
    println!(
        "  Cross-tab across all 33,961 tracks (cloud_status, sub3.off59, itma.has_custom_lyrics, 0x0036_plist):"
    );
    for ((cs, o59, hcl, k36), count) in &dist {
        println!(
            "    cloud={cs} sub3.off59={o59:?} itma.has_custom_lyrics={hcl} 0x0036=[{k36}] -> {count}"
        );
    }
}

fn check_codecs(raw: &RawLibraryFile) {
    type CodecTuple = (u8, Option<String>, u16, u32, Option<String>);
    type CodecGroupKey = (u8, u8, String, Option<CodecTuple>, Option<CodecTuple>);
    let mut table: BTreeMap<CodecGroupKey, (usize, String)> = BTreeMap::new();

    for t in tracks_of(raw) {
        let title = boma_str(&t.bomas, 0x0002).unwrap_or("?");
        let artist = boma_str(&t.bomas, 0x0004).unwrap_or("?");
        let path_str = boma_str(&t.bomas, 0x000B).unwrap_or("");
        let ext = Path::new(path_str)
            .extension()
            .and_then(|e| e.to_str())
            .map_or_else(|| String::from("none"), str::to_ascii_lowercase);

        let mut s1 = None;
        let mut s3 = None;
        for b in &t.bomas {
            if let BomaPayload::TrackNumerics(n) = &b.payload {
                let tup = (
                    n.asset_container_kind,
                    n.file_type_fourcc_str(),
                    n.container_format_kind,
                    n.cloud_codec_kind,
                    n.codec_fourcc_str(),
                );
                if n.subrecord_kind == 1 {
                    s1 = Some(tup);
                } else if n.subrecord_kind == 3 {
                    s3 = Some(tup);
                }
            }
        }
        let key = (
            t.header.cloud_status,
            t.header.downloaded_status,
            ext,
            s1,
            s3,
        );
        let entry = table.entry(key).or_insert_with(|| {
            (
                0,
                format!(
                    "[{:016X}] {} - {:?}",
                    t.header.track_persistent_id, artist, title
                ),
            )
        });
        entry.0 += 1;
    }

    for ((cs, dl, ext, s1, s3), (count, ex)) in &table {
        println!(
            "  count={count:5} | cloud={cs} dl={dl} ext={ext:6} | sub1={s1:?} | sub3={s3:?}\n    ex: {ex}"
        );
    }
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
                let off45 = n.unk_41_54.get(4).copied().unwrap_or(0);
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

#[cfg(feature = "sqlite")]
#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc artwork.sqlite cross-tabulation across 4 sections"
)]
fn check_artwork_states(raw: &RawLibraryFile, art_db: &Path) -> Result<()> {
    type SqlArtTuple = (i64, i64, String, i64, i64);
    type ArtGroupKey<'a> = (&'a str, u8, u32, u32, u32, bool, Option<SqlArtTuple>);
    let conn = rusqlite::Connection::open(art_db)?;
    {
        let mut s = conn.prepare("SELECT name, sql FROM sqlite_master WHERE type='table'")?;
        let mut r = s.query([])?;
        while let Some(row) = r.next()? {
            let name: String = row.get(0)?;
            let sql: String = row.get(1)?;
            println!("  sqlite table {name}: {sql}");
        }
    }
    let mut stmt = conn.prepare(
        "SELECT i.item_id, i.source_kind, COALESCE(s.location_type, -1), COALESCE(c.extension, 'none'), COALESCE(c.size_kind, -1), COALESCE(c.status, -1)
         FROM item_to_artwork i
         LEFT JOIN artwork_source s ON i.artwork_id = s.artwork_id
         LEFT JOIN cache_items c ON i.artwork_id = c.artwork_id",
    )?;
    let mut item_map: BTreeMap<u64, SqlArtTuple> = BTreeMap::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let raw_item_id: i64 = row.get(0)?;
        let persistent_id = raw_item_id.cast_unsigned();
        item_map.insert(
            persistent_id,
            (
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ),
        );
    }

    let mut groups: BTreeMap<ArtGroupKey<'_>, usize> = BTreeMap::new();

    for t in tracks_of(raw) {
        let h = &t.header;
        let has_uuid = h.artwork_uuid != [0; 16];
        let sq = item_map.get(&h.track_persistent_id).cloned();
        *groups
            .entry((
                "itma[328]",
                h.artwork_cache_state,
                h.artwork_source_kind,
                h.artwork_size_kind,
                h.artwork_image_format,
                has_uuid,
                sq,
            ))
            .or_default() += 1;
    }
    for a in artists_of(raw) {
        let h = &a.header;
        let has_uuid = h.artwork_uuid != [0; 16];
        let sq = item_map.get(&h.artist_persistent_id).cloned();
        *groups
            .entry((
                "iAma[100]",
                h.artwork_cache_state,
                h.artwork_source_kind,
                h.artwork_size_kind,
                h.artwork_image_format,
                has_uuid,
                sq,
            ))
            .or_default() += 1;
    }
    for p in playlists_of(raw) {
        let h = &p.header;
        let has_uuid = h.artwork_uuid != [0; 16];
        let sq = item_map.get(&h.playlist_persistent_id).cloned();
        *groups
            .entry((
                "lpma[279]",
                *h.unk_279,
                h.artwork_source_kind,
                h.artwork_size_kind,
                h.artwork_image_format,
                has_uuid,
                sq,
            ))
            .or_default() += 1;
    }
    for s in socials_of(raw) {
        let h = &s.header;
        let has_uuid = h.artwork_uuid != [0; 16];
        let sq = item_map.get(&h.profile_persistent_id).cloned();
        let b52 = h.artwork_cache_state;
        *groups
            .entry((
                "isma[52] ",
                b52,
                h.artwork_source_kind,
                h.artwork_size_kind,
                h.artwork_image_format,
                has_uuid,
                sq,
            ))
            .or_default() += 1;
    }

    for ((sec, st, src, sz, fmt, has_u, sq), count) in &groups {
        println!(
            "  {sec} state={st:2} (0b{st:04b}) | mdb:(src={src:3}, sz={sz}, fmt={fmt}, uuid={has_u:5}) | sqlite(src,loc_type,fmt,sz,status)={sq:?} -> {count}"
        );
    }
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
fn check_artwork_states(_raw: &RawLibraryFile, _art_db: &Path) -> Result<()> {
    Ok(())
}

fn check_plma_raw(_raw_1119: &[u8], corpus_dir: &Path) -> Result<()> {
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
        let Ok(disk) = fs::read(&p) else {
            continue;
        };
        let bytes = musicdb::decrypt_and_decompress_bytes(&disk, musicdb::DEFAULT_MUSICDB_KEY)?;
        let raw = RawLibraryFile::from_raw_bytes(&bytes)?;
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

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc corpus-wide TrackNumerics[116..120] scan"
)]
fn check_116_across_all_files(raw_1119: &RawLibraryFile, corpus_dir: &Path) -> Result<()> {
    // 1. Compare v116 with date_released, date_added, date_modified, date_cloud_synced in 2026-11-19
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

    // 2. Check all 43 files for any non-timestamp values at 116..120
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

fn collect_mdb(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            collect_mdb(&p, out)?;
        } else if p.extension().and_then(|e| e.to_str()) == Some("musicdb") {
            out.push(p);
        }
    }
    Ok(())
}

fn plma_of(raw: &RawLibraryFile) -> Option<&LibraryMasterHeader> {
    raw.sections.iter().find_map(|s| match &s.child {
        HsmaChild::LibraryMaster(p) => Some(&p.header),
        _ => None,
    })
}

fn albums_of(raw: &RawLibraryFile) -> &[AlbumItem] {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::AlbumList(a) => Some(a.albums.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

fn artists_of(raw: &RawLibraryFile) -> &[ArtistItem] {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::ArtistList(a) => Some(a.artists.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

fn tracks_of(raw: &RawLibraryFile) -> &[TrackItem] {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::TrackList(t) => Some(t.tracks.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

fn playlists_of(raw: &RawLibraryFile) -> &[PlaylistItem] {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::PlaylistList(p) => Some(p.playlists.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

fn socials_of(raw: &RawLibraryFile) -> &[SocialProfileItem] {
    raw.sections
        .iter()
        .find_map(|s| match &s.child {
            HsmaChild::SocialProfileList(p) => Some(p.profiles.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

fn boma_str(bomas: &[BomaSection], subtype: u32) -> Option<&str> {
    bomas
        .iter()
        .find(|b| b.subtype == subtype)
        .and_then(BomaSection::as_str)
}

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc verification suite for history.md"
)]
fn check_history_shaky_fields(raw_1119: &RawLibraryFile, corpus_dir: &Path) -> Result<()> {
    type WorkMovKey = (u8, u8, u8, bool, bool, bool, bool);
    type NonDlCodecKey = (u8, u8, u8, String, u16, u32, String, String);
    // 7.0 Bitfield analysis of artwork_cache_state (0, 3, 5, 7, 8, 12), artwork_source_kind, artwork_template_kind, and artwork.sqlite columns
    println!(
        "\n--- 7.0 BITFIELD ANALYSIS: artwork_cache_state, artwork_source_kind, artwork_template_kind vs artwork.sqlite ---"
    );
    #[cfg(feature = "sqlite")]
    {
        type FullArtRow = (i64, i64, i64, i64, i64, String, i64);
        let art_db =
            corpus_dir.join("snapshots/2026-11-19/Apple Music Library.musiclibrary/artwork.sqlite");
        if art_db.exists() {
            let conn = rusqlite::Connection::open(&art_db)?;
            let mut stmt = conn.prepare(
                "SELECT i.item_id, i.source_kind, i.remote_id, s.aspect, s.location_type, c.size_kind, c.extension, c.status \
                 FROM item_to_artwork i \
                 LEFT JOIN artwork_source s ON s.artwork_id = i.artwork_id \
                 LEFT JOIN cache_items c ON c.artwork_id = i.artwork_id",
            )?;
            let mut sql_map: BTreeMap<u64, FullArtRow> = BTreeMap::new();
            let rows = stmt.query_map([], |r| {
                let item_id: i64 = r.get(0)?;
                Ok((
                    item_id.cast_unsigned(),
                    (
                        r.get::<_, i64>(1).unwrap_or(-1),
                        r.get::<_, i64>(2).unwrap_or(-1),
                        r.get::<_, i64>(3).unwrap_or(-1),
                        r.get::<_, i64>(4).unwrap_or(-1),
                        r.get::<_, i64>(5).unwrap_or(-1),
                        r.get::<_, String>(6).unwrap_or_default(),
                        r.get::<_, i64>(7).unwrap_or(-1),
                    ),
                ))
            })?;
            for r in rows.flatten() {
                sql_map.insert(r.0, r.1);
            }

            // Cross-tabulate (section, cache_state_val, format_val, size_kind_val, source_kind_val) vs sqlite (source_kind, remote_id!=0, location_type, cache_status)
            let mut bit_dist: BTreeMap<(String, u32, String), usize> = BTreeMap::new();
            for t in tracks_of(raw_1119) {
                let st = u32::from(t.header.artwork_cache_state);
                let sql_desc = if let Some((sk, rem, asp, lt, csk, ext, cst)) =
                    sql_map.get(&t.header.track_persistent_id)
                {
                    format!(
                        "sql(sk={sk}[0b{sk:04b}], rem!=0:{}, asp={asp}, loc_type={lt}, c_sk={csk}, ext={ext}, c_status={cst}[0b{cst:04b}])",
                        *rem != 0
                    )
                } else {
                    String::from("sql(none)")
                };
                *bit_dist
                    .entry((
                        format!(
                            "itma(src={}, sz={}, fmt={})",
                            t.header.artwork_source_kind,
                            t.header.artwork_size_kind,
                            t.header.artwork_image_format
                        ),
                        st,
                        sql_desc,
                    ))
                    .or_insert(0) += 1;
            }
            for a in artists_of(raw_1119) {
                let st = u32::from(a.header.artwork_cache_state);
                let sql_desc = if let Some((sk, rem, asp, lt, csk, ext, cst)) =
                    sql_map.get(&a.header.artist_persistent_id)
                {
                    format!(
                        "sql(sk={sk}[0b{sk:04b}], rem!=0:{}, asp={asp}, loc_type={lt}, c_sk={csk}, ext={ext}, c_status={cst}[0b{cst:04b}])",
                        *rem != 0
                    )
                } else {
                    String::from("sql(none)")
                };
                *bit_dist
                    .entry((
                        format!(
                            "iAma(src={}, sz={}, fmt={})",
                            a.header.artwork_source_kind,
                            a.header.artwork_size_kind,
                            a.header.artwork_image_format
                        ),
                        st,
                        sql_desc,
                    ))
                    .or_insert(0) += 1;
            }
            for p in playlists_of(raw_1119) {
                let st = p.header.artwork_cache_state;
                let sql_desc = if let Some((sk, rem, asp, lt, csk, ext, cst)) =
                    sql_map.get(&p.header.playlist_persistent_id)
                {
                    format!(
                        "sql(sk={sk}[0b{sk:04b}], rem!=0:{}, asp={asp}, loc_type={lt}, c_sk={csk}, ext={ext}, c_status={cst}[0b{cst:04b}])",
                        *rem != 0
                    )
                } else {
                    String::from("sql(none)")
                };
                *bit_dist
                    .entry((
                        format!(
                            "lpma(src={}, sz={}, fmt={}, unk192={})",
                            p.header.artwork_source_kind,
                            p.header.artwork_size_kind,
                            p.header.artwork_image_format,
                            *p.header.unk_192
                        ),
                        st,
                        sql_desc,
                    ))
                    .or_insert(0) += 1;
            }
            for s in socials_of(raw_1119) {
                let st = u32::from(s.header.artwork_cache_state);
                let sql_desc = if let Some((sk, rem, asp, lt, csk, ext, cst)) =
                    sql_map.get(&s.header.profile_persistent_id)
                {
                    format!(
                        "sql(sk={sk}[0b{sk:04b}], rem!=0:{}, asp={asp}, loc_type={lt}, c_sk={csk}, ext={ext}, c_status={cst}[0b{cst:04b}])",
                        *rem != 0
                    )
                } else {
                    String::from("sql(none)")
                };
                *bit_dist
                    .entry((
                        format!(
                            "isma(src={}, sz={}, fmt={})",
                            s.header.artwork_source_kind,
                            s.header.artwork_size_kind,
                            s.header.artwork_image_format
                        ),
                        st,
                        sql_desc,
                    ))
                    .or_insert(0) += 1;
            }
            for ((sec, st, sql_desc), cnt) in &bit_dist {
                println!("  {sec} | state={st} (0b{st:04b}) | {sql_desc} => {cnt}");
            }
        }
    }

    // 7.1 Check 1: TrackNumerics::gapless_heuristic_info (116..120) on the 177 catalog tracks + iTunes XML GaplessHeuristicInfo
    println!("\n--- 7.1 TrackNumerics[116..120] ON 177 CATALOG TRACKS & DIFF ACROSS SNAPSHOTS ---");
    let mut cat_116_dist: BTreeMap<(u8, u8, bool, bool), usize> = BTreeMap::new();
    let mut cat_116_samples = Vec::new();
    for t in tracks_of(raw_1119) {
        if t.header.cloud_status == 8 {
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    let v = *n.unk_116_cloud_timestamp;
                    let has_116 = v != 0;
                    let eq_352 = v != 0 && v == t.header.date_cloud_synced.raw();
                    *cat_116_dist
                        .entry((
                            t.header.playlist_only,
                            t.header.downloaded_status,
                            has_116,
                            eq_352,
                        ))
                        .or_insert(0) += 1;
                    if has_116 && cat_116_samples.len() < 8 {
                        cat_116_samples.push(format!(
                            "[{:016X}] {} - {:?} (pl_only={}, dl={}, sub={}, 116={:?}, added={:?}, mod={:?}, sync={:?}, itma352={:?})",
                            t.header.track_persistent_id,
                            boma_str(&t.bomas, 0x0004).unwrap_or(""),
                            boma_str(&t.bomas, 0x0002).unwrap_or(""),
                            t.header.playlist_only,
                            t.header.downloaded_status,
                            n.subrecord_kind,
                            AppleTimestamp::from(v).to_utc(),
                            n.date_added.to_utc(),
                            n.date_modified.to_utc(),
                            n.date_cloud_synced.to_utc(),
                            t.header.date_cloud_synced.to_utc(),
                        ));
                    }
                }
            }
        }
    }
    for ((pl_only, dl, has_116, eq_352), cnt) in &cat_116_dist {
        println!(
            "  cloud=8: playlist_only={pl_only}, downloaded={dl}, has_116={has_116}, eq_itma352={eq_352} => {cnt} subrecords"
        );
    }
    for s in &cat_116_samples {
        println!("    sample: {s}");
    }

    // Also check iTunes Music Library.xml for GaplessHeuristicInfo
    let xml_path = corpus_dir.join("library-data/2026-11-01/iTunes Music Library.xml");
    if xml_path.exists() {
        let xml_text = fs::read_to_string(&xml_path)?;
        let mut gh_vals: BTreeMap<String, usize> = BTreeMap::new();
        for line in xml_text.lines() {
            if let Some(pos) = line.find("<key>GaplessHeuristicInfo</key><integer>") {
                let rest = &line[pos + "<key>GaplessHeuristicInfo</key><integer>".len()..];
                if let Some(end) = rest.find("</integer>") {
                    *gh_vals.entry(rest[..end].to_string()).or_insert(0) += 1;
                }
            }
        }
        println!("  iTunes Music Library.xml <key>GaplessHeuristicInfo</key> values: {gh_vals:?}");
    }

    // 7.2 Check 2: TrackItemHeader::date_cloud_synced (itma[352..356])
    println!("\n--- 7.2 itma[352..356] (date_cloud_synced) DISTRIBUTION & CORRELATION ---");
    let mut dist_352: BTreeMap<(u8, u8, bool), usize> = BTreeMap::new();
    let mut match_352_vs_other: BTreeMap<&str, usize> = BTreeMap::new();
    for t in tracks_of(raw_1119) {
        let v352 = t.header.date_cloud_synced.raw();
        *dist_352
            .entry((t.header.cloud_status, t.header.playlist_only, v352 != 0))
            .or_insert(0) += 1;
        if v352 != 0 {
            let mut matched = "other";
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    if v352 == n.date_added.raw() {
                        matched = "eq_date_added";
                        break;
                    } else if v352 == n.date_cloud_synced.raw() {
                        matched = "eq_date_cloud_synced";
                        break;
                    } else if v352 == *n.unk_116_cloud_timestamp {
                        matched = "eq_num_116";
                        break;
                    } else if v352 == n.date_modified.raw() {
                        matched = "eq_date_modified";
                        break;
                    } else if v352.abs_diff(n.date_added.raw()) <= 60 {
                        matched = "within_60s_of_date_added";
                    } else if v352.abs_diff(n.date_cloud_synced.raw()) <= 60 {
                        matched = "within_60s_of_date_cloud_synced";
                    } else if v352.abs_diff(*n.unk_116_cloud_timestamp) <= 60 {
                        matched = "within_60s_of_num_116";
                    }
                }
            }
            *match_352_vs_other.entry(matched).or_insert(0) += 1;
        }
    }
    for ((cs, pl, nz), cnt) in &dist_352 {
        println!("  itma[352..356]: cloud_status={cs}, playlist_only={pl}, non_zero={nz} => {cnt}");
    }
    println!("  itma[352..356] non-zero correlations: {match_352_vs_other:?}");

    // 7.3 Check 3: itma[51], itma[52], itma[53] (show_composer, use_work_and_movement, unk_53_55[0])
    println!("\n--- 7.3 itma[51], itma[52], itma[53] ACROSS 2026-11-19 ---");
    let mut wm_dist: BTreeMap<WorkMovKey, usize> = BTreeMap::new();
    for t in tracks_of(raw_1119) {
        let has_work = boma_str(&t.bomas, 0x003F).is_some();
        let has_mov_name = boma_str(&t.bomas, 0x0040).is_some();
        let has_mov_num = t.header.movement_number != 0 || t.header.movement_total != 0;
        let has_composer = boma_str(&t.bomas, 0x000C).is_some();
        let k = (
            t.header.checkbox_show_composer_in_all_views,
            t.header.checkbox_use_work_and_movement,
            t.header.unk_53_55.first().copied().unwrap_or(0),
            has_work,
            has_mov_name,
            has_mov_num,
            has_composer,
        );
        *wm_dist.entry(k).or_insert(0) += 1;
    }
    for (k, cnt) in &wm_dist {
        println!(
            "  (off51={}, off52={}, off53={}, has_work={}, has_mov_name={}, has_mov_num={}, has_composer_0x0C={}) => {cnt}",
            k.0, k.1, k.2, k.3, k.4, k.5, k.6
        );
    }

    // 7.4 Check 4: itma[66..72] (auto_sort_flags) per-byte breakdown vs sort bomas & cloud_status
    println!("\n--- 7.4 itma[66..72] (auto_sort_flags) PER-BYTE BREAKDOWN ---");
    // Primary and sort bomas for the 6 candidate slots:
    // Slot 0 (off 66): Title (0x02) -> Sort Title (0x1E)
    // Slot 1 (off 67): Album (0x03) -> Sort Album (0x1F)
    // Slot 2 (off 68): Artist (0x04) -> Sort Artist (0x20)
    // Slot 3 (off 69): Album Artist (0x1B) -> Sort Album Artist (0x21)
    // Slot 4 (off 70): Composer (0x0C) -> Sort Composer (0x22)
    // Slot 5 (off 71): Show/Series (0x18 / 0x12F) -> Sort Show (0x53)?
    let slot_bomas: [(usize, &str, u32, u32); 6] = [
        (0, "off66(Title?)", 0x0002, 0x001E),
        (1, "off67(Album?)", 0x0003, 0x001F),
        (2, "off68(Artist?)", 0x0004, 0x0020),
        (3, "off69(AlbumArtist?)", 0x001B, 0x0021),
        (4, "off70(Composer?)", 0x000C, 0x0022),
        (5, "off71(Show?)", 0x0018, 0x0053),
    ];
    for (idx, label, pri_sub, sort_sub) in slot_bomas {
        let mut slot_dist: BTreeMap<(u8, u8, bool, bool), usize> = BTreeMap::new();
        for t in tracks_of(raw_1119) {
            let val = t.header.auto_sort_flags.get(idx).copied().unwrap_or(0);
            let pri = boma_str(&t.bomas, pri_sub).unwrap_or("");
            let has_sort = boma_str(&t.bomas, sort_sub).is_some();
            let starts_article = pri.starts_with("The ")
                || pri.starts_with("the ")
                || pri.starts_with("A ")
                || pri.starts_with("An ");
            *slot_dist
                .entry((t.header.cloud_status, val, has_sort, starts_article))
                .or_insert(0) += 1;
        }
        println!("  Slot {idx} {label}:");
        for ((cs, val, has_sort, art), cnt) in &slot_dist {
            println!(
                "    cloud={cs}, val=0x{val:02X} (0b{val:08b}), has_sort_boma={has_sort}, starts_article={art} => {cnt}"
            );
        }
    }

    // 7.5 Check 5: Non-downloaded Matched (cloud=2, dl=3) and Uploaded (cloud=3, dl=3) subrecord_kind=3 codec
    println!(
        "\n--- 7.5 NON-DOWNLOADED MATCHED (cloud=2, dl=3) & UPLOADED (cloud=3, dl=3) CODECS ---"
    );
    let mut nondl_dist: BTreeMap<NonDlCodecKey, usize> = BTreeMap::new();
    for t in tracks_of(raw_1119) {
        if (t.header.cloud_status == 2 || t.header.cloud_status == 3)
            && t.header.downloaded_status != 1
        {
            let kind_str = boma_str(&t.bomas, 0x0006).unwrap_or("").to_string();
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    *nondl_dist
                        .entry((
                            t.header.cloud_status,
                            t.header.downloaded_status,
                            n.asset_container_kind,
                            n.file_type_fourcc_str().unwrap_or_default(),
                            n.container_format_kind,
                            n.cloud_codec_kind,
                            n.codec_fourcc_str().unwrap_or_default(),
                            kind_str.clone(),
                        ))
                        .or_insert(0) += 1;
                }
            }
        }
    }
    for ((cs, dl, o54, f4, fmt, cck, c4, kstr), cnt) in &nondl_dist {
        println!(
            "  cloud={cs} dl={dl}: off54={o54}, file_4cc={f4:?}, fmt={fmt}, cloud_codec={cck}, codec_4cc={c4:?}, kind={kstr:?} => {cnt}"
        );
    }

    // 7.6 Check 6: PlaylistItemHeader play_count / true_play_count / date_last_played (174..186) & ipfa.item_persistent_id_2 (44..52)
    println!(
        "\n--- 7.6 PLAYLIST PLAY COUNTS (lpma[174..186]) & ipfa[44..52] (item_persistent_id_2) ---"
    );
    let mut track_plays_by_id: BTreeMap<u64, u32> = BTreeMap::new();
    for t in tracks_of(raw_1119) {
        for b in &t.bomas {
            if let BomaPayload::TrackPlaysSkips(ps) = &b.payload {
                track_plays_by_id.insert(t.header.track_persistent_id, ps.play_count);
            }
        }
    }
    let mut nz_lpma_plays = 0usize;
    for p in playlists_of(raw_1119) {
        if p.header.play_count != 0
            || p.header.true_play_count != 0
            || p.header.date_last_played.raw() != 0
        {
            nz_lpma_plays += 1;
            let name = boma_str(&p.bomas, 0x00C8).unwrap_or("<unnamed>");
            let mut sum_track_plays = 0u64;
            for b in &p.bomas {
                if let BomaPayload::PlaylistItem(ipfa) = &b.payload {
                    sum_track_plays += u64::from(
                        *track_plays_by_id
                            .get(&ipfa.track_persistent_id)
                            .unwrap_or(&0),
                    );
                }
            }
            if nz_lpma_plays <= 25 {
                println!(
                    "  [{:016X}] {:?} (dist_kind={}, tracks={}) | play_cnt={} true_play_cnt={} last_played={:?} | sum_track_plays={sum_track_plays}",
                    p.header.playlist_persistent_id,
                    name,
                    p.header.distinguished_kind,
                    p.header.track_count,
                    p.header.play_count,
                    p.header.true_play_count,
                    p.header.date_last_played.to_utc(),
                );
            }
        }
    }
    println!(
        "  Total playlists with non-zero play_count/date_last_played: {nz_lpma_plays} / {}",
        playlists_of(raw_1119).len()
    );

    // Check ipfa.item_persistent_id_2 across all playlists in 2026-11-01 (format 22.0, which uses 68-byte ipfa 0x00CE)
    // and check IPfa (0x00DA) in 2026-11-19!
    let mdb_1101 =
        corpus_dir.join("library-data/2026-11-01/Apple Music Library.musiclibrary/Library.musicdb");
    if mdb_1101.exists() {
        let bytes_1101 = musicdb::decrypt_and_decompress_bytes(
            &fs::read(&mdb_1101)?,
            musicdb::DEFAULT_MUSICDB_KEY,
        )?;
        let raw_1101 = RawLibraryFile::from_raw_bytes(&bytes_1101)?;
        let mut ipfa_total = 0usize;
        let mut ipfa_id2_eq_id1 = 0usize;
        let mut ipfa_id2_eq_prev = 0usize;
        let mut ipfa_id2_other_nz = 0usize;
        let mut ipfa_id2_zero = 0usize;
        let mut ipfa_pos_dist: BTreeMap<&str, usize> = BTreeMap::new();
        for p in playlists_of(&raw_1101) {
            let ipfas: Vec<_> = p
                .bomas
                .iter()
                .filter_map(|b| match &b.payload {
                    BomaPayload::PlaylistItem(i) => Some(i),
                    _ => None,
                })
                .collect();
            let len = ipfas.len();
            for (idx, i) in ipfas.iter().enumerate() {
                ipfa_total += 1;
                if i.item_persistent_id_2 == 0 {
                    ipfa_id2_zero += 1;
                } else if i.item_persistent_id_2 == i.item_persistent_id {
                    ipfa_id2_eq_id1 += 1;
                    let pos_kind = if idx == 0 && len == 1 {
                        "only_item"
                    } else if idx == 0 {
                        "first_item"
                    } else if idx + 1 == len {
                        "last_item"
                    } else {
                        "middle_item"
                    };
                    *ipfa_pos_dist.entry(pos_kind).or_insert(0) += 1;
                } else if idx > 0
                    && ipfas
                        .get(idx - 1)
                        .is_some_and(|prev| i.item_persistent_id_2 == prev.item_persistent_id)
                {
                    ipfa_id2_eq_prev += 1;
                } else {
                    ipfa_id2_other_nz += 1;
                }
            }
        }
        println!(
            "  2026-11-01 ipfa[44..52] across {ipfa_total} items: zero={ipfa_id2_zero}, eq_own_id={ipfa_id2_eq_id1} (positions: {ipfa_pos_dist:?}), eq_prev_id={ipfa_id2_eq_prev}, other_nz={ipfa_id2_other_nz}"
        );
    }

    // 7.7 Check 7: iama.star_rating (40) & star_rating_inheritance (41) and itma.checkbox_part_of_gapless_album (49)
    println!(
        "\n--- 7.7 iama[40..42] (star_rating & star_rating_inheritance) & itma[49] (gapless_album) ---"
    );
    let mut album_rating_dist: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    for a in albums_of(raw_1119) {
        *album_rating_dist
            .entry((a.header.star_rating, a.header.star_rating_inheritance))
            .or_insert(0) += 1;
    }
    println!("  iama (star_rating, star_rating_inheritance) dist: {album_rating_dist:?}");

    let mut off49_dist: BTreeMap<(u8, u8, u8), usize> = BTreeMap::new();
    for t in tracks_of(raw_1119) {
        *off49_dist
            .entry((
                t.header.checkbox_part_of_gapless_album,
                t.header.cloud_status,
                t.header.playlist_only,
            ))
            .or_insert(0) += 1;
    }
    println!("  itma[49] (val, cloud_status, playlist_only) dist in 2026-11-19: {off49_dist:?}");
    if mdb_1101.exists() {
        let bytes_1101 = musicdb::decrypt_and_decompress_bytes(
            &fs::read(&mdb_1101)?,
            musicdb::DEFAULT_MUSICDB_KEY,
        )?;
        let raw_1101 = RawLibraryFile::from_raw_bytes(&bytes_1101)?;
        let mut off49_1101: BTreeMap<(u8, u8, u8), usize> = BTreeMap::new();
        for t in tracks_of(&raw_1101) {
            *off49_1101
                .entry((
                    t.header.checkbox_part_of_gapless_album,
                    t.header.cloud_status,
                    t.header.playlist_only,
                ))
                .or_insert(0) += 1;
        }
        println!(
            "  itma[49] (val, cloud_status, playlist_only) dist in 2026-11-01: {off49_1101:?}"
        );
    }

    // 7.8 Check 8: VideoInfo on video tracks, custom_lyrics_hash on "{{Instrumental}}", and PlmaSub1F6/1FF across corpus
    println!("\n--- 7.8 VideoInfo, custom_lyrics_hash, and PlmaSub1F6/PlmaSub1FF ---");
    for t in tracks_of(raw_1119) {
        if t.header.media_kind != 1 {
            let subs: Vec<_> = t
                .bomas
                .iter()
                .map(|b| format!("0x{:04X}(len={})", b.subtype, b.total_size))
                .collect();
            println!(
                "  Non-music track [{:016X}] media_kind=0x{:X} {} - {:?}: bomas={subs:?}",
                t.header.track_persistent_id,
                t.header.media_kind,
                boma_str(&t.bomas, 0x0004).unwrap_or(""),
                boma_str(&t.bomas, 0x0002).unwrap_or(""),
            );
        }
        for b in &t.bomas {
            if b.subtype == 0x0024 {
                println!(
                    "  boma 0x0024 on [{:016X}] {} - {:?}: payload={:?}",
                    t.header.track_persistent_id,
                    boma_str(&t.bomas, 0x0004).unwrap_or(""),
                    boma_str(&t.bomas, 0x0002).unwrap_or(""),
                    b.payload,
                );
            }
        }
    }
    // Also check all corpus files for any boma 0x0024 (VideoInfo)!
    for rel in [
        "library-data/2024/Apple Music Library.musiclibrary/Library.musicdb",
        "library-data/2026-11-01/Apple Music Library.musiclibrary/Library.musicdb",
        "libraries-music-samples/Monterey/SmallerLibrary.musicdb",
        "libraries-music-samples/Sequoia/MiniLibraryV7.musicdb",
    ] {
        let p = corpus_dir.join(rel);
        if p.exists()
            && let Ok(bytes) =
                musicdb::decrypt_and_decompress_bytes(&fs::read(&p)?, musicdb::DEFAULT_MUSICDB_KEY)
            && let Ok(r) = RawLibraryFile::from_raw_bytes(&bytes)
        {
            for t in tracks_of(&r) {
                for b in &t.bomas {
                    if b.subtype == 0x0024 {
                        println!(
                            "  {rel} boma 0x0024 on [{:016X}] {:?}: payload={:?}",
                            t.header.track_persistent_id,
                            boma_str(&t.bomas, 0x0002).unwrap_or(""),
                            b.payload,
                        );
                    }
                }
            }
        }
    }

    // Check custom_lyrics_hash (TrackNumerics[124..128]) vs boma 0x0036 cloud-lyrics and itma.has_local_custom_lyrics (44) / has_custom_lyrics (255)
    let mut lyrics_hash_dist: BTreeMap<(u8, u8, bool, bool), usize> = BTreeMap::new();
    let mut instrumental_hashes: Vec<(String, u32, u32)> = Vec::new();
    for t in tracks_of(raw_1119) {
        let mut cloud_lyrics_val = None;
        let mut hash_sub1 = 0u32;
        let mut hash_sub3 = 0u32;
        for b in &t.bomas {
            match &b.payload {
                BomaPayload::TrackNumerics(n) => {
                    if n.subrecord_kind == 1 {
                        hash_sub1 = n.custom_lyrics_hash;
                    } else {
                        hash_sub3 = n.custom_lyrics_hash;
                    }
                }
                BomaPayload::RawStringUtf8 { raw_bytes, .. } if b.subtype == 0x0036 => {
                    if let Ok(plist::Value::Dictionary(d)) =
                        plist::Value::from_reader(Cursor::new(raw_bytes))
                        && let Some(plist::Value::String(s)) = d.get("cloud-lyrics")
                    {
                        cloud_lyrics_val = Some(s.clone());
                    }
                }
                _ => {}
            }
        }
        let has_hash = hash_sub1 != 0 || hash_sub3 != 0;
        *lyrics_hash_dist
            .entry((
                t.header.has_local_custom_lyrics,
                t.header.has_custom_lyrics,
                cloud_lyrics_val.is_some(),
                has_hash,
            ))
            .or_insert(0) += 1;
        if let Some(s) = &cloud_lyrics_val
            && (s == "{{Instrumental}}" || s == "(Instrumental)")
            && instrumental_hashes.len() < 12
        {
            instrumental_hashes.push((
                format!(
                    "{} - {:?} ({s:?})",
                    boma_str(&t.bomas, 0x0004).unwrap_or(""),
                    boma_str(&t.bomas, 0x0002).unwrap_or("")
                ),
                hash_sub1,
                hash_sub3,
            ));
        }
    }
    println!(
        "  custom_lyrics_hash dist (itma44_local, itma255_token, has_0x36_lyrics, has_num124_hash): {lyrics_hash_dist:?}"
    );
    for (desc, h1, h3) in &instrumental_hashes {
        println!("    instrumental hash: {desc} => sub1=0x{h1:08X} ({h1}), sub3=0x{h3:08X} ({h3})");
    }

    // Check PlmaSub1F6 and PlmaSub1FF across key corpus snapshots
    let check_paths = [
        "library-data/2024/Apple Music Library.musiclibrary/Library.musicdb",
        "library-data/2025/Apple Music Library.musiclibrary/Library.musicdb",
        "library-data/Previous Libraries/Apple Music Library [2025-09-28 20.36.49].musiclibrary/Library.musicdb",
        "library-data/Previous Libraries/Apple Music Library [2026-10-28 22.33.41].musiclibrary/Library.musicdb",
        "library-data/2026-11-01/Apple Music Library.musiclibrary/Library.musicdb",
        "library-data/2026-11-07/Apple Music Library.musiclibrary/Library.musicdb",
        "snapshots/00_baseline/Library.musicdb",
        "snapshots/06_noop_reopen/Library.musicdb",
        "snapshots/2026-11-11/Apple Music Library.musiclibrary/Library.musicdb",
        "snapshots/2026-11-19/Apple Music Library.musiclibrary/Library.musicdb",
        "libraries-music-samples/Monterey/SmallerLibrary.musicdb",
        "libraries-music-samples/Sequoia/MiniLibraryV7.musicdb",
    ];
    for rel in check_paths {
        let p = corpus_dir.join(rel);
        if !p.exists() {
            continue;
        }
        let bytes =
            musicdb::decrypt_and_decompress_bytes(&fs::read(&p)?, musicdb::DEFAULT_MUSICDB_KEY)?;
        let r = RawLibraryFile::from_raw_bytes(&bytes)?;
        for s in &r.sections {
            if let HsmaChild::LibraryMaster(plma) = &s.child {
                let mut f6 = String::from("none");
                let mut ff = String::from("none");
                for b in &plma.bomas {
                    if let BomaPayload::Unknown { subtype, raw_bytes } = &b.payload {
                        if *subtype == 0x01F6 {
                            f6 = format!(
                                "len={} bytes={:02X?}",
                                raw_bytes.len(),
                                raw_bytes.as_slice()
                            );
                        } else if *subtype == 0x01FF {
                            ff = format!(
                                "len={} head={:02X?}",
                                raw_bytes.len(),
                                raw_bytes.get(..24).unwrap_or(raw_bytes)
                            );
                        }
                    }
                }
                println!("  {rel}:\n    0x01F6: {f6}\n    0x01FF: {ff}");
            }
        }
    }

    // 7.9 Also check the exact diff between 2026-11-14-offline-delete and 2026-11-19 on all mutated tracks
    println!("\n--- 7.9 DIFF BETWEEN 2026-11-14-OFFLINE-DELETE AND 2026-11-19 ---");
    let base_p = corpus_dir.join(
        "snapshots/2026-11-14-offline-delete/Apple Music Library.musiclibrary/Library.musicdb",
    );
    if base_p.exists() {
        let base_bytes = musicdb::decrypt_and_decompress_bytes(
            &fs::read(&base_p)?,
            musicdb::DEFAULT_MUSICDB_KEY,
        )?;
        let raw_base = RawLibraryFile::from_raw_bytes(&base_bytes)?;
        let old_by_id: BTreeMap<u64, &TrackItem> = tracks_of(&raw_base)
            .iter()
            .map(|t| (t.header.track_persistent_id, t))
            .collect();
        let mut diff_cnt = 0usize;
        for n in tracks_of(raw_1119) {
            let tid = n.header.track_persistent_id;
            if let Some(o) = old_by_id.get(&tid)
                && (o.header.checkbox_part_of_gapless_album
                    != n.header.checkbox_part_of_gapless_album
                    || o.header.checkbox_show_composer_in_all_views
                        != n.header.checkbox_show_composer_in_all_views
                    || o.header.checkbox_use_work_and_movement
                        != n.header.checkbox_use_work_and_movement
                    || o.header.unk_53_55 != n.header.unk_53_55
                    || o.header.star_rating != n.header.star_rating
                    || o.header.auto_sort_flags != n.header.auto_sort_flags
                    || o.header.volume_adjustment != n.header.volume_adjustment
                    || o.header.has_local_custom_lyrics != n.header.has_local_custom_lyrics
                    || o.header.has_unsynced_local_changes != n.header.has_unsynced_local_changes
                    || o.header.unk_60_62 != n.header.unk_60_62)
            {
                diff_cnt += 1;
                if diff_cnt <= 20 {
                    let [o53, o54] = *o.header.unk_53_55;
                    let [n53, n54] = *n.header.unk_53_55;
                    println!(
                        "  Mutated Track [{tid:016X}] {} - {:?} (cloud={}):",
                        boma_str(&n.bomas, 0x0004).unwrap_or(""),
                        boma_str(&n.bomas, 0x0002).unwrap_or(""),
                        n.header.cloud_status,
                    );
                    println!(
                        "    off44(local_lyrics): {} -> {}, off46(unsynced): {} -> {}, off49..56: [{},{},{},{},{},{},{},{}] -> [{},{},{},{},{},{},{},{}]",
                        o.header.has_local_custom_lyrics,
                        n.header.has_local_custom_lyrics,
                        o.header.has_unsynced_local_changes,
                        n.header.has_unsynced_local_changes,
                        o.header.checkbox_part_of_gapless_album,
                        o.header.checkbox_remember_playback_position,
                        o.header.checkbox_show_composer_in_all_views,
                        o.header.checkbox_use_work_and_movement,
                        o53,
                        o54,
                        o.header.has_playback_or_bounds_activity,
                        *o.header.unk_56,
                        n.header.checkbox_part_of_gapless_album,
                        n.header.checkbox_remember_playback_position,
                        n.header.checkbox_show_composer_in_all_views,
                        n.header.checkbox_use_work_and_movement,
                        n53,
                        n54,
                        n.header.has_playback_or_bounds_activity,
                        *n.header.unk_56,
                    );
                    println!(
                        "    off60..62: {:02X?} -> {:02X?}, star_rating(65): {} -> {}, auto_sort_flags(66..72): {:02X?} -> {:02X?}",
                        *o.header.unk_60_62,
                        *n.header.unk_60_62,
                        o.header.star_rating,
                        n.header.star_rating,
                        o.header.auto_sort_flags,
                        n.header.auto_sort_flags,
                    );
                }
            }
        }
        println!("  Total tracks with metadata/flag changes between 11-14 and 11-19: {diff_cnt}");
    }

    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc verification of ffprobe/exiftool/movpkg discoveries"
)]
fn check_probe_verifications(raw_1119: &RawLibraryFile, corpus_dir: &Path) -> Result<()> {
    // 8.1 TrackNumerics[45] (unk_41_54[4]) vs (sub==3 || date_released != 0 || u116 != 0)
    let mut b45_vs_formula: BTreeMap<(u32, u8, bool, bool), usize> = BTreeMap::new();
    let mut b45_mismatches = Vec::new();
    for t in tracks_of(raw_1119) {
        let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
        let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
        for b in &t.bomas {
            if let BomaPayload::TrackNumerics(n) = &b.payload {
                let b45 = n.unk_41_54.get(4).copied().unwrap_or(0);
                let has_rel = n.date_released.raw() != 0;
                let has_u116 = *n.unk_116_cloud_timestamp != 0;
                *b45_vs_formula
                    .entry((n.subrecord_kind, b45, has_rel, has_u116))
                    .or_default() += 1;
                let expected = n.subrecord_kind == 3 || has_rel || has_u116;
                if (b45 == 1) != expected && b45_mismatches.len() < 15 {
                    b45_mismatches.push(format!(
                        "[{:016X}] sub={} b45={} drel={} u116={} cloud={} {} - {:?}",
                        t.header.track_persistent_id,
                        n.subrecord_kind,
                        b45,
                        n.date_released.raw(),
                        *n.unk_116_cloud_timestamp,
                        t.header.cloud_status,
                        artist,
                        title
                    ));
                }
            }
        }
    }
    println!("--- 8.1 TrackNumerics[45] vs (sub==3 || drel!=0 || u116!=0) on 2026-11-19 ---");
    for ((sub, b45, has_rel, has_u116), cnt) in &b45_vs_formula {
        println!("  sub={sub} b45={b45} drel={has_rel} u116={has_u116} -> {cnt}");
    }
    println!("  Formula mismatches ({}):", b45_mismatches.len());
    for m in &b45_mismatches {
        println!("    {m}");
    }

    // 8.1b Also check 2025 (pre-u116) for sub=1 b45 vs drel!=0
    let mdb_2025 =
        corpus_dir.join("library-data/2025/Apple Music Library.musiclibrary/Library.musicdb");
    if mdb_2025.exists() {
        let dec = musicdb::decrypt_and_decompress_bytes(
            &fs::read(&mdb_2025)?,
            musicdb::DEFAULT_MUSICDB_KEY,
        )?;
        let raw_2025 = RawLibraryFile::from_raw_bytes(&dec)?;
        let mut c25: BTreeMap<(u32, u8, bool, bool), usize> = BTreeMap::new();
        for t in tracks_of(&raw_2025) {
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    let b45 = n.unk_41_54.get(4).copied().unwrap_or(0);
                    *c25.entry((
                        n.subrecord_kind,
                        b45,
                        n.date_released.raw() != 0,
                        *n.unk_116_cloud_timestamp != 0,
                    ))
                    .or_default() += 1;
                }
            }
        }
        println!("--- 8.1b TrackNumerics[45] on 2025 (v1.5.4.96, pre-u116) ---");
        for ((sub, b45, has_rel, has_u116), cnt) in &c25 {
            println!("  sub={sub} b45={b45} drel={has_rel} u116={has_u116} -> {cnt}");
        }
    }

    // 8.1c Check Monterey itma[54] == 1 vs all TrackItemHeader/TrackNumerics fields
    let mdb_mont = corpus_dir.join("libraries-music-samples/Monterey/SmallerLibrary.musicdb");
    if mdb_mont.exists() {
        let dec = musicdb::decrypt_and_decompress_bytes(
            &fs::read(&mdb_mont)?,
            musicdb::DEFAULT_MUSICDB_KEY,
        )?;
        let raw_mont = RawLibraryFile::from_raw_bytes(&dec)?;
        let mut cm: BTreeMap<(u8, u8, u8, i16, bool), usize> = BTreeMap::new();
        for t in tracks_of(&raw_mont) {
            let [b53, b54] = *t.header.unk_53_55;
            let n0 = t.bomas.iter().find_map(|b| match &b.payload {
                BomaPayload::TrackNumerics(n) => Some(n),
                _ => None,
            });
            let emb = n0.map_or(0, |n| n.has_embedded_artwork);
            let art_sz = n0.is_some_and(|n| n.artwork_total_size_bytes > 0);
            *cm.entry((b53, b54, t.header.artwork_cache_state, emb, art_sz))
                .or_default() += 1;
        }
        println!(
            "--- 8.1c Monterey itma[53,54] vs artwork_cache_state/has_embedded_artwork/art_sz ---"
        );
        for ((b53, b54, acs, emb, asz), cnt) in &cm {
            println!(
                "  b53={b53} b54={b54} art_cache_state={acs} emb={emb} art_sz>0={asz} -> {cnt}"
            );
        }
    }

    // 8.2 Target tracks detail for b45 & b48
    println!(
        "\n--- 8.2 Target tracks detail (Beatles, AC/DC, Camel, Ringing Hollow, Borboletas) ---"
    );
    for t in tracks_of(raw_1119) {
        let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
        let album = boma_str(&t.bomas, 0x0003).unwrap_or("");
        let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
        let hit = (title == "Back in the U.S.S.R." && album == "The Beatles")
            || (title == "It Won't Be Long" && album == "With The Beatles")
            || (artist == "AC/DC" && (title == "Highway to Hell" || title == "Hells Bells"))
            || (artist == "Camel" && album == "Nude" && (title == "City Life" || title == "Nude"))
            || title == "Ringing Hollow"
            || title == "Borboletas";
        if hit {
            println!(
                "  [{:016X}] {artist} - {title:?} ({album:?}) cloud={}",
                t.header.track_persistent_id, t.header.cloud_status
            );
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    let b45 = n.unk_41_54.get(4).copied().unwrap_or(0);
                    let b48 = n.unk_41_54.get(7).copied().unwrap_or(0);
                    println!(
                        "    sub={} b45={b45} b48={b48} drel={} ({:?}) dmod={} dadd={} dl_traits=0x{:02X} drm=(u96=0x{:08X}, plat={}, vers=0x{:08X})",
                        n.subrecord_kind,
                        n.date_released.raw(),
                        n.date_released.to_utc().map(|d| d.to_string()),
                        n.date_modified.raw(),
                        n.date_added.raw(),
                        n.downloaded_audio_traits,
                        *n.unk_96_100,
                        n.drm_platform,
                        n.drm_version_restrictions,
                    );
                }
            }
        }
    }

    // 8.3 Corpus-wide scan across all .musicdb files for b45 vs date_released, b48 != 0, unk_96_108 != 0, unk_216_248 != 0, unk_336_340 != 0
    println!("\n--- 8.3 Corpus-wide scan across all .musicdb files ---");
    let mut files = Vec::new();
    collect_mdb(corpus_dir, &mut files)?;
    files.sort();
    let mut all_b45_mismatch_files = 0usize;
    let mut all_b45_total = 0usize;
    let mut all_b45_mismatches = 0usize;
    let mut all_b48: BTreeMap<(u8, String), usize> = BTreeMap::new();
    let mut drm_tuples: BTreeMap<(u32, u32, u32), usize> = BTreeMap::new();
    let mut dl_trait_pairs: BTreeMap<(u32, u8), usize> = BTreeMap::new();
    let mut drm_key_blobs: BTreeMap<String, usize> = BTreeMap::new();

    for path in &files {
        let Ok(bytes) = fs::read(path) else { continue };
        let Ok(dec) = musicdb::decrypt_and_decompress_bytes(&bytes, musicdb::DEFAULT_MUSICDB_KEY)
        else {
            continue;
        };
        let Ok(raw) = RawLibraryFile::from_raw_bytes(&dec) else {
            continue;
        };
        let rel_path = path
            .strip_prefix(corpus_dir)
            .unwrap_or(path)
            .display()
            .to_string();
        let mut file_mis = 0usize;
        for t in tracks_of(&raw) {
            let title = boma_str(&t.bomas, 0x0002).unwrap_or("");
            let artist = boma_str(&t.bomas, 0x0004).unwrap_or("");
            for b in &t.bomas {
                if let BomaPayload::TrackNumerics(n) = &b.payload {
                    all_b45_total += 1;
                    let b45 = n.unk_41_54.get(4).copied().unwrap_or(0);
                    let has_rel = n.date_released.raw() != 0;
                    if (b45 == 1) != has_rel {
                        all_b45_mismatches += 1;
                        file_mis += 1;
                    }
                    let b48 = n.unk_41_54.get(7).copied().unwrap_or(0);
                    if b48 != 0 {
                        *all_b48
                            .entry((b48, format!("{artist} - {title:?}")))
                            .or_default() += 1;
                    }
                    let w0 = *n.unk_96_100;
                    let w1 = n.drm_platform;
                    let w2 = n.drm_version_restrictions;
                    if w0 != 0 || w1 != 0 || w2 != 0 {
                        *drm_tuples.entry((w0, w1, w2)).or_default() += 1;
                    }
                    let dl_traits = n.downloaded_audio_traits;
                    if dl_traits != 0 {
                        *dl_trait_pairs
                            .entry((dl_traits, t.header.audio_traits))
                            .or_default() += 1;
                    }
                    let raw216 = *n.unk_216_248;
                    if raw216.iter().any(|&x| x != 0) {
                        *drm_key_blobs.entry(hex::encode(raw216)).or_default() += 1;
                    }
                }
            }
        }
        if file_mis > 0 {
            all_b45_mismatch_files += 1;
            println!("  b45 != has_date_released in {rel_path}: {file_mis} records");
        }
    }
    println!(
        "  Corpus-wide b45 vs (date_released != 0): {all_b45_mismatches}/{all_b45_total} mismatches across {all_b45_mismatch_files} files"
    );
    println!("  Corpus-wide b48 != 0 (distinct byte48, track):");
    for ((b48, trk), cnt) in &all_b48 {
        println!("    b48={b48} {trk} (across {cnt} snapshots)");
    }
    println!("  Corpus-wide DRM fields != 0 (w96, drm_platform, drm_version_restrictions):");
    for ((w0, w1, w2), cnt) in &drm_tuples {
        println!("    w96=0x{w0:08X} plat=0x{w1:08X} vers=0x{w2:08X} -> {cnt}");
    }
    println!("  Corpus-wide downloaded_audio_traits != 0 (downloaded, itma.audio_traits):");
    for ((dl_t, at), cnt) in &dl_trait_pairs {
        println!("    downloaded={dl_t} (0x{dl_t:02X}) itma.audio_traits=0x{at:02X} -> {cnt}");
    }
    println!("  Corpus-wide unk_216_248 != 0:");
    for (h, cnt) in &drm_key_blobs {
        println!("    {h} -> {cnt}");
    }

    Ok(())
}
