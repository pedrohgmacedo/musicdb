//! Layer-1 lossless binary AST for Apple Music `.musicdb` files (`hfma`, `hsma`, `plma`, `lama`, `lAma`, `ltma`, `lPma`, `DCma`, `Lsma`, `boma`).

/// `lama` and `iama` album list and album item binary structures.
pub mod album;
/// `lAma` and `iAma` artist list and artist item binary structures.
pub mod artist;
pub mod boma;
/// `DCma` and `dCma` pending offline cloud deletion binary structures.
pub mod cloud_deletion;
pub mod header;
/// `plma` library master section binary structures.
pub mod library_master;
/// `lPma`, `lpma`, `ipfa`, `IPfa`, and `SLst` playlist binary structures.
pub mod playlist;
/// `Lsma` and `isma` social profile binary structures.
pub mod social;
/// `AppleTimestamp` newtype for 32-bit HFS+ epoch timestamps.
pub mod timestamp;
/// `ltma`, `itma`, and `TrackNumerics` track binary structures.
pub mod track;
pub mod unknown;

use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use binrw::{BinRead, BinResult, BinWrite, Endian, binrw};

use crate::crypto::{DEFAULT_MUSICDB_KEY, compress_and_encrypt_bytes, load_raw_musicdb};
use crate::error::{MusicDbError, Result};

pub use album::{AlbumItem, AlbumItemHeader, AlbumListHeader, AlbumListSection};
pub use artist::{ArtistItem, ArtistItemHeader, ArtistListHeader, ArtistListSection};
pub use boma::{BomaPayload, BomaSection, BomaStringSection};
pub use cloud_deletion::{CloudDeletionItem, CloudDeletionListHeader, CloudDeletionListSection};
pub use header::{HfmaHeader, HsmaHeader, SectionKind};
pub use library_master::{LibraryMasterHeader, LibraryMasterSection, PlmaSub1F6, PlmaSub1FF};
pub use playlist::{
    CompactPlaylistTrackItem, PlaylistItem, PlaylistItemHeader, PlaylistListHeader,
    PlaylistListSection, PlaylistTrackItem, SlstSection, SmartPlaylistOptions, SmartPlaylistRule,
    SmartPlaylistRuleArguments, SmartRuleChild,
};
pub use social::{
    SocialProfileItem, SocialProfileItemHeader, SocialProfileListHeader, SocialProfileListSection,
};
pub use timestamp::AppleTimestamp;
pub use track::{
    TrackItem, TrackItemHeader, TrackListHeader, TrackListSection, TrackNumerics, TrackPlaysSkips,
    VideoInfo,
};
pub use unknown::{Unknown, UnknownSpan, UnknownValue, UnknownVisitor, VisitUnknowns};
use unknown::{read_unknown_tail, visit_unknowns};

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
    #[br(parse_with = read_unknown_tail, args(section_size, 8, "LPma"))]
    pub raw_bytes: Unknown<Vec<u8>>,
}

impl VisitUnknowns for PaddingSection {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "LPma", [(tail: self.raw_bytes, 8)]);
    }
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
    /// Subtype 8: `DCma` Pending Offline Cloud Deletions.
    PendingCloudDeletions(CloudDeletionListSection),
    /// Subtype 15: `Lsma` Social Profile List.
    SocialProfileList(SocialProfileListSection),
    /// Subtype 17: `LPma` Padding / Footer.
    Padding(PaddingSection),
    /// Any other / unknown `hsma` child (e.g. `Library Preferences.musicdb` or future subtype), preserved losslessly.
    Unknown {
        /// Section kind from [`HsmaHeader::kind`].
        kind: SectionKind,
        /// First 4 magic bytes of the child payload (or zeros if shorter than 4 bytes).
        signature: [u8; 4],
        /// Full raw bytes of the child section (`total_size - header_size`).
        raw_bytes: Vec<u8>,
    },
    /// A section intentionally skipped during [`RawLibraryFile::from_raw_bytes_filtered`],
    /// retaining its raw bytes so [`RawLibraryFile::to_raw_bytes`] remains 100% lossless.
    Skipped {
        /// Section kind from [`HsmaHeader::kind`].
        kind: SectionKind,
        /// Full unparsed raw bytes of the skipped child section (`total_size - header_size`).
        raw_bytes: Vec<u8>,
    },
}

impl HsmaSection {
    /// Reads an `HsmaSection` from `reader`, parsing its child section when `include_kind(header.kind())`
    /// returns `true`, or preserving the child bytes inside [`HsmaChild::Skipped`] when `false`.
    pub(crate) fn read_filtered<R, F>(reader: &mut R, include_kind: &F) -> BinResult<Self>
    where
        R: Read + Seek,
        F: Fn(SectionKind) -> bool,
    {
        let start_pos = reader.stream_position()?;
        let header = HsmaHeader::read_le(reader)?;
        let subtype = header.subtype;
        let kind = header.kind();
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

        if !include_kind(kind) {
            return Ok(Self {
                header,
                child: HsmaChild::Skipped {
                    kind,
                    raw_bytes: child_bytes,
                },
            });
        }

        let expected_end = start_pos.saturating_add(total_size);
        let mut cur = Cursor::new(&child_bytes);
        let Some(child) = Self::parse_known_child(kind, &child_bytes, &mut cur)? else {
            let mut sig = [0u8; 4];
            if let Some(prefix) = child_bytes.get(0..4) {
                sig.copy_from_slice(prefix);
            }
            return Ok(Self {
                header,
                child: HsmaChild::Unknown {
                    kind,
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
        kind: SectionKind,
        child_bytes: &[u8],
        cur: &mut Cursor<&Vec<u8>>,
    ) -> BinResult<Option<HsmaChild>> {
        let first_four = child_bytes.get(0..4);
        let parsed = match (kind, first_four) {
            (SectionKind::InnerHfma, Some(b"hfma")) if child_bytes.len() >= 160 => {
                Some(HsmaChild::InnerHfma(HfmaHeader::read_le(cur)?))
            }
            (SectionKind::LibraryMaster, Some(b"plma")) if child_bytes.len() >= 160 => Some(
                HsmaChild::LibraryMaster(LibraryMasterSection::read_le(cur)?),
            ),
            (SectionKind::AlbumList, Some(b"lama")) if child_bytes.len() >= 12 => {
                Some(HsmaChild::AlbumList(AlbumListSection::read_le(cur)?))
            }
            (SectionKind::ArtistList, Some(b"lAma")) if child_bytes.len() >= 12 => {
                Some(HsmaChild::ArtistList(ArtistListSection::read_le(cur)?))
            }
            (SectionKind::TrackList, Some(b"ltma")) if child_bytes.len() >= 12 => {
                Some(HsmaChild::TrackList(TrackListSection::read_le(cur)?))
            }
            (SectionKind::PlaylistList, Some(b"lPma")) if child_bytes.len() >= 12 => {
                Some(HsmaChild::PlaylistList(PlaylistListSection::read_le(cur)?))
            }
            (SectionKind::PendingCloudDeletions, Some(b"DCma")) if child_bytes.len() >= 12 => Some(
                HsmaChild::PendingCloudDeletions(CloudDeletionListSection::read_le(cur)?),
            ),
            (SectionKind::SocialProfileList, Some(b"Lsma")) if child_bytes.len() >= 12 => Some(
                HsmaChild::SocialProfileList(SocialProfileListSection::read_le(cur)?),
            ),
            (SectionKind::Padding, Some(b"LPma")) if child_bytes.len() >= 8 => {
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
            HsmaChild::PendingCloudDeletions(d) => d.write_le(writer)?,
            HsmaChild::SocialProfileList(s) => s.write_le(writer)?,
            HsmaChild::Padding(p) => p.write_le(writer)?,
            HsmaChild::Unknown { raw_bytes, .. } | HsmaChild::Skipped { raw_bytes, .. } => {
                writer.write_all(raw_bytes)?;
            }
        }
        Ok(())
    }
}

impl VisitUnknowns for HsmaSection {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.header.visit_unknowns("hsma", visitor);
        match &self.child {
            HsmaChild::InnerHfma(inner) => inner.visit_unknowns("hfma(inner)", visitor),
            HsmaChild::LibraryMaster(plma) => plma.visit_unknowns("plma", visitor),
            HsmaChild::AlbumList(lama) => lama.visit_unknowns("lama", visitor),
            HsmaChild::ArtistList(artists) => artists.visit_unknowns("lAma", visitor),
            HsmaChild::TrackList(ltma) => ltma.visit_unknowns("ltma", visitor),
            HsmaChild::PlaylistList(playlists) => playlists.visit_unknowns("lPma", visitor),
            HsmaChild::PendingCloudDeletions(dcma) => dcma.visit_unknowns("DCma", visitor),
            HsmaChild::SocialProfileList(lsma) => lsma.visit_unknowns("Lsma", visitor),
            HsmaChild::Padding(pad) => pad.visit_unknowns("LPma", visitor),
            HsmaChild::Unknown { .. } | HsmaChild::Skipped { .. } => {}
        }
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
    /// Resolves a `.musicdb` file path, `.musiclibrary` bundle directory, or parent folder containing
    /// a `.musiclibrary` bundle to the underlying `.musicdb` file path.
    #[must_use]
    pub fn resolve_musicdb_path(path: impl AsRef<Path>) -> std::path::PathBuf {
        let p = path.as_ref();
        if p.is_dir()
            && let Some(bundle) = find_musiclibrary_bundle_dir(p)
        {
            return bundle.join("Library.musicdb");
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

    /// Parses only selected `hsma` section kinds from a `.musicdb` file (or `.musiclibrary` bundle),
    /// storing unselected sections as [`HsmaChild::Skipped`] so serialization remains 100% lossless.
    ///
    /// # Examples
    /// ```no_run
    /// use musicdb::sections::HsmaChild;
    /// use musicdb::{RawLibraryFile, SectionKind};
    ///
    /// # fn main() -> musicdb::Result<()> {
    /// let raw = RawLibraryFile::open_filtered("Library.musiclibrary", |kind| {
    ///     !matches!(kind, SectionKind::TrackList | SectionKind::PlaylistList)
    /// })?;
    ///
    /// for sec in &raw.sections {
    ///     if let HsmaChild::Skipped { kind, raw_bytes } = &sec.child {
    ///         println!("skipped {kind}: {} bytes", raw_bytes.len());
    ///     }
    /// }
    ///
    /// let round_trip = raw.to_raw_bytes()?;
    /// # let _ = round_trip;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    /// Returns [`MusicDbError`] if reading, decrypting, decompressing, or parsing fails.
    pub fn open_filtered<F>(path: impl AsRef<Path>, include_kind: F) -> Result<Self>
    where
        F: Fn(SectionKind) -> bool,
    {
        let mdb_path = Self::resolve_musicdb_path(path);
        let raw_bytes = load_raw_musicdb(mdb_path)?;
        Self::from_raw_bytes_filtered(&raw_bytes, include_kind)
    }

    /// Parses already-decrypted and decompressed raw `.musicdb` bytes.
    ///
    /// # Errors
    /// Returns [`MusicDbError`] if the binary structure is malformed or truncated.
    pub fn from_raw_bytes(raw_bytes: &[u8]) -> Result<Self> {
        Self::from_raw_bytes_filtered(raw_bytes, |_| true)
    }

    /// Parses already-decrypted and decompressed raw `.musicdb` bytes, storing `hsma` sections where
    /// `include_kind(kind)` is `false` as [`HsmaChild::Skipped`] so round-tripping stays lossless.
    ///
    /// # Examples
    /// ```no_run
    /// use musicdb::sections::HsmaChild;
    /// use musicdb::{RawLibraryFile, SectionKind};
    ///
    /// # fn main() -> musicdb::Result<()> {
    /// let decrypted_bytes: Vec<u8> = Vec::new();
    /// let raw = RawLibraryFile::from_raw_bytes_filtered(&decrypted_bytes, |kind| {
    ///     !matches!(kind, SectionKind::TrackList | SectionKind::PlaylistList)
    /// })?;
    ///
    /// for sec in &raw.sections {
    ///     if let HsmaChild::Skipped { kind, raw_bytes } = &sec.child {
    ///         println!("skipped {kind}: {} bytes", raw_bytes.len());
    ///     }
    /// }
    ///
    /// let round_trip = raw.to_raw_bytes()?;
    /// # let _ = round_trip;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    /// Returns [`MusicDbError`] if any header or section boundary is malformed or truncated.
    pub fn from_raw_bytes_filtered<F>(raw_bytes: &[u8], include_kind: F) -> Result<Self>
    where
        F: Fn(SectionKind) -> bool,
    {
        let mut cur = Cursor::new(raw_bytes);
        let outer_header = HfmaHeader::read_le(&mut cur)?;

        let mut sections = Vec::new();
        while let Ok(pos) = usize::try_from(cur.position()) {
            let Some(next_four) = raw_bytes.get(pos..pos.saturating_add(4)) else {
                break;
            };
            if next_four == b"hsma" {
                let hsma = HsmaSection::read_filtered(&mut cur, &include_kind)?;
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

impl VisitUnknowns for RawLibraryFile {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.outer_header.visit_unknowns("hfma(outer)", visitor);
        for section in &self.sections {
            section.visit_unknowns("hsma", visitor);
        }
    }
}

/// Locates a `.musiclibrary` directory (containing `Library.musicdb`) inside `dir` when `dir`
/// is either the bundle itself or a parent directory containing one.
pub(crate) fn find_musiclibrary_bundle_dir(dir: &Path) -> Option<std::path::PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    if dir.join("Library.musicdb").is_file() {
        return Some(dir.to_path_buf());
    }

    let well_known = [
        dir.join("Apple Music Library.musiclibrary"),
        dir.join("Music Library.musiclibrary"),
        dir.join("Apple Music")
            .join("Apple Music Library.musiclibrary"),
        dir.join("Music").join("Music Library.musiclibrary"),
    ];
    for candidate in well_known {
        if candidate.join("Library.musicdb").is_file() {
            return Some(candidate);
        }
    }

    // Best-effort scan of direct subdirectories (ignoring symlinks and unreadable entries)
    // to find a `.musiclibrary` bundle when a wrapper/snapshot directory is passed.
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry_res in entries {
        if let Ok(entry) = entry_res
            && entry.file_type().is_ok_and(|ft| ft.is_dir())
        {
            subdirs.push(entry.path());
        }
    }
    subdirs.sort();

    for subdir in &subdirs {
        if subdir.extension().and_then(|e| e.to_str()) == Some("musiclibrary")
            && subdir.join("Library.musicdb").is_file()
        {
            return Some(subdir.clone());
        }
    }
    for subdir in subdirs {
        let name = subdir.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if matches!(name, "Previous Libraries" | "Media" | "Media.localized") {
            continue;
        }
        if subdir.join("Library.musicdb").is_file() {
            return Some(subdir);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_hsma_bytes(subtype: u32, payload: &[u8]) -> Vec<u8> {
        let header_size = 24u32;
        let total_size = header_size + u32::try_from(payload.len()).expect("payload fits in u32");
        let raw = RawLibraryFile {
            outer_header: HfmaHeader {
                header_size: 160,
                major_version: 21,
                minor_version: 1,
                ..Default::default()
            },
            sections: vec![HsmaSection {
                header: HsmaHeader {
                    header_size,
                    total_size,
                    subtype,
                    total_size_64: 0,
                    unk_24_end: Unknown::new(Vec::new()),
                },
                child: HsmaChild::Unknown {
                    kind: SectionKind::from(subtype),
                    signature: [0; 4],
                    raw_bytes: payload.to_vec(),
                },
            }],
            trailing_bytes: Vec::new(),
        };
        raw.to_raw_bytes().expect("serialize synthetic raw")
    }

    #[test]
    fn filtered_parsing_produces_skipped_with_section_kind_and_round_trips() {
        for (subtype, expected_kind, payload) in [
            (10, SectionKind::Trailer, b"SIG1payload".as_slice()),
            (99, SectionKind::Unknown(99), b"XYZ9extra".as_slice()),
        ] {
            let bytes = synthetic_hsma_bytes(subtype, payload);
            let parsed =
                RawLibraryFile::from_raw_bytes_filtered(&bytes, |_| false).expect("filtered parse");
            assert_eq!(parsed.sections.len(), 1);
            assert_eq!(
                parsed.sections[0].child,
                HsmaChild::Skipped {
                    kind: expected_kind,
                    raw_bytes: payload.to_vec(),
                }
            );
            assert_eq!(parsed.to_raw_bytes().expect("round-trip"), bytes);
        }
    }

    #[test]
    fn unfiltered_parsing_produces_unknown_with_section_kind_and_round_trips() {
        for (subtype, expected_kind, payload, expected_sig) in [
            (10, SectionKind::Trailer, b"TRLR1234".as_slice(), *b"TRLR"),
            (99, SectionKind::Unknown(99), b"FOO!".as_slice(), *b"FOO!"),
        ] {
            let bytes = synthetic_hsma_bytes(subtype, payload);
            let parsed = RawLibraryFile::from_raw_bytes(&bytes).expect("unfiltered parse");
            assert_eq!(parsed.sections.len(), 1);
            assert_eq!(
                parsed.sections[0].child,
                HsmaChild::Unknown {
                    kind: expected_kind,
                    signature: expected_sig,
                    raw_bytes: payload.to_vec(),
                }
            );
            assert_eq!(parsed.to_raw_bytes().expect("round-trip"), bytes);
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn hsma_child_unknown_and_skipped_serde_round_trip() {
        let cases = [
            HsmaChild::Skipped {
                kind: SectionKind::Trailer,
                raw_bytes: vec![1, 2, 3, 4],
            },
            HsmaChild::Skipped {
                kind: SectionKind::Unknown(99),
                raw_bytes: vec![5, 6],
            },
            HsmaChild::Unknown {
                kind: SectionKind::Trailer,
                signature: *b"TRLR",
                raw_bytes: vec![0x54, 0x52, 0x4c, 0x52, 9],
            },
            HsmaChild::Unknown {
                kind: SectionKind::Unknown(99),
                signature: *b"ABCD",
                raw_bytes: vec![0x41, 0x42, 0x43, 0x44],
            },
        ];

        for child in cases {
            let json = serde_json::to_string(&child).expect("serialize HsmaChild");
            let back: HsmaChild = serde_json::from_str(&json).expect("deserialize HsmaChild");
            assert_eq!(back, child);
        }
    }
}
