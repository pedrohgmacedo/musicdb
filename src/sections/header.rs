//! Outer and inner `hfma` file header and `hsma` major section boundary header structures.

use binrw::binrw;

use super::timestamp::AppleTimestamp;
use super::unknown::{Unknown, UnknownVisitor, VisitUnknowns, read_unknown_tail, visit_unknowns};

/// Outer and Inner `hfma` (File Header) section (typically 160 bytes).
#[binrw]
#[brw(little, magic = b"hfma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HfmaHeader {
    /// Offset 4: Header length in bytes (typically 160, minimum 160).
    pub header_size: u32,
    /// Offset 8: Total encrypted/compressed file size (0 in inner `hfma`).
    pub file_size: u32,
    /// Offset 12: File format major version (e.g., 21).
    pub major_version: u16,
    /// Offset 14: File format minor version (e.g., 1).
    pub minor_version: u16,
    /// Offset 16..48: Null-terminated UTF-8 version string of Apple Music / iTunes.
    pub version_bytes: [u8; 32],
    /// Offset 48..56: Library Persistent ID.
    pub library_persistent_id: u64,
    /// Offset 56..60: musicdb file type (e.g. 6 or 7 = Library.musicdb, 2 = Library Preferences.musicdb).
    pub musicdb_file_type: u32,
    /// Offset 60..64: Unknown (typically 20).
    pub unk_60: Unknown<u32>,
    /// Offset 64..68: Unknown flags (differs between outer and inner `hfma`).
    pub unk_64_flags: Unknown<[u8; 4]>,
    /// Offset 68..72: Total track count (0 in inner `hfma`).
    pub track_count: u32,
    /// Offset 72..76: Total playlist count (0 in inner `hfma`).
    pub playlist_count: u32,
    /// Offset 76..80: Total album count (0 in inner `hfma`).
    pub album_count: u32,
    /// Offset 80..84: Total artist count (0 in inner `hfma`).
    pub artist_count: u32,
    /// Offset 84..88: Maximum encrypted payload size (typically 102400; 0 in inner `hfma`).
    pub max_crypt_size: u32,
    /// Offset 88..92: Timezone offset in seconds from UTC (signed, e.g. -10800 for UTC-3).
    pub timezone_offset_seconds: i32,
    /// Offset 92..100: Apple Store account ID (0 when not signed in).
    pub apple_store_account_id: u64,
    /// Offset 100..104: Library modification date (seconds since 1904-01-01 00:00:00 UTC).
    pub date_modified: AppleTimestamp,
    /// Offset 104..108: Unknown (typically 1 in outer, 0 in inner).
    pub unk_104: Unknown<u32>,
    /// Offset 108..116: Original iTunes Library Persistent ID (if imported from iTunes).
    pub itunes_library_persistent_id: u64,
    /// Offset 116..120: Unknown (e.g. 0x00000123 in outer, 0 in inner).
    pub unk_116: Unknown<[u8; 4]>,
    /// Offset 120..128: Library revision hash (updated on every save; shared identically with `Library Preferences.musicdb`).
    pub revision_hash: u64,
    /// Offset 128..132: Secondary copy of `file_size` in format >= 23.1 (0 in older formats and inner `hfma`).
    pub file_size_mirror: u32,
    /// Offset 132..160: Remaining bytes up to 160.
    pub unk_132_160: Unknown<[u8; 28]>,
    /// Any extra bytes if `header_size > 160`.
    #[br(parse_with = read_unknown_tail, args(header_size, 160, "hfma"))]
    pub extra_bytes: Unknown<Vec<u8>>,
}

impl HfmaHeader {
    /// Returns the decoded UTF-8 application version string (e.g. `"1.6.0.151"`), trimmed at the first null byte.
    #[must_use]
    pub fn version_string(&self) -> String {
        let len = self
            .version_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.version_bytes.len());
        let slice = self.version_bytes.get(..len).unwrap_or(&self.version_bytes);
        String::from_utf8_lossy(slice).into_owned()
    }
}

impl VisitUnknowns for HfmaHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, section: &str, visitor: &mut V) {
        let label = if section.is_empty() { "hfma" } else { section };
        visit_unknowns!(
            visitor,
            label,
            [
                (self.unk_60, 60),
                (self.unk_64_flags, 64),
                (self.unk_104, 104),
                (self.unk_116, 116),
                (self.unk_132_160, 132),
                (tail: self.extra_bytes, 160),
            ]
        );
    }
}

/// `hsma` (Major Section Boundary Header, typically 56 bytes).
#[binrw]
#[brw(little, magic = b"hsma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HsmaHeader {
    /// Offset 4: Header size in bytes (typically 56, minimum 24).
    pub header_size: u32,
    /// Offset 8: Total size of this `hsma` + its child section tree in format <= 22 (0 in format >= 23, where `total_size_64` at offset 16 is used).
    pub total_size: u32,
    /// Offset 12: Section subtype:
    /// - 1: `ltma` (Tracks)
    /// - 2: `lPma` (Playlists)
    /// - 3: `hfma` (Inner File Header)
    /// - 4: `lama` (Albums)
    /// - 5: `lAma` (Artists)
    /// - 6: `plma` (Library Master)
    /// - 10: Trailer signature / hash
    /// - 15: `Lsma`
    /// - 17: `LPma` (Padding / Footer)
    pub subtype: u32,
    /// Offset 16..24: 64-bit total size used in format >= 23 (Apple Music 1.7+) when `total_size == 0` (0 in older formats).
    pub total_size_64: u64,
    /// Offset 24..56: Reserved / unknown bytes (typically all 0).
    #[br(parse_with = read_unknown_tail, args(header_size, 24, "hsma"))]
    pub unk_24_end: Unknown<Vec<u8>>,
}

impl VisitUnknowns for HsmaHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "hsma", [(tail: self.unk_24_end, 24)]);
    }
}

impl HsmaHeader {
    /// Effective total size in bytes of this `hsma` section (supporting both 32-bit <= v22 and 64-bit >= v23 headers).
    #[must_use]
    pub fn effective_total_size(&self) -> u64 {
        if self.total_size != 0 {
            u64::from(self.total_size)
        } else {
            self.total_size_64
        }
    }

    /// Returns the typed [`SectionKind`] for this header's `subtype`.
    #[must_use]
    pub fn kind(&self) -> SectionKind {
        SectionKind::from(self.subtype)
    }
}

/// Typed identifier for an `hsma` container section's `subtype` field.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SectionKind {
    /// Subtype 1: Track list (`ltma`).
    TrackList,
    /// Subtype 2: Playlist list (`lPma`).
    PlaylistList,
    /// Subtype 3: Inner file header (`hfma`).
    InnerHfma,
    /// Subtype 4: Album list (`lama`).
    AlbumList,
    /// Subtype 5: Artist list (`lAma`).
    ArtistList,
    /// Subtype 6: Library master (`plma`).
    LibraryMaster,
    /// Subtype 10: Trailer signature / hash.
    Trailer,
    /// Subtype 15: Social profile list (`Lsma`).
    SocialProfileList,
    /// Subtype 17: Padding / footer (`LPma`).
    Padding,
    /// Unrecognized `hsma` subtype value, preserved losslessly.
    Unknown(u32),
}

impl From<u32> for SectionKind {
    fn from(subtype: u32) -> Self {
        match subtype {
            1 => Self::TrackList,
            2 => Self::PlaylistList,
            3 => Self::InnerHfma,
            4 => Self::AlbumList,
            5 => Self::ArtistList,
            6 => Self::LibraryMaster,
            10 => Self::Trailer,
            15 => Self::SocialProfileList,
            17 => Self::Padding,
            other => Self::Unknown(other),
        }
    }
}

impl From<SectionKind> for u32 {
    fn from(kind: SectionKind) -> Self {
        match kind {
            SectionKind::TrackList => 1,
            SectionKind::PlaylistList => 2,
            SectionKind::InnerHfma => 3,
            SectionKind::AlbumList => 4,
            SectionKind::ArtistList => 5,
            SectionKind::LibraryMaster => 6,
            SectionKind::Trailer => 10,
            SectionKind::SocialProfileList => 15,
            SectionKind::Padding => 17,
            SectionKind::Unknown(raw) => raw,
        }
    }
}

impl std::fmt::Display for SectionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TrackList => f.write_str("TrackList"),
            Self::PlaylistList => f.write_str("PlaylistList"),
            Self::InnerHfma => f.write_str("InnerHfma"),
            Self::AlbumList => f.write_str("AlbumList"),
            Self::ArtistList => f.write_str("ArtistList"),
            Self::LibraryMaster => f.write_str("LibraryMaster"),
            Self::Trailer => f.write_str("Trailer"),
            Self::SocialProfileList => f.write_str("SocialProfileList"),
            Self::Padding => f.write_str("Padding"),
            Self::Unknown(raw) => write!(f, "Unknown({raw})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_kind_round_trips_all_known_and_unknown_subtypes() {
        let known = [
            (1, SectionKind::TrackList, "TrackList"),
            (2, SectionKind::PlaylistList, "PlaylistList"),
            (3, SectionKind::InnerHfma, "InnerHfma"),
            (4, SectionKind::AlbumList, "AlbumList"),
            (5, SectionKind::ArtistList, "ArtistList"),
            (6, SectionKind::LibraryMaster, "LibraryMaster"),
            (10, SectionKind::Trailer, "Trailer"),
            (15, SectionKind::SocialProfileList, "SocialProfileList"),
            (17, SectionKind::Padding, "Padding"),
        ];

        for (raw, expected_kind, expected_display) in known {
            let kind = SectionKind::from(raw);
            assert_eq!(kind, expected_kind);
            assert_eq!(u32::from(kind), raw);
            assert_eq!(kind.to_string(), expected_display);
        }

        for unknown_raw in [0, 7, 99, u32::MAX] {
            let kind = SectionKind::from(unknown_raw);
            assert_eq!(kind, SectionKind::Unknown(unknown_raw));
            assert_eq!(u32::from(kind), unknown_raw);
            assert_eq!(kind.to_string(), format!("Unknown({unknown_raw})"));
        }
    }

    #[test]
    fn hsma_header_kind_matches_subtype() {
        let header = HsmaHeader {
            header_size: 56,
            total_size: 56,
            subtype: 4,
            total_size_64: 0,
            unk_24_end: Unknown::new(vec![0; 32]),
        };
        assert_eq!(header.kind(), SectionKind::AlbumList);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn section_kind_serde_round_trip() {
        for kind in [SectionKind::TrackList, SectionKind::Unknown(42)] {
            let json = serde_json::to_string(&kind).expect("serialize SectionKind");
            let parsed: SectionKind = serde_json::from_str(&json).expect("deserialize SectionKind");
            assert_eq!(parsed, kind);
        }
    }
}
