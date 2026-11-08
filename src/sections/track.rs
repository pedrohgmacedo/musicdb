use binrw::binrw;
use serde::{Deserialize, Serialize};

use super::boma::BomaSection;

/// `ltma` (Track List Master) header (typically 92 bytes).
#[binrw]
#[brw(little, magic = b"ltma")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LtmaHeader {
    /// Offset 4: Header size in bytes (typically 92).
    pub header_size: u32,
    /// Offset 8: Number of `itma` track subsections that follow.
    pub track_count: u32,
    /// Offset 12..92: Unknown / zero padding.
    #[br(count = header_size.saturating_sub(12))]
    pub unk_12_end: Vec<u8>,
}

/// Complete `ltma` section with all `itma` track items.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LtmaSection {
    pub header: LtmaHeader,
    #[br(count = header.track_count)]
    pub tracks: Vec<ItmaSection>,
}

/// `itma` (Track Item) header (typically 376 bytes).
#[binrw]
#[brw(little, magic = b"itma")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItmaHeader {
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
    pub unk_28_30: [u8; 2],
    /// Offset 30: Checkbox "Skip when shuffling" (0 or 1).
    pub checkbox_skip_when_shuffling: u8,
    /// Offset 31: Unknown boolean flag (changes 0 -> 1 on first play).
    pub unk_31_played_flag: u8,
    /// Offset 32..38: Unknown 6 bytes (offsets 33, 35 are boolean-like flags).
    pub unk_32_38: [u8; 6],
    /// Offset 38: Checkbox "Album is a compilation of songs by various artists" (0 or 1).
    pub checkbox_album_is_compilation: u8,
    /// Offset 39: Playlist-only / not-in-iCloud-library flag (`<key>Playlist Only</key><true/>` in iTunes XML; 0 = synced to user's iCloud Music Library, 1 = playlist-only or unsynced error).
    pub playlist_only: u8,
    /// Offset 40..42: Unknown 2 bytes.
    pub unk_40_42: [u8; 2],
    /// Offset 42: Checkbox disabled (0 = checked/enabled, 1 = unchecked/disabled).
    pub checkbox_disabled: u8,
    /// Offset 43: Unknown byte (typically 0).
    pub unk_43: u8,
    /// Offset 44: Local custom lyrics flag (1 = custom lyrics set locally in Get Info -> Lyrics, 0 = none).
    pub has_local_custom_lyrics: u8,
    /// Offset 45: Unknown byte.
    pub unk_45: u8,
    /// Offset 46: Unsynced local metadata changes flag (1 = track metadata/options edited locally and pending iCloud sync, 0 = clean/synced).
    pub has_unsynced_local_changes: u8,
    /// Offset 47..49: Unknown 2 bytes.
    pub unk_47_49: [u8; 2],
    /// Offset 49: Checkbox "Part of a gapless album" (`<key>Part Of Gapless Album</key><true/>` in iTunes XML; 0 or 1).
    pub checkbox_part_of_gapless_album: u8,
    /// Offset 50: Checkbox "Remember playback position" (0 or 1).
    pub checkbox_remember_playback_position: u8,
    /// Offset 51: Checkbox "Show composer in all views" (0 or 1).
    pub checkbox_show_composer_in_all_views: u8,
    /// Offset 52: Checkbox "Use work and movement" (0 or 1).
    pub checkbox_use_work_and_movement: u8,
    /// Offset 53: Work and movement active flag (1 when work/movement fields are active, 0 otherwise).
    pub has_work_and_movement_active: u8,
    /// Offset 54: Unknown byte.
    pub unk_54: u8,
    /// Offset 55: Playback or custom start/stop bounds activity flag (changes 0 -> 1 on first play or when playback start/stop bounds are set).
    pub has_playback_or_bounds_activity: u8,
    /// Offset 56: Unknown byte.
    pub unk_56: u8,
    /// Offset 57: Downloaded status (1 = downloaded locally on disk, 3 = in cloud / not downloaded).
    pub downloaded_status: u8,
    /// Offset 58: iCloud Status (`cloud_status`, Smart Playlist field `0x86`: 2 = Matched, 3 = Uploaded, 5 = Error/Ineligible, 8 = Apple Music, 9 = No Longer Available).
    pub cloud_status: u8,
    /// Offset 59: Content rating (0 = default, 1 = explicit, 2 = clean, 4 = parental guidance).
    pub content_rating: u8,
    /// Offset 60..62: Unknown 2 bytes.
    pub unk_60_62: [u8; 2],
    /// Offset 62..64: Suggestion/like flag (0 = none, 1 = removed, 2 = favorite/loved, 3 = suggest less/disliked).
    pub suggestion_flag: u16,
    /// Offset 64: Store/catalog ID presence flag (0 = uploaded local track with no catalog ID, 1 = matched/catalog audio track, 5/6 = music video).
    pub unk_64_store_flag: u8,
    /// Offset 65: Star rating (0..=100 in steps of 20).
    pub star_rating: u8,
    /// Offset 66..72: Sort string mode flags for `[Title, Album, Artist, Album Artist, Composer, _]` (`0x80` = auto-generated sort string, `0x01` = user-overridden custom sort string).
    pub auto_sort_flags: [u8; 6],
    /// Offset 72..82: Unknown 10 bytes (all zeros).
    pub unk_72_82: [u8; 10],
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
    pub unk_96_purchased_album_group: u32,
    /// Offset 100..108: Unknown 8 bytes.
    pub unk_100_108: [u8; 8],
    /// Offset 108..112: Media kind bitflags (1 = music, 0x10 = PDF/booklet, 0x20 = music video, 0x100000 = voice memo).
    pub media_kind: u32,
    /// Offset 112..116: Unknown (typically 1).
    pub unk_112: u32,
    /// Offset 116..118: Total tracks on disc/album.
    pub track_total: u16,
    /// Offset 118..148: Unknown 30 bytes.
    pub unk_118_148: [u8; 30],
    /// Offset 148..152: Playback start position in milliseconds (0 = start of track).
    pub playback_start_ms: u32,
    /// Offset 152..156: Playback stop position in milliseconds (0 = end of track).
    pub playback_stop_ms: u32,
    /// Offset 156..160: Unknown 4 bytes.
    pub unk_156: u32,
    /// Offset 160..162: Track number.
    pub track_number: u16,
    /// Offset 162..168: Unknown 6 bytes.
    pub unk_162_168: [u8; 6],
    /// Offset 168..172: Release year.
    pub year: u32,
    /// Offset 172..180: Album Persistent ID (`iama` reference).
    pub album_persistent_id: u64,
    /// Offset 180..188: Artist / Album Artist Persistent ID (`iAma` reference).
    pub artist_persistent_id: u64,
    /// Offset 188..192: Apple Music Store Artist ID.
    pub apple_music_store_artist_id: u32,
    /// Offset 192..220: Unknown 28 bytes.
    pub unk_192_220: [u8; 28],
    /// Offset 220..228: iCloud Music Library `sagaId` (or `0xFFFFFFFFFFFFFFFD` = -3 for Apple Music subscription tracks).
    pub cloud_saga_id_1: u64,
    /// Offset 228..236: 64-bit iTunes Store / Match ID (or `0x7FFFFFFF_FFFFFFEF` sentinel on some matched tracks).
    pub itunes_store_matched_id: u64,
    /// Offset 236..244: Primary iCloud Music Library `sagaId` (matches `sagaId=` in `boma 0x0038` redownload-params).
    pub cloud_saga_id: u64,
    /// Offset 244..248: Shared 4-byte value (matches `plma` offset 104).
    pub unk_244_shared_val: u32,
    /// Offset 248..252: Unknown 4 bytes.
    pub unk_248: u32,
    /// Offset 252..254: Unknown 2 bytes (typically 0).
    pub unk_252_254: u16,
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
    /// Offset 312..320: Unknown 8 bytes.
    pub unk_312_320: u64,
    /// Offset 320..328: iCloud Music Library `sagaId` repeated (matches offset 220).
    pub cloud_saga_id_2: u64,
    /// Offset 328..332: Unknown flag (typically 3, 5, 0, 7, 8).
    pub unk_328: u32,
    /// Offset 332..336: Unknown 4 bytes.
    pub unk_332: u32,
    /// Offset 336..340: Suggestion flag modified date (Apple timestamp).
    pub date_suggestion_flag_modified: u32,
    /// Offset 340..344: Unknown 4 bytes.
    pub unk_340: u32,
    /// Offset 344..348: Artwork cache size kind in `artwork.sqlite` (`cache_items.size_kind`; 1 = `_sk1`, 0 = none).
    pub artwork_size_kind: u32,
    /// Offset 348..352: Artwork cached image format in `artwork.sqlite` (`cache_items.extension`; 1 = `jpeg`, 2 = `png`, 0 = none).
    pub artwork_image_format: u32,
    /// Offset 352..356: Date added of the most recently added track (only set on that track, else 0).
    pub date_added_most_recent_track: u32,
    /// Offset 356..376: Unknown 20 bytes.
    pub unk_356_376: [u8; 20],
    /// Any extra bytes if `header_size > 376`.
    #[br(count = header_size.saturating_sub(376))]
    pub extra_bytes: Vec<u8>,
}

/// Complete `itma` section (header + `boma` children).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItmaSection {
    pub header: ItmaHeader,
    #[br(count = header.boma_count)]
    pub bomas: Vec<BomaSection>,
}

/// Payload of `boma` subtype `0x0001` (Track Numerics, 364 bytes).
/// Note: Offsets here are relative to the start of the `boma` payload (subtract 20 from raw `boma` offsets).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackNumerics {
    /// Offset 0..4: Global counter.
    pub global_counter: u32,
    /// Offset 4..8: Asset subrecord kind (`1` = local downloaded file subrecord, `3` = cloud/catalog subrecord).
    pub subrecord_kind: u32,
    /// Offset 8..16: Unique 64-bit Track Data ID (`tdID`) for this asset subrecord.
    pub track_data_id: u64,
    /// Offset 16..20: Unknown (typically 1).
    pub unk_16: u32,
    /// Offset 20..24: Unknown flags (`00 01 01 00` or 0).
    pub unk_20_flags: [u8; 4],
    /// Offset 24..27: Unknown 3 bytes (typically 0).
    pub unk_24_27: [u8; 3],
    /// Offset 27: Variable Bit Rate (VBR) flag (`1` = VBR audio track, `0` = CBR).
    pub is_vbr: u8,
    /// Offset 28..32: Local file subrecord flag (`1` on local downloaded file subrecords, `0` on cloud/catalog subrecords).
    pub is_local_subrecord: u32,
    /// Offset 32..34: iTunes Match flag (`[1, 1]` = `<key>Matched</key><true/>` in iTunes XML).
    pub is_itunes_matched: [u8; 2],
    /// Offset 34: Apple Music catalog subscription flag (`1` = `<key>Apple Music</key><true/>` in iTunes XML).
    pub is_apple_music_catalog: u8,
    /// Offset 35..60: Unknown 25 bytes.
    pub unk_35_60: [u8; 25],
    /// Offset 60..64: Sample rate in Hz (IEEE 754 f32, e.g. 44100.0).
    pub sample_rate: f32,
    /// Offset 64..68: File type FourCC in little-endian (e.g. `0x4D503320` = `"MP3 "`, `0x4D344120` = `"M4A "`).
    pub file_type_fourcc: u32,
    /// Offset 68..72: Secondary file type / unknown.
    pub unk_68_file_type: u32,
    /// Offset 72..74: File Folder Count (matches iTunes XML `File Folder Count`, e.g. 5).
    pub file_folder_count: u16,
    /// Offset 74..76: Library Folder Count (matches iTunes XML `Library Folder Count`, e.g. 1).
    pub library_folder_count: u16,
    /// Offset 76..78: Artwork count (matches iTunes XML `Artwork Count`).
    pub artwork_count: u16,
    /// Offset 78..80: Container format kind (`12` / `0x000C` = MP3 audio, `51` / `0x0033` = MPEG-4 audio M4A/M4P, `3` / `0x0003` = MPEG-4 video M4V).
    pub container_format_kind: u16,
    /// Offset 80..82: Embedded artwork presence flag (-1 / 0xFFFF when `artwork_count > 0`, 0 when no artwork).
    pub has_embedded_artwork: i16,
    /// Offset 82..84: Local file artwork state (`2` on local downloaded file subrecords, `0` on cloud/catalog subrecords).
    pub local_artwork_state: u16,
    /// Offset 84..88: Total size in bytes of all attached artworks.
    pub artwork_total_size_bytes: u32,
    /// Offset 88..92: Bit rate in kbps (matches iTunes XML `Bit Rate`).
    pub bit_rate: u32,
    /// Offset 92..96: Date Added (Apple local timestamp; non-zero iff track is in Master Playlist `####!####`).
    pub date_added: u32,
    /// Offset 96..108: Unknown 12 bytes.
    pub unk_96_108: [u8; 12],
    /// Offset 108..112: Gapless playback encoder delay in samples (word 1 of `iTunSMPB`, e.g. 2112 for AAC or 576 for LAME MP3).
    pub gapless_encoder_delay: u32,
    /// Offset 112..116: Gapless playback encoder padding in samples (word 2 of `iTunSMPB`).
    pub gapless_encoder_padding: u32,
    /// Offset 116..120: Gapless playback heuristic / frame info (`0xD87B....` or `0x0100....`).
    pub gapless_heuristic_info: u32,
    /// Offset 120..124: Gapless playback flags (`1` or `0x02000003` when `iTunSMPB` is present).
    pub gapless_playback_flags: u32,
    /// Offset 124..128: Custom lyrics checksum/hash (0 if no custom lyrics).
    pub custom_lyrics_hash: u32,
    /// Offset 128..132: Date Modified (Apple local timestamp).
    pub date_modified: u32,
    /// Offset 132..136: Volume normalization (Sound Check / iTunes XML `Normalization`, `iTunNORM`).
    pub normalization: u32,
    /// Offset 136..140: Purchase Date (Apple timestamp).
    pub date_purchased: u32,
    /// Offset 140..144: Release Date (Apple UTC timestamp).
    pub date_released: u32,
    /// Offset 144..148: Cloud codec kind (`2` = `"mp4a"` MPEG-4 audio, `3` = `"mpeg"` MP3 audio, `0` on local file subrecords and video).
    pub cloud_codec_kind: u32,
    /// Offset 148..152: Codec/Subtype FourCC in little-endian (e.g. `0x6D703461` = `"mp4a"`, `0x6D706567` = `"mpeg"`).
    pub codec_fourcc: u32,
    /// Offset 152..156: Unknown (0 or e.g. `0x00000123`).
    pub unk_152: u32,
    /// Offset 156..160: Track duration in milliseconds (`Total Time` in iTunes XML).
    pub duration_ms: u32,
    /// Offset 160..168: 64-bit Apple Music Store Album ID (`plID` in MP4 atoms; exceeds `u32::MAX` for modern releases).
    pub apple_music_store_album_id: u64,
    /// Offset 168..176: 64-bit Apple Music Store Artist ID (`atID` in MP4 atoms; exceeds `u32::MAX` for modern artists).
    pub apple_music_store_artist_id: u64,
    /// Offset 176..200: Unknown 24 bytes.
    pub unk_176_200: [u8; 24],
    /// Offset 200..204: Shared 4-byte value (matches `plma` offset 104).
    pub unk_200_shared_val: u32,
    /// Offset 204..208: Unknown 4 bytes.
    pub unk_204: u32,
    /// Offset 208..216: 64-bit Apple Music Store Album Artist ID (exceeds `u32::MAX` for modern artists).
    pub apple_music_store_album_artist_id: u64,
    /// Offset 216..256: Unknown 40 bytes.
    #[serde(with = "crate::sections::hex_array")]
    pub unk_216_256: [u8; 40],
    /// Offset 256..264: Gapless playback total sample count (words 3..4 of `iTunSMPB`).
    pub gapless_total_samples: u64,
    /// Offset 264..268: Apple Store Genre ID (`geID` MP4 atom, e.g. 21 = Rock, 14 = Pop, 7 = Electronic, 18 = Hip-Hop/Rap).
    pub store_genre_id: u32,
    /// Offset 268..272: Unknown 4 bytes (typically 0).
    pub unk_268_272: u32,
    /// Offset 272..280: Gapless playback byte offset to the 8th-from-last MPEG frame (word 5 of `iTunSMPB` for MP3s; 0 for M4A).
    pub gapless_last_eight_frames_offset: u64,
    /// Offset 280..284: Shared 4-byte value (matches `plma` offset 104).
    pub unk_280_shared_val: u32,
    /// Offset 284..288: Unknown 4 bytes.
    pub unk_284_288: u32,
    /// Offset 288..296: iCloud Music Library `sagaId` (or `0xFFFFFFFFFFFFFFFD` = -3 for Apple Music catalog tracks).
    pub cloud_saga_id_1: u64,
    /// Offset 296..300: File size in bytes (`Size` in iTunes XML).
    pub file_size: u32,
    /// Offset 300..304: Unknown 4 bytes.
    pub unk_300: u32,
    /// Offset 304..312: 64-bit Apple Music / iTunes Store Track `adamId` (matches `adamId=` in `boma 0x0038`).
    pub store_track_adam_id: u64,
    /// Offset 312..316: Apple iTunes Storefront / Country ID (`sfID`, e.g. 143441 = US, 143503 = Brazil, 143469 = Russia).
    pub storefront_id: u32,
    /// Offset 316..320: Unknown 4 bytes (typically 0).
    pub unk_316_320: u32,
    /// Offset 320..328: 64-bit Apple Music / iTunes Store Track `adamId` repeated.
    pub store_track_adam_id_2: u64,
    /// Offset 328..336: iCloud Music Library `sagaId` (matches `sagaId=` in `boma 0x0038`, or `0xFFFFFFFFFFFFFFFD` for catalog tracks).
    pub cloud_saga_id_2: u64,
    /// Offset 336..340: Unknown 4 bytes (typically 0).
    pub unk_336_340: u32,
    /// Offset 340..348: 64-bit Apple Music / iTunes Store Track `adamId` repeated.
    pub store_track_adam_id_3: u64,
    /// Offset 348..356: 64-bit Apple Music / iTunes Store Track `adamId` repeated.
    pub store_track_adam_id_4: u64,
    /// Offset 356..360: Cloud sync timestamp (Apple UTC timestamp, or 2 on local asset).
    pub date_cloud_synced: u32,
    /// Offset 360..364: Unknown 4 bytes.
    pub unk_360: u32,
}

impl TrackNumerics {
    /// Converts `file_type_fourcc` (e.g. `0x4D503320`) into its 4-character ASCII representation (e.g. `"MP3 "`).
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackPlaysSkips {
    /// Offset 0..8: Track Persistent ID.
    pub track_persistent_id: u64,
    /// Offset 8..12: Last played date (Apple timestamp, 0 if never or reset).
    pub date_last_played: u32,
    /// Offset 12..16: Play count (can be reset in GUI).
    pub play_count: u32,
    /// Offset 16..20: True play count (not affected by GUI reset).
    pub true_play_count: u32,
    /// Offset 20..24: First ever played date in UTC (Apple timestamp).
    pub date_first_played_utc: u32,
    /// Offset 24..28: Unknown 4 bytes.
    pub unk_24: u32,
    /// Offset 28..32: Last skipped date (Apple timestamp).
    pub date_last_skipped: u32,
    /// Offset 32..36: Skip count.
    pub skip_count: u32,
    /// Offset 36..40: True skip count.
    pub true_skip_count: u32,
    /// Offset 40..44: Unknown 4 bytes.
    pub unk_40: u32,
    /// Offset 44..52: Unknown 8 bytes.
    pub unk_44_52: [u8; 8],
}

/// Payload of `boma` subtype `0x0024` (Video Track Metadata, 84 bytes).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoInfo {
    /// Offset 0..4: Vertical resolution (height in pixels).
    pub height: u32,
    /// Offset 4..8: Horizontal resolution (width in pixels).
    pub width: u32,
    /// Offset 8..12: Unknown (typically 1).
    pub unk_8: u32,
    /// Offset 12..16: Video codec FourCC in little-endian (e.g. `0x61766331` = `"avc1"`).
    pub codec_fourcc: u32,
    /// Offset 16..20: Unknown 4 bytes.
    pub unk_16: u32,
    /// Offset 20..24: Unknown (e.g. `0x0000169F`).
    pub unk_20: u32,
    /// Offset 24..28: Unknown (often same as `unk_20`).
    pub unk_24: u32,
    /// Offset 28..32: Unknown 4 bytes.
    pub unk_28: u32,
    /// Offset 32..36: Unknown (e.g. 100).
    pub unk_32: u32,
    /// Offset 36..40: Unknown (e.g. 41).
    pub unk_36: u32,
    /// Offset 40..44: Unknown signed int (e.g. -1).
    pub unk_40: i32,
    /// Offset 44..48: Unknown (e.g. 30416).
    pub unk_44: u32,
    /// Offset 48..52: Unknown (often 24).
    pub unk_48: u32,
    /// Offset 52..56: Unknown (often 30000 - framerate * 1000?).
    pub frame_rate_milli: u32,
    /// Offset 56..84: Unknown 28 bytes.
    pub unk_56_84: [u8; 28],
}
