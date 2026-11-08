use binrw::binrw;
use serde::{Deserialize, Serialize};

/// Outer and Inner `hfma` (File Header) section (typically 160 bytes).
#[binrw]
#[brw(little, magic = b"hfma")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HfmaHeader {
    /// Offset 4: Header length in bytes (typically 160).
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
    pub unk_60: u32,
    /// Offset 64..68: Unknown flags (differs between outer and inner `hfma`).
    pub unk_64_flags: [u8; 4],
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
    pub date_modified: u32,
    /// Offset 104..108: Unknown (typically 1 in outer, 0 in inner).
    pub unk_104: u32,
    /// Offset 108..116: Original iTunes Library Persistent ID (if imported from iTunes).
    pub itunes_library_persistent_id: u64,
    /// Offset 116..120: Unknown (e.g. 0x00000123 in outer, 0 in inner).
    pub unk_116: [u8; 4],
    /// Offset 120..128: Library revision hash (updated on every save; shared identically with `Library Preferences.musicdb`).
    pub revision_hash: u64,
    /// Offset 128..132: Secondary copy of `file_size` in format >= 23.1 (0 in older formats and inner `hfma`).
    pub file_size_mirror: u32,
    /// Offset 132..160: Remaining bytes up to 160.
    pub unk_132_160: [u8; 28],
    /// Any extra bytes if `header_size > 160`.
    #[br(count = header_size.saturating_sub(160))]
    pub extra_bytes: Vec<u8>,
}

impl HfmaHeader {
    pub fn version_string(&self) -> String {
        let len = self
            .version_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.version_bytes.len());
        String::from_utf8_lossy(&self.version_bytes[..len]).into_owned()
    }
}

/// `hsma` (Major Section Boundary Header, typically 56 bytes).
#[binrw]
#[brw(little, magic = b"hsma")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HsmaHeader {
    /// Offset 4: Header size in bytes (typically 56).
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
    #[br(count = header_size.saturating_sub(24))]
    pub unk_24_end: Vec<u8>,
}

impl HsmaHeader {
    /// Effective total size in bytes of this `hsma` section (supporting both 32-bit <= v22 and 64-bit >= v23 headers).
    pub fn effective_total_size(&self) -> u64 {
        if self.total_size != 0 {
            self.total_size as u64
        } else {
            self.total_size_64
        }
    }
}
