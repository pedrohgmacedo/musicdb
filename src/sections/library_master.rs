use binrw::binrw;

use super::boma::BomaSection;
use super::read_bounded_vec;
use super::unknown::{Unknown, UnknownVisitor, VisitUnknowns, read_unknown_tail, visit_unknowns};

/// Library master header (`plma`, typically 194 bytes in modern Apple Music, 160 in Catalina).
#[binrw]
#[brw(little, magic = b"plma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LibraryMasterHeader {
    /// Offset 4: Header size in bytes (194 or 160, minimum 160).
    pub header_size: u32,
    /// Offset 8: Number of `boma` subsections that follow.
    pub boma_count: u32,
    /// Offset 12..24: Unknown bytes.
    pub unk_12_24: Unknown<[u8; 12]>,
    /// Offset 24: Checkbox "Settings -> General -> Show -> Songs list checkboxes" (0 or 1).
    pub checkbox_show_song_list_checkboxes: u8,
    /// Offset 25..52: Unknown bytes.
    pub unk_25_52: Unknown<[u8; 27]>,
    /// Offset 52..56: Unknown (e.g. 0x00020000).
    pub unk_52: Unknown<u32>,
    /// Offset 56..58: Unknown 2 bytes.
    pub unk_56_58: Unknown<[u8; 2]>,
    /// Offset 58..66: Library Persistent ID.
    pub library_persistent_id: u64,
    /// Offset 66..68: Unknown 2 bytes.
    pub unk_66_68: Unknown<[u8; 2]>,
    /// Offset 68..72: Unknown (e.g. 0x00080000).
    pub unk_68: Unknown<u32>,
    /// Offset 72..92: Unknown 20 bytes.
    pub unk_72_92: Unknown<[u8; 20]>,
    /// Offset 92..100: Library Persistent ID repeated.
    pub library_persistent_id_2: u64,
    /// Offset 100..104: Unknown 4 bytes.
    pub unk_100: Unknown<u32>,
    /// Offset 104..108: Shared 4-byte value (also seen in `itma` offset 244 and `TrackNumerics` 200, 280).
    pub unk_104_shared_val: Unknown<u32>,
    /// Offset 108..128: Unknown 20 bytes.
    pub unk_108_128: Unknown<[u8; 20]>,
    /// Offset 128..132: Unknown (typically 2).
    pub unk_128: Unknown<u32>,
    /// Offset 132..136: Unknown 4 bytes.
    pub unk_132: Unknown<u32>,
    /// Offset 136..140: Unknown (typically 256).
    pub unk_136: Unknown<u32>,
    /// Offset 140..144: Unknown 4 bytes.
    pub unk_140: Unknown<u32>,
    /// Offset 144..148: Unknown (e.g. 0x01010000).
    pub unk_144: Unknown<u32>,
    /// Offset 148: Checkbox "Settings -> Files -> Keep Music Media folder organized" (0 or 1).
    pub checkbox_keep_media_folder_organized: u8,
    /// Offset 149..160: Unknown 11 bytes.
    pub unk_149_160: Unknown<[u8; 11]>,
    /// Offset 160..`header_size`: Remaining bytes (34 bytes when `header_size == 194`).
    #[br(parse_with = read_unknown_tail, args(header_size, 160, "plma"))]
    pub unk_160_end: Unknown<Vec<u8>>,
}

impl VisitUnknowns for LibraryMasterHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(
            visitor,
            "plma",
            [
                (self.unk_12_24, 12),
                (self.unk_25_52, 25),
                (self.unk_52, 52),
                (self.unk_56_58, 56),
                (self.unk_66_68, 66),
                (self.unk_68, 68),
                (self.unk_72_92, 72),
                (self.unk_100, 100),
                (self.unk_104_shared_val, 104),
                (self.unk_108_128, 108),
                (self.unk_128, 128),
                (self.unk_132, 132),
                (self.unk_136, 136),
                (self.unk_140, 140),
                (self.unk_144, 144),
                (self.unk_149_160, 149),
                (tail: self.unk_160_end, 160),
            ]
        );
    }
}

/// Complete library master section (`plma`) including its `boma` children.
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LibraryMasterSection {
    /// The `plma` library master header.
    pub header: LibraryMasterHeader,
    /// Metadata `boma` children attached to the library master section.
    #[br(parse_with = read_bounded_vec, args(header.boma_count, 16, "plma.bomas"))]
    pub bomas: Vec<BomaSection>,
}

impl VisitUnknowns for LibraryMasterSection {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.header.visit_unknowns("plma", visitor);
        for boma in &self.bomas {
            boma.visit_unknowns("plma", visitor);
        }
    }
}

/// Payload of `boma` subtype `0x01F6` under `plma` (20 bytes).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlmaSub1F6 {
    /// Offset 0..4: Unknown 4-byte value (e.g. `01 00 14 00`).
    pub unk_0: Unknown<[u8; 4]>,
    /// Offset 4..20: 16-byte ID / UUID / hash.
    pub uuid_or_hash: [u8; 16],
}

impl VisitUnknowns for PlmaSub1F6 {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "plma::0x01F6", [(self.unk_0, 0)]);
    }
}

/// Payload of `boma` subtype `0x01FF` under `plma` (44 bytes).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlmaSub1FF {
    /// Offset 0..8: Unknown (typically 1 as u64).
    pub unk_0: Unknown<u64>,
    /// Offset 8..16: Library Persistent ID.
    pub library_persistent_id: u64,
    /// Offset 16..24: Library Persistent ID repeated (or 0 in fresh libraries).
    pub library_persistent_id_2: u64,
    /// Offset 24..44: Remaining 20 bytes.
    pub unk_24_44: Unknown<[u8; 20]>,
}

impl VisitUnknowns for PlmaSub1FF {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(
            visitor,
            "plma::0x01FF",
            [(self.unk_0, 0), (self.unk_24_44, 24)]
        );
    }
}
