//! `boma` (Binary Object / Metadata Container) section and payload structures.

use std::io::{Cursor, Read, Seek, Write};

use binrw::{BinRead, BinResult, BinWrite, Endian};

use super::playlist::{
    CompactPlaylistTrackItem, PlaylistTrackItem, SlstSection, SmartPlaylistOptions,
};
use super::remaining_stream_bytes;
use super::track::{TrackNumerics, TrackPlaysSkips, VideoInfo};
use super::unknown::{TailBytes, Unknown, UnknownVisitor, VisitUnknowns, visit_unknowns};
use crate::error::MusicDbError;

/// `boma` (Binary Object / Metadata Container) section.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BomaSection {
    /// Offset 4: Header size in bytes (always 20 = `0x14`, minimum 16).
    pub header_size: u32,
    /// Offset 8: Total size in bytes of this `boma` (header + payload).
    pub total_size: u32,
    /// Offset 12: Subtype identifier (`0x0001`, `0x0002`, `0x00CE`, etc.).
    pub subtype: u32,
    /// Offset 16..`header_size`: Remaining header bytes (4 bytes `16..20` when `header_size == 20`),
    /// stored inline without heap allocation up to 16 bytes.
    pub unk_16_end: Unknown<TailBytes>,
    /// Parsed payload of length `total_size - header_size`.
    pub payload: BomaPayload,
}

impl VisitUnknowns for BomaSection {
    fn visit_unknowns<V: UnknownVisitor>(&self, section: &str, visitor: &mut V) {
        let label: std::borrow::Cow<'_, str> = match section {
            "" => std::borrow::Cow::Borrowed("boma"),
            "plma" => std::borrow::Cow::Borrowed("plma::boma"),
            "iama" => std::borrow::Cow::Borrowed("iama::boma"),
            "iAma" => std::borrow::Cow::Borrowed("iAma::boma"),
            "itma" => std::borrow::Cow::Borrowed("itma::boma"),
            "lpma" => std::borrow::Cow::Borrowed("lpma::boma"),
            "isma" => std::borrow::Cow::Borrowed("isma::boma"),
            other => std::borrow::Cow::Owned(format!("{other}::boma")),
        };
        visit_unknowns!(visitor, &label, [(tail: self.unk_16_end, 16)]);
        match &self.payload {
            BomaPayload::TrackNumerics(n) => n.visit_unknowns("TrackNumerics", visitor),
            BomaPayload::TrackPlaysSkips(p) => p.visit_unknowns("TrackPlaysSkips", visitor),
            BomaPayload::VideoInfo(v) => v.visit_unknowns("VideoInfo", visitor),
            BomaPayload::PlaylistItem(ipfa) => ipfa.visit_unknowns("ipfa", visitor),
            BomaPayload::PlaylistItemCompact(ipfa) => ipfa.visit_unknowns("IPfa", visitor),
            BomaPayload::SmartPlaylistOptions(opts) => {
                opts.visit_unknowns("SmartPlaylistOptions", visitor);
            }
            BomaPayload::SmartPlaylistRules(slst) => slst.visit_unknowns("SLst", visitor),
            BomaPayload::StringSection(s) => s.visit_unknowns("boma::str", visitor),
            BomaPayload::RawStringUtf8 { .. }
            | BomaPayload::RawStringUtf16Le { .. }
            | BomaPayload::Book { .. }
            | BomaPayload::BinaryPlist { .. }
            | BomaPayload::Unknown { .. } => {}
        }
    }
}

/// Structured string payload inside a `boma` container.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BomaStringSection {
    /// Offset 0..4: String encoding (`1` = UTF-16LE, `2` = UTF-8).
    pub encoding: u32,
    /// Offset 4..8: Byte length of the string starting at offset 16.
    pub string_byte_len: u32,
    /// Offset 8..12: Counter incrementing per parent `boma` subtype.
    pub parent_subtype_counter: u32,
    /// Offset 12..16: Unknown 4 bytes (typically 0).
    pub unk_12: Unknown<u32>,
    /// Decoded string value.
    pub text: String,
    /// Exact raw bytes of the string for 100% lossless round-trip serialization.
    pub raw_string_bytes: Vec<u8>,
}

impl VisitUnknowns for BomaStringSection {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "boma::str", [(self.unk_12, 12)]);
    }
}

/// Payload variants of a `boma` container.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BomaPayload {
    /// Subtype `0x0001`: Track numeric properties (364 bytes).
    TrackNumerics(Box<TrackNumerics>),
    /// Subtype `0x0017`: Track play and skip statistics (52 bytes).
    TrackPlaysSkips(TrackPlaysSkips),
    /// Subtype `0x0024`: Video resolution and codec info (84 bytes).
    VideoInfo(Box<VideoInfo>),
    /// Subtype `0x00CE`: Playlist track reference (`ipfa`, 68 bytes in format <= 22).
    PlaylistItem(PlaylistTrackItem),
    /// Subtype `0x00DA`: Compact playlist track reference (`IPfa`, 32 bytes in format >= 23).
    PlaylistItemCompact(CompactPlaylistTrackItem),
    /// Subtype `0x00CA`: Smart playlist options (112 bytes).
    SmartPlaylistOptions(Box<SmartPlaylistOptions>),
    /// Subtype `0x00C9`: Smart playlist rules list (`SLst`, big-endian).
    SmartPlaylistRules(Box<SlstSection>),
    /// Structured string with 16-byte header (`encoding`, `byte_len`, `counter`, `unk_12`).
    StringSection(BomaStringSection),
    /// Raw UTF-8 string without 16-byte string header (e.g., XML plists `0x36`, `0x38`, `0xCD`, `0x192`, `0x1FC`).
    RawStringUtf8 {
        /// Decoded UTF-8 text.
        text: String,
        /// Exact raw payload bytes.
        raw_bytes: Vec<u8>,
    },
    /// Raw UTF-16LE string without 16-byte string header (e.g., `0x1FD`, `0x200` on Windows).
    RawStringUtf16Le {
        /// Decoded UTF-16LE text.
        text: String,
        /// Exact raw payload bytes.
        raw_bytes: Vec<u8>,
    },
    /// macOS `book` (Bookmark / Alias) structure (e.g., `0x0042`, `0x01FC`, `0x01FD`, `0x0200` on macOS).
    Book {
        /// Extracted POSIX filesystem path from TOC key `0x1004` (`kBookmarkPath`), if present.
        extracted_path: Option<String>,
        /// Decoded UTF-8 string and URL components referenced by the bookmark TOC (`0x0101` / `0x0901`).
        components: Vec<String>,
        /// Exact raw bookmark payload bytes.
        raw_bytes: Vec<u8>,
    },
    /// Binary property list (`bplist00`) payload (e.g., `0x0135` album motion artwork metadata,
    /// and format 23.x `0x0036`, `0x0038`, `0x00CD`, and `0x0192` cloud metadata plists).
    BinaryPlist {
        /// `boma` subtype identifier.
        subtype: u32,
        /// Exact raw binary plist bytes.
        raw_bytes: Vec<u8>,
    },
    /// Unrecognized or raw `boma` payload preserved losslessly.
    Unknown {
        /// `boma` subtype identifier.
        subtype: u32,
        /// Exact raw payload bytes.
        raw_bytes: Vec<u8>,
    },
}

impl BomaSection {
    /// Returns the human-readable name of the `boma` subtype.
    #[must_use]
    pub fn subtype_name(&self) -> &'static str {
        boma_subtype_name(self.subtype)
    }

    /// Extracts the string value if this `boma` holds a string (`StringSection`, `RawStringUtf8`, `RawStringUtf16Le`, or `Book`).
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match &self.payload {
            BomaPayload::StringSection(s) => Some(&s.text),
            BomaPayload::RawStringUtf8 { text, .. }
            | BomaPayload::RawStringUtf16Le { text, .. } => Some(text),
            BomaPayload::Book { extracted_path, .. } => extracted_path.as_deref(),
            _ => None,
        }
    }

    /// Consumes this `boma` section and returns its decoded string without cloning, if applicable.
    pub(crate) fn into_string(self) -> Option<String> {
        match self.payload {
            BomaPayload::StringSection(s) => Some(s.text),
            BomaPayload::RawStringUtf8 { text, .. }
            | BomaPayload::RawStringUtf16Le { text, .. } => Some(text),
            BomaPayload::Book { extracted_path, .. } => extracted_path,
            _ => None,
        }
    }
}

/// Returns the human-readable field name for a known `boma` subtype ID.
#[must_use]
pub fn boma_subtype_name(subtype: u32) -> &'static str {
    match subtype {
        0x0001 => "track_numerics",
        0x0002 => "title",
        0x0003 => "album",
        0x0004 => "artist",
        0x0005 => "genre",
        0x0006 => "kind",
        0x0007 => "equalizer",
        0x0008 => "comments",
        0x000B => "file_url",
        0x000C => "composer",
        0x000E => "grouping",
        0x0012 => "episode_description",
        0x0016 => "episode_synopsis",
        0x0017 => "track_plays_skips",
        0x0018 => "series_title",
        0x0019 => "episode_number",
        0x001B => "album_artist",
        0x001C => "content_rating_str",
        0x001D => "plist_asset_info",
        0x001E => "sort_title",
        0x001F => "sort_album",
        0x0020 => "sort_artist",
        0x0021 => "sort_album_artist",
        0x0022 => "sort_composer",
        0x0024 => "video_info",
        0x002B => "isrc",
        0x002E => "copyright",
        0x0033 => "series_synopsis",
        0x0034 => "itunes_store_flavor",
        0x0036 => "plist_artwork",
        0x0038 => "plist_redownload_params",
        0x003B => "purchaser_email",
        0x003C => "purchaser_name",
        0x003F => "work_name",
        0x0040 => "movement_name",
        0x0042 => "book_path",
        0x0043 => "legacy_file_path",
        0x0053 => "sort_show",
        0x00C8 => "playlist_name",
        0x00C9 => "smart_playlist_rules",
        0x00CA => "smart_playlist_options",
        0x00CD => "plist_cover_artwork_recipe",
        0x00CE => "playlist_item_ipfa",
        0x00DA => "playlist_item_IPfa",
        0x012C => "album_title",
        0x012D => "album_artist_12d",
        0x012E => "album_artist_12e",
        0x012F => "series_title_12f",
        0x0133 => "cloud_album_id",
        0x0134 => "album_editorial_notes",
        0x0135 => "plist_album_motion_artwork",
        0x0136 => "catalog_album_genre",
        0x0190 => "artist_name",
        0x0191 => "sort_artist_name",
        0x0192 => "plist_artist_artwork_url",
        0x0193 => "cloud_artist_id",
        0x01F4 => "unknown_hex_1f4",
        0x01F6 => "unknown_hex_1f6",
        0x01F8 => "media_folder_uri",
        0x01FC => "imported_itl_path",
        0x01FD => "media_folder_path_1fd",
        0x01FE => "unknown_hex_1fe",
        0x01FF => "library_persistent_id_ref",
        0x0200 => "media_folder_path_200",
        0x0201 => "library_view_version_vector",
        0x02BC => "xml_block_2bc",
        0x02BE => "app_song_title",
        0x02BF => "app_song_artist",
        0x0320 => "social_profile_id",
        0x0321 => "social_profile_name",
        0x0322 => "social_profile_handle",
        0x0323 => "social_profile_avatar_id",
        0x0324 => "social_profile_avatar_url",
        0x03CC => "xml_block_3cc",
        _ => "unknown_boma",
    }
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let slice: [u8; 4] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(slice))
}

const BOOK_TOC_MAGIC: u32 = 0xFFFF_FFFE;
const BOOK_KEY_PATH: u32 = 0x1004;
const BOOK_TYPE_STRING: u32 = 0x0101;
const BOOK_TYPE_ARRAY: u32 = 0x0601;
const BOOK_TYPE_URL: u32 = 0x0901;
const MAX_BOOK_TOC_COUNT: usize = 64;
const MAX_BOOK_ARRAY_ITEMS: usize = 1024;

fn read_book_record(data_section: &[u8], rel: usize) -> Option<(u32, &[u8])> {
    let len_u32 = read_u32_le(data_section, rel)?;
    let data_type = read_u32_le(data_section, rel.checked_add(4)?)?;
    let len = usize::try_from(len_u32).ok()?;
    let start = rel.checked_add(8)?;
    let end = start.checked_add(len)?;
    let body = data_section.get(start..end)?;
    Some((data_type, body))
}

fn decode_book_utf8(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?.trim_end_matches('\0');
    (!text.is_empty()).then(|| text.to_owned())
}

fn read_book_string_array(data_section: &[u8], body: &[u8]) -> Vec<String> {
    let mut items = Vec::new();
    for chunk in body.as_chunks::<4>().0.iter().take(MAX_BOOK_ARRAY_ITEMS) {
        let Ok(item_rel) = usize::try_from(u32::from_le_bytes(*chunk)) else {
            continue;
        };
        if let Some((item_type, item_body)) = read_book_record(data_section, item_rel)
            && (item_type == BOOK_TYPE_STRING || item_type == BOOK_TYPE_URL)
            && let Some(s) = decode_book_utf8(item_body)
        {
            items.push(s);
        }
    }
    items
}

fn format_book_path(parts: &[String]) -> Option<String> {
    let cleaned: Vec<&str> = parts
        .iter()
        .map(|s| s.trim_matches('/'))
        .filter(|s| !s.is_empty())
        .collect();
    if cleaned.is_empty() {
        return parts.iter().any(|s| s == "/").then(|| "/".to_owned());
    }
    Some(format!("/{}", cleaned.join("/")))
}

fn parse_book_toc_entries(
    data_section: &[u8],
    entries_slice: &[u8],
    path_parts: &mut Option<Vec<String>>,
    components: &mut Vec<String>,
) {
    for entry in entries_slice.as_chunks::<12>().0 {
        let Some(raw_key) = read_u32_le(entry, 0) else {
            continue;
        };
        let key = raw_key & 0x7FFF_FFFF;
        let Some(val_rel) = read_u32_le(entry, 4).and_then(|v| usize::try_from(v).ok()) else {
            continue;
        };
        let Some((data_type, body)) = read_book_record(data_section, val_rel) else {
            continue;
        };
        match data_type {
            BOOK_TYPE_STRING | BOOK_TYPE_URL => {
                if let Some(s) = decode_book_utf8(body) {
                    if key == BOOK_KEY_PATH && path_parts.is_none() {
                        *path_parts = Some(vec![s.clone()]);
                    }
                    components.push(s);
                }
            }
            BOOK_TYPE_ARRAY => {
                let strings = read_book_string_array(data_section, body);
                if key == BOOK_KEY_PATH && path_parts.is_none() && !strings.is_empty() {
                    *path_parts = Some(strings.clone());
                }
                components.extend(strings);
            }
            // Skip binary CFData (`0x0201`), numbers, dates, booleans, dicts, and unknown types.
            _ => {}
        }
    }
}

fn parse_book_components(payload: &[u8]) -> (Option<String>, Vec<String>) {
    if payload.get(0..4) != Some(b"book") {
        return (None, Vec::new());
    }
    let Some(data_offset) = read_u32_le(payload, 12).and_then(|v| usize::try_from(v).ok()) else {
        return (None, Vec::new());
    };
    if data_offset < 16 {
        return (None, Vec::new());
    }
    let Some(data_section) = payload.get(data_offset..) else {
        return (None, Vec::new());
    };
    let Some(mut toc_rel) = read_u32_le(data_section, 0).and_then(|v| usize::try_from(v).ok())
    else {
        return (None, Vec::new());
    };

    let mut visited_tocs = Vec::new();
    let mut path_parts: Option<Vec<String>> = None;
    let mut components = Vec::new();

    while toc_rel != 0
        && visited_tocs.len() < MAX_BOOK_TOC_COUNT
        && !visited_tocs.contains(&toc_rel)
    {
        visited_tocs.push(toc_rel);
        let (Some(toc_magic), Some(next_toc_rel), Some(entry_count)) = (
            toc_rel
                .checked_add(4)
                .and_then(|o| read_u32_le(data_section, o)),
            toc_rel
                .checked_add(12)
                .and_then(|o| read_u32_le(data_section, o))
                .and_then(|v| usize::try_from(v).ok()),
            toc_rel
                .checked_add(16)
                .and_then(|o| read_u32_le(data_section, o))
                .and_then(|v| usize::try_from(v).ok()),
        ) else {
            break;
        };
        if toc_magic != BOOK_TOC_MAGIC {
            break;
        }
        let Some(entries_slice) = toc_rel
            .checked_add(20)
            .and_then(|start| {
                entry_count
                    .checked_mul(12)
                    .and_then(|bytes| start.checked_add(bytes))
                    .map(|end| (start, end))
            })
            .and_then(|(start, end)| data_section.get(start..end))
        else {
            break;
        };

        parse_book_toc_entries(
            data_section,
            entries_slice,
            &mut path_parts,
            &mut components,
        );
        toc_rel = next_toc_rel;
    }

    let extracted_path = path_parts.and_then(|parts| format_book_path(&parts));
    (extracted_path, components)
}

impl BinRead for BomaSection {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<Self> {
        let pos = reader.stream_position()?;
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if &magic != b"boma" {
            return Err(binrw::Error::BadMagic {
                pos,
                found: Box::new(magic),
            });
        }

        let header_size = u32::read_le(reader)?;
        let total_size = u32::read_le(reader)?;
        let subtype = u32::read_le(reader)?;

        let Some(unk_header_len_u32) = header_size.checked_sub(16) else {
            return Err(binrw::Error::Custom {
                pos,
                err: Box::new(MusicDbError::InvalidHeaderSize {
                    section: "boma",
                    header_size,
                    min_size: 16,
                }),
            });
        };

        let Some(payload_len_u32) = total_size.checked_sub(header_size) else {
            return Err(binrw::Error::Custom {
                pos,
                err: Box::new(MusicDbError::InvalidHeaderSize {
                    section: "boma.total_size",
                    header_size,
                    min_size: total_size,
                }),
            });
        };

        let needed = u64::from(unk_header_len_u32).saturating_add(u64::from(payload_len_u32));
        let body_pos = reader.stream_position()?;
        let remaining = remaining_stream_bytes(reader)?;
        if needed > remaining {
            return Err(binrw::Error::Custom {
                pos: body_pos,
                err: Box::new(MusicDbError::TruncatedSection {
                    section: format!("boma(subtype={subtype:#06x})"),
                    offset: body_pos,
                    needed,
                    available: remaining,
                }),
            });
        }

        let unk_header_len =
            usize::try_from(unk_header_len_u32).map_err(|_| binrw::Error::Custom {
                pos: body_pos,
                err: Box::new(MusicDbError::TruncatedSection {
                    section: format!("boma(subtype={subtype:#06x})"),
                    offset: body_pos,
                    needed,
                    available: remaining,
                }),
            })?;
        let payload_len = usize::try_from(payload_len_u32).map_err(|_| binrw::Error::Custom {
            pos: body_pos,
            err: Box::new(MusicDbError::TruncatedSection {
                section: format!("boma(subtype={subtype:#06x})"),
                offset: body_pos,
                needed,
                available: remaining,
            }),
        })?;

        let unk_16_end = TailBytes::read_exact_len(reader, unk_header_len)?;
        let payload = read_boma_payload(reader, subtype, payload_len)?;

        Ok(Self {
            header_size,
            total_size,
            subtype,
            unk_16_end: Unknown::new(unk_16_end),
            payload,
        })
    }
}

fn read_boma_payload<R: Read + Seek>(
    reader: &mut R,
    subtype: u32,
    payload_len: usize,
) -> BinResult<BomaPayload> {
    const STACK_IPFA_CAP: usize = 128;

    let payload_pos = reader.stream_position()?;

    // Fast path 1: fixed-header structured subtypes read directly from `reader` (zero Vec allocation
    // for standard sizes; forward-compatible trailing bytes preserved in `unk_tail`).
    match subtype {
        0x0001 if payload_len >= 364 => {
            let mut val = TrackNumerics::read_le(reader)?;
            if let Some(tail_len) = payload_len.checked_sub(364)
                && tail_len > 0
            {
                val.unk_tail = Unknown::new(TailBytes::read_exact_len(reader, tail_len)?);
            }
            return Ok(BomaPayload::TrackNumerics(Box::new(val)));
        }
        0x0017 if payload_len >= 52 => {
            let mut val = TrackPlaysSkips::read_le(reader)?;
            if let Some(tail_len) = payload_len.checked_sub(52)
                && tail_len > 0
            {
                val.unk_tail = Unknown::new(TailBytes::read_exact_len(reader, tail_len)?);
            }
            return Ok(BomaPayload::TrackPlaysSkips(val));
        }
        0x0024 if payload_len >= 84 => {
            let mut val = VideoInfo::read_le(reader)?;
            if let Some(tail_len) = payload_len.checked_sub(84)
                && tail_len > 0
            {
                val.unk_tail = Unknown::new(TailBytes::read_exact_len(reader, tail_len)?);
            }
            return Ok(BomaPayload::VideoInfo(Box::new(val)));
        }
        0x00CA if payload_len >= 112 => {
            let mut val = SmartPlaylistOptions::read_le(reader)?;
            if let Some(tail_len) = payload_len.checked_sub(112)
                && tail_len > 0
            {
                val.unk_tail = Unknown::new(TailBytes::read_exact_len(reader, tail_len)?);
            }
            return Ok(BomaPayload::SmartPlaylistOptions(Box::new(val)));
        }
        // Known raw binary `plma` subtypes: bypass `BomaStringSection` heuristic detection so
        // binary headers whose leading words happen to match `(1|2, payload_len - 16)` are never
        // misclassified as structured strings.
        // - `0x01F4` / `0x01F6` (20 B) / `0x01FE`: raw binary `plma` blobs.
        // - `0x01FF` (44 B): library persistent ID reference (`0..4 = 1u32`, `4..8 = 0u32`,
        //   `8..16 = library_persistent_id`, `16..24 = library_persistent_id`, `24..44 = [0; 20]`).
        0x01F4 | 0x01F6 | 0x01FE | 0x01FF => {
            let mut raw_bytes = vec![0u8; payload_len];
            reader.read_exact(&mut raw_bytes)?;
            return Ok(BomaPayload::Unknown { subtype, raw_bytes });
        }
        _ => {}
    }

    // Fast path 2: small stack-buffered `ipfa` (68 B) / `IPfa` (32 B) playlist items (zero Vec allocation).
    if matches!(subtype, 0x00CE if (52..=STACK_IPFA_CAP).contains(&payload_len))
        || matches!(subtype, 0x00DA if (24..=STACK_IPFA_CAP).contains(&payload_len))
    {
        let mut stack_buf = [0u8; STACK_IPFA_CAP];
        if let Some(slice) = stack_buf.get_mut(..payload_len) {
            reader.read_exact(slice)?;
            if let Some(structured) = parse_structured_boma_payload(subtype, slice, payload_pos)? {
                return Ok(structured);
            }
            return Ok(parse_unstructured_boma_payload(subtype, slice.to_vec()));
        }
    }

    // Rare variable-length structured payloads (`SLst`, oversized `ipfa`/`IPfa`).
    if matches!(subtype, 0x00C9 | 0x00CE | 0x00DA) {
        let mut payload_bytes = vec![0u8; payload_len];
        reader.read_exact(&mut payload_bytes)?;
        if let Some(structured) =
            parse_structured_boma_payload(subtype, &payload_bytes, payload_pos)?
        {
            return Ok(structured);
        }
        return Ok(parse_unstructured_boma_payload(subtype, payload_bytes));
    }

    read_boma_string_or_unstructured(reader, subtype, payload_len)
}

fn parse_boma_string_header(bytes: &[u8], payload_len: usize) -> Option<(u32, u32, u32, u32)> {
    if bytes.get(0..4) == Some(b"book") {
        return None;
    }
    let encoding = read_u32_le(bytes, 0)?;
    let string_byte_len = read_u32_le(bytes, 4)?;
    let parent_subtype_counter = read_u32_le(bytes, 8)?;
    let unk_12 = read_u32_le(bytes, 12)?;
    if (encoding == 2 || (encoding == 1 && string_byte_len.is_multiple_of(2)))
        && usize::try_from(string_byte_len)
            .ok()
            .and_then(|n| n.checked_add(16))
            == Some(payload_len)
    {
        Some((encoding, string_byte_len, parent_subtype_counter, unk_12))
    } else {
        None
    }
}

fn build_boma_string_section(
    header: (u32, u32, u32, u32),
    raw_string_bytes: Vec<u8>,
) -> BomaPayload {
    let (encoding, string_byte_len, parent_subtype_counter, unk_12) = header;
    let text = if encoding == 1 {
        decode_utf16le(&raw_string_bytes)
    } else {
        String::from_utf8_lossy(&raw_string_bytes).into_owned()
    };
    BomaPayload::StringSection(BomaStringSection {
        encoding,
        string_byte_len,
        parent_subtype_counter,
        unk_12: Unknown::new(unk_12),
        text,
        raw_string_bytes,
    })
}

fn read_boma_string_or_unstructured<R: Read>(
    reader: &mut R,
    subtype: u32,
    payload_len: usize,
) -> BinResult<BomaPayload> {
    // Fast path 3: peek the 16-byte `BomaStringSection` header on the stack so `raw_string_bytes`
    // is allocated once and read directly without an intermediate `payload_bytes` copy.
    if payload_len >= 16 {
        let mut head = [0u8; 16];
        reader.read_exact(&mut head)?;
        if let Some(str_header) = parse_boma_string_header(&head, payload_len) {
            let mut raw_string_bytes = vec![0u8; payload_len.saturating_sub(16)];
            reader.read_exact(&mut raw_string_bytes)?;
            return Ok(build_boma_string_section(str_header, raw_string_bytes));
        }

        let mut payload_bytes = vec![0u8; payload_len];
        if let Some(dst_head) = payload_bytes.get_mut(..16) {
            dst_head.copy_from_slice(&head);
        }
        if let Some(dst_tail) = payload_bytes.get_mut(16..) {
            reader.read_exact(dst_tail)?;
        }
        return Ok(parse_unstructured_boma_payload(subtype, payload_bytes));
    }

    let mut payload_bytes = vec![0u8; payload_len];
    reader.read_exact(&mut payload_bytes)?;
    Ok(parse_unstructured_boma_payload(subtype, payload_bytes))
}

fn parse_structured_boma_payload(
    subtype: u32,
    payload_bytes: &[u8],
    payload_offset: u64,
) -> BinResult<Option<BomaPayload>> {
    let first_four = payload_bytes.get(0..4);
    let parsed = match subtype {
        0x00CE if payload_bytes.len() >= 52 && first_four == Some(b"ipfa") => {
            let mut cur = Cursor::new(payload_bytes);
            let item = PlaylistTrackItem::read_le(&mut cur)?;
            (usize::try_from(cur.position()).ok() == Some(payload_bytes.len()))
                .then_some(BomaPayload::PlaylistItem(item))
        }
        0x00DA if payload_bytes.len() >= 24 && first_four == Some(b"IPfa") => {
            let mut cur = Cursor::new(payload_bytes);
            let item = CompactPlaylistTrackItem::read_le(&mut cur)?;
            (usize::try_from(cur.position()).ok() == Some(payload_bytes.len()))
                .then_some(BomaPayload::PlaylistItemCompact(item))
        }
        0x00C9 => {
            if payload_bytes.len() < 136 && (payload_bytes.len() < 4 || first_four == Some(b"SLst"))
            {
                return Err(binrw::Error::Custom {
                    pos: payload_offset,
                    err: Box::new(MusicDbError::TruncatedSection {
                        section: "SLst".to_string(),
                        offset: payload_offset,
                        needed: 136,
                        available: u64::try_from(payload_bytes.len()).unwrap_or(u64::MAX),
                    }),
                });
            }
            if first_four == Some(b"SLst") {
                let mut cur = Cursor::new(payload_bytes);
                let slst = SlstSection::read_be(&mut cur)?;
                let consumed = cur.position();
                let expected_len = u64::try_from(payload_bytes.len()).unwrap_or(u64::MAX);
                if consumed != expected_len {
                    return Err(binrw::Error::Custom {
                        pos: payload_offset.saturating_add(consumed),
                        err: Box::new(MusicDbError::SectionBoundaryMismatch {
                            section: "boma(0x00c9:SLst)".to_string(),
                            expected: payload_offset.saturating_add(expected_len),
                            actual: payload_offset.saturating_add(consumed),
                        }),
                    });
                }
                Some(BomaPayload::SmartPlaylistRules(Box::new(slst)))
            } else {
                None
            }
        }
        _ => None,
    };
    Ok(parsed)
}

fn parse_unstructured_boma_payload(subtype: u32, payload_bytes: Vec<u8>) -> BomaPayload {
    // Check if it's a macOS `book` structure
    if payload_bytes.len() >= 8 && payload_bytes.get(0..4) == Some(b"book") {
        let (extracted_path, components) = parse_book_components(&payload_bytes);
        return BomaPayload::Book {
            extracted_path,
            components,
            raw_bytes: payload_bytes,
        };
    }

    if let Some(str_header) = parse_boma_string_header(&payload_bytes, payload_bytes.len())
        && let Some(raw_slice) = payload_bytes.get(16..)
    {
        return build_boma_string_section(str_header, raw_slice.to_vec());
    }

    if payload_bytes.starts_with(b"bplist00") {
        return BomaPayload::BinaryPlist {
            subtype,
            raw_bytes: payload_bytes,
        };
    }

    // Check known Raw String subtypes
    match subtype {
        0x0036 | 0x0038 | 0x00CD | 0x0192 | 0x01FC | 0x02BC | 0x03CC => {
            let text = String::from_utf8_lossy(&payload_bytes).into_owned();
            return BomaPayload::RawStringUtf8 {
                text,
                raw_bytes: payload_bytes,
            };
        }
        0x01FD | 0x0200 if payload_bytes.len().is_multiple_of(2) => {
            let text = decode_utf16le(&payload_bytes);
            return BomaPayload::RawStringUtf16Le {
                text,
                raw_bytes: payload_bytes,
            };
        }
        _ => {}
    }

    BomaPayload::Unknown {
        subtype,
        raw_bytes: payload_bytes,
    }
}

fn decode_utf16le(bytes: &[u8]) -> String {
    let chunks = bytes.as_chunks::<2>().0;
    let mut out = String::with_capacity(chunks.len());
    // Fast path for ASCII UTF-16LE prefixes (covers the vast majority of music metadata tags).
    let ascii_prefix = chunks
        .iter()
        .take_while(|&&[lo, hi]| hi == 0 && lo < 0x80)
        .count();
    for &[lo, _] in chunks.get(..ascii_prefix).unwrap_or(&[]) {
        out.push(char::from(lo));
    }
    if let Some(rest) = chunks.get(ascii_prefix..)
        && !rest.is_empty()
    {
        for r in char::decode_utf16(rest.iter().copied().map(u16::from_le_bytes)) {
            out.push(r.unwrap_or(char::REPLACEMENT_CHARACTER));
        }
    }
    out
}

impl BinWrite for BomaSection {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<()> {
        writer.write_all(b"boma")?;
        self.header_size.write_le(writer)?;
        self.total_size.write_le(writer)?;
        self.subtype.write_le(writer)?;
        writer.write_all(&self.unk_16_end)?;

        match &self.payload {
            BomaPayload::TrackNumerics(n) => n.write_le(writer)?,
            BomaPayload::TrackPlaysSkips(p) => p.write_le(writer)?,
            BomaPayload::VideoInfo(v) => v.write_le(writer)?,
            BomaPayload::PlaylistItem(ipfa) => ipfa.write_le(writer)?,
            BomaPayload::PlaylistItemCompact(ipfa) => ipfa.write_le(writer)?,
            BomaPayload::SmartPlaylistOptions(opts) => opts.write_le(writer)?,
            BomaPayload::SmartPlaylistRules(slst) => slst.write_be(writer)?,
            BomaPayload::StringSection(s) => {
                s.encoding.write_le(writer)?;
                s.string_byte_len.write_le(writer)?;
                s.parent_subtype_counter.write_le(writer)?;
                s.unk_12.write_le(writer)?;
                writer.write_all(&s.raw_string_bytes)?;
            }
            BomaPayload::RawStringUtf8 { raw_bytes, .. }
            | BomaPayload::RawStringUtf16Le { raw_bytes, .. }
            | BomaPayload::Book { raw_bytes, .. }
            | BomaPayload::BinaryPlist { raw_bytes, .. }
            | BomaPayload::Unknown { raw_bytes, .. } => writer.write_all(raw_bytes)?,
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_utf16le_handles_ascii_unicode_and_unpaired_surrogates() {
        let to_utf16le =
            |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(u16::to_le_bytes).collect() };

        assert_eq!(decode_utf16le(&[]), "");
        assert_eq!(
            decode_utf16le(&to_utf16le("Moving Pictures")),
            "Moving Pictures"
        );
        assert_eq!(
            decode_utf16le(&to_utf16le("La Villa Strangiato — Rush 🎸")),
            "La Villa Strangiato — Rush 🎸"
        );
        // Unpaired high surrogate (0xD800) followed by 'A'
        let malformed = [0x00, 0xD8, 0x41, 0x00];
        assert_eq!(decode_utf16le(&malformed), "\u{FFFD}A");
    }

    #[test]
    fn format_23_binary_plists_parse_as_binary_plist_and_roundtrip() {
        let mut dict = plist::Dictionary::new();
        dict.insert(
            "artwork-url".to_owned(),
            plist::Value::String("https://example.invalid/rush/moving-pictures.jpg".to_owned()),
        );
        let mut bplist_bytes = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_binary(&mut bplist_bytes)
            .expect("encode binary plist");
        assert!(bplist_bytes.starts_with(b"bplist00"));

        let wrap_boma = |subtype: u32, payload: &[u8]| -> Vec<u8> {
            let total_size = u32::try_from(20 + payload.len()).expect("total_size");
            let mut buf = Vec::with_capacity(20 + payload.len());
            buf.extend_from_slice(b"boma");
            buf.extend_from_slice(&20u32.to_le_bytes());
            buf.extend_from_slice(&total_size.to_le_bytes());
            buf.extend_from_slice(&subtype.to_le_bytes());
            buf.extend_from_slice(&[0u8; 4]);
            buf.extend_from_slice(payload);
            buf
        };

        for subtype in [0x0036, 0x0038, 0x00CD, 0x0192] {
            let raw = wrap_boma(subtype, &bplist_bytes);
            let parsed = BomaSection::read_le(&mut Cursor::new(&raw)).expect("parse boma");
            assert!(
                matches!(
                    &parsed.payload,
                    BomaPayload::BinaryPlist { subtype: s, raw_bytes }
                        if *s == subtype && raw_bytes == &bplist_bytes
                ),
                "subtype {subtype:#06x} with bplist00 payload should parse as BinaryPlist, got {:?}",
                parsed.payload
            );
            let mut out = Cursor::new(Vec::new());
            parsed.write_le(&mut out).expect("write boma");
            assert_eq!(out.into_inner(), raw);
        }

        // Legacy XML plist on 0x0036 must still parse as RawStringUtf8.
        let xml =
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict/></plist>";
        let raw_xml = wrap_boma(0x0036, xml);
        let parsed_xml = BomaSection::read_le(&mut Cursor::new(&raw_xml)).expect("parse xml boma");
        assert!(matches!(
            parsed_xml.payload,
            BomaPayload::RawStringUtf8 { .. }
        ));
    }

    #[test]
    fn odd_length_utf16le_header_is_not_parsed_as_string_section() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u32.to_le_bytes()); // encoding = 1 (UTF-16LE)
        payload.extend_from_slice(&5u32.to_le_bytes()); // odd byte length (invalid UTF-16LE)
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(b"Rush!");

        let total_size = u32::try_from(20 + payload.len()).expect("total_size");
        let mut raw = Vec::with_capacity(20 + payload.len());
        raw.extend_from_slice(b"boma");
        raw.extend_from_slice(&20u32.to_le_bytes());
        raw.extend_from_slice(&total_size.to_le_bytes());
        raw.extend_from_slice(&0x0099u32.to_le_bytes());
        raw.extend_from_slice(&[0u8; 4]);
        raw.extend_from_slice(&payload);

        let parsed = BomaSection::read_le(&mut Cursor::new(&raw)).expect("parse boma");
        assert!(matches!(parsed.payload, BomaPayload::Unknown { .. }));
        let mut out = Cursor::new(Vec::new());
        parsed.write_le(&mut out).expect("round-trip boma");
        assert_eq!(out.into_inner(), raw);
    }

    fn append_book_record(data: &mut Vec<u8>, data_type: u32, payload: &[u8]) -> u32 {
        let rel = u32::try_from(data.len()).expect("rel fits u32");
        let len = u32::try_from(payload.len()).expect("len fits u32");
        data.extend_from_slice(&len.to_le_bytes());
        data.extend_from_slice(&data_type.to_le_bytes());
        data.extend_from_slice(payload);
        let pad = (4 - (payload.len() % 4)) % 4;
        data.resize(data.len() + pad, 0);
        rel
    }

    fn build_synthetic_rush_bookmark() -> Vec<u8> {
        let mut data = Vec::new();
        // Reserve 4 bytes at data_offset+0 for first_toc_rel
        data.extend_from_slice(&0u32.to_le_bytes());

        let off_users = append_book_record(&mut data, 0x0101, b"Users");
        let off_geddy = append_book_record(&mut data, 0x0101, b"geddy");
        // Stored once in data pool and referenced twice by the 0x1004 path array!
        let off_music = append_book_record(&mut data, 0x0101, b"Music");
        let off_rush = append_book_record(&mut data, 0x0101, b"Rush");
        let off_uuid_dir =
            append_book_record(&mut data, 0x0101, b"12345678-1234-1234-1234-1234567890ab");
        let off_song = append_book_record(&mut data, 0x0101, b"01 Tom Sawyer.m4a");
        let off_vol_name = append_book_record(&mut data, 0x0101, b"Macintosh HD");
        let off_vol_url = append_book_record(&mut data, 0x0901, b"file:///");
        let off_bin_blob =
            append_book_record(&mut data, 0x0201, &[0xFF, 0xFE, 0x80, 0x81, 0x00, 0x42]);

        let mut path_array_bytes = Vec::new();
        for off in [
            off_users,
            off_geddy,
            off_music,
            off_music,
            off_rush,
            off_uuid_dir,
            off_song,
        ] {
            path_array_bytes.extend_from_slice(&off.to_le_bytes());
        }
        let off_path_array = append_book_record(&mut data, 0x0601, &path_array_bytes);

        let toc_rel = u32::try_from(data.len()).expect("toc_rel fits u32");
        data[0..4].copy_from_slice(&toc_rel.to_le_bytes());

        let entries: &[(u32, u32)] = &[
            (0x1004, off_path_array),
            (0x2005, off_vol_url),
            (0x2010, off_vol_name),
            (0xF020, off_bin_blob),
        ];
        let toc_size = u32::try_from(12 + entries.len() * 12).expect("toc_size");
        data.extend_from_slice(&toc_size.to_le_bytes());
        data.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes()); // TOC magic
        data.extend_from_slice(&1u32.to_le_bytes()); // toc_id
        data.extend_from_slice(&0u32.to_le_bytes()); // next_toc_rel = 0
        data.extend_from_slice(&u32::try_from(entries.len()).unwrap().to_le_bytes());
        for &(key, val_rel) in entries {
            data.extend_from_slice(&key.to_le_bytes());
            data.extend_from_slice(&val_rel.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());
        }

        let header_size = 48usize;
        let total_len = u32::try_from(header_size + data.len()).expect("total_len");
        let mut blob = vec![0u8; header_size];
        blob[0..4].copy_from_slice(b"book");
        blob[4..8].copy_from_slice(&total_len.to_le_bytes());
        blob[8..12].copy_from_slice(&0x1004_0000u32.to_le_bytes());
        blob[12..16].copy_from_slice(&u32::try_from(header_size).unwrap().to_le_bytes());
        blob.extend_from_slice(&data);
        blob
    }

    #[test]
    fn book_toc_parser_extracts_path_with_deduped_components_and_uuid_folder() {
        let blob = build_synthetic_rush_bookmark();
        let total_size = u32::try_from(20 + blob.len()).expect("total_size");
        let mut raw = Vec::with_capacity(20 + blob.len());
        raw.extend_from_slice(b"boma");
        raw.extend_from_slice(&20u32.to_le_bytes());
        raw.extend_from_slice(&total_size.to_le_bytes());
        raw.extend_from_slice(&0x0042u32.to_le_bytes());
        raw.extend_from_slice(&[0u8; 4]);
        raw.extend_from_slice(&blob);

        let parsed = BomaSection::read_le(&mut Cursor::new(&raw)).expect("parse book boma");
        let BomaPayload::Book {
            extracted_path,
            components,
            raw_bytes,
        } = &parsed.payload
        else {
            panic!("expected BomaPayload::Book, got {:?}", parsed.payload);
        };
        assert_eq!(
            extracted_path.as_deref(),
            Some(
                "/Users/geddy/Music/Music/Rush/12345678-1234-1234-1234-1234567890ab/01 Tom Sawyer.m4a"
            )
        );
        assert!(
            components.iter().all(|c| !c.contains('\u{FFFD}')),
            "binary 0x0201 CFData must not leak replacement chars into components: {components:?}"
        );
        assert!(
            components.iter().filter(|c| c.as_str() == "Music").count() == 2,
            "deduplicated path component 'Music' should appear twice: {components:?}"
        );
        assert_eq!(raw_bytes, &blob);

        let mut out = Cursor::new(Vec::new());
        parsed.write_le(&mut out).expect("round-trip book boma");
        assert_eq!(out.into_inner(), raw);
    }

    #[test]
    fn book_toc_parser_handles_malformed_and_cyclic_inputs_without_panicking() {
        // 1. Minimal truncated header
        assert_eq!(parse_book_components(b"book\0\0\0\0"), (None, Vec::new()));

        // 2. Out-of-bounds data_offset
        let mut oob_data = vec![0u8; 20];
        oob_data[0..4].copy_from_slice(b"book");
        oob_data[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_book_components(&oob_data), (None, Vec::new()));

        // 3. Self-referential cyclic TOC with huge entry_count
        let mut cyclic = vec![0u8; 48];
        cyclic[0..4].copy_from_slice(b"book");
        cyclic[12..16].copy_from_slice(&48u32.to_le_bytes());
        cyclic.extend_from_slice(&4u32.to_le_bytes()); // first_toc_rel = 4
        cyclic.extend_from_slice(&12u32.to_le_bytes()); // toc_size
        cyclic.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes()); // magic
        cyclic.extend_from_slice(&0u32.to_le_bytes()); // toc_id
        cyclic.extend_from_slice(&4u32.to_le_bytes()); // next_toc_rel = 4 (cycle!)
        cyclic.extend_from_slice(&u32::MAX.to_le_bytes()); // absurd entry_count
        assert_eq!(parse_book_components(&cyclic), (None, Vec::new()));
    }
}
