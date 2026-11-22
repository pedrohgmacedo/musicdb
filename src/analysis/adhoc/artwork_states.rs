//! Ad-hoc probe: artwork cache state bytes across `itma[328]`, `iAma[100]`, `lpma[279]`, and
//! `isma[52]` joined against `artwork.sqlite`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;

use super::helpers::{artists_of, load_1119, playlists_of, socials_of, tracks_of};

pub(crate) fn run(corpus_dir: &Path) -> Result<()> {
    let raw = load_1119(corpus_dir)?;
    let art_db =
        corpus_dir.join("snapshots/2026-11-19/Apple Music Library.musiclibrary/artwork.sqlite");
    println!(
        "=== ARTWORK STATE BYTE (0, 3, 5, 8, 12) ACROSS itma[328], iAma[100], lpma[279], isma[52] vs artwork.sqlite ==="
    );
    check_artwork_states(&raw, &art_db)
}

#[cfg(feature = "sqlite")]
#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc artwork.sqlite cross-tabulation across 4 sections"
)]
fn check_artwork_states(raw: &musicdb::sections::RawLibraryFile, art_db: &Path) -> Result<()> {
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
fn check_artwork_states(_raw: &musicdb::sections::RawLibraryFile, _art_db: &Path) -> Result<()> {
    Ok(())
}
