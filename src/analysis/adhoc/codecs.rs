//! Ad-hoc probe: `TrackNumerics` `subrecord_kind=1` vs `subrecord_kind=3` codec/container cross-tab.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use musicdb::sections::BomaPayload;

use super::helpers::{boma_str, load_1119, tracks_of};

pub(crate) fn run(corpus_dir: &Path) -> Result<()> {
    type CodecTuple = (u8, Option<String>, u16, u32, Option<String>);
    type CodecGroupKey = (u8, u8, String, Option<CodecTuple>, Option<CodecTuple>);

    let raw = load_1119(corpus_dir)?;
    println!(
        "=== CODEC: subrecord_kind=1 (local file) vs subrecord_kind=3 (off54 / cloud_codec_kind / codec_fourcc) ==="
    );

    let mut table: BTreeMap<CodecGroupKey, (usize, String)> = BTreeMap::new();

    for t in tracks_of(&raw) {
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

    Ok(())
}
