use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use bitflags::bitflags;
use chrono::{DateTime, TimeZone, Utc};
use directories::UserDirs;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::sections::{
    BomaPayload, HsmaChild, IAmaSection, IamaSection, IsmaSection, ItmaSection, LpmaSection,
    PlmaSection, RawLibraryFile, SlstSection, SmartPlaylistOptions, SmartPlaylistRule,
    SmartRuleChild,
};

/// Seconds between 1904-01-01 00:00:00 UTC (Apple/Mac epoch) and 1970-01-01 00:00:00 UTC (Unix epoch).
pub const APPLE_TO_UNIX_EPOCH_OFFSET: i64 = 2_082_844_800;

/// Converts an Apple 32-bit timestamp (seconds since 1904-01-01 UTC/local) to a `DateTime<Utc>`.
/// Returns `None` if `apple_ts < 86_400` (0 or small non-timestamp sentinel values such as `2` on local `TrackNumerics[0].date_cloud_synced`).
pub fn apple_timestamp_to_utc(apple_ts: u32) -> Option<DateTime<Utc>> {
    if apple_ts < 86_400 {
        return None;
    }
    let unix_secs = (apple_ts as i64) - APPLE_TO_UNIX_EPOCH_OFFSET;
    Utc.timestamp_opt(unix_secs, 0).single()
}

/// Formats a 64-bit Persistent ID as a 16-character uppercase hexadecimal string (matching `iTunes Music Library.xml`).
pub fn format_persistent_id(id: u64) -> String {
    format!("{:016X}", id)
}

/// Formats a 16-byte artwork UUID as standard `XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX` uppercase string.
/// Returns `None` if all 16 bytes are zero.
pub fn format_artwork_uuid(bytes: &[u8; 16]) -> Option<String> {
    if bytes.iter().all(|&b| b == 0) {
        return None;
    }
    Some(format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

bitflags! {
    /// Raw bitflags for `suggestion_flag` (`itma` offset 126..128, `iama` offset 34..36, `iAma` offset 34..36, `lpma` offset 62..64).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct SuggestionFlags: u16 {
        /// Bit 0 (`0x0001`): Set on Removed (`0x0001`), SuggestLess (`0x0003`), and locally modified SuggestLess (`0x0101`).
        const BIT_0            = 0x0001;
        /// Bit 1 (`0x0002`): Set on Favorite (`0x0002`, `0x0102`) and synced SuggestLess (`0x0003`, `0x0103`).
        const FAVORITE         = 0x0002;
        /// Bits 0..=1 (`0x0003`): Synced SuggestLess (`BIT_0 | FAVORITE`).
        const SUGGEST_LESS     = Self::BIT_0.bits() | Self::FAVORITE.bits();
        /// Bit 8 (`0x0100`): Local user-modification flag set when modified before/without cloud sync.
        const LOCALLY_MODIFIED = 0x0100;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuggestionFlag {
    Neutral,
    Removed,
    Favorite,
    SuggestLess,
    Unknown(u16),
}

impl From<u16> for SuggestionFlag {
    fn from(v: u16) -> Self {
        let flags = SuggestionFlags::from_bits_retain(v);
        let local = flags.contains(SuggestionFlags::LOCALLY_MODIFIED);
        let base = flags & !SuggestionFlags::LOCALLY_MODIFIED;

        if base.is_empty() && !local {
            Self::Neutral
        } else if base == SuggestionFlags::FAVORITE {
            Self::Favorite
        } else if base == SuggestionFlags::SUGGEST_LESS || (base == SuggestionFlags::BIT_0 && local)
        {
            Self::SuggestLess
        } else if base == SuggestionFlags::BIT_0 {
            Self::Removed
        } else {
            Self::Unknown(v)
        }
    }
}

impl SuggestionFlag {
    /// Returns the `SuggestionFlags` bitmask representation of a raw `suggestion_flag` value.
    pub fn flags(raw: u16) -> SuggestionFlags {
        SuggestionFlags::from_bits_retain(raw)
    }

    /// Returns `true` if the `0x0100` local user-modified bit is set (e.g. `0x0102` when favorited offline, `0x0101` when marked Suggest Less offline).
    pub fn is_locally_modified(raw: u16) -> bool {
        SuggestionFlags::from_bits_retain(raw).contains(SuggestionFlags::LOCALLY_MODIFIED)
    }
}

/// iCloud Music Library status (`itma` offset 58, Smart Playlist field `0x86` = 134).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloudStatus {
    /// `0`: Local track when iCloud Music Library is disabled or not applicable (`LocalOnly`).
    LocalOnly,
    /// `1`: Purchased from the iTunes Store (`Purchased`).
    Purchased,
    /// `2`: Local track matched by iTunes Match / Apple Music (`Matched`).
    Matched,
    /// `3`: Local track uploaded directly to iCloud Music Library (`Uploaded`).
    Uploaded,
    /// `4`: Track is ineligible for iCloud Music Library (`Ineligible`).
    Ineligible,
    /// `5`: Track failed to upload/match or was removed (`Error` / `Removed`).
    Error,
    /// `6`: Cloud sync error (`SyncError`).
    SyncError,
    /// `7`: Duplicate track skipped by iCloud Music Library (`Duplicate`).
    Duplicate,
    /// `8`: Apple Music subscription catalog track (`Apple Music`).
    AppleMusic,
    /// `9`: Track removed from Apple Music catalog (`No Longer Available`).
    NoLongerAvailable,
    /// `10`: Track not uploaded (`NotUploaded`).
    NotUploaded,
    /// Any other status value.
    Unknown(u8),
}

impl From<u8> for CloudStatus {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::LocalOnly,
            1 => Self::Purchased,
            2 => Self::Matched,
            3 => Self::Uploaded,
            4 => Self::Ineligible,
            5 => Self::Error,
            6 => Self::SyncError,
            7 => Self::Duplicate,
            8 => Self::AppleMusic,
            9 => Self::NoLongerAvailable,
            10 => Self::NotUploaded,
            other => Self::Unknown(other),
        }
    }
}

/// Content advisory rating (`itma` offset 59).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentRating {
    None,
    Explicit,
    Clean,
    ParentalGuidance,
    Unknown(u8),
}

impl From<u8> for ContentRating {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Explicit,
            2 => Self::Clean,
            4 => Self::ParentalGuidance,
            other => Self::Unknown(other),
        }
    }
}

/// Local download vs. cloud-only status (`itma` offset 57).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadedStatus {
    DownloadedLocal,
    CloudOnly,
    Unknown(u8),
}

impl From<u8> for DownloadedStatus {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::DownloadedLocal,
            3 => Self::CloudOnly,
            other => Self::Unknown(other),
        }
    }
}

bitflags! {
    /// Raw bitflags for a track's media kind (`itma` offset 108..112, Smart Playlist field `0x3C` = 60).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct MediaKindFlags: u32 {
        const MUSIC         = 0x0000_0001;
        const MOVIE         = 0x0000_0002;
        const PODCAST       = 0x0000_0004;
        const AUDIOBOOK     = 0x0000_0008;
        const BOOKLET_PDF   = 0x0000_0010;
        const MUSIC_VIDEO   = 0x0000_0020;
        const TV_SHOW       = 0x0000_0040;
        const STAGE_PLAY    = 0x0000_0080;
        const DIGITAL_BOOK  = 0x0000_0100;
        const HOME_VIDEO    = 0x0000_0400;
        const RINGTONE      = 0x0000_4000;
        const DIGITAL_AUDIO = 0x0000_8000;
        const ITUNES_EXTRAS = 0x0001_0000;
        const VOICE_MEMO    = 0x0010_0000;
        const ITUNES_U      = 0x0020_0000;

        /// Composite mask (`0x0000_0005`): Music + Podcast.
        const MUSIC_OR_PODCAST = Self::MUSIC.bits() | Self::PODCAST.bits();
        /// Composite mask (`0x0000_0042`): Movie + TV Show.
        const MOVIE_OR_TV_SHOW = Self::MOVIE.bits() | Self::TV_SHOW.bits();
        /// Composite mask (`0x0010_21B1`): All Music & Audio in Smart Playlists.
        const ALL_MUSIC_AND_AUDIO = 0x0010_21B1;
        /// Composite mask (`0x0020_8004`): Podcast + iTunes U + Digital Audio.
        const PODCAST_OR_VOICE_MEMO = Self::PODCAST.bits() | Self::DIGITAL_AUDIO.bits() | Self::ITUNES_U.bits();
        /// Composite mask (`0x0021_8004`): Podcast + iTunes Extras + iTunes U + Digital Audio.
        const PODCAST_ITUNES_U_OR_VOICE_MEMO = Self::PODCAST_OR_VOICE_MEMO.bits() | Self::ITUNES_EXTRAS.bits();
    }
}

/// Media kind (`itma` offset 108..112, Smart Playlist field `0x3C` = 60).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    Music,
    Movie,
    Podcast,
    Audiobook,
    BookletPdf,
    MusicVideo,
    TvShow,
    MovieOrTvShow,
    HomeVideo,
    Ringtone,
    ItunesExtras,
    VoiceMemo,
    AllMusicAndAudio,
    PodcastOrVoiceMemo,
    PodcastItunesUOrVoiceMemo,
    Other(u32),
}

impl MediaKind {
    /// Returns the `MediaKindFlags` bitmask representation of a raw `media_kind` value.
    pub fn flags(raw: u32) -> MediaKindFlags {
        MediaKindFlags::from_bits_retain(raw)
    }
}

impl From<u32> for MediaKind {
    fn from(v: u32) -> Self {
        match MediaKindFlags::from_bits_retain(v) {
            MediaKindFlags::MUSIC => Self::Music,
            MediaKindFlags::MOVIE => Self::Movie,
            MediaKindFlags::PODCAST | MediaKindFlags::MUSIC_OR_PODCAST => Self::Podcast,
            MediaKindFlags::AUDIOBOOK => Self::Audiobook,
            MediaKindFlags::BOOKLET_PDF => Self::BookletPdf,
            MediaKindFlags::MUSIC_VIDEO => Self::MusicVideo,
            MediaKindFlags::TV_SHOW => Self::TvShow,
            MediaKindFlags::MOVIE_OR_TV_SHOW => Self::MovieOrTvShow,
            MediaKindFlags::HOME_VIDEO => Self::HomeVideo,
            MediaKindFlags::RINGTONE => Self::Ringtone,
            MediaKindFlags::ITUNES_EXTRAS => Self::ItunesExtras,
            MediaKindFlags::VOICE_MEMO => Self::VoiceMemo,
            MediaKindFlags::ALL_MUSIC_AND_AUDIO => Self::AllMusicAndAudio,
            MediaKindFlags::PODCAST_OR_VOICE_MEMO => Self::PodcastOrVoiceMemo,
            MediaKindFlags::PODCAST_ITUNES_U_OR_VOICE_MEMO => Self::PodcastItunesUOrVoiceMemo,
            _ => Self::Other(v),
        }
    }
}

bitflags! {
    /// Raw bitflags for a sortable text field's sort mode (`itma` offset 66..72).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct SortModeFlags: u8 {
        /// Bit 0 (`0x01`): A sort string (`boma` `0x001E..0x0022`) is populated on the track.
        const HAS_SORT_STRING = 0x01;
        /// Bit 1 (`0x02`): User explicitly customized/overrode the sort string.
        const CUSTOM_OVERRIDE = 0x02;
        /// Bit 7 (`0x80`): Sort string was auto-generated by Apple Music (e.g. stripping leading `"The "` or `"A "`).
        const AUTO_GENERATED  = 0x80;
    }
}

/// Sort string generation mode (`itma` offset 66..72).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
    None,
    AutoGenerated,
    CustomUserOverride,
    Unknown(u8),
}

impl SortMode {
    /// Returns the `SortModeFlags` bitmask representation of a raw sort mode byte.
    pub fn flags(raw: u8) -> SortModeFlags {
        SortModeFlags::from_bits_retain(raw)
    }
}

impl From<u8> for SortMode {
    fn from(v: u8) -> Self {
        let flags = SortModeFlags::from_bits_retain(v);
        if flags.is_empty() {
            Self::None
        } else if flags.contains(SortModeFlags::AUTO_GENERATED)
            && (flags & !(SortModeFlags::AUTO_GENERATED | SortModeFlags::HAS_SORT_STRING))
                .is_empty()
        {
            Self::AutoGenerated
        } else if flags.contains(SortModeFlags::HAS_SORT_STRING)
            && (flags & !(SortModeFlags::HAS_SORT_STRING | SortModeFlags::CUSTOM_OVERRIDE))
                .is_empty()
        {
            Self::CustomUserOverride
        } else {
            Self::Unknown(v)
        }
    }
}

/// Decoded sort modes for a track's 5 sortable text fields (`itma` offset 66..71).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackSortModes {
    pub title: SortMode,
    pub album: SortMode,
    pub artist: SortMode,
    pub album_artist: SortMode,
    pub composer: SortMode,
}

impl From<[u8; 6]> for TrackSortModes {
    fn from(bytes: [u8; 6]) -> Self {
        Self {
            title: SortMode::from(bytes[0]),
            album: SortMode::from(bytes[1]),
            artist: SortMode::from(bytes[2]),
            album_artist: SortMode::from(bytes[3]),
            composer: SortMode::from(bytes[4]),
        }
    }
}

/// Decoded Equalizer preset (`boma 0x0007`, e.g. `"#!#116#!#"` -> `preset_id: Some(116), name: "Rock"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EqualizerPreset {
    pub raw: String,
    pub preset_id: Option<u32>,
    pub name: String,
}

impl EqualizerPreset {
    pub fn parse(raw: &str) -> Self {
        if let Some(inner) = raw.strip_prefix("#!#").and_then(|s| s.strip_suffix("#!#"))
            && let Ok(id) = inner.parse::<u32>()
        {
            let name = match id {
                100 => "Acoustic",
                101 => "Bass Booster",
                102 => "Bass Reducer",
                103 => "Classical",
                104 => "Dance",
                105 => "Deep",
                106 => "Electronic",
                107 => "Flat",
                108 => "Hip-Hop",
                109 => "Jazz",
                110 => "Latin",
                111 => "Loudness",
                112 => "Lounge",
                113 => "Piano",
                114 => "Pop",
                115 => "R&B",
                116 => "Rock",
                117 => "Small Speakers",
                118 => "Spoken Word",
                119 => "Treble Booster",
                120 => "Treble Reducer",
                121 => "Vocal Booster",
                _ => "Built-in Preset",
            };
            return Self {
                raw: raw.to_string(),
                preset_id: Some(id),
                name: name.to_string(),
            };
        }
        Self {
            raw: raw.to_string(),
            preset_id: None,
            name: raw.to_string(),
        }
    }
}

/// Cloud visibility / sharing mode of a playlist (`lpma` offset 308..312).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaylistShareMode {
    Private,
    SubscribedCatalog,
    PublicShared,
    Unknown(u32),
}

impl From<u32> for PlaylistShareMode {
    fn from(v: u32) -> Self {
        match v {
            0 => Self::Private,
            2 => Self::SubscribedCatalog,
            4 => Self::PublicShared,
            other => Self::Unknown(other),
        }
    }
}

/// Built-in Apple Music system playlist kind (`lpma` offset 79..81).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistinguishedPlaylistKind {
    None,
    Movies,
    TvShows,
    Music,
    Audiobooks,
    Podcasts,
    VoiceMemos,
    Purchased,
    ITunesDj,
    Genius,
    MyTopRated,
    RecentlyPlayed,
    RecentlyAdded,
    MusicVideos,
    ClassicalMusic,
    FavoriteSongs,
    TvAndMovies,
    Downloaded,
    Other(u16),
}

impl From<u16> for DistinguishedPlaylistKind {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::None,
            2 => Self::Movies,
            3 => Self::TvShows,
            4 => Self::Music,
            5 => Self::Audiobooks,
            10 => Self::Podcasts,
            14 => Self::VoiceMemos,
            19 => Self::Purchased,
            22 => Self::ITunesDj,
            26 => Self::Genius,
            42 => Self::MyTopRated,
            43 => Self::RecentlyPlayed,
            44 => Self::RecentlyAdded,
            47 => Self::MusicVideos,
            48 => Self::ClassicalMusic,
            49 => Self::FavoriteSongs,
            64 => Self::TvAndMovies,
            65 | 66 => Self::Downloaded,
            other => Self::Other(other),
        }
    }
}

/// High-level semantic classification of a playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaylistKind {
    Master,
    Distinguished,
    Folder,
    Smart,
    SubscribedCatalog,
    Standard,
}

/// Smart Playlist configuration (`boma 0x00CA` options + `boma 0x00C9` `SLst` rule tree).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartPlaylistConfig {
    pub live_updating: bool,
    pub match_rules: bool,
    pub match_only_checked_items: bool,
    pub limit: Option<SmartPlaylistLimit>,
    pub rule_group: Option<SmartRuleGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartPlaylistLimit {
    pub count: u32,
    pub unit: SmartLimitUnit,
    pub selection: SmartSelectionMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmartLimitUnit {
    Minutes,
    Megabytes,
    Items,
    Hours,
    Gigabytes,
    Unknown(u8),
}

impl From<u8> for SmartLimitUnit {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::Minutes,
            2 => Self::Megabytes,
            3 => Self::Items,
            4 => Self::Hours,
            5 => Self::Gigabytes,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmartSelectionMethod {
    Random,
    Title,
    Album,
    Artist,
    Genre,
    MostRecentlyAdded,
    LeastRecentlyAdded,
    MostOftenPlayed,
    LeastOftenPlayed,
    MostRecentlyPlayed,
    LeastRecentlyPlayed,
    HighestRating,
    LowestRating,
    Unknown { method: u8, negate: u8 },
}

impl SmartSelectionMethod {
    pub fn from_raw(method: u8, negate: u8) -> Self {
        match (method, negate) {
            (0x02, _) => Self::Random,
            (0x05, _) => Self::Title,
            (0x06, _) => Self::Album,
            (0x07, _) => Self::Artist,
            (0x09, _) => Self::Genre,
            (0x15, 0) => Self::MostRecentlyAdded,
            (0x15, _) => Self::LeastRecentlyAdded,
            (0x19, 0) => Self::MostOftenPlayed,
            (0x19, _) => Self::LeastOftenPlayed,
            (0x1A, 0) => Self::MostRecentlyPlayed,
            (0x1A, _) => Self::LeastRecentlyPlayed,
            (0x1C, 0) => Self::HighestRating,
            (0x1C, _) => Self::LowestRating,
            (m, n) => Self::Unknown {
                method: m,
                negate: n,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmartConjunction {
    All,
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartRuleGroup {
    pub conjunction: SmartConjunction,
    pub rules: Vec<SmartRuleNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "node_type")]
pub enum SmartRuleNode {
    Group(SmartRuleGroup),
    Condition(SmartCondition),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmartField {
    Title,
    Album,
    Artist,
    BitRate,
    SampleRate,
    Year,
    Genre,
    Kind,
    DateModified,
    TrackNumber,
    Size,
    Time,
    Comments,
    DateAdded,
    Composer,
    Plays,
    LastPlayed,
    DiscNumber,
    Rating,
    Checked,
    Compilation,
    Bpm,
    AlbumArtwork,
    Grouping,
    Playlist,
    Purchased,
    Description,
    Category,
    AlbumRating,
    MediaKind,
    Skips,
    LastSkipped,
    AlbumArtist,
    SortTitle,
    SortAlbum,
    SortArtist,
    SortAlbumArtist,
    SortComposer,
    SortShow,
    VideoRating,
    Location,
    CloudStatus,
    Favorite,
    AlbumFavorite,
    Work,
    MovementName,
    MovementNumber,
    HasVideo,
    Unknown(u32),
}

impl From<u32> for SmartField {
    fn from(id: u32) -> Self {
        match id {
            0x02 => Self::Title,
            0x03 => Self::Album,
            0x04 => Self::Artist,
            0x05 => Self::BitRate,
            0x06 => Self::SampleRate,
            0x07 => Self::Year,
            0x08 => Self::Genre,
            0x09 => Self::Kind,
            0x0A => Self::DateModified,
            0x0B => Self::TrackNumber,
            0x0C | 0x1C => Self::Size,
            0x0D => Self::Time,
            0x0E => Self::Comments,
            0x10 => Self::DateAdded,
            0x12 => Self::Composer,
            0x16 => Self::Plays,
            0x17 => Self::LastPlayed,
            0x18 => Self::DiscNumber,
            0x19 => Self::Rating,
            0x1D => Self::Checked,
            0x1F => Self::Compilation,
            0x23 => Self::Bpm,
            0x25 => Self::AlbumArtwork,
            0x27 => Self::Grouping,
            0x28 => Self::Playlist,
            0x29 => Self::Purchased,
            0x36 => Self::Description,
            0x37 => Self::Category,
            0x39 | 0x5A => Self::AlbumRating,
            0x3C => Self::MediaKind,
            0x44 => Self::Skips,
            0x45 => Self::LastSkipped,
            0x47 => Self::AlbumArtist,
            0x4E => Self::SortTitle,
            0x4F => Self::SortAlbum,
            0x50 => Self::SortArtist,
            0x51 => Self::SortAlbumArtist,
            0x52 => Self::SortComposer,
            0x53 => Self::SortShow,
            0x59 => Self::VideoRating,
            0x85 => Self::Location,
            0x86 => Self::CloudStatus,
            0x9A => Self::Favorite,
            0x9C => Self::AlbumFavorite,
            0x9E | 0x9F => Self::Work,
            0xA0 => Self::MovementName,
            0xA1 => Self::MovementNumber,
            0xA4 => Self::HasVideo,
            other => Self::Unknown(other),
        }
    }
}

bitflags! {
    /// Raw bitflags for a Smart Playlist comparison operator (`SLst` rule offset 4..8, Big-Endian).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct SmartOperatorFlags: u32 {
        const IS            = 0x0000_0001;
        const CONTAINS      = 0x0000_0002;
        const STARTS_WITH   = 0x0000_0004;
        const ENDS_WITH     = 0x0000_0008;
        const GREATER_THAN  = 0x0000_0010;
        const GREATER_OR_EQ = 0x0000_0020;
        const LESS_THAN     = 0x0000_0040;
        const LESS_OR_EQ    = 0x0000_0080;
        const IN_RANGE      = 0x0000_0100;
        const IN_THE_LAST   = 0x0000_0200;
        const BITWISE_SET   = 0x0000_0400;
        const STRING_MODE   = 0x0100_0000;
        const NEGATE        = 0x0200_0000;
        const SIGN_MODIFIER = 0x0400_0000;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmartOperator {
    Is,
    IsNot,
    Contains,
    DoesNotContain,
    StartsWith,
    EndsWith,
    GreaterThan,
    LessThan,
    InRange,
    NotInRange,
    InTheLast,
    NotInTheLast,
    Unknown(u32),
}

impl SmartOperator {
    /// Returns the `SmartOperatorFlags` bitmask representation of a raw `SLst` operator value.
    pub fn flags(raw: u32) -> SmartOperatorFlags {
        SmartOperatorFlags::from_bits_retain(raw)
    }
}

impl From<u32> for SmartOperator {
    fn from(cmp: u32) -> Self {
        let flags = SmartOperatorFlags::from_bits_retain(cmp);
        let negated = flags.contains(SmartOperatorFlags::NEGATE);
        let base = flags
            & !(SmartOperatorFlags::STRING_MODE
                | SmartOperatorFlags::NEGATE
                | SmartOperatorFlags::SIGN_MODIFIER);

        match (base, negated) {
            (SmartOperatorFlags::IS | SmartOperatorFlags::BITWISE_SET, false) => Self::Is,
            (SmartOperatorFlags::IS | SmartOperatorFlags::BITWISE_SET, true) => Self::IsNot,
            (SmartOperatorFlags::CONTAINS, false) => Self::Contains,
            (SmartOperatorFlags::CONTAINS, true) => Self::DoesNotContain,
            (SmartOperatorFlags::STARTS_WITH, false) => Self::StartsWith,
            (SmartOperatorFlags::ENDS_WITH, false) => Self::EndsWith,
            (SmartOperatorFlags::GREATER_THAN, false) => Self::GreaterThan,
            (SmartOperatorFlags::LESS_THAN, false) => Self::LessThan,
            (SmartOperatorFlags::IN_RANGE, false) => Self::InRange,
            (SmartOperatorFlags::IN_RANGE, true) => Self::NotInRange,
            (SmartOperatorFlags::IN_THE_LAST, false) => Self::InTheLast,
            (SmartOperatorFlags::IN_THE_LAST, true) => Self::NotInTheLast,
            _ => Self::Unknown(cmp),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelativeTimeUnit {
    Days,
    Weeks,
    Months,
    Unknown(i64),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "value_type")]
pub enum SmartRuleValue {
    Text {
        text: String,
    },
    Boolean {
        value: bool,
    },
    Integer {
        value: i64,
    },
    IntegerRange {
        min: i64,
        max: i64,
    },
    StarRating {
        stars: u8,
        raw_0_100: i64,
    },
    StarRatingRange {
        min_stars: u8,
        max_stars: u8,
        min_raw: i64,
        max_raw: i64,
    },
    RelativeTime {
        amount: u64,
        unit: RelativeTimeUnit,
        seconds_per_unit: i64,
    },
    Date {
        apple_timestamp: i64,
        utc: Option<DateTime<Utc>>,
    },
    DateRange {
        start_apple: i64,
        end_apple: i64,
        start_utc: Option<DateTime<Utc>>,
        end_utc: Option<DateTime<Utc>>,
    },
    PlaylistRef {
        playlist_persistent_id: String,
        playlist_name: Option<String>,
    },
    CloudStatus {
        status: CloudStatus,
    },
    SuggestionFlag {
        flag: SuggestionFlag,
    },
    MediaKind {
        kind: MediaKind,
    },
    LocationKind {
        code: i64,
        name: String,
    },
    Raw {
        hex: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartCondition {
    pub field_id: u32,
    pub field: SmartField,
    pub operator: SmartOperator,
    pub comparison_raw: u32,
    pub value: SmartRuleValue,
    pub description: String,
}

/// View & sorting preferences loaded from `.musiclibrary/preferences/Playlist_<ID>.plist` or `Songs.plist`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistViewPreferences {
    pub view_as: Option<String>,
    pub sort_field: Option<String>,
    pub sort_ascending: Option<bool>,
    pub visible_columns: Vec<String>,
}

/// High-level Domain Model of a parsed `.musicdb` library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub metadata: LibraryMetadata,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,
    pub social_profiles: Vec<SocialProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialProfile {
    pub persistent_id: String,
    pub persistent_id_u64: u64,
    pub profile_id: Option<String>,
    pub name: Option<String>,
    pub handle: Option<String>,
    pub avatar_id: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryMetadata {
    pub persistent_id: String,
    pub persistent_id_u64: u64,
    pub itunes_persistent_id: Option<String>,
    pub revision_hash: String,
    pub file_size_mirror: u32,
    pub app_version: String,
    pub format_major_version: u16,
    pub format_minor_version: u16,
    pub file_type: u32,
    pub declared_track_count: u32,
    pub declared_album_count: u32,
    pub declared_artist_count: u32,
    pub declared_playlist_count: u32,
    pub timezone_offset_seconds: i32,
    pub apple_store_account_id: u64,
    pub date_modified: Option<DateTime<Utc>>,
    pub show_song_list_checkboxes: Option<bool>,
    pub keep_media_folder_organized: Option<bool>,
    pub copy_files_to_media_folder: Option<bool>,
    pub media_folder_uri: Option<String>,
    pub media_folder_path: Option<String>,
    pub imported_itl_path: Option<String>,
    pub songs_view_preferences: Option<PlaylistViewPreferences>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub persistent_id: String,
    pub persistent_id_u64: u64,
    pub itunes_persistent_id: Option<String>,
    pub first_track_persistent_id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub cloud_album_id: Option<String>,
    pub editorial_notes: Option<String>,
    pub is_compilation: bool,
    pub star_rating: u8,
    pub stars: u8,
    pub star_rating_is_computed: bool,
    pub suggestion_flag: SuggestionFlag,
    pub suggestion_locally_modified: bool,
    pub date_suggestion_flag_modified: Option<DateTime<Utc>>,
    pub date_last_played: Option<DateTime<Utc>>,
    pub date_cloud_synced: Option<DateTime<Utc>>,
    pub date_released: Option<DateTime<Utc>>,
    pub apple_music_store_album_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    pub persistent_id: String,
    pub persistent_id_u64: u64,
    pub itunes_persistent_id: Option<String>,
    pub name: Option<String>,
    pub sort_name: Option<String>,
    pub cloud_artist_id: Option<String>,
    pub suggestion_flag: SuggestionFlag,
    pub suggestion_locally_modified: bool,
    pub date_suggestion_flag_modified: Option<DateTime<Utc>>,
    pub date_cloud_synced: Option<DateTime<Utc>>,
    pub apple_music_store_artist_id: u64,
    pub artwork_uuid: Option<String>,
    pub artwork_url_plist: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub persistent_id: String,
    pub persistent_id_u64: u64,
    pub title: Option<String>,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub composer: Option<String>,
    pub genre: Option<String>,
    pub store_genre_id: Option<u32>,
    pub kind: Option<String>,
    pub comments: Option<String>,
    pub grouping: Option<String>,
    pub work_name: Option<String>,
    pub movement_name: Option<String>,
    pub movement_number: u16,
    pub movement_total: u16,
    pub sort_title: Option<String>,
    pub sort_album: Option<String>,
    pub sort_artist: Option<String>,
    pub sort_album_artist: Option<String>,
    pub sort_composer: Option<String>,
    pub sort_modes: TrackSortModes,
    pub auto_sort_flags: [u8; 6],
    pub track_number: u16,
    pub track_total: u16,
    pub disc_number: u16,
    pub disc_total: u16,
    pub year: u32,
    pub bpm: u16,
    pub duration_ms: Option<u32>,
    pub file_size: Option<u32>,
    pub bit_rate: Option<u32>,
    pub is_vbr: Option<bool>,
    pub sample_rate: Option<f32>,
    pub file_type_fourcc: Option<String>,
    pub codec_fourcc: Option<String>,
    pub volume_adjustment: i32,
    pub normalization: Option<u32>,
    pub gapless_encoder_delay: Option<u32>,
    pub gapless_encoder_padding: Option<u32>,
    pub gapless_total_samples: Option<u64>,
    pub gapless_last_eight_frames_offset: Option<u64>,
    pub equalizer: Option<String>,
    pub equalizer_preset: Option<EqualizerPreset>,
    pub star_rating: u8,
    pub stars: u8,
    pub suggestion_flag: SuggestionFlag,
    pub disabled: bool,
    pub skip_when_shuffling: bool,
    pub remember_playback_position: bool,
    pub show_composer_in_all_views: bool,
    pub use_work_and_movement: bool,
    pub album_is_compilation: bool,
    pub part_of_gapless_album: bool,
    pub playlist_only: bool,
    pub purchased: bool,
    pub has_custom_lyrics: bool,
    pub has_local_custom_lyrics: bool,
    pub has_unsynced_local_changes: bool,
    pub custom_lyrics_hash: Option<u32>,
    pub is_itunes_matched: Option<bool>,
    pub is_apple_music_catalog: Option<bool>,
    pub cloud_saga_id: Option<u64>,
    pub store_track_adam_id: Option<u64>,
    pub apple_music_store_album_id: Option<u64>,
    pub apple_music_store_artist_id: Option<u64>,
    pub apple_music_store_album_artist_id: Option<u64>,
    pub storefront_id: Option<u32>,
    pub purchaser_email: Option<String>,
    pub purchaser_name: Option<String>,
    pub downloaded_status: u8,
    pub downloaded_state: DownloadedStatus,
    pub cloud_status: CloudStatus,
    pub cloud_status_raw: u8,
    pub cloud_album_id: Option<String>,
    pub cloud_artist_id: Option<String>,
    pub cloud_artwork_token: Option<String>,
    pub cloud_lyrics_token: Option<String>,
    pub content_rating: u8,
    pub content_advisory: ContentRating,
    pub media_kind: u32,
    pub media_type: MediaKind,
    pub playback_start_ms: u32,
    pub playback_stop_ms: u32,
    pub play_count: Option<u32>,
    pub true_play_count: Option<u32>,
    pub skip_count: Option<u32>,
    pub true_skip_count: Option<u32>,
    pub date_added: Option<DateTime<Utc>>,
    pub date_modified: Option<DateTime<Utc>>,
    pub date_last_played: Option<DateTime<Utc>>,
    pub date_first_played_utc: Option<DateTime<Utc>>,
    pub date_last_skipped: Option<DateTime<Utc>>,
    pub date_released: Option<DateTime<Utc>>,
    pub date_purchased: Option<DateTime<Utc>>,
    pub date_cloud_synced: Option<DateTime<Utc>>,
    pub file_url: Option<String>,
    pub legacy_file_path: Option<String>,
    pub book_path: Option<String>,
    pub album_persistent_id: Option<String>,
    pub artist_persistent_id: Option<String>,
    pub artwork_uuid: Option<String>,
    pub artwork_count: Option<u16>,
    pub has_embedded_artwork: Option<bool>,
    pub artwork_total_size_bytes: Option<u32>,
    pub file_folder_count: Option<u16>,
    pub library_folder_count: Option<u16>,
    pub isrc: Option<String>,
    pub copyright: Option<String>,
    pub extra_strings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub persistent_id: String,
    pub persistent_id_u64: u64,
    pub parent_persistent_id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub kind: PlaylistKind,
    pub is_master: bool,
    pub is_folder: bool,
    pub is_smart: bool,
    pub show_in_profile_and_search: bool,
    pub distinguished_kind: u16,
    pub system_kind: DistinguishedPlaylistKind,
    pub suggestion_flag: SuggestionFlag,
    pub date_created: Option<DateTime<Utc>>,
    pub date_modified: Option<DateTime<Utc>>,
    pub date_last_played: Option<DateTime<Utc>>,
    pub date_cloud_published: Option<DateTime<Utc>>,
    pub date_cloud_synced: Option<DateTime<Utc>>,
    pub play_count: u32,
    pub true_play_count: u32,
    pub refresh_interval_seconds: u32,
    pub cloud_share_mode: u32,
    pub share_mode: PlaylistShareMode,
    pub artwork_template_kind: u32,
    pub cloud_saga_id: Option<u64>,
    pub universal_library_id: Option<String>,
    pub cloud_uuid: Option<String>,
    pub subscribed_container_url: Option<String>,
    pub owner_name: Option<String>,
    pub owner_id: Option<String>,
    pub author_social_profile_persistent_id: Option<String>,
    pub cloud_artwork_url: Option<String>,
    pub cloud_artwork_token: Option<String>,
    pub artwork_uuid: Option<String>,
    pub declared_track_count: u32,
    pub track_persistent_ids: Vec<String>,
    pub child_playlist_persistent_ids: Vec<String>,
    pub smart_playlist: Option<SmartPlaylistConfig>,
    pub view_preferences: Option<PlaylistViewPreferences>,
}

/// Searches the OS user Music directory (via `directories::UserDirs`) for a default
/// Apple Music `.musiclibrary` bundle (`Apple Music/Apple Music Library.musiclibrary` on Windows,
/// `Music/Music Library.musiclibrary` on macOS).
pub fn discover_default_library_path() -> Option<PathBuf> {
    let user_dirs = UserDirs::new()?;
    let music_dir = user_dirs
        .audio_dir()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| user_dirs.home_dir().join("Music"));

    let candidates = [
        music_dir
            .join("Apple Music")
            .join("Apple Music Library.musiclibrary"),
        music_dir.join("Music").join("Music Library.musiclibrary"),
        music_dir.join("Apple Music Library.musiclibrary"),
        music_dir.join("Music Library.musiclibrary"),
    ];

    candidates.into_iter().find(|p| p.exists())
}

impl Library {
    /// Opens an Apple Music library from the user's default OS Music directory if found.
    pub fn open_default() -> Result<Self> {
        let path = discover_default_library_path().ok_or_else(|| {
            crate::error::MusicDbError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not locate a default Apple Music Library in the user's Music directory",
            ))
        })?;
        Self::open(path)
    }

    /// Opens an Apple Music library from either a `.musiclibrary` bundle directory
    /// or a `Library.musicdb` file path, automatically enriching with `.plist` sidecar preferences
    /// (`Preferences.plist`, `preferences/Songs.plist`, `preferences/Playlist_<ID>.plist`) when present.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let p = path.as_ref();
        let raw = RawLibraryFile::open(p)?;
        let mut lib = Self::from_raw(&raw);
        let bundle_dir = if p.is_dir() {
            Some(p.to_path_buf())
        } else {
            p.parent().map(Path::to_path_buf)
        };
        if let Some(dir) = bundle_dir {
            lib.load_sidecar_preferences(&dir);
        }
        Ok(lib)
    }

    /// Builds the high-level `Library` domain model from a parsed `RawLibraryFile`.
    pub fn from_raw(raw: &RawLibraryFile) -> Self {
        let h = &raw.outer_header;
        let mut metadata = LibraryMetadata {
            persistent_id: format_persistent_id(h.library_persistent_id),
            persistent_id_u64: h.library_persistent_id,
            itunes_persistent_id: (h.itunes_library_persistent_id != 0)
                .then(|| format_persistent_id(h.itunes_library_persistent_id)),
            revision_hash: format_persistent_id(h.revision_hash),
            file_size_mirror: h.file_size_mirror,
            app_version: h.version_string(),
            format_major_version: h.major_version,
            format_minor_version: h.minor_version,
            file_type: h.musicdb_file_type,
            declared_track_count: h.track_count,
            declared_album_count: h.album_count,
            declared_artist_count: h.artist_count,
            declared_playlist_count: h.playlist_count,
            timezone_offset_seconds: h.timezone_offset_seconds,
            apple_store_account_id: h.apple_store_account_id,
            date_modified: apple_timestamp_to_utc(h.date_modified),
            show_song_list_checkboxes: None,
            keep_media_folder_organized: None,
            copy_files_to_media_folder: None,
            media_folder_uri: None,
            media_folder_path: None,
            imported_itl_path: None,
            songs_view_preferences: None,
        };

        let mut albums = Vec::new();
        let mut artists = Vec::new();
        let mut tracks = Vec::new();
        let mut playlists = Vec::new();
        let mut social_profiles = Vec::new();

        for hsma in &raw.sections {
            match &hsma.child {
                HsmaChild::LibraryMaster(plma) => {
                    populate_plma_metadata(plma, &mut metadata);
                }
                HsmaChild::AlbumList(lama) => {
                    albums.extend(lama.albums.iter().map(Album::from_raw));
                }
                HsmaChild::ArtistList(l_ama) => {
                    artists.extend(l_ama.artists.iter().map(Artist::from_raw));
                }
                HsmaChild::TrackList(ltma) => {
                    tracks.extend(ltma.tracks.iter().map(Track::from_raw));
                }
                HsmaChild::PlaylistList(l_pma) => {
                    playlists.extend(l_pma.playlists.iter().map(Playlist::from_raw));
                }
                HsmaChild::SocialProfileList(lsma) => {
                    social_profiles.extend(lsma.profiles.iter().map(SocialProfile::from_raw));
                }
                HsmaChild::InnerHfma(_) | HsmaChild::Padding(_) | HsmaChild::Unknown { .. } => {}
            }
        }

        // Resolve cross-playlist references (folder children & target playlist names in Smart Playlist rules).
        let playlist_names: HashMap<String, String> = playlists
            .iter()
            .filter_map(|pl| {
                pl.name
                    .as_ref()
                    .map(|n| (pl.persistent_id.clone(), n.clone()))
            })
            .collect();
        let mut children_by_parent: HashMap<String, Vec<String>> = HashMap::new();
        for pl in &playlists {
            if let Some(parent_id) = &pl.parent_persistent_id {
                children_by_parent
                    .entry(parent_id.clone())
                    .or_default()
                    .push(pl.persistent_id.clone());
            }
        }
        for pl in &mut playlists {
            if let Some(children) = children_by_parent.remove(&pl.persistent_id) {
                pl.child_playlist_persistent_ids = children;
            }
            if let Some(smart) = &mut pl.smart_playlist
                && let Some(group) = &mut smart.rule_group
            {
                resolve_playlist_names_in_group(group, &playlist_names);
            }
        }

        Self {
            metadata,
            albums,
            artists,
            tracks,
            playlists,
            social_profiles,
        }
    }

    /// Loads `.musiclibrary` sidecar `.plist` files (`Preferences.plist`, `preferences/Songs.plist`,
    /// and `preferences/Playlist_<PersistentID>.plist`) into the domain model if they exist in `bundle_dir`.
    pub fn load_sidecar_preferences(&mut self, bundle_dir: &Path) {
        let prefs_plist = bundle_dir.join("Preferences.plist");
        if prefs_plist.is_file()
            && let Ok(plist::Value::Dictionary(root)) = plist::Value::from_file(&prefs_plist)
            && let Some(copy_flag) = root
                .get("com.apple.Music")
                .and_then(|v| v.as_dictionary())
                .and_then(|d| d.get(" CopyFilesToMediaFolder"))
                .or_else(|| {
                    root.get("com.apple.Music")
                        .and_then(|v| v.as_dictionary())
                        .and_then(|d| d.get("CopyFilesToMediaFolder"))
                })
                .and_then(|v| v.as_boolean())
        {
            self.metadata.copy_files_to_media_folder = Some(copy_flag);
        }

        let prefs_subdir = if bundle_dir.join("preferences").is_dir() {
            bundle_dir.join("preferences")
        } else {
            bundle_dir.join("Preferences")
        };
        let songs_plist = prefs_subdir.join("Songs.plist");
        if songs_plist.is_file()
            && let Some(vp) = parse_playlist_view_plist(&songs_plist)
        {
            self.metadata.songs_view_preferences = Some(vp);
        }

        if prefs_subdir.is_dir() {
            for pl in &mut self.playlists {
                let candidate_upper =
                    prefs_subdir.join(format!("Playlist_{}.plist", pl.persistent_id));
                let candidate_lower = prefs_subdir.join(format!(
                    "Playlist_{}.plist",
                    pl.persistent_id.to_lowercase()
                ));
                if candidate_upper.is_file() {
                    pl.view_preferences = parse_playlist_view_plist(&candidate_upper);
                } else if candidate_lower.is_file() {
                    pl.view_preferences = parse_playlist_view_plist(&candidate_lower);
                }
            }
        }
    }

    /// Finds a track by its 16-char hex Persistent ID (case-insensitive).
    pub fn track_by_id(&self, persistent_id: &str) -> Option<&Track> {
        self.tracks
            .iter()
            .find(|t| t.persistent_id.eq_ignore_ascii_case(persistent_id))
    }

    /// Finds an album by its 16-char hex Persistent ID (case-insensitive).
    pub fn album_by_id(&self, persistent_id: &str) -> Option<&Album> {
        self.albums
            .iter()
            .find(|a| a.persistent_id.eq_ignore_ascii_case(persistent_id))
    }

    /// Finds an artist by its 16-char hex Persistent ID (case-insensitive).
    pub fn artist_by_id(&self, persistent_id: &str) -> Option<&Artist> {
        self.artists
            .iter()
            .find(|a| a.persistent_id.eq_ignore_ascii_case(persistent_id))
    }

    /// Finds a playlist by its 16-char hex Persistent ID (case-insensitive).
    pub fn playlist_by_id(&self, persistent_id: &str) -> Option<&Playlist> {
        self.playlists
            .iter()
            .find(|p| p.persistent_id.eq_ignore_ascii_case(persistent_id))
    }

    /// Finds a playlist by exact name (or case-insensitive exact/substring fallback).
    pub fn playlist_by_name(&self, name: &str) -> Option<&Playlist> {
        let q = name.to_lowercase();
        self.playlists
            .iter()
            .find(|p| p.name.as_deref() == Some(name))
            .or_else(|| {
                self.playlists.iter().find(|p| {
                    p.name
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(name))
                })
            })
            .or_else(|| {
                self.playlists.iter().find(|p| {
                    p.name
                        .as_deref()
                        .is_some_and(|n| n.to_lowercase().contains(&q))
                })
            })
    }

    /// Resolves the ordered list of `&Track` items belonging to `playlist`.
    pub fn playlist_tracks(&self, playlist: &Playlist) -> Vec<&Track> {
        let track_map: HashMap<&str, &Track> = self
            .tracks
            .iter()
            .map(|t| (t.persistent_id.as_str(), t))
            .collect();
        playlist
            .track_persistent_ids
            .iter()
            .filter_map(|pid| track_map.get(pid.as_str()).copied())
            .collect()
    }

    /// Returns the direct child playlists of a folder playlist.
    pub fn playlist_children(&self, folder: &Playlist) -> Vec<&Playlist> {
        self.playlists
            .iter()
            .filter(|p| {
                p.parent_persistent_id
                    .as_deref()
                    .is_some_and(|parent| parent.eq_ignore_ascii_case(&folder.persistent_id))
            })
            .collect()
    }

    /// Returns all tracks belonging to `album_persistent_id`, sorted by disc and track number.
    pub fn album_tracks(&self, album_persistent_id: &str) -> Vec<&Track> {
        let mut out: Vec<&Track> = self
            .tracks
            .iter()
            .filter(|t| {
                t.album_persistent_id
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(album_persistent_id))
            })
            .collect();
        out.sort_by_key(|t| (t.disc_number, t.track_number));
        out
    }

    /// Returns all tracks belonging to `artist_persistent_id`.
    pub fn artist_tracks(&self, artist_persistent_id: &str) -> Vec<&Track> {
        self.tracks
            .iter()
            .filter(|t| {
                t.artist_persistent_id
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(artist_persistent_id))
            })
            .collect()
    }

    /// Serializes the entire `Library` domain model as pretty-printed JSON.
    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

impl SocialProfile {
    pub fn from_raw(isma: &IsmaSection) -> Self {
        let mut profile_id = None;
        let mut name = None;
        let mut handle = None;
        let mut avatar_id = None;
        let mut avatar_url = None;

        for boma in &isma.bomas {
            if let Some(s) = boma.as_str() {
                match boma.subtype {
                    0x0320 => profile_id = Some(s.to_string()),
                    0x0321 => name = Some(s.to_string()),
                    0x0322 => handle = Some(s.to_string()),
                    0x0323 => avatar_id = Some(s.to_string()),
                    0x0324 => avatar_url = Some(s.to_string()),
                    _ => {}
                }
            }
        }

        Self {
            persistent_id: format_persistent_id(isma.header.profile_persistent_id),
            persistent_id_u64: isma.header.profile_persistent_id,
            profile_id,
            name,
            handle,
            avatar_id,
            avatar_url,
        }
    }
}

fn populate_plma_metadata(plma: &PlmaSection, meta: &mut LibraryMetadata) {
    meta.show_song_list_checkboxes = Some(plma.header.checkbox_show_song_list_checkboxes != 0);
    meta.keep_media_folder_organized = Some(plma.header.checkbox_keep_media_folder_organized != 0);
    for boma in &plma.bomas {
        if let Some(s) = boma.as_str() {
            match boma.subtype {
                0x01F8 => meta.media_folder_uri = Some(s.to_string()),
                0x01FC => meta.imported_itl_path = Some(s.to_string()),
                0x01FD | 0x0200 => meta.media_folder_path = Some(s.to_string()),
                _ => {}
            }
        }
    }
}

impl Album {
    pub fn from_raw(iama: &IamaSection) -> Self {
        let h = &iama.header;
        let mut title = None;
        let mut artist = None;
        let mut album_artist = None;
        let mut cloud_album_id = None;
        let mut editorial_notes = None;

        for boma in &iama.bomas {
            if let Some(s) = boma.as_str() {
                match boma.subtype {
                    0x012C => title = Some(s.to_string()),
                    0x012D => artist = Some(s.to_string()),
                    0x012E => album_artist = Some(s.to_string()),
                    0x0133 => cloud_album_id = Some(s.to_string()),
                    0x0134 => editorial_notes = Some(s.to_string()),
                    _ => {}
                }
            }
        }

        Self {
            persistent_id: format_persistent_id(h.album_persistent_id),
            persistent_id_u64: h.album_persistent_id,
            itunes_persistent_id: (h.album_persistent_id_2 != 0)
                .then(|| format_persistent_id(h.album_persistent_id_2)),
            first_track_persistent_id: (h.first_track_persistent_id != 0)
                .then(|| format_persistent_id(h.first_track_persistent_id)),
            title,
            artist,
            album_artist,
            cloud_album_id,
            editorial_notes,
            is_compilation: h.is_compilation != 0,
            star_rating: h.star_rating,
            stars: h.star_rating / 20,
            star_rating_is_computed: h.star_rating_inheritance == 0x20,
            suggestion_flag: SuggestionFlag::from(h.suggestion_flag),
            suggestion_locally_modified: SuggestionFlag::is_locally_modified(h.suggestion_flag),
            date_suggestion_flag_modified: apple_timestamp_to_utc(h.date_suggestion_flag_modified),
            date_last_played: apple_timestamp_to_utc(h.date_last_played),
            date_cloud_synced: h.date_cloud_synced.and_then(apple_timestamp_to_utc),
            date_released: h.date_released.and_then(apple_timestamp_to_utc),
            apple_music_store_album_id: h.apple_music_store_album_id.filter(|&id| id != 0),
        }
    }
}

impl Artist {
    pub fn from_raw(i_ama: &IAmaSection) -> Self {
        let h = &i_ama.header;
        let mut name = None;
        let mut sort_name = None;
        let mut cloud_artist_id = None;
        let mut artwork_url_plist = None;

        for boma in &i_ama.bomas {
            if let Some(s) = boma.as_str() {
                match boma.subtype {
                    0x0190 | 0x0004 => name = Some(s.to_string()),
                    0x0191 => sort_name = Some(s.to_string()),
                    0x0192 => artwork_url_plist = Some(s.to_string()),
                    0x0193 => cloud_artist_id = Some(s.to_string()),
                    _ => {}
                }
            }
        }

        Self {
            persistent_id: format_persistent_id(h.artist_persistent_id),
            persistent_id_u64: h.artist_persistent_id,
            itunes_persistent_id: (h.artist_persistent_id_2 != 0)
                .then(|| format_persistent_id(h.artist_persistent_id_2)),
            name,
            sort_name,
            cloud_artist_id,
            suggestion_flag: SuggestionFlag::from(h.suggestion_flag),
            suggestion_locally_modified: SuggestionFlag::is_locally_modified(h.suggestion_flag),
            date_suggestion_flag_modified: apple_timestamp_to_utc(h.date_suggestion_flag_modified),
            date_cloud_synced: h.date_cloud_synced.and_then(apple_timestamp_to_utc),
            apple_music_store_artist_id: if h.apple_music_store_artist_id != 0 {
                h.apple_music_store_artist_id
            } else {
                h.apple_music_store_artist_id_2
            },
            artwork_uuid: format_artwork_uuid(&h.artwork_uuid),
            artwork_url_plist,
        }
    }
}

impl Track {
    pub fn from_raw(itma: &ItmaSection) -> Self {
        let h = &itma.header;

        let cloud_saga_id = (h.cloud_saga_id != 0 && h.cloud_saga_id != 0xFFFF_FFFF_FFFF_FFFD)
            .then_some(h.cloud_saga_id);

        let mut track = Self {
            persistent_id: format_persistent_id(h.track_persistent_id),
            persistent_id_u64: h.track_persistent_id,
            title: None,
            album: None,
            artist: None,
            album_artist: None,
            composer: None,
            genre: None,
            store_genre_id: None,
            kind: None,
            comments: None,
            grouping: None,
            work_name: None,
            movement_name: None,
            movement_number: h.movement_number,
            movement_total: h.movement_total,
            sort_title: None,
            sort_album: None,
            sort_artist: None,
            sort_album_artist: None,
            sort_composer: None,
            sort_modes: TrackSortModes::from(h.auto_sort_flags),
            auto_sort_flags: h.auto_sort_flags,
            track_number: h.track_number,
            track_total: h.track_total,
            disc_number: h.disc_number,
            disc_total: h.disc_total,
            year: h.year,
            bpm: h.bpm,
            duration_ms: None,
            file_size: None,
            bit_rate: None,
            is_vbr: None,
            sample_rate: None,
            file_type_fourcc: None,
            codec_fourcc: None,
            volume_adjustment: h.volume_adjustment,
            normalization: None,
            gapless_encoder_delay: None,
            gapless_encoder_padding: None,
            gapless_total_samples: None,
            gapless_last_eight_frames_offset: None,
            equalizer: None,
            equalizer_preset: None,
            star_rating: h.star_rating,
            stars: h.star_rating / 20,
            suggestion_flag: SuggestionFlag::from(h.suggestion_flag),
            disabled: h.checkbox_disabled != 0,
            skip_when_shuffling: h.checkbox_skip_when_shuffling != 0,
            remember_playback_position: h.checkbox_remember_playback_position != 0,
            show_composer_in_all_views: h.checkbox_show_composer_in_all_views != 0,
            use_work_and_movement: h.checkbox_use_work_and_movement != 0,
            album_is_compilation: h.checkbox_album_is_compilation != 0,
            part_of_gapless_album: h.checkbox_part_of_gapless_album != 0,
            playlist_only: h.playlist_only != 0,
            purchased: h.cloud_sync_state == 3 || h.unk_96_purchased_album_group != 0,
            has_custom_lyrics: h.has_custom_lyrics != 0 || h.has_local_custom_lyrics != 0,
            has_local_custom_lyrics: h.has_local_custom_lyrics != 0,
            has_unsynced_local_changes: h.has_unsynced_local_changes != 0,
            custom_lyrics_hash: None,
            is_itunes_matched: None,
            is_apple_music_catalog: None,
            cloud_saga_id,
            store_track_adam_id: None,
            apple_music_store_album_id: None,
            apple_music_store_artist_id: (h.apple_music_store_artist_id != 0)
                .then_some(h.apple_music_store_artist_id as u64),
            apple_music_store_album_artist_id: None,
            storefront_id: None,
            purchaser_email: None,
            purchaser_name: None,
            downloaded_status: h.downloaded_status,
            downloaded_state: DownloadedStatus::from(h.downloaded_status),
            cloud_status: CloudStatus::from(h.cloud_status),
            cloud_status_raw: h.cloud_status,
            cloud_album_id: None,
            cloud_artist_id: None,
            cloud_artwork_token: None,
            cloud_lyrics_token: None,
            content_rating: h.content_rating,
            content_advisory: ContentRating::from(h.content_rating),
            media_kind: h.media_kind,
            media_type: MediaKind::from(h.media_kind),
            playback_start_ms: h.playback_start_ms,
            playback_stop_ms: h.playback_stop_ms,
            play_count: None,
            true_play_count: None,
            skip_count: None,
            true_skip_count: None,
            date_added: None,
            date_modified: None,
            date_last_played: None,
            date_first_played_utc: None,
            date_last_skipped: None,
            date_released: None,
            date_purchased: None,
            date_cloud_synced: None,
            file_url: None,
            legacy_file_path: None,
            book_path: None,
            album_persistent_id: (h.album_persistent_id != 0)
                .then(|| format_persistent_id(h.album_persistent_id)),
            artist_persistent_id: (h.artist_persistent_id != 0)
                .then(|| format_persistent_id(h.artist_persistent_id)),
            artwork_uuid: format_artwork_uuid(&h.artwork_uuid),
            artwork_count: None,
            has_embedded_artwork: None,
            artwork_total_size_bytes: None,
            file_folder_count: None,
            library_folder_count: None,
            isrc: None,
            copyright: None,
            extra_strings: BTreeMap::new(),
        };

        // Note: As documented in Apple-Music-Editor, if duplicate boma subtypes appear on a track,
        // only the first occurrence is active in Apple Music.
        for boma in &itma.bomas {
            match &boma.payload {
                BomaPayload::TrackNumerics(num) => {
                    if track.duration_ms.is_none() {
                        track.duration_ms = Some(num.duration_ms);
                        track.file_size = Some(num.file_size);
                        track.bit_rate = Some(num.bit_rate);
                        track.is_vbr = Some(num.is_vbr != 0);
                        track.sample_rate = Some(num.sample_rate);
                        track.file_type_fourcc = num.file_type_fourcc_str();
                        track.codec_fourcc = num.codec_fourcc_str();
                        track.normalization = (num.normalization != 0).then_some(num.normalization);
                        track.gapless_encoder_delay =
                            (num.gapless_encoder_delay != 0).then_some(num.gapless_encoder_delay);
                        track.gapless_encoder_padding = (num.gapless_encoder_padding != 0)
                            .then_some(num.gapless_encoder_padding);
                        track.gapless_total_samples =
                            (num.gapless_total_samples != 0).then_some(num.gapless_total_samples);
                        track.gapless_last_eight_frames_offset =
                            (num.gapless_last_eight_frames_offset != 0)
                                .then_some(num.gapless_last_eight_frames_offset);
                        track.store_genre_id =
                            (num.store_genre_id != 0).then_some(num.store_genre_id);
                        track.custom_lyrics_hash =
                            (num.custom_lyrics_hash != 0).then_some(num.custom_lyrics_hash);
                        track.date_added = apple_timestamp_to_utc(num.date_added);
                        track.date_modified = apple_timestamp_to_utc(num.date_modified);
                        track.date_purchased = apple_timestamp_to_utc(num.date_purchased);
                        if num.date_purchased != 0 {
                            track.purchased = true;
                        }
                        track.date_released = apple_timestamp_to_utc(num.date_released);
                        track.date_cloud_synced = apple_timestamp_to_utc(num.date_cloud_synced);
                        track.is_itunes_matched = Some(num.is_itunes_matched != [0, 0]);
                        track.is_apple_music_catalog = Some(num.is_apple_music_catalog != 0);
                        if num.store_track_adam_id != 0 {
                            track.store_track_adam_id = Some(num.store_track_adam_id);
                        }
                        if num.apple_music_store_album_id != 0 {
                            track.apple_music_store_album_id = Some(num.apple_music_store_album_id);
                        }
                        if num.apple_music_store_artist_id != 0 {
                            track.apple_music_store_artist_id =
                                Some(num.apple_music_store_artist_id);
                        }
                        if num.apple_music_store_album_artist_id != 0 {
                            track.apple_music_store_album_artist_id =
                                Some(num.apple_music_store_album_artist_id);
                        }
                        if num.storefront_id != 0 {
                            track.storefront_id = Some(num.storefront_id);
                        }
                        track.artwork_count = Some(num.artwork_count);
                        track.has_embedded_artwork = Some(num.has_embedded_artwork != 0);
                        track.artwork_total_size_bytes = Some(num.artwork_total_size_bytes);
                        track.file_folder_count = Some(num.file_folder_count);
                        track.library_folder_count = Some(num.library_folder_count);
                    } else {
                        // Downloaded local tracks have two `TrackNumerics` records:
                        // `TrackNumerics[0]` holds local file properties (`date_cloud_synced = 2`),
                        // while `TrackNumerics[1]` holds the iCloud Music Library catalog/sync record.
                        if track.date_cloud_synced.is_none() {
                            track.date_cloud_synced = apple_timestamp_to_utc(num.date_cloud_synced);
                        }
                        if track.date_released.is_none() {
                            track.date_released = apple_timestamp_to_utc(num.date_released);
                        }
                        if track.store_track_adam_id.is_none() && num.store_track_adam_id != 0 {
                            track.store_track_adam_id = Some(num.store_track_adam_id);
                        }
                        if track.apple_music_store_album_id.is_none()
                            && num.apple_music_store_album_id != 0
                        {
                            track.apple_music_store_album_id = Some(num.apple_music_store_album_id);
                        }
                        if (track.apple_music_store_artist_id.is_none()
                            || track.apple_music_store_artist_id
                                == Some(num.apple_music_store_artist_id & 0xFFFF_FFFF))
                            && num.apple_music_store_artist_id != 0
                        {
                            track.apple_music_store_artist_id =
                                Some(num.apple_music_store_artist_id);
                        }
                        if track.apple_music_store_album_artist_id.is_none()
                            && num.apple_music_store_album_artist_id != 0
                        {
                            track.apple_music_store_album_artist_id =
                                Some(num.apple_music_store_album_artist_id);
                        }
                        if track.storefront_id.is_none() && num.storefront_id != 0 {
                            track.storefront_id = Some(num.storefront_id);
                        }
                        if track.store_genre_id.is_none() && num.store_genre_id != 0 {
                            track.store_genre_id = Some(num.store_genre_id);
                        }
                        if num.is_itunes_matched != [0, 0] {
                            track.is_itunes_matched = Some(true);
                        }
                        if num.is_apple_music_catalog != 0 {
                            track.is_apple_music_catalog = Some(true);
                        }
                        if track.codec_fourcc.is_none() {
                            track.codec_fourcc = num.codec_fourcc_str();
                        }
                    }
                }
                BomaPayload::TrackPlaysSkips(ps) => {
                    if track.play_count.is_none() {
                        track.play_count = Some(ps.play_count);
                        track.true_play_count = Some(ps.true_play_count);
                        track.skip_count = Some(ps.skip_count);
                        track.true_skip_count = Some(ps.true_skip_count);
                        track.date_last_played = apple_timestamp_to_utc(ps.date_last_played);
                        track.date_first_played_utc =
                            apple_timestamp_to_utc(ps.date_first_played_utc);
                        track.date_last_skipped = apple_timestamp_to_utc(ps.date_last_skipped);
                    }
                }
                BomaPayload::Book { extracted_path, .. } => {
                    if track.book_path.is_none() {
                        track.book_path = extracted_path.clone();
                    }
                }
                _ => {
                    if let Some(s) = boma.as_str() {
                        let val = s.to_string();
                        match boma.subtype {
                            0x0002 => {
                                track.title.get_or_insert(val);
                            }
                            0x0003 => {
                                track.album.get_or_insert(val);
                            }
                            0x0004 => {
                                track.artist.get_or_insert(val);
                            }
                            0x0005 => {
                                track.genre.get_or_insert(val);
                            }
                            0x0006 => {
                                track.kind.get_or_insert(val);
                            }
                            0x0007 => {
                                if track.equalizer.is_none() {
                                    track.equalizer_preset = Some(EqualizerPreset::parse(&val));
                                    track.equalizer = Some(val);
                                }
                            }
                            0x0008 => {
                                track.comments.get_or_insert(val);
                            }
                            0x000B => {
                                track.file_url.get_or_insert(val);
                            }
                            0x000C => {
                                track.composer.get_or_insert(val);
                            }
                            0x000E => {
                                track.grouping.get_or_insert(val);
                            }
                            0x001B => {
                                track.album_artist.get_or_insert(val);
                            }
                            0x001E => {
                                track.sort_title.get_or_insert(val);
                            }
                            0x001F => {
                                track.sort_album.get_or_insert(val);
                            }
                            0x0020 => {
                                track.sort_artist.get_or_insert(val);
                            }
                            0x0021 => {
                                track.sort_album_artist.get_or_insert(val);
                            }
                            0x0022 => {
                                track.sort_composer.get_or_insert(val);
                            }
                            0x002B => {
                                track.isrc.get_or_insert(val);
                            }
                            0x002E => {
                                track.copyright.get_or_insert(val);
                            }
                            0x003B => {
                                track.purchaser_email.get_or_insert(val);
                            }
                            0x003C => {
                                track.purchaser_name.get_or_insert(val);
                            }
                            0x003F => {
                                track.work_name.get_or_insert(val);
                            }
                            0x0040 => {
                                track.movement_name.get_or_insert(val);
                            }
                            0x0043 => {
                                track.legacy_file_path.get_or_insert(val);
                            }
                            0x0036 => {
                                if let BomaPayload::RawStringUtf8 { raw_bytes, .. } = &boma.payload
                                    && let Ok(plist::Value::Dictionary(dict)) =
                                        plist::Value::from_reader(std::io::Cursor::new(raw_bytes))
                                {
                                    if let Some(v) =
                                        dict.get("cloud-album-id").and_then(|v| v.as_string())
                                    {
                                        track.cloud_album_id.get_or_insert_with(|| v.to_string());
                                    }
                                    if let Some(v) =
                                        dict.get("cloud-artist-id").and_then(|v| v.as_string())
                                    {
                                        track.cloud_artist_id.get_or_insert_with(|| v.to_string());
                                    }
                                    if let Some(v) =
                                        dict.get("cloud-artwork-token").and_then(|v| v.as_string())
                                    {
                                        track
                                            .cloud_artwork_token
                                            .get_or_insert_with(|| v.to_string());
                                    }
                                    if let Some(v) =
                                        dict.get("cloud-lyrics-token").and_then(|v| v.as_string())
                                    {
                                        track
                                            .cloud_lyrics_token
                                            .get_or_insert_with(|| v.to_string());
                                    }
                                }
                                track
                                    .extra_strings
                                    .entry(format!(
                                        "0x{:04X}_{}",
                                        boma.subtype,
                                        boma.subtype_name()
                                    ))
                                    .or_insert(val);
                            }
                            other => {
                                track
                                    .extra_strings
                                    .entry(format!("0x{:04X}_{}", other, boma.subtype_name()))
                                    .or_insert(val);
                            }
                        }
                    }
                }
            }
        }

        track
    }
}

impl Playlist {
    pub fn from_raw(lpma: &LpmaSection) -> Self {
        let h = &lpma.header;
        let mut name = None;
        let mut description = None;
        let mut universal_library_id = None;
        let mut cloud_uuid = None;
        let mut subscribed_container_url = None;
        let mut owner_name = None;
        let mut owner_id = None;
        let mut cloud_artwork_url = None;
        let mut cloud_artwork_token = None;
        let mut raw_smart_rules: Option<&SlstSection> = None;
        let mut raw_smart_options: Option<&SmartPlaylistOptions> = None;
        let mut track_persistent_ids = Vec::new();

        for boma in &lpma.bomas {
            match &boma.payload {
                BomaPayload::PlaylistItem(ipfa) => {
                    track_persistent_ids.push(format_persistent_id(ipfa.track_persistent_id));
                }
                BomaPayload::PlaylistItemCompact(ipfa) => {
                    track_persistent_ids.push(format_persistent_id(ipfa.track_persistent_id));
                }
                BomaPayload::SmartPlaylistRules(slst) => {
                    raw_smart_rules = Some(slst.as_ref());
                }
                BomaPayload::SmartPlaylistOptions(opts) => {
                    raw_smart_options = Some(opts.as_ref());
                }
                _ => {
                    if boma.subtype == 0x00C8
                        && let Some(s) = boma.as_str()
                    {
                        name = Some(s.to_string());
                    } else if boma.subtype == 0x00CD
                        && let BomaPayload::RawStringUtf8 { raw_bytes, .. } = &boma.payload
                        && let Ok(plist::Value::Dictionary(dict)) =
                            plist::Value::from_reader(std::io::Cursor::new(raw_bytes))
                    {
                        universal_library_id = dict
                            .get("universal-library-id")
                            .and_then(|v| v.as_string())
                            .map(str::to_string);
                        cloud_uuid = dict
                            .get("uuid")
                            .and_then(|v| v.as_string())
                            .map(str::to_string);
                        subscribed_container_url = dict
                            .get("subscribed-container-url")
                            .and_then(|v| v.as_string())
                            .map(str::to_string);
                        owner_name = dict
                            .get("ownerName")
                            .and_then(|v| v.as_string())
                            .map(str::to_string);
                        owner_id = dict
                            .get("ownerID")
                            .and_then(|v| v.as_string())
                            .map(str::to_string);
                        description = dict
                            .get("description")
                            .and_then(|v| v.as_string())
                            .map(str::to_string);
                        cloud_artwork_url = dict
                            .get("cloud-artwork-url")
                            .and_then(|v| v.as_string())
                            .map(str::to_string);
                        cloud_artwork_token = dict
                            .get("cloud-artwork-token")
                            .and_then(|v| v.as_string())
                            .map(str::to_string);
                    }
                }
            }
        }

        let is_master = h.is_master_playlist != 0;
        let is_folder = h.unk_48_folder_flag[1] != 0;
        let distinguished_kind =
            u16::from_le_bytes([h.distinguished_kind_bytes[1], h.distinguished_kind_bytes[2]]);
        let system_kind = DistinguishedPlaylistKind::from(distinguished_kind);
        let share_mode = PlaylistShareMode::from(h.cloud_share_mode);

        // Note: Apple Music internally attaches a hidden `SLst` rule list to Folder playlists
        // (matching `ANY` child playlist IDs). Only non-folder playlists with `SLst` / `0x00CA`
        // are true user/system Smart Playlists.
        let has_smart_blocks = raw_smart_rules.is_some() || raw_smart_options.is_some();
        let is_smart = !is_folder && has_smart_blocks;
        let smart_playlist = if has_smart_blocks && !is_folder {
            Some(decode_smart_playlist(raw_smart_options, raw_smart_rules))
        } else {
            None
        };

        let kind = if is_master {
            PlaylistKind::Master
        } else if is_folder {
            PlaylistKind::Folder
        } else if distinguished_kind != 0 {
            PlaylistKind::Distinguished
        } else if is_smart {
            PlaylistKind::Smart
        } else if share_mode == PlaylistShareMode::SubscribedCatalog
            || subscribed_container_url.is_some()
        {
            PlaylistKind::SubscribedCatalog
        } else {
            PlaylistKind::Standard
        };

        let cloud_saga_id = (h.cloud_playlist_saga_id_1 != 0).then_some(h.cloud_playlist_saga_id_1);
        let author_social_profile_persistent_id = h
            .author_social_profile_persistent_id
            .filter(|&id| id != 0)
            .map(format_persistent_id);
        let date_cloud_synced = h
            .date_suggestion_flag_modified
            .or(h.date_cloud_synced_2)
            .and_then(apple_timestamp_to_utc);

        Self {
            persistent_id: format_persistent_id(h.playlist_persistent_id),
            persistent_id_u64: h.playlist_persistent_id,
            parent_persistent_id: (h.parent_persistent_id != 0)
                .then(|| format_persistent_id(h.parent_persistent_id)),
            name,
            description,
            kind,
            is_master,
            is_folder,
            is_smart,
            show_in_profile_and_search: h.checkbox_show_in_profile_and_search != 0,
            distinguished_kind,
            system_kind,
            suggestion_flag: SuggestionFlag::from(h.suggestion_flag),
            date_created: apple_timestamp_to_utc(h.date_created),
            date_modified: apple_timestamp_to_utc(h.date_modified),
            date_last_played: apple_timestamp_to_utc(h.date_last_played),
            date_cloud_published: apple_timestamp_to_utc(h.date_cloud_published),
            date_cloud_synced,
            play_count: h.play_count,
            true_play_count: h.true_play_count,
            refresh_interval_seconds: h.refresh_interval_seconds,
            cloud_share_mode: h.cloud_share_mode,
            share_mode,
            artwork_template_kind: h.artwork_template_kind,
            cloud_saga_id,
            universal_library_id,
            cloud_uuid,
            subscribed_container_url,
            owner_name,
            owner_id,
            author_social_profile_persistent_id,
            cloud_artwork_url,
            cloud_artwork_token,
            artwork_uuid: format_artwork_uuid(&h.artwork_uuid),
            declared_track_count: h.track_count,
            track_persistent_ids,
            child_playlist_persistent_ids: Vec::new(),
            smart_playlist,
            view_preferences: None,
        }
    }
}

fn decode_smart_playlist(
    opts: Option<&SmartPlaylistOptions>,
    slst: Option<&SlstSection>,
) -> SmartPlaylistConfig {
    let (live_updating, match_rules, match_only_checked_items, limit) = if let Some(o) = opts {
        let lim = (o.checkbox_enable_limit != 0).then(|| SmartPlaylistLimit {
            count: o.limit_count,
            unit: SmartLimitUnit::from(o.limit_unit),
            selection: SmartSelectionMethod::from_raw(
                o.limit_selection_method,
                o.negate_limit_selection_method,
            ),
        });
        (
            o.checkbox_live_updating != 0,
            o.checkbox_enable_matching_rules != 0,
            o.checkbox_match_only_checked_items != 0,
            lim,
        )
    } else {
        (true, true, false, None)
    };

    let rule_group = slst.map(decode_slst_group);

    SmartPlaylistConfig {
        live_updating,
        match_rules,
        match_only_checked_items,
        limit,
        rule_group,
    }
}

fn decode_slst_group(slst: &SlstSection) -> SmartRuleGroup {
    let conjunction = if slst.conjunction == 0 {
        SmartConjunction::All
    } else {
        SmartConjunction::Any
    };
    let rules = slst.rules.iter().map(decode_smart_rule).collect();
    SmartRuleGroup { conjunction, rules }
}

fn decode_smart_rule(rule: &SmartPlaylistRule) -> SmartRuleNode {
    if let SmartRuleChild::Nested(nested) = &rule.child {
        return SmartRuleNode::Group(decode_slst_group(nested));
    }

    let field = SmartField::from(rule.field_id);
    let operator = SmartOperator::from(rule.comparison_method);
    let value = match &rule.child {
        SmartRuleChild::StringUtf16Be { text, .. } => SmartRuleValue::Text { text: text.clone() },
        SmartRuleChild::Arguments(args) => {
            if matches!(
                operator,
                SmartOperator::InTheLast | SmartOperator::NotInTheLast
            ) {
                let amount = (-args.arg_1).max(0) as u64;
                let unit = match args.arg_2 {
                    86_400 | 1 => RelativeTimeUnit::Days,
                    604_800 | 2 => RelativeTimeUnit::Weeks,
                    2_628_000 | 3 => RelativeTimeUnit::Months,
                    other => RelativeTimeUnit::Unknown(other),
                };
                SmartRuleValue::RelativeTime {
                    amount,
                    unit,
                    seconds_per_unit: args.arg_2,
                }
            } else {
                match field {
                    SmartField::Compilation
                    | SmartField::Checked
                    | SmartField::AlbumArtwork
                    | SmartField::Purchased
                    | SmartField::HasVideo => SmartRuleValue::Boolean {
                        value: rule.comparison_method == 0x0000_0001 && args.arg_0 != 0,
                    },
                    SmartField::Rating | SmartField::AlbumRating => {
                        if matches!(operator, SmartOperator::InRange | SmartOperator::NotInRange) {
                            SmartRuleValue::StarRatingRange {
                                min_stars: (args.arg_0.clamp(0, 100) / 20) as u8,
                                max_stars: (args.arg_3.clamp(0, 100) / 20) as u8,
                                min_raw: args.arg_0,
                                max_raw: args.arg_3,
                            }
                        } else {
                            SmartRuleValue::StarRating {
                                stars: (args.arg_0.clamp(0, 100) / 20) as u8,
                                raw_0_100: args.arg_0,
                            }
                        }
                    }
                    SmartField::Playlist => SmartRuleValue::PlaylistRef {
                        playlist_persistent_id: format_persistent_id(args.arg_0 as u64),
                        playlist_name: None,
                    },
                    SmartField::CloudStatus => SmartRuleValue::CloudStatus {
                        status: CloudStatus::from(args.arg_0 as u8),
                    },
                    SmartField::Favorite | SmartField::AlbumFavorite => {
                        SmartRuleValue::SuggestionFlag {
                            flag: SuggestionFlag::from(args.arg_0 as u16),
                        }
                    }
                    SmartField::MediaKind => SmartRuleValue::MediaKind {
                        kind: MediaKind::from(args.arg_0 as u32),
                    },
                    SmartField::Location => {
                        let name = match args.arg_0 {
                            1 => "Computer",
                            2 => "iCloud",
                            16 => "On This Computer",
                            _ => "Unknown Location",
                        };
                        SmartRuleValue::LocationKind {
                            code: args.arg_0,
                            name: name.to_string(),
                        }
                    }
                    SmartField::DateAdded
                    | SmartField::DateModified
                    | SmartField::LastPlayed
                    | SmartField::LastSkipped => {
                        if matches!(operator, SmartOperator::InRange | SmartOperator::NotInRange) {
                            SmartRuleValue::DateRange {
                                start_apple: args.arg_0,
                                end_apple: args.arg_3,
                                start_utc: u32::try_from(args.arg_0)
                                    .ok()
                                    .and_then(apple_timestamp_to_utc),
                                end_utc: u32::try_from(args.arg_3)
                                    .ok()
                                    .and_then(apple_timestamp_to_utc),
                            }
                        } else {
                            SmartRuleValue::Date {
                                apple_timestamp: args.arg_0,
                                utc: u32::try_from(args.arg_0)
                                    .ok()
                                    .and_then(apple_timestamp_to_utc),
                            }
                        }
                    }
                    _ => {
                        if matches!(operator, SmartOperator::InRange | SmartOperator::NotInRange) {
                            SmartRuleValue::IntegerRange {
                                min: args.arg_0,
                                max: args.arg_3,
                            }
                        } else {
                            SmartRuleValue::Integer { value: args.arg_0 }
                        }
                    }
                }
            }
        }
        SmartRuleChild::Raw(bytes) => SmartRuleValue::Raw {
            hex: hex::encode(bytes),
        },
        SmartRuleChild::Nested(_) => unreachable!(),
    };

    let description = format_smart_condition_description(rule, &value);

    SmartRuleNode::Condition(SmartCondition {
        field_id: rule.field_id,
        field,
        operator,
        comparison_raw: rule.comparison_method,
        value,
        description,
    })
}

fn format_smart_condition_description(rule: &SmartPlaylistRule, value: &SmartRuleValue) -> String {
    let fname = rule.field_name();
    let cmp = rule.comparison_description();
    let val_str = match value {
        SmartRuleValue::Text { text } => format!("{:?}", text),
        SmartRuleValue::Boolean { value } => value.to_string(),
        SmartRuleValue::Integer { value } => value.to_string(),
        SmartRuleValue::IntegerRange { min, max } => format!("{} to {}", min, max),
        SmartRuleValue::StarRating { stars, .. } => format!("{} stars", stars),
        SmartRuleValue::StarRatingRange {
            min_stars,
            max_stars,
            ..
        } => format!("{} to {} stars", min_stars, max_stars),
        SmartRuleValue::RelativeTime { amount, unit, .. } => {
            format!("{} {:?}", amount, unit)
        }
        SmartRuleValue::Date {
            utc,
            apple_timestamp,
        } => utc
            .map(|d| d.to_rfc3339())
            .unwrap_or_else(|| apple_timestamp.to_string()),
        SmartRuleValue::DateRange {
            start_utc,
            end_utc,
            start_apple,
            end_apple,
        } => format!(
            "{} to {}",
            start_utc
                .map(|d| d.to_rfc3339())
                .unwrap_or_else(|| start_apple.to_string()),
            end_utc
                .map(|d| d.to_rfc3339())
                .unwrap_or_else(|| end_apple.to_string())
        ),
        SmartRuleValue::PlaylistRef {
            playlist_persistent_id,
            playlist_name,
        } => match playlist_name {
            Some(n) => format!("{:?} ({})", n, playlist_persistent_id),
            None => playlist_persistent_id.clone(),
        },
        SmartRuleValue::CloudStatus { status } => format!("{:?}", status),
        SmartRuleValue::SuggestionFlag { flag } => format!("{:?}", flag),
        SmartRuleValue::MediaKind { kind } => format!("{:?}", kind),
        SmartRuleValue::LocationKind { name, .. } => name.clone(),
        SmartRuleValue::Raw { hex } => format!("0x{}", hex),
    };
    format!("{} {} {}", fname, cmp, val_str)
}

fn resolve_playlist_names_in_group(
    group: &mut SmartRuleGroup,
    playlist_names: &HashMap<String, String>,
) {
    for node in &mut group.rules {
        match node {
            SmartRuleNode::Group(g) => resolve_playlist_names_in_group(g, playlist_names),
            SmartRuleNode::Condition(cond) => {
                if let SmartRuleValue::PlaylistRef {
                    playlist_persistent_id,
                    playlist_name,
                } = &mut cond.value
                    && let Some(resolved) = playlist_names.get(playlist_persistent_id)
                {
                    *playlist_name = Some(resolved.clone());
                    cond.description = format!(
                        "Playlist {} {:?} ({})",
                        match cond.operator {
                            SmartOperator::IsNot => "is not",
                            _ => "is",
                        },
                        resolved,
                        playlist_persistent_id
                    );
                }
            }
        }
    }
}

fn parse_playlist_view_plist(path: &Path) -> Option<PlaylistViewPreferences> {
    let val = plist::Value::from_file(path).ok()?;
    let dict = val.as_dictionary()?;

    let mut view_as = dict.get("LibraryPage_ViewAs").and_then(|v| {
        if let Some(s) = v.as_string() {
            Some(s.to_string())
        } else {
            v.as_signed_integer().map(|code| match code {
                1 => "Songs".to_string(),
                7 => "Albums".to_string(),
                11 => "Playlist".to_string(),
                other => format!("ViewAs({})", other),
            })
        }
    });

    let view_dict = if let Some(views) = dict
        .get("LibraryPage_Views")
        .and_then(|v| v.as_dictionary())
    {
        if view_as.is_none() {
            view_as = views.keys().next().cloned();
        }
        view_as
            .as_deref()
            .and_then(|k| views.get(k))
            .or_else(|| views.values().next())
            .and_then(|v| v.as_dictionary())
    } else {
        Some(dict)
    };

    let mut sort_field = None;
    let mut sort_ascending = None;
    let mut visible_columns = Vec::new();

    if let Some(vd) = view_dict {
        sort_field = vd.get("LibraryPage_View_SortingPrimary").and_then(|v| {
            if let Some(s) = v.as_string() {
                Some(s.to_string())
            } else {
                v.as_signed_integer().map(|code| match code {
                    0 => "PlaylistOrder".to_string(),
                    102 => "AlbumByArtist".to_string(),
                    104 => "Artist".to_string(),
                    106 => "Title".to_string(),
                    117 => "Album".to_string(),
                    1027 => "CloudOrArtist".to_string(),
                    25001 => "Custom".to_string(),
                    other => format!("SortField({})", other),
                })
            }
        });

        sort_ascending = vd
            .get("LibraryPage_View_SortingReverse")
            .and_then(|v| v.as_boolean())
            .map(|rev| !rev);

        if let Some(cols_arr) = vd
            .get("LibraryPage_View_ListColumns")
            .and_then(|v| v.as_array())
        {
            for col in cols_arr {
                if let Some(cd) = col.as_dictionary() {
                    if sort_ascending.is_none()
                        && let Some(asc) = cd
                            .get("LibraryPage_View_ListColumn_Ascending")
                            .and_then(|v| v.as_boolean())
                    {
                        sort_ascending = Some(asc);
                    }
                    if let Some(kind) = cd
                        .get("LibraryPage_View_ListColumn_Kind")
                        .and_then(|v| v.as_signed_integer())
                    {
                        let col_name = match kind {
                            0 => "CheckBox".to_string(),
                            1 => "Position".to_string(),
                            3 => "Title".to_string(),
                            4 => "Artist".to_string(),
                            7 => "Album".to_string(),
                            10 => "Genre".to_string(),
                            11 => "Rating".to_string(),
                            22 => "Plays".to_string(),
                            27 => "CloudStatus".to_string(),
                            30 => "Duration".to_string(),
                            46 => "Favorite".to_string(),
                            other => format!("Column({})", other),
                        };
                        visible_columns.push(col_name);
                    }
                }
            }
        } else if let Some(cols_dict) = vd
            .get("LibraryPage_View_ListColumns")
            .and_then(|v| v.as_dictionary())
        {
            for k in cols_dict.keys() {
                visible_columns.push(k.clone());
            }
        }
    }

    Some(PlaylistViewPreferences {
        view_as,
        sort_field,
        sort_ascending,
        visible_columns,
    })
}
