//! Layer-1 lossless binary AST for Apple Music `.musicdb` files (`hfma`, `hsma`, `plma`, `lama`, `lAma`, `ltma`, `lPma`, `Lsma`, `boma`).

/// `lama` and `iama` album list and album item binary structures.
pub mod album;
/// `lAma` and `iAma` artist list and artist item binary structures.
pub mod artist;
pub mod boma;
pub mod header;
/// `plma` library master section binary structures.
pub mod library_master;
/// `lPma`, `lpma`, `ipfa`, `IPfa`, and `SLst` playlist binary structures.
pub mod playlist;
/// `Lsma` and `isma` social profile binary structures.
pub mod social;
/// `ltma`, `itma`, and `TrackNumerics` track binary structures.
pub mod track;

use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use binrw::{BinRead, BinResult, BinWrite, Endian, binrw};

use crate::crypto::{DEFAULT_MUSICDB_KEY, compress_and_encrypt_bytes, load_raw_musicdb};
use crate::error::{MusicDbError, Result};

pub use album::{AlbumItem, AlbumItemHeader, AlbumListHeader, AlbumListSection};
pub use artist::{ArtistItem, ArtistItemHeader, ArtistListHeader, ArtistListSection};
pub use boma::{BomaPayload, BomaSection, BomaStringSection};
pub use header::{HfmaHeader, HsmaHeader};
pub use library_master::{LibraryMasterHeader, LibraryMasterSection, PlmaSub1F6, PlmaSub1FF};
pub use playlist::{
    CompactPlaylistTrackItem, PlaylistItem, PlaylistItemHeader, PlaylistListHeader,
    PlaylistListSection, PlaylistTrackItem, SlstSection, SmartPlaylistOptions, SmartPlaylistRule,
    SmartPlaylistRuleArguments, SmartRuleChild,
};
pub use social::{
    SocialProfileItem, SocialProfileItemHeader, SocialProfileListHeader, SocialProfileListSection,
};
pub use track::{
    TrackItem, TrackItemHeader, TrackListHeader, TrackListSection, TrackNumerics, TrackPlaysSkips,
    VideoInfo,
};

/// Returns the number of bytes remaining from the current cursor position to the end of `reader`.
pub(crate) fn remaining_stream_bytes<R: Seek>(reader: &mut R) -> BinResult<u64> {
    let pos = reader.stream_position()?;
    let end = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(pos))?;
    Ok(end.saturating_sub(pos))
}

/// Validates that `header_size >= min_header_size` and reads the remaining `header_size - min_header_size`
/// header bytes after checking that enough bytes remain in `reader`.
pub(crate) fn read_header_tail<R: Read + Seek>(
    reader: &mut R,
    _endian: Endian,
    (header_size, min_header_size, section): (u32, u32, &'static str),
) -> BinResult<Vec<u8>> {
    let pos = reader.stream_position()?;
    let Some(tail_len_u32) = header_size.checked_sub(min_header_size) else {
        return Err(binrw::Error::Custom {
            pos,
            err: Box::new(MusicDbError::InvalidHeaderSize {
                section,
                header_size,
                min_size: min_header_size,
            }),
        });
    };
    let tail_len_u64 = u64::from(tail_len_u32);
    let remaining = remaining_stream_bytes(reader)?;
    if tail_len_u64 > remaining {
        return Err(binrw::Error::Custom {
            pos,
            err: Box::new(MusicDbError::TruncatedSection {
                section: section.to_string(),
                offset: pos,
                needed: tail_len_u64,
                available: remaining,
            }),
        });
    }
    let tail_len = usize::try_from(tail_len_u32).map_err(|_| binrw::Error::Custom {
        pos,
        err: Box::new(MusicDbError::TruncatedSection {
            section: section.to_string(),
            offset: pos,
            needed: tail_len_u64,
            available: remaining,
        }),
    })?;
    let mut buf = vec![0u8; tail_len];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

/// Reads `count` items of type `T`, verifying that `count * min_item_bytes <= remaining_stream_bytes`
/// before allocating the vector.
pub(crate) fn read_bounded_vec<T, R>(
    reader: &mut R,
    endian: Endian,
    (count, min_item_bytes, section): (u32, u64, &'static str),
) -> BinResult<Vec<T>>
where
    T: for<'a> BinRead<Args<'a> = ()>,
    R: Read + Seek,
{
    let pos = reader.stream_position()?;
    let remaining = remaining_stream_bytes(reader)?;
    let min_needed = u64::from(count)
        .checked_mul(min_item_bytes)
        .ok_or_else(|| binrw::Error::Custom {
            pos,
            err: Box::new(MusicDbError::TruncatedSection {
                section: section.to_string(),
                offset: pos,
                needed: u64::MAX,
                available: remaining,
            }),
        })?;
    if min_needed > remaining {
        return Err(binrw::Error::Custom {
            pos,
            err: Box::new(MusicDbError::TruncatedSection {
                section: section.to_string(),
                offset: pos,
                needed: min_needed,
                available: remaining,
            }),
        });
    }
    let cap = usize::try_from(count).map_err(|_| binrw::Error::Custom {
        pos,
        err: Box::new(MusicDbError::TruncatedSection {
            section: section.to_string(),
            offset: pos,
            needed: min_needed,
            available: remaining,
        }),
    })?;
    let mut items = Vec::with_capacity(cap);
    for _ in 0..count {
        items.push(T::read_options(reader, endian, ())?);
    }
    Ok(items)
}

/// Serde helper for fixed-size byte arrays larger than 32 bytes, encoded as lowercase hex strings.
#[cfg(feature = "serde")]
pub(crate) mod hex_array {
    use serde::de::Error;
    use serde::{Deserializer, Serializer};

    /// Serializes a fixed-size byte array as a hex string.
    ///
    /// # Errors
    /// Returns a serializer error if writing the string fails.
    pub(crate) fn serialize<S, const N: usize>(
        bytes: &[u8; N],
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    /// Deserializes a fixed-size byte array from a hex string.
    ///
    /// # Errors
    /// Returns a deserializer error if the input is not valid hex or does not decode to `N` bytes.
    pub(crate) fn deserialize<'de, D, const N: usize>(
        deserializer: D,
    ) -> std::result::Result<[u8; N], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        let vec = hex::decode(&s).map_err(D::Error::custom)?;
        vec.try_into()
            .map_err(|v: Vec<u8>| D::Error::custom(format!("expected {N} bytes, got {}", v.len())))
    }
}

/// Padding / footer section (`LPma`, subtype 17 of `hsma`).
#[binrw]
#[brw(little, magic = b"LPma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PaddingSection {
    /// Offset 4: Section size in bytes (typically 96, minimum 8).
    pub section_size: u32,
    /// Offset 8..`section_size`: Remaining bytes (offset 12 is typically 6).
    #[br(parse_with = read_header_tail, args(section_size, 8, "LPma"))]
    pub raw_bytes: Vec<u8>,
}

/// A major `hsma` section boundary and its parsed child section tree.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HsmaSection {
    /// The `hsma` boundary header (`56` bytes in standard libraries).
    pub header: HsmaHeader,
    /// The parsed, unknown, or skipped child section inside this `hsma` boundary.
    pub child: HsmaChild,
}

/// Child section inside an `hsma` boundary.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HsmaChild {
    /// Subtype 3: Inner `hfma` header.
    InnerHfma(HfmaHeader),
    /// Subtype 6: `plma` Library Master.
    LibraryMaster(LibraryMasterSection),
    /// Subtype 4: `lama` Album List.
    AlbumList(AlbumListSection),
    /// Subtype 5: `lAma` Artist List.
    ArtistList(ArtistListSection),
    /// Subtype 1: `ltma` Track List.
    TrackList(TrackListSection),
    /// Subtype 2: `lPma` Playlist List.
    PlaylistList(PlaylistListSection),
    /// Subtype 15: `Lsma` Social Profile List.
    SocialProfileList(SocialProfileListSection),
    /// Subtype 17: `LPma` Padding / Footer.
    Padding(PaddingSection),
    /// Any other / unknown `hsma` child (e.g. `Library Preferences.musicdb` or future subtype), preserved losslessly.
    Unknown {
        /// Section subtype from [`HsmaHeader::subtype`].
        subtype: u32,
        /// First 4 magic bytes of the child payload (or zeros if shorter than 4 bytes).
        signature: [u8; 4],
        /// Full raw bytes of the child section (`total_size - header_size`).
        raw_bytes: Vec<u8>,
    },
    /// A section intentionally skipped during [`RawLibraryFile::from_raw_bytes_filtered`],
    /// retaining its raw bytes so [`RawLibraryFile::to_raw_bytes`] remains 100% lossless.
    Skipped {
        /// Section subtype from [`HsmaHeader::subtype`].
        subtype: u32,
        /// Full unparsed raw bytes of the skipped child section (`total_size - header_size`).
        raw_bytes: Vec<u8>,
    },
}

impl HsmaSection {
    /// Reads an `HsmaSection` from `reader`, parsing its child section when `include_subtype(subtype)`
    /// returns `true`, or preserving the child bytes inside [`HsmaChild::Skipped`] when `false`.
    pub(crate) fn read_filtered<R, F>(reader: &mut R, include_subtype: &F) -> BinResult<Self>
    where
        R: Read + Seek,
        F: Fn(u32) -> bool,
    {
        let start_pos = reader.stream_position()?;
        let header = HsmaHeader::read_le(reader)?;
        let subtype = header.subtype;
        let total_size = header.effective_total_size();
        let header_size_u64 = u64::from(header.header_size);
        let child_pos = reader.stream_position()?;

        let Some(child_len_u64) = total_size.checked_sub(header_size_u64) else {
            return Err(binrw::Error::Custom {
                pos: start_pos,
                err: Box::new(MusicDbError::InvalidHeaderSize {
                    section: "hsma.total_size",
                    header_size: header.header_size,
                    min_size: u32::try_from(total_size).unwrap_or(u32::MAX),
                }),
            });
        };

        let remaining = remaining_stream_bytes(reader)?;
        if child_len_u64 > remaining {
            return Err(binrw::Error::Custom {
                pos: child_pos,
                err: Box::new(MusicDbError::TruncatedSection {
                    section: format!("hsma(subtype={subtype})"),
                    offset: child_pos,
                    needed: child_len_u64,
                    available: remaining,
                }),
            });
        }

        let child_len = usize::try_from(child_len_u64).map_err(|_| binrw::Error::Custom {
            pos: child_pos,
            err: Box::new(MusicDbError::TruncatedSection {
                section: format!("hsma(subtype={subtype})"),
                offset: child_pos,
                needed: child_len_u64,
                available: remaining,
            }),
        })?;

        let mut child_bytes = vec![0u8; child_len];
        reader.read_exact(&mut child_bytes)?;

        if !include_subtype(subtype) {
            return Ok(Self {
                header,
                child: HsmaChild::Skipped {
                    subtype,
                    raw_bytes: child_bytes,
                },
            });
        }

        let expected_end = start_pos.saturating_add(total_size);
        let mut cur = Cursor::new(&child_bytes);
        let Some(child) = Self::parse_known_child(subtype, &child_bytes, &mut cur)? else {
            let mut sig = [0u8; 4];
            if let Some(prefix) = child_bytes.get(0..4) {
                sig.copy_from_slice(prefix);
            }
            return Ok(Self {
                header,
                child: HsmaChild::Unknown {
                    subtype,
                    signature: sig,
                    raw_bytes: child_bytes,
                },
            });
        };

        // Ensure the parsed child consumed every single byte of `child_bytes`!
        let consumed = cur.position();
        if consumed != child_len_u64 {
            let actual = start_pos
                .saturating_add(header_size_u64)
                .saturating_add(consumed);
            return Err(binrw::Error::Custom {
                pos: actual,
                err: Box::new(MusicDbError::SectionBoundaryMismatch {
                    section: format!("hsma(subtype={subtype})"),
                    expected: expected_end,
                    actual,
                }),
            });
        }

        Ok(Self { header, child })
    }

    fn parse_known_child(
        subtype: u32,
        child_bytes: &[u8],
        cur: &mut Cursor<&Vec<u8>>,
    ) -> BinResult<Option<HsmaChild>> {
        let first_four = child_bytes.get(0..4);
        let parsed = match (subtype, first_four) {
            (3, Some(b"hfma")) if child_bytes.len() >= 160 => {
                Some(HsmaChild::InnerHfma(HfmaHeader::read_le(cur)?))
            }
            (6, Some(b"plma")) if child_bytes.len() >= 160 => Some(HsmaChild::LibraryMaster(
                LibraryMasterSection::read_le(cur)?,
            )),
            (4, Some(b"lama")) if child_bytes.len() >= 12 => {
                Some(HsmaChild::AlbumList(AlbumListSection::read_le(cur)?))
            }
            (5, Some(b"lAma")) if child_bytes.len() >= 12 => {
                Some(HsmaChild::ArtistList(ArtistListSection::read_le(cur)?))
            }
            (1, Some(b"ltma")) if child_bytes.len() >= 12 => {
                Some(HsmaChild::TrackList(TrackListSection::read_le(cur)?))
            }
            (2, Some(b"lPma")) if child_bytes.len() >= 12 => {
                Some(HsmaChild::PlaylistList(PlaylistListSection::read_le(cur)?))
            }
            (15, Some(b"Lsma")) if child_bytes.len() >= 12 => Some(HsmaChild::SocialProfileList(
                SocialProfileListSection::read_le(cur)?,
            )),
            (17, Some(b"LPma")) if child_bytes.len() >= 8 => {
                Some(HsmaChild::Padding(PaddingSection::read_le(cur)?))
            }
            _ => None,
        };
        Ok(parsed)
    }
}

impl BinRead for HsmaSection {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<Self> {
        Self::read_filtered(reader, &|_| true)
    }
}

impl BinWrite for HsmaSection {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<()> {
        self.header.write_le(writer)?;
        match &self.child {
            HsmaChild::InnerHfma(h) => h.write_le(writer)?,
            HsmaChild::LibraryMaster(p) => p.write_le(writer)?,
            HsmaChild::AlbumList(l) => l.write_le(writer)?,
            HsmaChild::ArtistList(l) => l.write_le(writer)?,
            HsmaChild::TrackList(l) => l.write_le(writer)?,
            HsmaChild::PlaylistList(l) => l.write_le(writer)?,
            HsmaChild::SocialProfileList(s) => s.write_le(writer)?,
            HsmaChild::Padding(p) => p.write_le(writer)?,
            HsmaChild::Unknown { raw_bytes, .. } | HsmaChild::Skipped { raw_bytes, .. } => {
                writer.write_all(raw_bytes)?;
            }
        }
        Ok(())
    }
}

/// Complete Layer-1 Raw AST of a `.musicdb` file.
/// Preserves 100% of bytes for lossless round-tripping and unknown-byte analysis.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RawLibraryFile {
    /// Outer uncompressed `hfma` envelope header.
    pub outer_header: HfmaHeader,
    /// Sequence of `hsma` major sections inside the decrypted + decompressed payload.
    pub sections: Vec<HsmaSection>,
    /// Any trailing bytes after the last `hsma` section (e.g. `ssma` EOF padding if outside `hsma`).
    pub trailing_bytes: Vec<u8>,
}

impl RawLibraryFile {
    /// Resolves either a `.musiclibrary` bundle directory or a direct `.musicdb` file path
    /// to the underlying `.musicdb` file path.
    #[must_use]
    pub fn resolve_musicdb_path(path: impl AsRef<Path>) -> std::path::PathBuf {
        let p = path.as_ref();
        if p.is_dir() {
            let candidate = p.join("Library.musicdb");
            if candidate.exists() {
                return candidate;
            }
        }
        p.to_path_buf()
    }

    /// Reads only the uncompressed 160-byte outer `hfma` header from disk in O(1) without decrypting or decompressing the library payload.
    ///
    /// # Errors
    /// Returns [`MusicDbError`] if the file cannot be opened or does not contain a valid `hfma` header.
    pub fn read_outer_header(path: impl AsRef<Path>) -> Result<HfmaHeader> {
        let mdb_path = Self::resolve_musicdb_path(path);
        let mut file = std::fs::File::open(mdb_path)?;
        let header = HfmaHeader::read_le(&mut file)?;
        Ok(header)
    }

    /// Parses a `.musicdb` file (or `.musiclibrary` bundle directory) from disk, decrypting and decompressing automatically.
    ///
    /// # Errors
    /// Returns [`MusicDbError`] if reading, decrypting, decompressing, or parsing fails.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mdb_path = Self::resolve_musicdb_path(path);
        let raw_bytes = load_raw_musicdb(mdb_path)?;
        Self::from_raw_bytes(&raw_bytes)
    }

    /// Parses only selected `hsma` section subtypes from a `.musicdb` file (or `.musiclibrary` bundle),
    /// storing unselected sections as [`HsmaChild::Skipped`] so serialization remains 100% lossless.
    ///
    /// # Errors
    /// Returns [`MusicDbError`] if reading, decrypting, decompressing, or parsing fails.
    pub fn open_filtered<F>(path: impl AsRef<Path>, include_subtype: F) -> Result<Self>
    where
        F: Fn(u32) -> bool,
    {
        let mdb_path = Self::resolve_musicdb_path(path);
        let raw_bytes = load_raw_musicdb(mdb_path)?;
        Self::from_raw_bytes_filtered(&raw_bytes, include_subtype)
    }

    /// Parses already-decrypted and decompressed raw `.musicdb` bytes.
    ///
    /// # Errors
    /// Returns [`MusicDbError`] if the binary structure is malformed or truncated.
    pub fn from_raw_bytes(raw_bytes: &[u8]) -> Result<Self> {
        Self::from_raw_bytes_filtered(raw_bytes, |_| true)
    }

    /// Parses already-decrypted and decompressed raw `.musicdb` bytes, storing `hsma` sections where
    /// `include_subtype(subtype)` is `false` as [`HsmaChild::Skipped`] so round-tripping stays lossless.
    ///
    /// # Errors
    /// Returns [`MusicDbError`] if any header or section boundary is malformed or truncated.
    pub fn from_raw_bytes_filtered<F>(raw_bytes: &[u8], include_subtype: F) -> Result<Self>
    where
        F: Fn(u32) -> bool,
    {
        let mut cur = Cursor::new(raw_bytes);
        let outer_header = HfmaHeader::read_le(&mut cur)?;

        let mut sections = Vec::new();
        while let Ok(pos) = usize::try_from(cur.position()) {
            let Some(next_four) = raw_bytes.get(pos..pos.saturating_add(4)) else {
                break;
            };
            if next_four == b"hsma" {
                let hsma = HsmaSection::read_filtered(&mut cur, &include_subtype)?;
                sections.push(hsma);
            } else {
                break;
            }
        }

        let pos = usize::try_from(cur.position()).unwrap_or(raw_bytes.len());
        let trailing_bytes = raw_bytes.get(pos..).unwrap_or_default().to_vec();

        Ok(Self {
            outer_header,
            sections,
            trailing_bytes,
        })
    }

    /// Serializes the `RawLibraryFile` back into raw (uncompressed, unencrypted) bytes.
    ///
    /// # Errors
    /// Returns [`MusicDbError`] if binary serialization fails.
    pub fn to_raw_bytes(&self) -> Result<Vec<u8>> {
        let mut cur = Cursor::new(Vec::new());
        self.outer_header.write_le(&mut cur)?;
        for section in &self.sections {
            section.write_le(&mut cur)?;
        }
        cur.write_all(&self.trailing_bytes)?;
        Ok(cur.into_inner())
    }

    /// Serializes, compresses (zlib), and encrypts (AES-128-ECB) back into `.musicdb` disk format.
    ///
    /// # Errors
    /// Returns [`MusicDbError`] if binary serialization, compression, or encryption fails.
    pub fn to_encrypted_bytes(&self) -> Result<Vec<u8>> {
        let raw = self.to_raw_bytes()?;
        compress_and_encrypt_bytes(&raw, DEFAULT_MUSICDB_KEY)
    }
}
