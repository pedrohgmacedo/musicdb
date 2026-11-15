use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use plist::{Dictionary, Value as PlistValue};
use serde::{Deserialize, Serialize};

use musicdb::model::{Track, format_persistent_id};
use musicdb::sections::{BomaPayload, HsmaChild, RawLibraryFile, TrackItem};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct XmlComparisonReport {
    pub(crate) xml_track_count: usize,
    pub(crate) musicdb_track_count: usize,
    pub(crate) matched_by_persistent_id: usize,
    pub(crate) field_match_counts: BTreeMap<String, (usize, usize)>,
    pub(crate) unmapped_xml_keys_found_in_binary: BTreeMap<String, Vec<String>>,
}

/// Cross-references a `RawLibraryFile` against an `iTunes Music Library.xml` file.
pub(crate) fn compare_with_itunes_xml(
    raw: &RawLibraryFile,
    xml_path: impl AsRef<Path>,
) -> anyhow::Result<XmlComparisonReport> {
    let root = PlistValue::from_file(xml_path)?;
    let dict = root
        .as_dictionary()
        .ok_or_else(|| anyhow::anyhow!("Root of XML plist is not a dictionary"))?;

    let tracks_dict = dict
        .get("Tracks")
        .and_then(PlistValue::as_dictionary)
        .ok_or_else(|| anyhow::anyhow!("Missing 'Tracks' dictionary in XML plist"))?;

    let mut mdb_tracks: HashMap<String, &TrackItem> = HashMap::new();
    for hsma in &raw.sections {
        if let HsmaChild::TrackList(ltma) = &hsma.child {
            for track in &ltma.tracks {
                mdb_tracks.insert(
                    format_persistent_id(track.header.track_persistent_id),
                    track,
                );
            }
        }
    }

    let mut matched_by_persistent_id = 0usize;
    let mut field_match_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut unmapped_xml_keys_found_in_binary: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for track_val in tracks_dict.values() {
        let Some(tdict) = track_val.as_dictionary() else {
            continue;
        };
        let Some(pid) = tdict.get("Persistent ID").and_then(PlistValue::as_string) else {
            continue;
        };
        let Some(&itma) = mdb_tracks.get(&pid.to_uppercase()) else {
            continue;
        };
        matched_by_persistent_id += 1;
        let mdb_track = Track::from(itma);

        compare_xml_strings(tdict, &mdb_track, &mut field_match_counts);
        compare_xml_numerics(tdict, itma, &mdb_track, &mut field_match_counts);
        compare_xml_flags_and_misc(tdict, &mdb_track, &mut field_match_counts);

        for key in tdict.keys() {
            unmapped_xml_keys_found_in_binary
                .entry(key.clone())
                .or_default();
        }
    }

    Ok(XmlComparisonReport {
        xml_track_count: tracks_dict.len(),
        musicdb_track_count: mdb_tracks.len(),
        matched_by_persistent_id,
        field_match_counts,
        unmapped_xml_keys_found_in_binary,
    })
}

fn record_match<T: PartialEq>(
    counts: &mut BTreeMap<String, (usize, usize)>,
    name: &str,
    xml_val: &T,
    mdb_val: &T,
) {
    let entry = counts.entry(name.to_string()).or_insert((0, 0));
    entry.1 += 1;
    if xml_val == mdb_val {
        entry.0 += 1;
    }
}

fn compare_xml_strings(
    tdict: &Dictionary,
    mdb_track: &Track,
    counts: &mut BTreeMap<String, (usize, usize)>,
) {
    let fields: [(&str, &str, Option<&str>); 16] = [
        ("Name", "Name (title)", mdb_track.title.as_deref()),
        ("Artist", "Artist", mdb_track.artist.as_deref()),
        ("Album", "Album", mdb_track.album.as_deref()),
        (
            "Album Artist",
            "Album Artist",
            mdb_track.album_artist.as_deref(),
        ),
        ("Composer", "Composer", mdb_track.composer.as_deref()),
        ("Genre", "Genre", mdb_track.genre.as_deref()),
        ("Kind", "Kind", mdb_track.kind.as_deref()),
        ("Sort Name", "Sort Name", mdb_track.sort_title.as_deref()),
        ("Sort Album", "Sort Album", mdb_track.sort_album.as_deref()),
        (
            "Sort Artist",
            "Sort Artist",
            mdb_track.sort_artist.as_deref(),
        ),
        (
            "Sort Album Artist",
            "Sort Album Artist",
            mdb_track.sort_album_artist.as_deref(),
        ),
        (
            "Sort Composer",
            "Sort Composer",
            mdb_track.sort_composer.as_deref(),
        ),
        ("Comments", "Comments", mdb_track.comments.as_deref()),
        ("Grouping", "Grouping", mdb_track.grouping.as_deref()),
        ("Work", "Work", mdb_track.work_name.as_deref()),
        (
            "Movement Name",
            "Movement Name",
            mdb_track.movement_name.as_deref(),
        ),
    ];
    for (xml_key, label, mdb_val) in fields {
        if let Some(val) = tdict.get(xml_key).and_then(PlistValue::as_string) {
            record_match(counts, label, &Some(val), &mdb_val);
        }
    }
}

fn compare_xml_numerics(
    tdict: &Dictionary,
    itma: &TrackItem,
    mdb_track: &Track,
    counts: &mut BTreeMap<String, (usize, usize)>,
) {
    let opt_u32_fields: [(&str, &str, Option<u32>); 6] = [
        (
            "Total Time",
            "Total Time (duration_ms)",
            mdb_track.duration_ms,
        ),
        ("Size", "Size (file_size)", mdb_track.file_size),
        ("Bit Rate", "Bit Rate", mdb_track.bit_rate),
        ("Normalization", "Normalization", mdb_track.normalization),
        ("Play Count", "Play Count", mdb_track.play_count),
        ("Skip Count", "Skip Count", mdb_track.skip_count),
    ];
    for (xml_key, label, mdb_val) in opt_u32_fields {
        if let Some(v) = tdict.get(xml_key).and_then(PlistValue::as_unsigned_integer) {
            record_match(counts, label, &u32::try_from(v).ok(), &mdb_val);
        }
    }
    if let Some(v) = tdict
        .get("Sample Rate")
        .and_then(PlistValue::as_unsigned_integer)
    {
        record_match(
            counts,
            "Sample Rate",
            &Some(v.to_string()),
            &mdb_track.sample_rate.map(|f| format!("{f}")),
        );
    }

    let u16_fields: [(&str, &str, u16); 7] = [
        ("Track Number", "Track Number", mdb_track.track_number),
        ("Track Count", "Track Count", mdb_track.track_total),
        ("Disc Number", "Disc Number", mdb_track.disc_number),
        ("Disc Count", "Disc Count", mdb_track.disc_total),
        ("BPM", "BPM", mdb_track.bpm),
        (
            "Movement Number",
            "Movement Number",
            mdb_track.movement_number,
        ),
        ("Movement Count", "Movement Count", mdb_track.movement_total),
    ];
    for (xml_key, label, mdb_val) in u16_fields {
        if let Some(v) = tdict.get(xml_key).and_then(PlistValue::as_unsigned_integer) {
            record_match(counts, label, &u16::try_from(v).ok(), &Some(mdb_val));
        }
    }

    compare_xml_special_numerics(tdict, itma, mdb_track, counts);
}

fn compare_xml_special_numerics(
    tdict: &Dictionary,
    itma: &TrackItem,
    mdb_track: &Track,
    counts: &mut BTreeMap<String, (usize, usize)>,
) {
    if let Some(v) = tdict.get("Year").and_then(PlistValue::as_unsigned_integer) {
        record_match(
            counts,
            "Year",
            &u32::try_from(v).ok(),
            &Some(mdb_track.year),
        );
    }
    if let Some(v) = tdict
        .get("Artwork Count")
        .and_then(PlistValue::as_unsigned_integer)
    {
        record_match(
            counts,
            "Artwork Count",
            &u16::try_from(v).ok(),
            &mdb_track.artwork_count,
        );
    }
    for (xml_key, mdb_val) in [
        ("File Folder Count", mdb_track.file_folder_count),
        ("Library Folder Count", mdb_track.library_folder_count),
    ] {
        if let Some(v) = tdict.get(xml_key).and_then(PlistValue::as_signed_integer) {
            record_match(counts, xml_key, &u16::try_from(v).ok(), &mdb_val);
        }
    }
    if let Some(v) = tdict
        .get("Volume Adjustment")
        .and_then(PlistValue::as_signed_integer)
    {
        record_match(
            counts,
            "Volume Adjustment",
            &i32::try_from(v).ok(),
            &Some(mdb_track.volume_adjustment),
        );
    }
    if let Some(v) = tdict
        .get("File Type")
        .and_then(PlistValue::as_unsigned_integer)
    {
        let num_ft = itma.bomas.iter().find_map(|b| match &b.payload {
            BomaPayload::TrackNumerics(n) => Some(n.file_type_fourcc),
            _ => None,
        });
        record_match(
            counts,
            "File Type (FourCC u32)",
            &u32::try_from(v).ok(),
            &num_ft,
        );
    }
    if let Some(v) = tdict
        .get("Play Date")
        .and_then(PlistValue::as_unsigned_integer)
    {
        // In iTunes XML, "Play Date" is seconds since 1904-01-01 in local time.
        let ps_date = itma.bomas.iter().find_map(|b| match &b.payload {
            BomaPayload::TrackPlaysSkips(ps) => Some(ps.date_last_played.raw()),
            _ => None,
        });
        record_match(
            counts,
            "Play Date (Apple local epoch)",
            &u32::try_from(v).ok(),
            &ps_date,
        );
    }
}

fn compare_xml_flags_and_misc(
    tdict: &Dictionary,
    mdb_track: &Track,
    counts: &mut BTreeMap<String, (usize, usize)>,
) {
    let bool_fields: [(&str, &str, bool); 6] = [
        (
            "Part Of Gapless Album",
            "Part Of Gapless Album",
            mdb_track.part_of_gapless_album,
        ),
        ("Playlist Only", "Playlist Only", mdb_track.playlist_only),
        ("Compilation", "Compilation", mdb_track.album_is_compilation),
        ("Purchased", "Purchased", mdb_track.purchased),
        ("Disabled", "Disabled", mdb_track.disabled),
        (
            "Skip When Shuffling",
            "Skip When Shuffling",
            mdb_track.skip_when_shuffling,
        ),
    ];
    for (xml_key, label, mdb_val) in bool_fields {
        if let Some(v) = tdict.get(xml_key).and_then(PlistValue::as_boolean) {
            record_match(counts, label, &v, &mdb_val);
        }
    }
    if let Some(v) = tdict.get("Matched").and_then(PlistValue::as_boolean) {
        record_match(
            counts,
            "Matched (iTunes Match)",
            &Some(v),
            &mdb_track.is_itunes_matched,
        );
    }
    if let Some(v) = tdict.get("Apple Music").and_then(PlistValue::as_boolean) {
        record_match(
            counts,
            "Apple Music (catalog)",
            &Some(v),
            &mdb_track.is_apple_music_catalog,
        );
    }
}
