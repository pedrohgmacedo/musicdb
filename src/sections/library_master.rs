use binrw::binrw;
use serde::{Deserialize, Serialize};

use super::boma::BomaSection;

/// `plma` (Library Master) header (typically 194 bytes in modern Apple Music, 160 in Catalina).
#[binrw]
#[brw(little, magic = b"plma")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlmaHeader {
    /// Offset 4: Header size in bytes (194 or 160).
    pub header_size: u32,
    /// Offset 8: Number of `boma` subsections that follow.
    pub boma_count: u32,
    /// Offset 12..24: Unknown bytes.
    pub unk_12_24: [u8; 12],
    /// Offset 24: Checkbox "Settings -> General -> Show -> Songs list checkboxes" (0 or 1).
    pub checkbox_show_song_list_checkboxes: u8,
    /// Offset 25..52: Unknown bytes.
    pub unk_25_52: [u8; 27],
    /// Offset 52..56: Unknown (e.g. 0x00020000).
    pub unk_52: u32,
    /// Offset 56..58: Unknown 2 bytes.
    pub unk_56_58: [u8; 2],
    /// Offset 58..66: Library Persistent ID.
    pub library_persistent_id: u64,
    /// Offset 66..68: Unknown 2 bytes.
    pub unk_66_68: [u8; 2],
    /// Offset 68..72: Unknown (e.g. 0x00080000).
    pub unk_68: u32,
    /// Offset 72..92: Unknown 20 bytes.
    pub unk_72_92: [u8; 20],
    /// Offset 92..100: Library Persistent ID repeated.
    pub library_persistent_id_2: u64,
    /// Offset 100..104: Unknown 4 bytes.
    pub unk_100: u32,
    /// Offset 104..108: Shared 4-byte value (also seen in `itma` offset 244 and `TrackNumerics` 200, 280).
    pub unk_104_shared_val: u32,
    /// Offset 108..128: Unknown 20 bytes.
    pub unk_108_128: [u8; 20],
    /// Offset 128..132: Unknown (typically 2).
    pub unk_128: u32,
    /// Offset 132..136: Unknown 4 bytes.
    pub unk_132: u32,
    /// Offset 136..140: Unknown (typically 256).
    pub unk_136: u32,
    /// Offset 140..144: Unknown 4 bytes.
    pub unk_140: u32,
    /// Offset 144..148: Unknown (e.g. 0x01010000).
    pub unk_144: u32,
    /// Offset 148: Checkbox "Settings -> Files -> Keep Music Media folder organized" (0 or 1).
    pub checkbox_keep_media_folder_organized: u8,
    /// Offset 149..160: Unknown 11 bytes.
    pub unk_149_160: [u8; 11],
    /// Offset 160..header_size: Remaining bytes (34 bytes when `header_size == 194`).
    #[br(count = header_size.saturating_sub(160))]
    pub unk_160_end: Vec<u8>,
}

/// Complete `plma` section including its `boma` children.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlmaSection {
    pub header: PlmaHeader,
    #[br(count = header.boma_count)]
    pub bomas: Vec<BomaSection>,
}

/// Payload of `boma` subtype `0x01F6` under `plma` (20 bytes).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlmaSub1F6 {
    /// Offset 0..4: Unknown 4-byte value (e.g. `01 00 14 00`).
    pub unk_0: [u8; 4],
    /// Offset 4..20: 16-byte ID / UUID / hash.
    pub uuid_or_hash: [u8; 16],
}

/// Payload of `boma` subtype `0x01FF` under `plma` (44 bytes).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlmaSub1FF {
    /// Offset 0..8: Unknown (typically 1 as u64).
    pub unk_0: u64,
    /// Offset 8..16: Library Persistent ID.
    pub library_persistent_id: u64,
    /// Offset 16..24: Library Persistent ID repeated (or 0 in fresh libraries).
    pub library_persistent_id_2: u64,
    /// Offset 24..44: Remaining 20 bytes.
    pub unk_24_44: [u8; 20],
}
