//! `boma` (Binary Object / Metadata Container) section and payload structures.

use std::io::{Cursor, Read, Seek, Write};

use binrw::{BinRead, BinResult, BinWrite, Endian};

use super::library_master::{PlmaSub1F6, PlmaSub1FF};
use super::playlist::{
    CompactPlaylistTrackItem, PlaylistTrackItem, SlstSection, SmartPlaylistOptions,
};
use super::remaining_stream_bytes;
use super::track::{TrackNumerics, TrackPlaysSkips, VideoInfo};
use super::unknown::{Unknown, UnknownVisitor, VisitUnknowns, visit_unknowns};
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
    /// Offset 16..`header_size`: Remaining header bytes (4 bytes `16..20` when `header_size == 20`).
    pub unk_16_end: Unknown<Vec<u8>>,
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
            BomaPayload::Plma1F6(s) => s.visit_unknowns("plma::0x01F6", visitor),
            BomaPayload::Plma1FF(s) => s.visit_unknowns("plma::0x01FF", visitor),
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
    /// Subtype `0x01F6` under `plma` (20 bytes).
    Plma1F6(PlmaSub1F6),
    /// Subtype `0x01FF` under `plma` (44 bytes).
    Plma1FF(PlmaSub1FF),
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
        /// Extracted filesystem path from `0x0101` path components, if present.
        extracted_path: Option<String>,
        /// All decoded string components inside the bookmark record.
        components: Vec<String>,
        /// Exact raw bookmark payload bytes.
        raw_bytes: Vec<u8>,
    },
    /// Binary property list (`bplist00`) payload (e.g., `0x0135` album motion artwork metadata).
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
        0x01F6 => "plma_1f6",
        0x01F8 => "media_folder_uri",
        0x01FC => "imported_itl_path",
        0x01FD => "media_folder_path_1fd",
        0x01FE => "unknown_hex_1fe",
        0x01FF => "plma_1ff",
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

fn parse_book_components(payload: &[u8]) -> (Option<String>, Vec<String>) {
    let mut all_parts = Vec::new();
    let mut path_parts = Vec::new();
    let mut pos = 4usize; // skip "book"
    while let (Some(str_len_u32), Some(marker)) = (
        read_u32_le(payload, pos),
        read_u32_le(payload, pos.saturating_add(4)),
    ) {
        let str_len = usize::try_from(str_len_u32).unwrap_or(usize::MAX);
        let str_start = pos.saturating_add(8);
        let str_end = str_start.saturating_add(str_len);
        if (marker == 0x0101 || marker == 0x0901 || marker == 0x0201)
            && (1..4096).contains(&str_len)
            && let Some(str_bytes) = payload.get(str_start..str_end)
        {
            let s = String::from_utf8_lossy(str_bytes)
                .trim_end_matches('\0')
                .to_string();
            if !s.is_empty() {
                all_parts.push(s.clone());
                if marker == 0x0101 {
                    let is_uuid = s.len() == 36 && s.chars().filter(|&c| c == '-').count() == 4;
                    let is_protocol = s.starts_with("file:///");
                    let is_separator = s == "/";
                    if !is_uuid && !is_protocol && !is_separator {
                        path_parts.push(s);
                    }
                }
            }
            let padding = (4 - (str_len % 4)) % 4;
            pos = str_end.saturating_add(padding);
        } else {
            pos = pos.saturating_add(4);
        }
    }
    let extracted_path = if path_parts.is_empty() {
        None
    } else {
        Some(path_parts.join("/"))
    };
    (extracted_path, all_parts)
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

        let mut unk_16_end = vec![0u8; unk_header_len];
        reader.read_exact(&mut unk_16_end)?;

        let mut payload_bytes = vec![0u8; payload_len];
        reader.read_exact(&mut payload_bytes)?;

        let payload = parse_boma_payload(subtype, payload_bytes)?;

        Ok(Self {
            header_size,
            total_size,
            subtype,
            unk_16_end: Unknown::new(unk_16_end),
            payload,
        })
    }
}

fn parse_structured_boma_payload(
    subtype: u32,
    payload_bytes: &[u8],
) -> BinResult<Option<BomaPayload>> {
    let first_four = payload_bytes.get(0..4);
    let parsed = match subtype {
        0x0001 if payload_bytes.len() == 364 => {
            let mut cur = Cursor::new(payload_bytes);
            Some(BomaPayload::TrackNumerics(Box::new(
                TrackNumerics::read_le(&mut cur)?,
            )))
        }
        0x0017 if payload_bytes.len() == 52 => {
            let mut cur = Cursor::new(payload_bytes);
            Some(BomaPayload::TrackPlaysSkips(TrackPlaysSkips::read_le(
                &mut cur,
            )?))
        }
        0x0024 if payload_bytes.len() == 84 => {
            let mut cur = Cursor::new(payload_bytes);
            Some(BomaPayload::VideoInfo(Box::new(VideoInfo::read_le(
                &mut cur,
            )?)))
        }
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
        0x00CA if payload_bytes.len() == 112 => {
            let mut cur = Cursor::new(payload_bytes);
            Some(BomaPayload::SmartPlaylistOptions(Box::new(
                SmartPlaylistOptions::read_le(&mut cur)?,
            )))
        }
        0x00C9 if payload_bytes.len() >= 136 && first_four == Some(b"SLst") => {
            let mut cur = Cursor::new(payload_bytes);
            if let Ok(slst) = SlstSection::read_be(&mut cur)
                && usize::try_from(cur.position()).ok() == Some(payload_bytes.len())
            {
                Some(BomaPayload::SmartPlaylistRules(Box::new(slst)))
            } else {
                None
            }
        }
        0x01F6 if payload_bytes.len() == 20 => {
            let mut cur = Cursor::new(payload_bytes);
            Some(BomaPayload::Plma1F6(PlmaSub1F6::read_le(&mut cur)?))
        }
        0x01FF if payload_bytes.len() == 44 => {
            let mut cur = Cursor::new(payload_bytes);
            Some(BomaPayload::Plma1FF(PlmaSub1FF::read_le(&mut cur)?))
        }
        _ => None,
    };
    Ok(parsed)
}

fn parse_boma_payload(subtype: u32, payload_bytes: Vec<u8>) -> BinResult<BomaPayload> {
    if let Some(structured) = parse_structured_boma_payload(subtype, &payload_bytes)? {
        return Ok(structured);
    }

    // Check if it's a macOS `book` structure
    if payload_bytes.len() >= 8 && payload_bytes.get(0..4) == Some(b"book") {
        let (extracted_path, components) = parse_book_components(&payload_bytes);
        return Ok(BomaPayload::Book {
            extracted_path,
            components,
            raw_bytes: payload_bytes,
        });
    }

    // Check if it's a structured `BomaStringSection` (16-byte header where encoding in [1, 2] and byte_len + 16 == payload_bytes.len())
    if let (Some(encoding), Some(string_byte_len), Some(parent_subtype_counter), Some(unk_12)) = (
        read_u32_le(&payload_bytes, 0),
        read_u32_le(&payload_bytes, 4),
        read_u32_le(&payload_bytes, 8),
        read_u32_le(&payload_bytes, 12),
    ) && (encoding == 1 || encoding == 2)
        && usize::try_from(string_byte_len)
            .ok()
            .and_then(|n| n.checked_add(16))
            == Some(payload_bytes.len())
        && let Some(raw_slice) = payload_bytes.get(16..)
    {
        let raw_string_bytes = raw_slice.to_vec();
        let text = if encoding == 1 {
            decode_utf16le(&raw_string_bytes)
        } else {
            String::from_utf8_lossy(&raw_string_bytes).into_owned()
        };

        return Ok(BomaPayload::StringSection(BomaStringSection {
            encoding,
            string_byte_len,
            parent_subtype_counter,
            unk_12: Unknown::new(unk_12),
            text,
            raw_string_bytes,
        }));
    }

    // Check known Raw String subtypes
    match subtype {
        0x0036 | 0x0038 | 0x00CD | 0x0192 | 0x01FC | 0x02BC | 0x03CC => {
            let text = String::from_utf8_lossy(&payload_bytes).into_owned();
            return Ok(BomaPayload::RawStringUtf8 {
                text,
                raw_bytes: payload_bytes,
            });
        }
        0x01FD | 0x0200 if payload_bytes.len().is_multiple_of(2) => {
            let text = decode_utf16le(&payload_bytes);
            return Ok(BomaPayload::RawStringUtf16Le {
                text,
                raw_bytes: payload_bytes,
            });
        }
        _ => {}
    }

    if payload_bytes.starts_with(b"bplist00") {
        return Ok(BomaPayload::BinaryPlist {
            subtype,
            raw_bytes: payload_bytes,
        });
    }

    Ok(BomaPayload::Unknown {
        subtype,
        raw_bytes: payload_bytes,
    })
}

fn decode_utf16le(bytes: &[u8]) -> String {
    char::decode_utf16(
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .copied()
            .map(u16::from_le_bytes),
    )
    .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
    .collect()
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
            BomaPayload::Plma1F6(s) => s.write_le(writer)?,
            BomaPayload::Plma1FF(s) => s.write_le(writer)?,
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
