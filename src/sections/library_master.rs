use binrw::binrw;

use super::boma::BomaSection;
use super::read_bounded_vec;
use super::timestamp::AppleTimestamp;
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
    /// Offset 12..16: Unknown 4 bytes (`1` on iCloud Music Library databases, `0` on signed-out local libraries).
    pub unk_12: Unknown<u32>,
    /// Offset 16..20: Last iCloud Music Library sync timestamp (UTC Apple timestamp).
    pub date_cloud_synced: AppleTimestamp,
    /// Offset 20..24: Last local mutation / sync-check timestamp (Apple timestamp).
    pub date_last_modified: AppleTimestamp,
    /// Offset 24: Checkbox "Settings -> General -> Show -> Songs list checkboxes" (0 or 1).
    pub checkbox_show_song_list_checkboxes: u8,
    /// Offset 25..46: Unknown 21 bytes.
    pub unk_25_46: Unknown<[u8; 21]>,
    /// Offset 46..54: Signed-in Apple Store account ID (`dsid`, matches `hfma.apple_store_account_id`; 0 when signed out).
    pub apple_store_account_id_1: u64,
    /// Offset 54..58: Unknown 4 bytes (e.g. `[0x04, 0x01, 0x00, 0x00]` on iCloud-synced libraries, `[0x02, 0x00, 0x00, 0x00]` when signed out).
    pub unk_54_58: Unknown<[u8; 4]>,
    /// Offset 58..66: Library Persistent ID.
    pub library_persistent_id: u64,
    /// Offset 66..68: Monotonic iCloud Music Library database revision (0 when not synced to iCloud).
    pub cloud_database_revision: u16,
    /// Offset 68..72: Unknown (e.g. 0x00080000).
    pub unk_68: Unknown<u32>,
    /// Offset 72..84: Unknown 12 bytes.
    pub unk_72_84: Unknown<[u8; 12]>,
    /// Offset 84..92: Signed-in Apple Store account ID (`dsid`, repeated; matches `hfma.apple_store_account_id`; 0 when signed out).
    pub apple_store_account_id_2: u64,
    /// Offset 92..100: Library Persistent ID repeated.
    pub library_persistent_id_2: u64,
    /// Offset 100..104: Unknown 4 bytes.
    pub unk_100: Unknown<u32>,
    /// Offset 104..112: Signed-in Apple Store account ID (`dsid`, matches `hfma.apple_store_account_id`; 0 when signed out).
    pub apple_store_account_id: u64,
    /// Offset 112..120: Cloud purchase-history (`Jalisco`) revision / last-modified timestamp (Unix epoch seconds since `1970-01-01T00:00:00Z`; `0` on signed-out or local libraries).
    pub date_cloud_purchase_history_modified: u64,
    /// Offset 120..124: Future iCloud refresh / lease expiration timestamp (typically +7 days UTC).
    pub date_cloud_lease_expires: AppleTimestamp,
    /// Offset 124..128: Unknown 4 bytes.
    pub unk_124: Unknown<u32>,
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
    /// Offset 160..182: Unknown 22 bytes (present when `header_size >= 182`; offsets `174..178` and `178..182` hold monotonically increasing app/schema version counters across Apple Music releases).
    #[br(if(header_size >= 182))]
    pub unk_160_182: Option<Unknown<[u8; 22]>>,
    /// Offset 182..186: Secondary ~7-day periodic / local lease expiration timestamp (UTC Apple timestamp; tracks `date_cloud_lease_expires` within seconds when online, advances independently offline, and is `0` on non-iCloud local libraries; present when `header_size >= 186`).
    #[br(if(header_size >= 186))]
    pub date_local_lease_expires: Option<AppleTimestamp>,
    /// Remaining bytes up to `header_size` beyond 186 (e.g. 8 zero bytes `186..194` when `header_size == 194`).
    #[br(parse_with = read_unknown_tail, args(header_size, LibraryMasterHeader::tail_start_offset(header_size), "plma"))]
    pub unk_tail: Unknown<Vec<u8>>,
}

impl LibraryMasterHeader {
    /// Returns the minimum byte offset at which `unk_tail` begins for a given `header_size`.
    #[must_use]
    pub(crate) const fn tail_start_offset(header_size: u32) -> u32 {
        if header_size >= 186 {
            186
        } else if header_size >= 182 {
            182
        } else {
            160
        }
    }
}

impl VisitUnknowns for LibraryMasterHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        let tail_start = usize::try_from(Self::tail_start_offset(self.header_size)).unwrap_or(160);
        visit_unknowns!(
            visitor,
            "plma",
            [
                (self.unk_12, 12),
                (self.unk_25_46, 25),
                (self.unk_54_58, 54),
                (self.unk_68, 68),
                (self.unk_72_84, 72),
                (self.unk_100, 100),
                (self.unk_124, 124),
                (self.unk_128, 128),
                (self.unk_132, 132),
                (self.unk_136, 136),
                (self.unk_140, 140),
                (self.unk_144, 144),
                (self.unk_149_160, 149),
                (?self.unk_160_182, 160),
                (tail: self.unk_tail, tail_start),
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
