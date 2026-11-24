use binrw::binrw;

use super::boma::BomaSection;
use super::read_bounded_vec;
use super::timestamp::AppleTimestamp;
use super::unknown::{Unknown, UnknownVisitor, VisitUnknowns, read_unknown_tail, visit_unknowns};

/// Track list master header (`ltma`, typically 92 bytes).
#[binrw]
#[brw(little, magic = b"ltma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackListHeader {
    /// Offset 4: Header size in bytes (typically 92, minimum 12).
    pub header_size: u32,
    /// Offset 8: Number of `itma` track subsections that follow.
    pub track_count: u32,
    /// Offset 12..92: Unknown / zero padding.
    #[br(parse_with = read_unknown_tail, args(header_size, 12, "ltma"))]
    pub unk_12_end: Unknown<Vec<u8>>,
}

impl VisitUnknowns for TrackListHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(visitor, "ltma", [(tail: self.unk_12_end, 12)]);
    }
}

/// Complete track list section (`ltma`) with all track items (`itma`).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackListSection {
    /// The `ltma` list header.
    pub header: TrackListHeader,
    /// All `itma` track records in the library.
    #[br(parse_with = read_bounded_vec, args(header.track_count, 376, "ltma.tracks"))]
    pub tracks: Vec<TrackItem>,
}

impl VisitUnknowns for TrackListSection {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.header.visit_unknowns("ltma", visitor);
        for track in &self.tracks {
            track.visit_unknowns("itma", visitor);
        }
    }
}

/// Track item header (`itma`, typically 376 bytes).
#[binrw]
#[brw(little, magic = b"itma")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackItemHeader {
    /// Offset 4: Header size in bytes (typically 376).
    pub header_size: u32,
    /// Offset 8: Total size in bytes of this `itma` + all its `boma` children.
    pub total_size: u32,
    /// Offset 12: Number of `boma` children that follow.
    pub boma_count: u32,
    /// Offset 16..24: Track Persistent ID.
    pub track_persistent_id: u64,
    /// Offset 24..28: Global counter (increments across `itma` and `TrackNumerics` in file order).
    pub global_counter: u32,
    /// Offset 28..30: Unknown 2 bytes.
    pub unk_28_30: Unknown<[u8; 2]>,
    /// Offset 30: Checkbox "Skip when shuffling" (0 or 1).
    pub checkbox_skip_when_shuffling: u8,
    /// Offset 31: Ever-played flag (`1` if the track has ever been played; persists across play-count resets).
    pub has_ever_been_played: u8,
    /// Offset 32: Unknown byte (`1` on local `.mp4` video and purchased cloud videos, `0` on audio and catalog subscription videos).
    pub unk_32: Unknown<u8>,
    /// Offset 33: Cloud-synced track flag (`1` on iCloud-synced and Purchased cloud tracks where `cloud_sync_state != 0`, `0` on `LocalOnly` and `Error/Ineligible` tracks).
    pub is_cloud_synced: u8,
    /// Offset 34..38: Unknown 4 bytes (offset 35 is `1` across tracks in iCloud-enabled libraries and `0` in non-iCloud local libraries).
    pub unk_34_38: Unknown<[u8; 4]>,
    /// Offset 38: Checkbox "Album is a compilation of songs by various artists" (0 or 1).
    pub checkbox_album_is_compilation: u8,
    /// Offset 39: Playlist-only / not-in-iCloud-library flag (`<key>Playlist Only</key><true/>` in iTunes XML; 0 = synced to user's iCloud Music Library, 1 = playlist-only or unsynced error).
    pub playlist_only: u8,
    /// Offset 40..42: Unknown 2 bytes.
    pub unk_40_42: Unknown<[u8; 2]>,
    /// Offset 42: Checkbox disabled (0 = checked/enabled, 1 = unchecked/disabled).
    pub checkbox_disabled: u8,
    /// Offset 43: Unknown byte (typically 0).
    pub unk_43: Unknown<u8>,
    /// Offset 44: Local custom lyrics flag (1 = custom lyrics set locally in Get Info -> Lyrics, 0 = none).
    pub has_local_custom_lyrics: u8,
    /// Offset 45: Unknown byte.
    pub unk_45: Unknown<u8>,
    /// Offset 46: Unsynced local metadata changes flag (`1` = metadata/options edited locally on an already-synced iCloud track and pending push, `0` = clean/synced or newly added offline `LocalOnly` track whose unsynced status is carried by `cloud_status == 0` / `cloud_sync_state == 0`).
    pub has_unsynced_local_changes: u8,
    /// Offset 47..49: Unknown 2 bytes (offset 47 is `1` on local untagged WAV/AIFF PCM imports with no artist/album tags, `0` elsewhere).
    pub unk_47_49: Unknown<[u8; 2]>,
    /// Offset 49: Checkbox "Part of a gapless album" (`<key>Part Of Gapless Album</key><true/>` in iTunes XML; 0 or 1).
    pub checkbox_part_of_gapless_album: u8,
    /// Offset 50: Checkbox "Remember playback position" (0 or 1).
    pub checkbox_remember_playback_position: u8,
    /// Offset 51: Checkbox "Show composer in all views" (0 or 1).
    pub checkbox_show_composer_in_all_views: u8,
    /// Offset 52: Checkbox "Use work and movement" (0 or 1).
    pub checkbox_use_work_and_movement: u8,
    /// Offset 53: Unknown byte (`1` on 100% of tracks carrying extended text metadata `boma`s `0x0012`/`0x002E`/`0x003F`/`0x0040` in modern Windows iCloud libraries — 33,969/33,969 exact match).
    pub unk_53: Unknown<u8>,
    /// Offset 54: Asset info plist presence flag (`1` iff `boma 0x001D` `plist_asset_info` / MP4 `iTunMOVI` atom is present, `0` otherwise; verified by controlled M4A `pgap` / `iTunMOVI` / MP3 artwork A/B tests and 33,980/33,980 bijection).
    pub has_asset_info_plist: u8,
    /// Offset 55: Playback or custom start/stop bounds activity flag (changes 0 -> 1 on first play or when playback start/stop bounds are set).
    pub has_playback_or_bounds_activity: u8,
    /// Offset 56: Unknown byte.
    pub unk_56: Unknown<u8>,
    /// Offset 57: Downloaded status (`0` = unspecified/legacy local track in Monterey, `1` = downloaded locally on disk, `3` = in cloud / not downloaded, `5` = no longer available in catalog).
    pub downloaded_status: u8,
    /// Offset 58: iCloud Status (`cloud_status`, Smart Playlist field `0x86`: 2 = Matched, 3 = Uploaded, 5 = Error/Ineligible, 8 = Apple Music, 9 = No Longer Available).
    pub cloud_status: u8,
    /// Offset 59: Content rating (0 = default, 1 = explicit, 2 = clean, 4 = parental guidance).
    pub content_rating: u8,
    /// Offset 60..62: Unknown 2 bytes.
    pub unk_60_62: Unknown<[u8; 2]>,
    /// Offset 62..64: Suggestion/like flag (0 = none, 1 = removed, 2 = favorite/loved, 3 = suggest less/disliked).
    pub suggestion_flag: u16,
    /// Offset 64: Store/catalog ID presence flag (0 = uploaded local track with no catalog ID, 1 = matched/catalog audio track, 5/6 = music video).
    pub unk_64_store_flag: Unknown<u8>,
    /// Offset 65: Star rating (0..=100 in steps of 20).
    pub star_rating: u8,
    /// Offset 66..72: Sort string bitflags for `[Title, Album, Artist, Album Artist, Composer, Show]` (bit 0 `0x01` = sort `boma` present, bit 1 `0x02` = user/tag custom sort override, bit 7 `0x80` = local auto-sort rule; e.g. `0x80` = auto absent, `0x81` = auto article-stripped, `0x01` = catalog sort string, `0x03` = custom override).
    pub auto_sort_flags: [u8; 6],
    /// Offset 72..82: Unknown 10 bytes (all zeros).
    pub unk_72_82: Unknown<[u8; 10]>,
    /// Offset 82..84: Beats per minute (BPM).
    pub bpm: u16,
    /// Offset 84..86: Disc number.
    pub disc_number: u16,
    /// Offset 86..88: Total movements (classical).
    pub movement_total: u16,
    /// Offset 88..90: Movement number (classical).
    pub movement_number: u16,
    /// Offset 90..92: Total discs.
    pub disc_total: u16,
    /// Offset 92..96: Volume adjustment (-255..=255 corresponding to -100%..=100%).
    pub volume_adjustment: i32,
    /// Offset 96..100: Unknown (non-zero for purchased tracks, shared within same album).
    pub unk_96_purchased_album_group: Unknown<u32>,
    /// Offset 100..108: Unknown 8 bytes.
    pub unk_100_108: Unknown<[u8; 8]>,
    /// Offset 108..112: Media kind bitflags (1 = music, 0x10 = PDF/booklet, 0x20 = music video, 0x100000 = voice memo).
    pub media_kind: u32,
    /// Offset 112..116: Unknown (typically 1).
    pub unk_112: Unknown<u32>,
    /// Offset 116..118: Total tracks on disc/album.
    pub track_total: u16,
    /// Offset 118..148: Unknown 30 bytes.
    pub unk_118_148: Unknown<[u8; 30]>,
    /// Offset 148..152: Playback start position in milliseconds (0 = start of track).
    pub playback_start_ms: u32,
    /// Offset 152..156: Playback stop position in milliseconds (0 = end of track).
    pub playback_stop_ms: u32,
    /// Offset 156..160: Unknown 4 bytes.
    pub unk_156: Unknown<u32>,
    /// Offset 160..162: Track number.
    pub track_number: u16,
    /// Offset 162..168: Unknown 6 bytes.
    pub unk_162_168: Unknown<[u8; 6]>,
    /// Offset 168..172: Release year.
    pub year: u32,
    /// Offset 172..180: Album Persistent ID (`iama` reference).
    pub album_persistent_id: u64,
    /// Offset 180..188: Artist / Album Artist Persistent ID (`iAma` reference).
    pub artist_persistent_id: u64,
    /// Offset 188..192: Apple Music Store Artist ID.
    pub apple_music_store_artist_id: u32,
    /// Offset 192..220: Unknown 28 bytes.
    pub unk_192_220: Unknown<[u8; 28]>,
    /// Offset 220..228: iCloud Music Library `sagaId` (or `0xFFFFFFFFFFFFFFFD` = -3 for Apple Music subscription tracks).
    pub cloud_saga_id_1: u64,
    /// Offset 228..236: 64-bit iTunes Store / Match ID (or `0x7FFFFFFF_FFFFFFEF` sentinel on some matched tracks).
    pub itunes_store_matched_id: u64,
    /// Offset 236..244: Primary iCloud Music Library `sagaId` (matches `sagaId=` in `boma 0x0038` redownload-params).
    pub cloud_saga_id: u64,
    /// Offset 244..252: Signed-in Apple Store account ID (`dsid`, matches `hfma.apple_store_account_id`; 0 when signed out).
    pub apple_store_account_id: u64,
    /// Offset 252..254: Unknown 2 bytes (typically 0).
    pub unk_252_254: Unknown<u16>,
    /// Offset 254: Cloud sync state (2 = synced to iCloud Music Library, 3 = purchased, 0 = unsynced local error).
    pub cloud_sync_state: u8,
    /// Offset 255: Custom lyrics flag (1 = track has custom lyrics / `cloud-lyrics-token` in `boma 0x0036`, 0 = none).
    pub has_custom_lyrics: u8,
    /// Offset 256..272: Default Artwork UUID in `artwork.sqlite` (16 bytes).
    pub artwork_uuid: [u8; 16],
    /// Offset 272..280: Secondary Track Persistent ID (or 0).
    pub track_persistent_id_2: u64,
    /// Offset 280..284: Title sort order index (multiple of 1000).
    pub title_sort_order: u32,
    /// Offset 284..288: Album sort order index (multiple of 1000).
    pub album_sort_order: u32,
    /// Offset 288..292: Artist sort order index (multiple of 1000).
    pub artist_sort_order: u32,
    /// Offset 292..296: Genre sort order index (multiple of 1000).
    pub genre_sort_order: u32,
    /// Offset 296..300: Composer sort order index (multiple of 1000).
    pub composer_sort_order: u32,
    /// Offset 300..304: Album artist sort order index (multiple of 1000).
    pub album_artist_sort_order: u32,
    /// Offset 304..308: Album artist or artist sort order index (multiple of 1000).
    pub album_artist_or_artist_sort_order: u32,
    /// Offset 308..312: Artwork source kind in `artwork.sqlite` (`item_to_artwork.source_kind`; 6 = Local Embedded Artwork, 12 = Apple Music Catalog Artwork, 13 = Cloud Uploaded/Matched Artwork).
    pub artwork_source_kind: u32,
    /// Offset 312..320: Unknown 8 bytes (64-bit artwork content hash / persistent ID on macOS; 0 on Windows).
    pub unk_312_320: Unknown<u64>,
    /// Offset 320..328: iCloud Music Library `sagaId` repeated (matches offset 220).
    pub cloud_saga_id_2: u64,
    /// Offset 328: Artwork cache state bitfield matching `iAma[100]`, `lpma[300..304]`, and `isma[52]` (`0` = none, `3` = local embedded artwork not in `artwork.sqlite`, `5` = cached dynamic/recipe artwork, `8` = cached static/uploaded/catalog artwork in `artwork.sqlite`).
    pub artwork_cache_state: u8,
    /// Offset 329..331: Unknown 2 bytes (typically `[0, 0]`; `0x64` at offset 329 on purchased music videos).
    pub unk_329_331: Unknown<[u8; 2]>,
    /// Offset 331: Apple Music catalog `audioTraits` bitmask (`0x01` = Dolby Atmos, `0x02` = Dolby Audio / Surround 5.1, `0x04` = Lossless, `0x08` = Lossy Stereo / AAC, `0x10` = Hi-Res Lossless, `0x20` = Spatial Audio).
    pub audio_traits: u8,
    /// Offset 332..336: Unknown 4 bytes.
    pub unk_332: Unknown<u32>,
    /// Offset 336..340: Suggestion flag modified date (Apple timestamp).
    pub date_suggestion_flag_modified: AppleTimestamp,
    /// Offset 340..344: Unknown 4 bytes.
    pub unk_340: Unknown<u32>,
    /// Offset 344..348: Artwork cache size kind in `artwork.sqlite` (`cache_items.size_kind`; 1 = `_sk1`, 0 = none).
    pub artwork_size_kind: u32,
    /// Offset 348..352: Artwork cached image format in `artwork.sqlite` (`cache_items.extension`; 1 = `jpeg`, 2 = `png`, 0 = none).
    pub artwork_image_format: u32,
    /// Offset 352..356: Track library/cloud UTC sync timestamp (Apple UTC timestamp; mirrors `TrackNumerics.date_cloud_synced`, also stamped on offline local imports and local downloads; `0` when unset).
    pub date_cloud_synced: AppleTimestamp,
    /// Offset 356..376: Unknown 20 bytes.
    pub unk_356_376: Unknown<[u8; 20]>,
    /// Any extra bytes if `header_size > 376`.
    #[br(parse_with = read_unknown_tail, args(header_size, 376, "itma"))]
    pub extra_bytes: Unknown<Vec<u8>>,
}

impl VisitUnknowns for TrackItemHeader {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(
            visitor,
            "itma",
            [
                (self.unk_28_30, 28),
                (self.unk_32, 32),
                (self.unk_34_38, 34),
                (self.unk_40_42, 40),
                (self.unk_43, 43),
                (self.unk_45, 45),
                (self.unk_47_49, 47),
                (self.unk_53, 53),
                (self.unk_56, 56),
                (self.unk_60_62, 60),
                (self.unk_64_store_flag, 64),
                (self.unk_72_82, 72),
                (self.unk_96_purchased_album_group, 96),
                (self.unk_100_108, 100),
                (self.unk_112, 112),
                (self.unk_118_148, 118),
                (self.unk_156, 156),
                (self.unk_162_168, 162),
                (self.unk_192_220, 192),
                (self.unk_252_254, 252),
                (self.unk_312_320, 312),
                (self.unk_329_331, 329),
                (self.unk_332, 332),
                (self.unk_340, 340),
                (self.unk_356_376, 356),
                (tail: self.extra_bytes, 376),
            ]
        );
    }
}

/// Complete track item section (`itma` header + `boma` children).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackItem {
    /// The `itma` track header.
    pub header: TrackItemHeader,
    /// Metadata `boma` children attached to this track.
    #[br(parse_with = read_bounded_vec, args(header.boma_count, 16, "itma.bomas"))]
    pub bomas: Vec<BomaSection>,
}

impl VisitUnknowns for TrackItem {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        self.header.visit_unknowns("itma", visitor);
        for boma in &self.bomas {
            boma.visit_unknowns("itma", visitor);
        }
    }
}

/// Payload of `boma` subtype `0x0001` (Track Numerics, 364 bytes).
/// Note: Offsets here are relative to the start of the `boma` payload (subtract 20 from raw `boma` offsets).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackNumerics {
    /// Offset 0..4: Global counter.
    pub global_counter: u32,
    /// Offset 4..8: Asset subrecord kind (`1` = local downloaded file subrecord, `3` = cloud/catalog subrecord).
    pub subrecord_kind: u32,
    /// Offset 8..16: Unique 64-bit Track Data ID (`tdID`) for this asset subrecord.
    pub track_data_id: u64,
    /// Offset 16..20: Unknown (typically 1).
    pub unk_16: Unknown<u32>,
    /// Offset 20: Cloud catalog unavailability flag (`1` = track no longer available in Apple Music catalog, `0` = available/local).
    pub is_cloud_unavailable: u8,
    /// Offset 21..24: Unknown flags (`[1, 1, 0]` immediately on import for local MP4/M4A/ALAC containers, on synced local files, on downloaded `subrecord_kind == 3`, and on local MP3s once played; `[0, 0, 0]` on newly imported unplayed MP3/WAV/AIFF even after background gapless/Sound Check scanning and on many undownloaded `subrecord_kind == 3` records).
    pub unk_21_24: Unknown<[u8; 3]>,
    /// Offset 24..27: Unknown 3 bytes (typically 0).
    pub unk_24_27: Unknown<[u8; 3]>,
    /// Offset 27: Variable Bit Rate (VBR) flag (`1` = VBR audio track, `0` = CBR).
    pub is_vbr: u8,
    /// Offset 28..32: Local file subrecord flag (`1` on local downloaded file subrecords, `0` on cloud/catalog subrecords).
    pub is_local_subrecord: u32,
    /// Offset 32..34: iTunes Match flag (`[1, 1]` = `<key>Matched</key><true/>` in iTunes XML).
    pub is_itunes_matched: [u8; 2],
    /// Offset 34: Apple Music catalog subscription flag (`1` = `<key>Apple Music</key><true/>` in iTunes XML).
    pub is_apple_music_catalog: u8,
    /// Offset 35: Unknown byte.
    pub unk_35: Unknown<u8>,
    /// Offset 36: Apple Digital Master flag (`1` = Apple Digital Master, `0` = standard).
    pub is_apple_digital_master: u8,
    /// Offset 37..39: Unknown 2 bytes.
    pub unk_37_39: Unknown<[u8; 2]>,
    /// Offset 39: Catalog availability flag (`1` = available in Apple Music catalog, `0` = uploaded or no longer available).
    pub is_available_in_catalog: u8,
    /// Offset 40: Local MP3 flag (`1` iff `subrecord_kind == 1 && container_format_kind == 12`, `0` otherwise).
    pub is_local_mp3: u8,
    /// Offset 41: Local video asset flag (`1` on local/downloaded video `subrecord_kind == 1` such as `.m4v` and DRM-free `.mp4`, `0` on audio and cloud `subrecord_kind == 3`).
    pub is_local_video: u8,
    /// Offset 42..54: Unknown 12 bytes (offset 45 = `[3]` is `1` iff `subrecord_kind == 3 || date_released != 0 || unk_116_cloud_timestamp != 0 || store_track_adam_id != 0` in modern libraries; offset 46 = `[4]` is `1` on 720p/1080p local video `subrecord_kind == 1` in modern libraries; offset 48 = `[6]` is `1` on a legacy matched MP3 with trailing `APEv2` — stayed `0` on fresh local playback/gapless/Sound Check — and `3` on downloaded HLS Dolby Atmos `.movpkg`; offsets 49..53 = `[7..12]` encode iCloud sync/origin states).
    pub unk_42_54: Unknown<[u8; 12]>,
    /// Offset 54: Cloud/source asset container or Apple Store flavor ID (`1` = MP3/MPEG audio, `2` = `"2:256"` M4A/AAC audio, `18` = `"18:1080p"` 1080p HD video matching `boma 0x0034` `itunes_store_flavor`; `0` on undownloaded video / `.movpkg` / local `subrecord_kind == 1`).
    pub asset_container_kind: u8,
    /// Offset 55..59: Unknown 4 bytes (offset 58 is `2` on a downloaded 1080p HD `.m4v` `subrecord_kind == 1` carrying MP4 `hdvd = 2`, and `0` on local 720p `.mp4` without an `hdvd` atom, all audio tracks, and `subrecord_kind == 3`).
    pub unk_55_59: Unknown<[u8; 4]>,
    /// Offset 59: Apple Music catalog time-synced lyrics availability on `subrecord_kind == 3` (`1` = catalog time-synced lyrics available, `2` = no catalog time-synced lyrics / Matched or Uploaded track, `0` = local `subrecord_kind == 1` or format `<= 22.0`).
    pub catalog_lyrics_state: u8,
    /// Offset 60..64: Sample rate in Hz (IEEE 754 f32, e.g. 44100.0).
    pub sample_rate: f32,
    /// Offset 64..68: File type `FourCC` in little-endian (e.g. `0x4D503320` = `"MP3 "`, `0x4D344120` = `"M4A "`, `0x4D345620` = `"M4V "`, `0x4D503420` = `"MP4 "`, `0x4D4F5650` = `"MOVP"`, `0x57415620` = `"WAV "`, `0x41494646` = `"AIFF"`).
    pub file_type_fourcc: u32,
    /// Offset 68..72: Secondary file type / unknown.
    pub unk_68_file_type: Unknown<u32>,
    /// Offset 72..74: File Folder Count (matches iTunes XML `File Folder Count`, e.g. 5).
    pub file_folder_count: u16,
    /// Offset 74..76: Library Folder Count (matches iTunes XML `Library Folder Count`, e.g. 1).
    pub library_folder_count: u16,
    /// Offset 76..78: Artwork count (matches iTunes XML `Artwork Count`).
    pub artwork_count: u16,
    /// Offset 78..80: Container format kind (`3` / `0x0003` = MPEG-4 video M4V cloud subrecord, `4` / `0x0004` = HLS media package `.movpkg`, `12` / `0x000C` = MP3 audio, `51` / `0x0033` = MPEG-4 AAC/video M4A/M4P/M4V/MP4, `60` / `0x003C` = Apple Lossless ALAC `.m4a`, `70` / `0x0046` = uncompressed PCM WAV/AIFF).
    pub container_format_kind: u16,
    /// Offset 80..82: Signed 16-bit artwork source/status indicator (`-1` / `0xFFFF` = embedded artwork scanned in local file, `1` = cloud/catalog artwork on `subrecord_kind == 3`, `128` / `0x0080` = auto-extracted video poster frame on local `.mp4` without cover art, `0` = none/unscanned).
    pub has_embedded_artwork: i16,
    /// Offset 82..84: Local file artwork state (`2` on most local downloaded file subrecords, `0` on cloud/catalog subrecords and observed `0` on local ALAC `.m4a`).
    pub local_artwork_state: u16,
    /// Offset 84..88: Total size in bytes of all attached artworks.
    pub artwork_total_size_bytes: u32,
    /// Offset 88..92: Bit rate in kbps (matches iTunes XML `Bit Rate`).
    pub bit_rate: u32,
    /// Offset 92..96: Date Added (Apple local timestamp; non-zero iff track is in Master Playlist `####!####`).
    pub date_added: AppleTimestamp,
    /// Offset 96..100: Unknown 4 bytes (all zeros across observed libraries).
    pub unk_96_100: Unknown<u32>,
    /// Offset 100..104: MP4 `sinf` / `FairPlay` DRM platform ID (`sinf/schi/righ` `Platform` low word; `0` = macOS/none, `1` = Windows / Matched AAC / `.m4p`, `2` = alternate platform).
    pub drm_platform: u32,
    /// Offset 104..108: `FairPlay` DRM version restrictions (`sinf/schi/righ` `VersionRestrictions`, e.g. `0x07010100` on `FairPlay`-encrypted `.m4p` / `.movpkg` audio downloads and `0x06010100` on `FairPlay`-encrypted `.m4v` music video downloads; `0` on DRM-free tracks).
    pub drm_version_restrictions: u32,
    /// Offset 108..112: Gapless playback encoder delay in samples (word 1 of `iTunSMPB`, e.g. 2112 for AAC or 576 for LAME MP3).
    pub gapless_encoder_delay: u32,
    /// Offset 112..116: Gapless playback encoder padding in samples (word 2 of `iTunSMPB`).
    pub gapless_encoder_padding: u32,
    /// Offset 116..120: Unknown server-supplied `AppleTimestamp` populated on Matched and playlist-only catalog subrecords in Apple Music 1.6.4+ (0 in older versions and on Uploaded tracks).
    pub unk_116_cloud_timestamp: Unknown<u32>,
    /// Offset 120..124: Gapless playback flags (`1` or `0x02000003` when `iTunSMPB` is present).
    pub gapless_playback_flags: u32,
    /// Offset 124..128: 32-bit hash of embedded/custom lyrics (`USLT` / `©lyr`) on local `subrecord_kind == 1` records (0 if no embedded/custom lyrics; identical hash for identical lyrics text).
    pub custom_lyrics_hash: u32,
    /// Offset 128..132: Date Modified (Apple local timestamp).
    pub date_modified: AppleTimestamp,
    /// Offset 132..136: Volume normalization (Sound Check / iTunes XML `Normalization`, `iTunNORM`).
    pub normalization: u32,
    /// Offset 136..140: Purchase Date (Apple timestamp).
    pub date_purchased: AppleTimestamp,
    /// Offset 140..144: Release Date (Apple UTC timestamp).
    pub date_released: AppleTimestamp,
    /// Offset 144..148: Cloud/ingested source audio codec kind on `subrecord_kind == 3` (`2` = `"mp4a"` AAC/M4A, `3` = `"mpeg"` MP3, reflecting the originally ingested/matched source format on Matched/Uploaded tracks and `"mp4a"` on catalog tracks; `0` on local `subrecord_kind == 1` and video).
    pub cloud_codec_kind: u32,
    /// Offset 148..152: Codec/Subtype `FourCC` in little-endian (e.g. `0x6D703461` = `"mp4a"`, `0x6D706567` = `"mpeg"`).
    pub codec_fourcc: u32,
    /// Offset 152..156: Unknown (0 or e.g. `0x00000123`).
    pub unk_152: Unknown<u32>,
    /// Offset 156..160: Track duration in milliseconds (`Total Time` in iTunes XML).
    pub duration_ms: u32,
    /// Offset 160..168: 64-bit Apple Music Store Album ID (`plID` in MP4 atoms; exceeds `u32::MAX` for modern releases).
    pub apple_music_store_album_id: u64,
    /// Offset 168..176: 64-bit Apple Music Store Artist ID (`atID` in MP4 atoms; exceeds `u32::MAX` for modern artists).
    pub apple_music_store_artist_id: u64,
    /// Offset 176..200: Unknown 24 bytes.
    pub unk_176_200: Unknown<[u8; 24]>,
    /// Offset 200..208: Signed-in Apple Store account ID (`dsid`, matches `hfma.apple_store_account_id`; 0 when signed out).
    pub apple_store_account_id: u64,
    /// Offset 208..216: 64-bit Apple Music Store Album Artist ID (exceeds `u32::MAX` for modern artists).
    pub apple_music_store_album_artist_id: u64,
    /// Offset 216..248: Unknown 32 bytes (offsets 232..248 hold `[0x0200000B_u64, 0x0400000B_u64]` `FairPlay` DRM key/rights IDs in the `0x__00000B` `KeyID` namespace on all `.m4p` / `.m4v` / `.movpkg` offline downloads with non-zero `drm_version_restrictions`; zeros otherwise).
    pub unk_216_248: Unknown<[u8; 32]>,
    /// Offset 248..256: `FairPlay` DRM authorized Apple Store account ID (`dsid`) on downloaded `.m4p` / `.movpkg` tracks (0 otherwise).
    pub drm_account_id: u64,
    /// Offset 256..264: Gapless playback total sample count (words 3..4 of `iTunSMPB`).
    pub gapless_total_samples: u64,
    /// Offset 264..268: Apple Store Genre ID (`geID` MP4 atom, e.g. 21 = Rock, 14 = Pop, 7 = Electronic, 18 = Hip-Hop/Rap).
    pub store_genre_id: u32,
    /// Offset 268..272: Unknown 4 bytes (typically 0).
    pub unk_268_272: Unknown<u32>,
    /// Offset 272..280: Gapless playback byte offset to the 8th-from-last MPEG frame (word 5 of `iTunSMPB` for MP3s; 0 for M4A).
    pub gapless_last_eight_frames_offset: u64,
    /// Offset 280..288: Purchaser Apple Store account ID (`dsid`) on local purchased/matched `.m4a` files with `boma 0x003C` (`purchaser_name`; 0 otherwise).
    pub purchaser_account_id: u64,
    /// Offset 288..296: iCloud Music Library `sagaId` (or `0xFFFFFFFFFFFFFFFD` = -3 for Apple Music catalog tracks).
    pub cloud_saga_id_1: u64,
    /// Offset 296..300: File size in bytes (`Size` in iTunes XML).
    pub file_size: u32,
    /// Offset 300..304: Unknown 4 bytes.
    pub unk_300: Unknown<u32>,
    /// Offset 304..312: 64-bit Apple Music / iTunes Store Track `adamId` (matches `adamId=` in `boma 0x0038`).
    pub store_track_adam_id: u64,
    /// Offset 312..316: Apple iTunes Storefront / Country ID (`sfID`, e.g. 143441 = US, 143503 = Brazil, 143469 = Russia).
    pub storefront_id: u32,
    /// Offset 316..320: Unknown 4 bytes (typically 0).
    pub unk_316_320: Unknown<u32>,
    /// Offset 320..328: 64-bit Apple Music / iTunes Store Track `adamId` repeated.
    pub store_track_adam_id_2: u64,
    /// Offset 328..336: iCloud Music Library `sagaId` (matches `sagaId=` in `boma 0x0038`, or `0xFFFFFFFFFFFFFFFD` for catalog tracks).
    pub cloud_saga_id_2: u64,
    /// Offset 336..340: Downloaded local asset `audioTraits` bitmask on `subrecord_kind == 1` (same bit layout as [`TrackItemHeader::audio_traits`], e.g. `0x21` = Dolby Atmos + Spatial Audio on downloaded `.movpkg` HLS Atmos assets; `0` on stereo AAC/MP3 or cloud `subrecord_kind == 3`).
    pub downloaded_audio_traits: u32,
    /// Offset 340..348: 64-bit Apple Music / iTunes Store Track `adamId` repeated.
    pub store_track_adam_id_3: u64,
    /// Offset 348..356: 64-bit Apple Music / iTunes Store Track `adamId` repeated.
    pub store_track_adam_id_4: u64,
    /// Offset 356..360: Library/cloud UTC sync timestamp (Apple UTC timestamp; also stamped with UTC import time on offline local imports, or `2` on legacy local assets).
    pub date_cloud_synced: AppleTimestamp,
    /// Offset 360..364: Unknown 4 bytes.
    pub unk_360: Unknown<u32>,
}

impl VisitUnknowns for TrackNumerics {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(
            visitor,
            "TrackNumerics",
            [
                (self.unk_16, 16),
                (self.unk_21_24, 21),
                (self.unk_24_27, 24),
                (self.unk_35, 35),
                (self.unk_37_39, 37),
                (self.unk_42_54, 42),
                (self.unk_55_59, 55),
                (self.unk_68_file_type, 68),
                (self.unk_96_100, 96),
                (self.unk_116_cloud_timestamp, 116),
                (self.unk_152, 152),
                (self.unk_176_200, 176),
                (self.unk_216_248, 216),
                (self.unk_268_272, 268),
                (self.unk_300, 300),
                (self.unk_316_320, 316),
                (self.unk_360, 360),
            ]
        );
    }
}

impl TrackNumerics {
    /// Converts `file_type_fourcc` (e.g. `0x4D503320`) into its 4-character ASCII representation (e.g. `"MP3 "`).
    #[must_use]
    pub fn file_type_fourcc_str(&self) -> Option<String> {
        if self.file_type_fourcc == 0 {
            return None;
        }
        let bytes = self.file_type_fourcc.to_be_bytes();
        if bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
            Some(String::from_utf8_lossy(&bytes).into_owned())
        } else {
            None
        }
    }

    /// Converts `codec_fourcc` (e.g. `0x6D703461`) into its 4-character ASCII representation (e.g. `"mp4a"`).
    #[must_use]
    pub fn codec_fourcc_str(&self) -> Option<String> {
        if self.codec_fourcc == 0 {
            return None;
        }
        let bytes = self.codec_fourcc.to_be_bytes();
        if bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
            Some(String::from_utf8_lossy(&bytes).into_owned())
        } else {
            None
        }
    }
}

/// Payload of `boma` subtype `0x0017` (Track Play & Skip Statistics, 52 bytes).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackPlaysSkips {
    /// Offset 0..8: Track Persistent ID.
    pub track_persistent_id: u64,
    /// Offset 8..12: Last played date (Apple timestamp, 0 if never or reset).
    pub date_last_played: AppleTimestamp,
    /// Offset 12..16: Play count (can be reset in GUI).
    pub play_count: u32,
    /// Offset 16..20: True play count (not affected by GUI reset).
    pub true_play_count: u32,
    /// Offset 20..24: First ever played date in UTC (Apple timestamp).
    pub date_first_played_utc: AppleTimestamp,
    /// Offset 24..28: Unknown 4 bytes.
    pub unk_24: Unknown<u32>,
    /// Offset 28..32: Last skipped date (Apple timestamp).
    pub date_last_skipped: AppleTimestamp,
    /// Offset 32..36: Skip count.
    pub skip_count: u32,
    /// Offset 36..40: True skip count.
    pub true_skip_count: u32,
    /// Offset 40..44: Unknown 4 bytes.
    pub unk_40: Unknown<u32>,
    /// Offset 44..52: Unknown 8 bytes.
    pub unk_44_52: Unknown<[u8; 8]>,
}

impl VisitUnknowns for TrackPlaysSkips {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(
            visitor,
            "TrackPlaysSkips",
            [(self.unk_24, 24), (self.unk_40, 40), (self.unk_44_52, 44)]
        );
    }
}

/// Payload of `boma` subtype `0x0024` (Video Track Metadata, 84 bytes).
#[binrw]
#[brw(little)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VideoInfo {
    /// Offset 0..4: Vertical resolution (height in pixels).
    pub height: u32,
    /// Offset 4..8: Horizontal resolution (width in pixels).
    pub width: u32,
    /// Offset 8..12: Unknown (typically `1`; video track ID or track count).
    pub unk_8: Unknown<u32>,
    /// Offset 12..16: Video codec `FourCC` in little-endian (e.g. `0x61766331` = `"avc1"`).
    pub codec_fourcc: u32,
    /// Offset 16..20: Unknown 4 bytes (observed `0` on DRM-free 720p `.mp4` and `6060` on `FairPlay` 1080p `.m4v`).
    pub unk_16: Unknown<u32>,
    /// Offset 20..24: Video stream bit rate metric in kbps (e.g. `5153` matching exact average kbps on a 1080p `.m4v`; observed `975` on a 5s 1482 kbps local 720p `.mp4`, so Apple Music's calculation on short/local MP4s needs further verification).
    pub video_bit_rate_kbps: u32,
    /// Offset 24..28: Unknown 4 bytes (matches `video_bit_rate_kbps` on all observed files: `975`, `5153`, `5791`; likely target/average video bit rate).
    pub unk_24: Unknown<u32>,
    /// Offset 28..32: Unknown 4 bytes (observed `0`).
    pub unk_28: Unknown<u32>,
    /// Offset 32..36: H.264/AVC `profile_idc` from `avcC` (e.g. `100` = High Profile).
    pub avc_profile_idc: u32,
    /// Offset 36..40: H.264/AVC `level_idc` from `avcC` (e.g. `31` = Level 3.1, `40` = Level 4.0, `41` = Level 4.1).
    pub avc_level_idc: u32,
    /// Offset 40..44: Unknown signed int (observed `-1` / `0xFFFFFFFF`).
    pub unk_40: Unknown<i32>,
    /// Offset 44..48: Video frame rate in millihertz (`fps * 1000`, e.g. `25000` for 25.000 fps, `29970` for 29.970 fps, `30416` for 30.416 fps).
    pub frame_rate_milli: u32,
    /// Offset 48..52: Video color bit depth from `VisualSampleEntry` (e.g. `24`).
    pub color_bit_depth: u32,
    /// Offset 52..56: Unknown 4 bytes (observed `256` on both 25 fps 1080p and 29.97 fps 720p Windows videos, and `30000` in AME docs; not frame rate).
    pub unk_52: Unknown<u32>,
    /// Offset 56..84: Unknown 28 bytes (first `u32` observed as `29970` on a 29.970 fps local 720p `.mp4` and `17500` / `0x0000445C` on a 25 fps 1080p `.m4v`, remaining 24 bytes zeros).
    pub unk_56_84: Unknown<[u8; 28]>,
}

impl VisitUnknowns for VideoInfo {
    fn visit_unknowns<V: UnknownVisitor>(&self, _section: &str, visitor: &mut V) {
        visit_unknowns!(
            visitor,
            "VideoInfo",
            [
                (self.unk_8, 8),
                (self.unk_16, 16),
                (self.unk_24, 24),
                (self.unk_28, 28),
                (self.unk_40, 40),
                (self.unk_52, 52),
                (self.unk_56_84, 56),
            ]
        );
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use binrw::{BinRead, BinWrite};

    use super::VideoInfo;

    #[test]
    fn video_info_parses_frame_rate_at_offset_44_and_roundtrips() {
        // Synthetic 84-byte boma 0x0024 payload (e.g. Rush — "Tom Sawyer" 1920x820 25fps H.264 video).
        let mut raw = [0u8; 84];
        raw[0..4].copy_from_slice(&820u32.to_le_bytes()); // height
        raw[4..8].copy_from_slice(&1920u32.to_le_bytes()); // width
        raw[8..12].copy_from_slice(&1u32.to_le_bytes()); // unk_8
        raw[12..16].copy_from_slice(&0x6176_6331u32.to_le_bytes()); // "avc1"
        raw[16..20].copy_from_slice(&6060u32.to_le_bytes()); // unk_16
        raw[20..24].copy_from_slice(&5153u32.to_le_bytes()); // video_bit_rate_kbps
        raw[24..28].copy_from_slice(&5153u32.to_le_bytes()); // unk_24
        raw[32..36].copy_from_slice(&100u32.to_le_bytes()); // avc_profile_idc (High)
        raw[36..40].copy_from_slice(&40u32.to_le_bytes()); // avc_level_idc (4.0)
        raw[40..44].copy_from_slice(&(-1i32).to_le_bytes()); // unk_40
        raw[44..48].copy_from_slice(&25_000u32.to_le_bytes()); // frame_rate_milli (25.000 fps)
        raw[48..52].copy_from_slice(&24u32.to_le_bytes()); // color_bit_depth
        raw[52..56].copy_from_slice(&256u32.to_le_bytes()); // unk_52 (not frame rate)
        raw[56..60].copy_from_slice(&17_500u32.to_le_bytes()); // unk_56_84[0..4]

        let parsed = VideoInfo::read_le(&mut Cursor::new(&raw)).expect("parse VideoInfo");
        assert_eq!(parsed.height, 820);
        assert_eq!(parsed.width, 1920);
        assert_eq!(parsed.codec_fourcc, 0x6176_6331);
        assert_eq!(parsed.video_bit_rate_kbps, 5153);
        assert_eq!(parsed.avc_profile_idc, 100);
        assert_eq!(parsed.avc_level_idc, 40);
        assert_eq!(parsed.frame_rate_milli, 25_000);
        assert_eq!(parsed.color_bit_depth, 24);
        assert_eq!(*parsed.unk_52, 256);

        let mut out = Cursor::new(Vec::new());
        parsed.write_le(&mut out).expect("write VideoInfo");
        assert_eq!(out.into_inner().as_slice(), &raw);
    }
}
