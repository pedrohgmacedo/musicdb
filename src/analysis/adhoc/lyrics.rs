//! Ad-hoc probe: `TrackNumerics[59]` (`catalog_lyrics_state`) vs `boma 0x0036` plist keys.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

use anyhow::Result;
use musicdb::sections::BomaPayload;

use super::helpers::{boma_str, load_1119, tracks_of};

#[expect(
    clippy::too_many_lines,
    reason = "ad-hoc lyrics diagnostic cross-tabulation"
)]
pub(crate) fn run(corpus_dir: &Path) -> Result<()> {
    let raw = load_1119(corpus_dir)?;
    println!("=== LYRICS: TrackNumerics[59] vs boma 0x0036 ON TARGET TRACKS & FULL LIBRARY ===");

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

    for t in tracks_of(&raw) {
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
    for t in tracks_of(&raw) {
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

    Ok(())
}
