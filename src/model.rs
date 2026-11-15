//! High-level domain model for Apple Music `.musicdb` libraries (`Library`, `Track`, `Album`, `Artist`, `Playlist`, `SocialProfile`).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use bitflags::bitflags;
use chrono::{DateTime, TimeZone, Utc};

use crate::error::{MusicDbError, Result};
use crate::sections::{
    AlbumItem, ArtistItem, BomaPayload, BomaSection, HsmaChild, LibraryMasterSection, PlaylistItem,
    RawLibraryFile, SlstSection, SmartPlaylistOptions, SmartPlaylistRule, SmartRuleChild,
    SocialProfileItem, TrackItem, TrackNumerics,
};

/// Seconds between 1904-01-01 00:00:00 UTC (Apple/Mac HFS+ epoch) and 1970-01-01 00:00:00 UTC (Unix epoch).
pub const APPLE_TO_UNIX_EPOCH_OFFSET: i64 = 2_082_844_800;

/// Converts an Apple 32-bit timestamp (seconds since 1904-01-01 UTC/local) to a [`DateTime<Utc>`].
/// Returns `None` if `apple_ts < 86_400` (0 or small non-timestamp sentinel values such as `2` on local `TrackNumerics[0].date_cloud_synced`).
#[must_use]
pub fn apple_timestamp_to_utc(apple_ts: u32) -> Option<DateTime<Utc>> {
    if apple_ts < 86_400 {
        return None;
    }
    let unix_secs = i64::from(apple_ts) - APPLE_TO_UNIX_EPOCH_OFFSET;
    Utc.timestamp_opt(unix_secs, 0).single()
}

/// Formats a 64-bit Persistent ID as a 16-character uppercase hexadecimal string (matching `iTunes Music Library.xml`).
#[must_use]
pub fn format_persistent_id(id: u64) -> String {
    format!("{id:016X}")
}

/// Formats a 16-byte artwork UUID as a standard `XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX` uppercase string.
/// Returns `None` if all 16 bytes are zero.
#[must_use]
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

/// 64-bit Apple Music Persistent ID, formatted as 16 uppercase hexadecimal digits (`{:016X}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PersistentId(pub u64);

impl PersistentId {
    /// Zero Persistent ID (`0x0000_0000_0000_0000`).
    pub const ZERO: Self = Self(0);

    /// Creates a [`PersistentId`] from its raw `u64` value.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the underlying `u64` value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Returns `true` if the underlying `u64` is `0`.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Wraps `id` in `Some(PersistentId(id))` if `id != 0`, or `None` if `id == 0`.
    #[must_use]
    pub const fn from_non_zero(id: u64) -> Option<Self> {
        if id == 0 { None } else { Some(Self(id)) }
    }
}

impl fmt::Display for PersistentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016X}", self.0)
    }
}

impl fmt::UpperHex for PersistentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016X}", self.0)
    }
}

impl fmt::LowerHex for PersistentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl From<u64> for PersistentId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<PersistentId> for u64 {
    fn from(value: PersistentId) -> Self {
        value.0
    }
}

impl AsRef<u64> for PersistentId {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

/// Error returned when parsing a [`PersistentId`] from a hexadecimal string fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePersistentIdError(pub std::num::ParseIntError);

impl fmt::Display for ParsePersistentIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid hexadecimal persistent ID: {}", self.0)
    }
}

impl std::error::Error for ParsePersistentIdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl FromStr for PersistentId {
    type Err = ParsePersistentIdError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let trimmed = s
            .trim()
            .strip_prefix("0x")
            .or_else(|| s.trim().strip_prefix("0X"))
            .unwrap_or_else(|| s.trim());
        u64::from_str_radix(trimmed, 16)
            .map(Self)
            .map_err(ParsePersistentIdError)
    }
}

impl PartialEq<str> for PersistentId {
    fn eq(&self, other: &str) -> bool {
        other.parse::<Self>().ok() == Some(*self)
    }
}

impl PartialEq<&str> for PersistentId {
    fn eq(&self, other: &&str) -> bool {
        (*other).parse::<Self>().ok() == Some(*self)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for PersistentId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for PersistentId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<Self>().map_err(serde::de::Error::custom)
    }
}

/// Conversion helper trait allowing [`Library`] lookup methods (`track_by_id`, `album_by_id`, etc.)
/// to accept a [`PersistentId`], `&PersistentId`, `u64`, or hexadecimal `&str`/`&String`.
pub trait IntoPersistentId {
    /// Converts `self` into a [`PersistentId`] if valid.
    fn into_persistent_id(self) -> Option<PersistentId>;
}

impl IntoPersistentId for PersistentId {
    fn into_persistent_id(self) -> Option<PersistentId> {
        Some(self)
    }
}

impl IntoPersistentId for &PersistentId {
    fn into_persistent_id(self) -> Option<PersistentId> {
        Some(*self)
    }
}

impl IntoPersistentId for u64 {
    fn into_persistent_id(self) -> Option<PersistentId> {
        Some(PersistentId(self))
    }
}

impl IntoPersistentId for &str {
    fn into_persistent_id(self) -> Option<PersistentId> {
        self.parse::<PersistentId>().ok()
    }
}

impl IntoPersistentId for &String {
    fn into_persistent_id(self) -> Option<PersistentId> {
        self.as_str().parse::<PersistentId>().ok()
    }
}

impl IntoPersistentId for String {
    fn into_persistent_id(self) -> Option<PersistentId> {
        self.as_str().parse::<PersistentId>().ok()
    }
}

bitflags! {
    /// Raw bitflags for `suggestion_flag` (`itma` offset 62..64, `iama` offset 42..44, `iAma` offset 101..103, `lpma` offset 223..225).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// User recommendation / favorite state (`Neutral`, `Favorite`, `SuggestLess`, `Removed`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SuggestionFlag {
    /// `0`: Default / unrated state.
    #[default]
    Neutral,
    /// `1`: Removed from library.
    Removed,
    /// `2` (`0x0002` or `0x0102`): Favorited / loved by the user.
    Favorite,
    /// `3` (`0x0003` or `0x0101` / `0x0103`): Marked "Suggest Less" / disliked by the user.
    SuggestLess,
    /// Unrecognized raw `u16` value.
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
    /// Returns the canonical raw `u16` code for this [`SuggestionFlag`].
    #[must_use]
    pub const fn raw(self) -> u16 {
        match self {
            Self::Neutral => 0,
            Self::Removed => 1,
            Self::Favorite => 2,
            Self::SuggestLess => 3,
            Self::Unknown(v) => v,
        }
    }

    /// Returns the [`SuggestionFlags`] bitmask representation of a raw `suggestion_flag` value.
    #[must_use]
    pub const fn flags(raw: u16) -> SuggestionFlags {
        SuggestionFlags::from_bits_retain(raw)
    }

    /// Returns `true` if the `0x0100` local user-modified bit is set (e.g. `0x0102` when favorited offline, `0x0101` when marked Suggest Less offline).
    #[must_use]
    pub const fn is_locally_modified(raw: u16) -> bool {
        SuggestionFlags::from_bits_retain(raw).contains(SuggestionFlags::LOCALLY_MODIFIED)
    }
}

impl fmt::Display for SuggestionFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Neutral => f.write_str("Neutral"),
            Self::Removed => f.write_str("Removed"),
            Self::Favorite => f.write_str("Favorite"),
            Self::SuggestLess => f.write_str("SuggestLess"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// iCloud Music Library status (`itma` offset 58, Smart Playlist field `0x86` = 134).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CloudStatus {
    /// `0`: Local track when iCloud Music Library is disabled or not applicable (`LocalOnly`).
    #[default]
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

impl CloudStatus {
    /// Returns the raw `u8` status code stored at `itma` offset 58.
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::LocalOnly => 0,
            Self::Purchased => 1,
            Self::Matched => 2,
            Self::Uploaded => 3,
            Self::Ineligible => 4,
            Self::Error => 5,
            Self::SyncError => 6,
            Self::Duplicate => 7,
            Self::AppleMusic => 8,
            Self::NoLongerAvailable => 9,
            Self::NotUploaded => 10,
            Self::Unknown(v) => v,
        }
    }
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

impl fmt::Display for CloudStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocalOnly => f.write_str("LocalOnly"),
            Self::Purchased => f.write_str("Purchased"),
            Self::Matched => f.write_str("Matched"),
            Self::Uploaded => f.write_str("Uploaded"),
            Self::Ineligible => f.write_str("Ineligible"),
            Self::Error => f.write_str("Error"),
            Self::SyncError => f.write_str("SyncError"),
            Self::Duplicate => f.write_str("Duplicate"),
            Self::AppleMusic => f.write_str("AppleMusic"),
            Self::NoLongerAvailable => f.write_str("NoLongerAvailable"),
            Self::NotUploaded => f.write_str("NotUploaded"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Content advisory rating (`itma` offset 59).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ContentRating {
    /// `0`: No content advisory rating.
    #[default]
    None,
    /// `1`: Explicit content.
    Explicit,
    /// `2`: Clean version.
    Clean,
    /// `4`: Parental guidance suggested.
    ParentalGuidance,
    /// Unrecognized raw `u8` content rating value.
    Unknown(u8),
}

impl ContentRating {
    /// Returns the raw `u8` rating code stored at `itma` offset 59.
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Explicit => 1,
            Self::Clean => 2,
            Self::ParentalGuidance => 4,
            Self::Unknown(v) => v,
        }
    }
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

impl fmt::Display for ContentRating {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Explicit => f.write_str("Explicit"),
            Self::Clean => f.write_str("Clean"),
            Self::ParentalGuidance => f.write_str("ParentalGuidance"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Local download vs. cloud-only status (`itma` offset 57).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DownloadedStatus {
    /// `1`: Downloaded locally on disk.
    #[default]
    DownloadedLocal,
    /// `3`: Stored in cloud / not downloaded locally.
    CloudOnly,
    /// Unrecognized raw `u8` value.
    Unknown(u8),
}

impl DownloadedStatus {
    /// Returns the raw `u8` status code stored at `itma` offset 57.
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::DownloadedLocal => 1,
            Self::CloudOnly => 3,
            Self::Unknown(v) => v,
        }
    }
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

impl fmt::Display for DownloadedStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DownloadedLocal => f.write_str("DownloadedLocal"),
            Self::CloudOnly => f.write_str("CloudOnly"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

bitflags! {
    /// Raw bitflags for a track's media kind (`itma` offset 108..112, Smart Playlist field `0x3C` = 60).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct MediaKindFlags: u32 {
        /// `0x0000_0001`: Music audio track.
        const MUSIC         = 0x0000_0001;
        /// `0x0000_0002`: Movie video track.
        const MOVIE         = 0x0000_0002;
        /// `0x0000_0004`: Podcast episode.
        const PODCAST       = 0x0000_0004;
        /// `0x0000_0008`: Audiobook track.
        const AUDIOBOOK     = 0x0000_0008;
        /// `0x0000_0010`: Digital PDF booklet.
        const BOOKLET_PDF   = 0x0000_0010;
        /// `0x0000_0020`: Music video track.
        const MUSIC_VIDEO   = 0x0000_0020;
        /// `0x0000_0040`: TV show episode.
        const TV_SHOW       = 0x0000_0040;
        /// `0x0000_0080`: Stage play.
        const STAGE_PLAY    = 0x0000_0080;
        /// `0x0000_0100`: Interactive digital book.
        const DIGITAL_BOOK  = 0x0000_0100;
        /// `0x0000_0400`: Home video.
        const HOME_VIDEO    = 0x0000_0400;
        /// `0x0000_4000`: Ringtone / alert tone.
        const RINGTONE      = 0x0000_4000;
        /// `0x0000_8000`: Digital audio item.
        const DIGITAL_AUDIO = 0x0000_8000;
        /// `0x0001_0000`: iTunes Extras package.
        const ITUNES_EXTRAS = 0x0001_0000;
        /// `0x0010_0000`: Voice memo recording.
        const VOICE_MEMO    = 0x0010_0000;
        /// `0x0020_0000`: iTunes U educational media.
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
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MediaKind {
    /// Standard music track (`0x01`).
    #[default]
    Music,
    /// Feature film / movie (`0x02`).
    Movie,
    /// Podcast episode (`0x04` or `0x05`).
    Podcast,
    /// Audiobook (`0x08`).
    Audiobook,
    /// Digital booklet PDF (`0x10`).
    BookletPdf,
    /// Music video (`0x20`).
    MusicVideo,
    /// TV show episode (`0x40`).
    TvShow,
    /// Movie or TV show composite (`0x42`).
    MovieOrTvShow,
    /// Home video (`0x400`).
    HomeVideo,
    /// Ringtone (`0x4000`).
    Ringtone,
    /// iTunes Extras (`0x10000`).
    ItunesExtras,
    /// Voice Memo (`0x100000`).
    VoiceMemo,
    /// Smart playlist mask matching all music and audio (`0x0010_21B1`).
    AllMusicAndAudio,
    /// Smart playlist mask matching podcasts and voice memos (`0x0020_8004`).
    PodcastOrVoiceMemo,
    /// Smart playlist mask matching podcasts, iTunes U, and voice memos (`0x0021_8004`).
    PodcastItunesUOrVoiceMemo,
    /// Any other raw bitmask value.
    Other(u32),
}

impl MediaKind {
    /// Returns the raw `u32` media kind bitmask stored at `itma` offset 108..112.
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Music => MediaKindFlags::MUSIC.bits(),
            Self::Movie => MediaKindFlags::MOVIE.bits(),
            Self::Podcast => MediaKindFlags::PODCAST.bits(),
            Self::Audiobook => MediaKindFlags::AUDIOBOOK.bits(),
            Self::BookletPdf => MediaKindFlags::BOOKLET_PDF.bits(),
            Self::MusicVideo => MediaKindFlags::MUSIC_VIDEO.bits(),
            Self::TvShow => MediaKindFlags::TV_SHOW.bits(),
            Self::MovieOrTvShow => MediaKindFlags::MOVIE_OR_TV_SHOW.bits(),
            Self::HomeVideo => MediaKindFlags::HOME_VIDEO.bits(),
            Self::Ringtone => MediaKindFlags::RINGTONE.bits(),
            Self::ItunesExtras => MediaKindFlags::ITUNES_EXTRAS.bits(),
            Self::VoiceMemo => MediaKindFlags::VOICE_MEMO.bits(),
            Self::AllMusicAndAudio => MediaKindFlags::ALL_MUSIC_AND_AUDIO.bits(),
            Self::PodcastOrVoiceMemo => MediaKindFlags::PODCAST_OR_VOICE_MEMO.bits(),
            Self::PodcastItunesUOrVoiceMemo => {
                MediaKindFlags::PODCAST_ITUNES_U_OR_VOICE_MEMO.bits()
            }
            Self::Other(v) => v,
        }
    }

    /// Returns the [`MediaKindFlags`] bitmask representation of a raw `media_kind` value.
    #[must_use]
    pub const fn flags(raw: u32) -> MediaKindFlags {
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

impl fmt::Display for MediaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Music => f.write_str("Music"),
            Self::Movie => f.write_str("Movie"),
            Self::Podcast => f.write_str("Podcast"),
            Self::Audiobook => f.write_str("Audiobook"),
            Self::BookletPdf => f.write_str("BookletPdf"),
            Self::MusicVideo => f.write_str("MusicVideo"),
            Self::TvShow => f.write_str("TvShow"),
            Self::MovieOrTvShow => f.write_str("MovieOrTvShow"),
            Self::HomeVideo => f.write_str("HomeVideo"),
            Self::Ringtone => f.write_str("Ringtone"),
            Self::ItunesExtras => f.write_str("ItunesExtras"),
            Self::VoiceMemo => f.write_str("VoiceMemo"),
            Self::AllMusicAndAudio => f.write_str("AllMusicAndAudio"),
            Self::PodcastOrVoiceMemo => f.write_str("PodcastOrVoiceMemo"),
            Self::PodcastItunesUOrVoiceMemo => f.write_str("PodcastItunesUOrVoiceMemo"),
            Self::Other(v) => write!(f, "Other({v})"),
        }
    }
}

bitflags! {
    /// Raw bitflags for a sortable text field's sort mode (`itma` offset 66..72).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SortMode {
    /// `0x00`: No sort string override is present.
    #[default]
    None,
    /// `0x80` / `0x81`: Auto-generated sort string (e.g. stripping leading `"The "`).
    AutoGenerated,
    /// `0x01` / `0x03`: User-specified custom sort string override.
    CustomUserOverride,
    /// Any other raw sort mode byte.
    Unknown(u8),
}

impl SortMode {
    /// Returns the raw byte representation of this [`SortMode`].
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::None => 0x00,
            Self::AutoGenerated => 0x80,
            Self::CustomUserOverride => 0x01,
            Self::Unknown(v) => v,
        }
    }

    /// Returns the [`SortModeFlags`] bitmask representation of a raw sort mode byte.
    #[must_use]
    pub const fn flags(raw: u8) -> SortModeFlags {
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

impl fmt::Display for SortMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::AutoGenerated => f.write_str("AutoGenerated"),
            Self::CustomUserOverride => f.write_str("CustomUserOverride"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Decoded sort modes for a track's 5 sortable text fields (`itma` offset 66..71).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackSortModes {
    /// Sort mode for `sort_title`.
    pub title: SortMode,
    /// Sort mode for `sort_album`.
    pub album: SortMode,
    /// Sort mode for `sort_artist`.
    pub artist: SortMode,
    /// Sort mode for `sort_album_artist`.
    pub album_artist: SortMode,
    /// Sort mode for `sort_composer`.
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
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EqualizerPreset {
    /// Raw string stored in `boma 0x0007` (e.g. `"#!#116#!#"` or custom preset name).
    pub raw: String,
    /// Numeric built-in preset ID (`100..=121`) if `raw` uses the `#!#<id>#!#` encoding.
    pub preset_id: Option<u32>,
    /// Human-readable equalizer preset name (e.g. `"Rock"`, `"Acoustic"`).
    pub name: String,
}

impl From<&str> for EqualizerPreset {
    fn from(raw: &str) -> Self {
        Self::parse(raw)
    }
}

impl EqualizerPreset {
    /// Parses a raw `boma 0x0007` equalizer string into an [`EqualizerPreset`].
    #[must_use]
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
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlaylistShareMode {
    /// `0`: Private user playlist.
    #[default]
    Private,
    /// `2`: Subscribed Apple Music catalog playlist.
    SubscribedCatalog,
    /// `4`: Publicly shared user playlist shown on profile.
    PublicShared,
    /// Unrecognized raw `u32` share mode value.
    Unknown(u32),
}

impl PlaylistShareMode {
    /// Returns the raw `u32` share mode code stored at `lpma` offset 308..312.
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Private => 0,
            Self::SubscribedCatalog => 2,
            Self::PublicShared => 4,
            Self::Unknown(v) => v,
        }
    }
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

impl fmt::Display for PlaylistShareMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Private => f.write_str("Private"),
            Self::SubscribedCatalog => f.write_str("SubscribedCatalog"),
            Self::PublicShared => f.write_str("PublicShared"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Built-in Apple Music system playlist kind (`lpma` offset 78..80).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DistinguishedPlaylistKind {
    /// `0`: Regular user playlist or folder (not a distinguished system playlist).
    #[default]
    None,
    /// `2`: Movies system playlist.
    Movies,
    /// `3`: TV Shows system playlist.
    TvShows,
    /// `4`: Music system playlist.
    Music,
    /// `5`: Audiobooks system playlist.
    Audiobooks,
    /// `10`: Podcasts system playlist.
    Podcasts,
    /// `14`: Voice Memos system playlist.
    VoiceMemos,
    /// `19`: Purchased music playlist.
    Purchased,
    /// `22`: Legacy iTunes DJ playlist.
    ITunesDj,
    /// `26`: Genius playlist.
    Genius,
    /// `42`: My Top Rated smart playlist.
    MyTopRated,
    /// `43`: Recently Played smart playlist.
    RecentlyPlayed,
    /// `44`: Recently Added smart playlist.
    RecentlyAdded,
    /// `47`: Music Videos system playlist.
    MusicVideos,
    /// `48`: Classical Music system playlist.
    ClassicalMusic,
    /// `49` / `61`: Favourite Songs system playlist.
    FavoriteSongs,
    /// `63`: Internal `Hidden Cloud PlaylistOnly Tracks` container.
    HiddenCloudPlaylistOnlyTracks,
    /// `64`: TV & Movies system playlist.
    TvAndMovies,
    /// `65` / `66`: Downloaded music system playlist.
    Downloaded,
    /// Any other raw distinguished kind value.
    Other(u16),
}

impl DistinguishedPlaylistKind {
    /// Returns the raw `u16` distinguished kind code stored at `lpma` offset 79..81.
    #[must_use]
    pub const fn raw(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Movies => 2,
            Self::TvShows => 3,
            Self::Music => 4,
            Self::Audiobooks => 5,
            Self::Podcasts => 10,
            Self::VoiceMemos => 14,
            Self::Purchased => 19,
            Self::ITunesDj => 22,
            Self::Genius => 26,
            Self::MyTopRated => 42,
            Self::RecentlyPlayed => 43,
            Self::RecentlyAdded => 44,
            Self::MusicVideos => 47,
            Self::ClassicalMusic => 48,
            Self::FavoriteSongs => 49,
            Self::HiddenCloudPlaylistOnlyTracks => 63,
            Self::TvAndMovies => 64,
            Self::Downloaded => 65,
            Self::Other(v) => v,
        }
    }
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
            49 | 61 => Self::FavoriteSongs,
            63 => Self::HiddenCloudPlaylistOnlyTracks,
            64 => Self::TvAndMovies,
            65 | 66 => Self::Downloaded,
            other => Self::Other(other),
        }
    }
}

impl fmt::Display for DistinguishedPlaylistKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Movies => f.write_str("Movies"),
            Self::TvShows => f.write_str("TvShows"),
            Self::Music => f.write_str("Music"),
            Self::Audiobooks => f.write_str("Audiobooks"),
            Self::Podcasts => f.write_str("Podcasts"),
            Self::VoiceMemos => f.write_str("VoiceMemos"),
            Self::Purchased => f.write_str("Purchased"),
            Self::ITunesDj => f.write_str("ITunesDj"),
            Self::Genius => f.write_str("Genius"),
            Self::MyTopRated => f.write_str("MyTopRated"),
            Self::RecentlyPlayed => f.write_str("RecentlyPlayed"),
            Self::RecentlyAdded => f.write_str("RecentlyAdded"),
            Self::MusicVideos => f.write_str("MusicVideos"),
            Self::ClassicalMusic => f.write_str("ClassicalMusic"),
            Self::FavoriteSongs => f.write_str("FavoriteSongs"),
            Self::HiddenCloudPlaylistOnlyTracks => f.write_str("HiddenCloudPlaylistOnlyTracks"),
            Self::TvAndMovies => f.write_str("TvAndMovies"),
            Self::Downloaded => f.write_str("Downloaded"),
            Self::Other(v) => write!(f, "Other({v})"),
        }
    }
}

/// High-level semantic classification of a playlist.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlaylistKind {
    /// Internal master library playlist (`####!####`).
    Master,
    /// Built-in Apple Music system playlist (`Favourite Songs`, `Music Videos`, etc.).
    Distinguished,
    /// Playlist folder containing child playlists/folders.
    Folder,
    /// User-created or system Smart Playlist evaluated via `SLst` rules.
    Smart,
    /// Subscribed Apple Music catalog playlist.
    SubscribedCatalog,
    /// Standard user-curated track playlist.
    #[default]
    Standard,
}

impl fmt::Display for PlaylistKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Master => f.write_str("Master"),
            Self::Distinguished => f.write_str("Distinguished"),
            Self::Folder => f.write_str("Folder"),
            Self::Smart => f.write_str("Smart"),
            Self::SubscribedCatalog => f.write_str("SubscribedCatalog"),
            Self::Standard => f.write_str("Standard"),
        }
    }
}

/// Smart Playlist configuration (`boma 0x00CA` options + `boma 0x00C9` `SLst` rule tree).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmartPlaylistConfig {
    /// Whether "Live updating" is enabled.
    pub live_updating: bool,
    /// Whether rule matching is enabled.
    pub match_rules: bool,
    /// Whether "Match only checked items" is enabled.
    pub match_only_checked_items: bool,
    /// Optional maximum item/duration/size limit (`Limit to ...`).
    pub limit: Option<SmartPlaylistLimit>,
    /// Root `SLst` rule group tree.
    pub rule_group: Option<SmartRuleGroup>,
}

/// Maximum item, duration, or storage size limit on a [`SmartPlaylistConfig`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmartPlaylistLimit {
    /// Maximum count in units of [`Self::unit`].
    pub count: u32,
    /// Unit for [`Self::count`] (`Items`, `Minutes`, `Hours`, `Megabytes`, `Gigabytes`).
    pub unit: SmartLimitUnit,
    /// Selection ordering when truncating to the limit (`Random`, `MostRecentlyAdded`, etc.).
    pub selection: SmartSelectionMethod,
}

/// Unit for a [`SmartPlaylistLimit`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmartLimitUnit {
    /// `1`: Limit by duration in minutes.
    Minutes,
    /// `2`: Limit by size in megabytes (MB).
    Megabytes,
    /// `3`: Limit by number of items/tracks.
    #[default]
    Items,
    /// `4`: Limit by duration in hours.
    Hours,
    /// `5`: Limit by size in gigabytes (GB).
    Gigabytes,
    /// Unrecognized raw `u8` limit unit.
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

impl fmt::Display for SmartLimitUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Minutes => f.write_str("Minutes"),
            Self::Megabytes => f.write_str("Megabytes"),
            Self::Items => f.write_str("Items"),
            Self::Hours => f.write_str("Hours"),
            Self::Gigabytes => f.write_str("Gigabytes"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Selection ordering method for a [`SmartPlaylistLimit`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmartSelectionMethod {
    /// `0x02`: Random selection.
    #[default]
    Random,
    /// `0x05`: Alphabetical by title.
    Title,
    /// `0x06`: Alphabetical by album.
    Album,
    /// `0x07`: Alphabetical by artist.
    Artist,
    /// `0x09`: Alphabetical by genre.
    Genre,
    /// `0x15` (`negate = 0`): Most recently added.
    MostRecentlyAdded,
    /// `0x15` (`negate = 1`): Least recently added.
    LeastRecentlyAdded,
    /// `0x19` (`negate = 0`): Most often played.
    MostOftenPlayed,
    /// `0x19` (`negate = 1`): Least often played.
    LeastOftenPlayed,
    /// `0x1A` (`negate = 0`): Most recently played.
    MostRecentlyPlayed,
    /// `0x1A` (`negate = 1`): Least recently played.
    LeastRecentlyPlayed,
    /// `0x1C` (`negate = 0`): Highest star rating.
    HighestRating,
    /// `0x1C` (`negate = 1`): Lowest star rating.
    LowestRating,
    /// Unrecognized raw `(method, negate)` pair.
    Unknown {
        /// Raw `limit_selection_method` byte.
        method: u8,
        /// Raw `negate_limit_selection_method` byte.
        negate: u8,
    },
}

impl From<(u8, u8)> for SmartSelectionMethod {
    fn from((method, negate): (u8, u8)) -> Self {
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

impl fmt::Display for SmartSelectionMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Random => f.write_str("Random"),
            Self::Title => f.write_str("Title"),
            Self::Album => f.write_str("Album"),
            Self::Artist => f.write_str("Artist"),
            Self::Genre => f.write_str("Genre"),
            Self::MostRecentlyAdded => f.write_str("MostRecentlyAdded"),
            Self::LeastRecentlyAdded => f.write_str("LeastRecentlyAdded"),
            Self::MostOftenPlayed => f.write_str("MostOftenPlayed"),
            Self::LeastOftenPlayed => f.write_str("LeastOftenPlayed"),
            Self::MostRecentlyPlayed => f.write_str("MostRecentlyPlayed"),
            Self::LeastRecentlyPlayed => f.write_str("LeastRecentlyPlayed"),
            Self::HighestRating => f.write_str("HighestRating"),
            Self::LowestRating => f.write_str("LowestRating"),
            Self::Unknown { method, negate } => {
                write!(f, "Unknown {{ method: {method}, negate: {negate} }}")
            }
        }
    }
}

/// Logical conjunction (`All` = `AND`, `Any` = `OR`) of a [`SmartRuleGroup`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmartConjunction {
    /// `0`: Match ALL of the child rules (`AND`).
    #[default]
    All,
    /// `1`: Match ANY of the child rules (`OR`).
    Any,
}

impl fmt::Display for SmartConjunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("All"),
            Self::Any => f.write_str("Any"),
        }
    }
}

/// A group of Smart Playlist rules joined by a [`SmartConjunction`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmartRuleGroup {
    /// Logical conjunction (`All` or `Any`) combining `rules`.
    pub conjunction: SmartConjunction,
    /// Ordered child rules or nested rule groups.
    pub rules: Vec<SmartRuleNode>,
}

/// A node in a [`SmartRuleGroup`] tree (either a nested group or a leaf condition).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "node_type"))]
pub enum SmartRuleNode {
    /// Nested `SLst` rule group (`AND` / `OR` sub-tree).
    Group(SmartRuleGroup),
    /// Single leaf comparison rule.
    Condition(SmartCondition),
}

/// Target metadata field of a [`SmartCondition`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmartField {
    /// `0x02`: Track title.
    Title,
    /// `0x03`: Album title.
    Album,
    /// `0x04`: Artist name.
    Artist,
    /// `0x05`: Audio bit rate in kbps.
    BitRate,
    /// `0x06`: Audio sample rate in Hz.
    SampleRate,
    /// `0x07`: Release year.
    Year,
    /// `0x08`: Genre name.
    Genre,
    /// `0x09`: File kind description string.
    Kind,
    /// `0x0A`: Date modified.
    DateModified,
    /// `0x0B`: Track number on disc.
    TrackNumber,
    /// `0x0C` / `0x1C`: File size.
    Size,
    /// `0x0D`: Track duration.
    Time,
    /// `0x0E`: Comments text.
    Comments,
    /// `0x10`: Date added to library.
    DateAdded,
    /// `0x12`: Composer name.
    Composer,
    /// `0x16`: Play count.
    Plays,
    /// `0x17`: Date last played.
    LastPlayed,
    /// `0x18`: Disc number.
    DiscNumber,
    /// `0x19`: User star rating.
    Rating,
    /// `0x1D`: Track checkbox state (`Checked`).
    Checked,
    /// `0x1F`: Compilation flag.
    Compilation,
    /// `0x23`: Beats per minute (BPM).
    Bpm,
    /// `0x25`: Has album artwork boolean.
    AlbumArtwork,
    /// `0x27`: Grouping text.
    Grouping,
    /// `0x28`: Membership in another playlist (`Playlist is / is not ...`).
    Playlist,
    /// `0x29`: Purchased flag.
    Purchased,
    /// `0x36`: Description text.
    Description,
    /// `0x37`: Category text.
    Category,
    /// `0x39` / `0x5A`: Album star rating.
    AlbumRating,
    /// `0x3C`: Media kind bitmask.
    MediaKind,
    /// `0x44`: Skip count.
    Skips,
    /// `0x45`: Date last skipped.
    LastSkipped,
    /// `0x47`: Album artist name.
    AlbumArtist,
    /// `0x4E`: Sort title.
    SortTitle,
    /// `0x4F`: Sort album.
    SortAlbum,
    /// `0x50`: Sort artist.
    SortArtist,
    /// `0x51`: Sort album artist.
    SortAlbumArtist,
    /// `0x52`: Sort composer.
    SortComposer,
    /// `0x53`: Sort show.
    SortShow,
    /// `0x59`: Video rating.
    VideoRating,
    /// `0x85`: Track storage location (`Computer`, `iCloud`).
    Location,
    /// `0x86`: iCloud Music Library status (`Matched`, `Uploaded`, `Apple Music`, etc.).
    CloudStatus,
    /// `0x9A`: Track favorite / suggest-less flag.
    Favorite,
    /// `0x9C`: Album favorite / suggest-less flag.
    AlbumFavorite,
    /// `0x9E` / `0x9F`: Classical work name.
    Work,
    /// `0xA0`: Classical movement name.
    MovementName,
    /// `0xA1`: Classical movement number.
    MovementNumber,
    /// `0xA4`: Has video boolean.
    HasVideo,
    /// Unrecognized `SLst` field ID.
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

impl fmt::Display for SmartField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Title => f.write_str("Title"),
            Self::Album => f.write_str("Album"),
            Self::Artist => f.write_str("Artist"),
            Self::BitRate => f.write_str("BitRate"),
            Self::SampleRate => f.write_str("SampleRate"),
            Self::Year => f.write_str("Year"),
            Self::Genre => f.write_str("Genre"),
            Self::Kind => f.write_str("Kind"),
            Self::DateModified => f.write_str("DateModified"),
            Self::TrackNumber => f.write_str("TrackNumber"),
            Self::Size => f.write_str("Size"),
            Self::Time => f.write_str("Time"),
            Self::Comments => f.write_str("Comments"),
            Self::DateAdded => f.write_str("DateAdded"),
            Self::Composer => f.write_str("Composer"),
            Self::Plays => f.write_str("Plays"),
            Self::LastPlayed => f.write_str("LastPlayed"),
            Self::DiscNumber => f.write_str("DiscNumber"),
            Self::Rating => f.write_str("Rating"),
            Self::Checked => f.write_str("Checked"),
            Self::Compilation => f.write_str("Compilation"),
            Self::Bpm => f.write_str("Bpm"),
            Self::AlbumArtwork => f.write_str("AlbumArtwork"),
            Self::Grouping => f.write_str("Grouping"),
            Self::Playlist => f.write_str("Playlist"),
            Self::Purchased => f.write_str("Purchased"),
            Self::Description => f.write_str("Description"),
            Self::Category => f.write_str("Category"),
            Self::AlbumRating => f.write_str("AlbumRating"),
            Self::MediaKind => f.write_str("MediaKind"),
            Self::Skips => f.write_str("Skips"),
            Self::LastSkipped => f.write_str("LastSkipped"),
            Self::AlbumArtist => f.write_str("AlbumArtist"),
            Self::SortTitle => f.write_str("SortTitle"),
            Self::SortAlbum => f.write_str("SortAlbum"),
            Self::SortArtist => f.write_str("SortArtist"),
            Self::SortAlbumArtist => f.write_str("SortAlbumArtist"),
            Self::SortComposer => f.write_str("SortComposer"),
            Self::SortShow => f.write_str("SortShow"),
            Self::VideoRating => f.write_str("VideoRating"),
            Self::Location => f.write_str("Location"),
            Self::CloudStatus => f.write_str("CloudStatus"),
            Self::Favorite => f.write_str("Favorite"),
            Self::AlbumFavorite => f.write_str("AlbumFavorite"),
            Self::Work => f.write_str("Work"),
            Self::MovementName => f.write_str("MovementName"),
            Self::MovementNumber => f.write_str("MovementNumber"),
            Self::HasVideo => f.write_str("HasVideo"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

bitflags! {
    /// Raw bitflags for a Smart Playlist comparison operator (`SLst` rule offset 4..8, Big-Endian).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct SmartOperatorFlags: u32 {
        /// `0x0000_0001`: Equality (`is`).
        const IS            = 0x0000_0001;
        /// `0x0000_0002`: Substring match (`contains`).
        const CONTAINS      = 0x0000_0002;
        /// `0x0000_0004`: Prefix match (`starts with`).
        const STARTS_WITH   = 0x0000_0004;
        /// `0x0000_0008`: Suffix match (`ends with`).
        const ENDS_WITH     = 0x0000_0008;
        /// `0x0000_0010`: Strictly greater than (`is greater than`).
        const GREATER_THAN  = 0x0000_0010;
        /// `0x0000_0020`: Greater than or equal to.
        const GREATER_OR_EQ = 0x0000_0020;
        /// `0x0000_0040`: Strictly less than (`is less than`).
        const LESS_THAN     = 0x0000_0040;
        /// `0x0000_0080`: Less than or equal to.
        const LESS_OR_EQ    = 0x0000_0080;
        /// `0x0000_0100`: Inclusive range (`is in the range`).
        const IN_RANGE      = 0x0000_0100;
        /// `0x0000_0200`: Relative time window (`is in the last`).
        const IN_THE_LAST   = 0x0000_0200;
        /// `0x0000_0400`: Bitmask intersection (`MediaKind`).
        const BITWISE_SET   = 0x0000_0400;
        /// `0x0100_0000`: UTF-16BE string comparison mode.
        const STRING_MODE   = 0x0100_0000;
        /// `0x0200_0000`: Logical negation (`not`).
        const NEGATE        = 0x0200_0000;
        /// `0x0400_0000`: Sign modifier on relative date comparisons.
        const SIGN_MODIFIER = 0x0400_0000;
    }
}

/// Comparison operator of a [`SmartCondition`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmartOperator {
    /// Exact match (`is`).
    Is,
    /// Negated exact match (`is not`).
    IsNot,
    /// Substring match (`contains`).
    Contains,
    /// Negated substring match (`does not contain`).
    DoesNotContain,
    /// Prefix match (`starts with`).
    StartsWith,
    /// Suffix match (`ends with`).
    EndsWith,
    /// Strictly greater than (`is greater than`).
    GreaterThan,
    /// Strictly less than (`is less than`).
    LessThan,
    /// Inclusive range (`is in the range`).
    InRange,
    /// Negated inclusive range (`is not in the range`).
    NotInRange,
    /// Relative rolling time window (`is in the last`).
    InTheLast,
    /// Negated relative rolling time window (`is not in the last`).
    NotInTheLast,
    /// Unrecognized raw `u32` operator bitmask.
    Unknown(u32),
}

impl SmartOperator {
    /// Returns the [`SmartOperatorFlags`] bitmask representation of a raw `SLst` operator value.
    #[must_use]
    pub const fn flags(raw: u32) -> SmartOperatorFlags {
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

impl fmt::Display for SmartOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Is => f.write_str("Is"),
            Self::IsNot => f.write_str("IsNot"),
            Self::Contains => f.write_str("Contains"),
            Self::DoesNotContain => f.write_str("DoesNotContain"),
            Self::StartsWith => f.write_str("StartsWith"),
            Self::EndsWith => f.write_str("EndsWith"),
            Self::GreaterThan => f.write_str("GreaterThan"),
            Self::LessThan => f.write_str("LessThan"),
            Self::InRange => f.write_str("InRange"),
            Self::NotInRange => f.write_str("NotInRange"),
            Self::InTheLast => f.write_str("InTheLast"),
            Self::NotInTheLast => f.write_str("NotInTheLast"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Time unit for `InTheLast` / `NotInTheLast` Smart Playlist rules.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RelativeTimeUnit {
    /// Days (`86_400` seconds).
    Days,
    /// Weeks (`604_800` seconds).
    Weeks,
    /// Months (`2_628_000` seconds).
    Months,
    /// Unrecognized raw unit value.
    Unknown(i64),
}

impl fmt::Display for RelativeTimeUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Days => f.write_str("Days"),
            Self::Weeks => f.write_str("Weeks"),
            Self::Months => f.write_str("Months"),
            Self::Unknown(v) => write!(f, "Unknown({v})"),
        }
    }
}

/// Typed comparison argument value of a [`SmartCondition`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "value_type"))]
pub enum SmartRuleValue {
    /// UTF-16BE string comparison operand.
    Text {
        /// Decoded text string.
        text: String,
    },
    /// Boolean comparison operand (`Checked`, `Compilation`, `Purchased`, `AlbumArtwork`, `HasVideo`).
    Boolean {
        /// Decoded boolean value.
        value: bool,
    },
    /// Single 64-bit signed integer operand.
    Integer {
        /// Integer value.
        value: i64,
    },
    /// Inclusive integer range (`min..=max`).
    IntegerRange {
        /// Lower bound (inclusive).
        min: i64,
        /// Upper bound (inclusive).
        max: i64,
    },
    /// Single star rating operand (`0..=5` stars, stored as `0..=100` in steps of 20).
    StarRating {
        /// Number of stars (`0..=5`).
        stars: u8,
        /// Raw `0..=100` rating value.
        raw_0_100: i64,
    },
    /// Star rating range (`min_stars..=max_stars`).
    StarRatingRange {
        /// Minimum stars (`0..=5`).
        min_stars: u8,
        /// Maximum stars (`0..=5`).
        max_stars: u8,
        /// Minimum raw `0..=100` value.
        min_raw: i64,
        /// Maximum raw `0..=100` value.
        max_raw: i64,
    },
    /// Relative rolling time window (`in the last N days/weeks/months`).
    RelativeTime {
        /// Number of units.
        amount: u64,
        /// Time unit (`Days`, `Weeks`, `Months`).
        unit: RelativeTimeUnit,
        /// Raw seconds per unit from `arg_2`.
        seconds_per_unit: i64,
    },
    /// Absolute timestamp comparison operand.
    Date {
        /// Raw Apple HFS+ timestamp (seconds since 1904-01-01).
        apple_timestamp: i64,
        /// Converted UTC timestamp if valid.
        utc: Option<DateTime<Utc>>,
    },
    /// Inclusive date range comparison operand.
    DateRange {
        /// Start Apple HFS+ timestamp.
        start_apple: i64,
        /// End Apple HFS+ timestamp.
        end_apple: i64,
        /// Converted start UTC timestamp.
        start_utc: Option<DateTime<Utc>>,
        /// Converted end UTC timestamp.
        end_utc: Option<DateTime<Utc>>,
    },
    /// Reference to another playlist by [`PersistentId`].
    PlaylistRef {
        /// Target playlist Persistent ID.
        playlist_persistent_id: PersistentId,
        /// Resolved name of the target playlist if present in the library.
        playlist_name: Option<String>,
    },
    /// [`CloudStatus`] comparison operand.
    CloudStatus {
        /// Target cloud status.
        status: CloudStatus,
    },
    /// [`SuggestionFlag`] comparison operand (`Favorite`, `AlbumFavorite`).
    SuggestionFlag {
        /// Target suggestion flag.
        flag: SuggestionFlag,
    },
    /// [`MediaKind`] comparison operand.
    MediaKind {
        /// Target media kind.
        kind: MediaKind,
    },
    /// Track storage location comparison operand (`Computer`, `iCloud`, `On This Computer`).
    LocationKind {
        /// Raw location code.
        code: i64,
        /// Human-readable location name.
        name: String,
    },
    /// Unparsed raw rule child bytes encoded as hex.
    Raw {
        /// Lowercase hexadecimal string of the raw payload bytes.
        hex: String,
    },
}

/// A single decoded Smart Playlist condition inside a [`SmartRuleGroup`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmartCondition {
    /// Raw `SLst` field ID.
    pub field_id: u32,
    /// Decoded target [`SmartField`].
    pub field: SmartField,
    /// Decoded [`SmartOperator`].
    pub operator: SmartOperator,
    /// Raw 32-bit comparison method bitflags.
    pub comparison_raw: u32,
    /// Decoded comparison operand value.
    pub value: SmartRuleValue,
    /// Human-readable summary of the condition (e.g. `"Rating is greater than 3 stars"`).
    pub description: String,
}

/// View & sorting preferences loaded from `.musiclibrary/preferences/Playlist_<ID>.plist` or `Songs.plist`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlaylistViewPreferences {
    /// View mode (e.g. `"List"`, `"Songs"`, `"Albums"`, `"Playlist"`).
    pub view_as: Option<String>,
    /// Primary sort column name (e.g. `"CloudOrArtist"`, `"Album"`, `"Title"`).
    pub sort_field: Option<String>,
    /// Whether the primary sort order is ascending (`true`) or descending (`false`).
    pub sort_ascending: Option<bool>,
    /// Ordered list of visible column names in List/Songs view.
    pub visible_columns: Vec<String>,
}

/// High-level Domain Model of a parsed `.musicdb` library.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Library {
    /// Library-wide metadata from `hfma`, `plma`, and `Preferences.plist`.
    pub metadata: LibraryMetadata,
    /// All album records (`iama`) in the library.
    pub albums: Vec<Album>,
    /// All artist records (`iAma`) in the library.
    pub artists: Vec<Artist>,
    /// All track records (`itma`) in the library.
    pub tracks: Vec<Track>,
    /// All playlist records (`lpma`) in the library.
    pub playlists: Vec<Playlist>,
    /// All Apple Music social profile records (`isma`) in the library.
    pub social_profiles: Vec<SocialProfile>,
    #[cfg_attr(feature = "serde", serde(skip))]
    track_index: HashMap<PersistentId, usize>,
    #[cfg_attr(feature = "serde", serde(skip))]
    album_index: HashMap<PersistentId, usize>,
    #[cfg_attr(feature = "serde", serde(skip))]
    artist_index: HashMap<PersistentId, usize>,
    #[cfg_attr(feature = "serde", serde(skip))]
    playlist_index: HashMap<PersistentId, usize>,
    #[cfg_attr(feature = "serde", serde(skip))]
    social_profile_index: HashMap<PersistentId, usize>,
}

/// Apple Music social profile record (`isma`, `hsma` subtype 15).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SocialProfile {
    /// Social Profile Persistent ID (referenced by [`Playlist::author_social_profile_persistent_id`]).
    pub persistent_id: PersistentId,
    /// Public Apple Music profile ID (`boma 0x0320`, e.g. `"p.7J6g4W8v0R"`).
    pub profile_id: Option<String>,
    /// Display name (`boma 0x0321`).
    pub name: Option<String>,
    /// User handle without `@` (`boma 0x0322`).
    pub handle: Option<String>,
    /// Avatar identifier (`boma 0x0323`).
    pub avatar_id: Option<String>,
    /// Avatar image URL (`boma 0x0324`).
    pub avatar_url: Option<String>,
}

/// Library-level metadata combined from `hfma`, `plma`, `Preferences.plist`, and `preferences/Songs.plist`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LibraryMetadata {
    /// Primary 64-bit Library Persistent ID.
    pub persistent_id: PersistentId,
    /// Original iTunes Library Persistent ID if migrated from `.itl`.
    pub itunes_persistent_id: Option<PersistentId>,
    /// Revision hash updated on every save (shared with `Library Preferences.musicdb`).
    pub revision_hash: PersistentId,
    /// Secondary file size mirror in format >= 23.1.
    pub file_size_mirror: u32,
    /// Apple Music / iTunes application version string (e.g. `"1.6.0.151"`).
    pub app_version: String,
    /// Binary format major version (e.g. `20`, `21`, `23`).
    pub format_major_version: u16,
    /// Binary format minor version (e.g. `2`, `20`, `40`, `1`).
    pub format_minor_version: u16,
    /// `.musicdb` file type code (`6` or `7` for `Library.musicdb`, `2` for `Library Preferences.musicdb`).
    pub file_type: u32,
    /// Declared track count in `hfma`.
    pub declared_track_count: u32,
    /// Declared album count in `hfma`.
    pub declared_album_count: u32,
    /// Declared artist count in `hfma`.
    pub declared_artist_count: u32,
    /// Declared playlist count in `hfma`.
    pub declared_playlist_count: u32,
    /// Local timezone offset from UTC in seconds (e.g. `-10800` for UTC-3).
    pub timezone_offset_seconds: i32,
    /// Signed-in Apple Store account `dsid` (`0` if signed out).
    pub apple_store_account_id: u64,
    /// Library modification timestamp from `hfma`.
    pub date_modified: Option<DateTime<Utc>>,
    /// "Settings -> General -> Show -> Songs list checkboxes" setting from `plma`.
    pub show_song_list_checkboxes: Option<bool>,
    /// "Settings -> Files -> Keep Music Media folder organized" setting from `plma`.
    pub keep_media_folder_organized: Option<bool>,
    /// "Settings -> Files -> Copy files to Music Media folder when adding to library" from `Preferences.plist`.
    pub copy_files_to_media_folder: Option<bool>,
    /// Media folder `file://` URI from `plma` `boma 0x01F8`.
    pub media_folder_uri: Option<String>,
    /// Media folder filesystem path from `plma` `boma 0x01FD` / `0x0200`.
    pub media_folder_path: Option<String>,
    /// Legacy `.itl` file path from `plma` `boma 0x01FC`.
    pub imported_itl_path: Option<String>,
    /// Global Songs view preferences loaded from `preferences/Songs.plist`.
    pub songs_view_preferences: Option<PlaylistViewPreferences>,
}

/// Decoded Album record (`iama`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Album {
    /// Album Persistent ID (`iama` offset 16..24).
    pub persistent_id: PersistentId,
    /// Secondary / iTunes Album Persistent ID (`iama` offset 64..72).
    pub itunes_persistent_id: Option<PersistentId>,
    /// Persistent ID of the first track in the album (`iama` offset 32..40).
    pub first_track_persistent_id: Option<PersistentId>,
    /// Album title (`boma 0x012C`).
    pub title: Option<String>,
    /// Primary artist name (`boma 0x012D`).
    pub artist: Option<String>,
    /// Album artist name (`boma 0x012E`).
    pub album_artist: Option<String>,
    /// iCloud Music Library album ID (`boma 0x0133`).
    pub cloud_album_id: Option<String>,
    /// Album editorial notes (`boma 0x0134`).
    pub editorial_notes: Option<String>,
    /// Compilation album flag (`iama` offset 25).
    pub is_compilation: bool,
    /// Raw star rating (`0..=100` in steps of 20).
    pub star_rating: u8,
    /// Whether `star_rating` is computed from track ratings (`0x20`) rather than set directly (`0x01`).
    pub star_rating_is_computed: bool,
    /// User suggestion / favorite flag (`iama` offset 42..44).
    pub suggestion_flag: SuggestionFlag,
    /// Whether `suggestion_flag` has the `0x0100` locally-modified bit set.
    pub suggestion_locally_modified: bool,
    /// Timestamp when `suggestion_flag` was last modified (`iama` offset 96..100).
    pub date_suggestion_flag_modified: Option<DateTime<Utc>>,
    /// Most recent last-played timestamp across tracks on this album (`iama` offset 100..104).
    pub date_last_played: Option<DateTime<Utc>>,
    /// iCloud sync timestamp (`iama` offset 120..124).
    pub date_cloud_synced: Option<DateTime<Utc>>,
    /// Album release date (`iama` offset 124..128).
    pub date_released: Option<DateTime<Utc>>,
    /// 64-bit Apple Music Store Album ID (`plID`, `iama` offset 104..112).
    pub apple_music_store_album_id: Option<u64>,
}

impl Album {
    /// Returns the star rating on a `0..=5` star scale (`star_rating / 20`).
    #[must_use]
    pub const fn stars(&self) -> u8 {
        self.star_rating / 20
    }
}

/// Decoded Artist record (`iAma`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Artist {
    /// Artist Persistent ID (`iAma` offset 16..24).
    pub persistent_id: PersistentId,
    /// Secondary / iTunes Artist Persistent ID (`iAma` offset 80..88).
    pub itunes_persistent_id: Option<PersistentId>,
    /// Artist name (`boma 0x0190`).
    pub name: Option<String>,
    /// Sort artist name (`boma 0x0191`).
    pub sort_name: Option<String>,
    /// iCloud Music Library artist ID (`boma 0x0193`).
    pub cloud_artist_id: Option<String>,
    /// User suggestion / favorite flag (`iAma` offset 101..103).
    pub suggestion_flag: SuggestionFlag,
    /// Whether `suggestion_flag` has the `0x0100` locally-modified bit set.
    pub suggestion_locally_modified: bool,
    /// Timestamp when `suggestion_flag` was last modified (`iAma` offset 60..64).
    pub date_suggestion_flag_modified: Option<DateTime<Utc>>,
    /// iCloud sync timestamp (`iAma` offset 120..124).
    pub date_cloud_synced: Option<DateTime<Utc>>,
    /// 64-bit Apple Music Store Artist ID (`atID`, `iAma` offset 52..60).
    pub apple_music_store_artist_id: u64,
    /// Artwork UUID in `artwork.sqlite` (`iAma` offset 64..80).
    pub artwork_uuid: Option<String>,
    /// Raw XML plist containing artist artwork URLs (`boma 0x0192`).
    pub artwork_url_plist: Option<String>,
}

/// Decoded Track record (`itma` + `boma` children).
#[non_exhaustive]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Apple Music tracks store 12 independent boolean metadata checkboxes in itma"
)]
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Track {
    /// Track Persistent ID (`itma` offset 16..24).
    pub persistent_id: PersistentId,
    /// Track title (`boma 0x0002`).
    pub title: Option<String>,
    /// Album title (`boma 0x0003`).
    pub album: Option<String>,
    /// Track artist name (`boma 0x0004`).
    pub artist: Option<String>,
    /// Album artist name (`boma 0x001B`).
    pub album_artist: Option<String>,
    /// Composer name (`boma 0x000C`).
    pub composer: Option<String>,
    /// Genre name (`boma 0x0005`).
    pub genre: Option<String>,
    /// Apple Store Genre ID (`geID`, `TrackNumerics` offset 264..268).
    pub store_genre_id: Option<u32>,
    /// Localized file kind description (`boma 0x0006`).
    pub kind: Option<String>,
    /// User comments (`boma 0x0008`).
    pub comments: Option<String>,
    /// Grouping tag (`boma 0x000E`).
    pub grouping: Option<String>,
    /// Classical work name (`boma 0x003F`).
    pub work_name: Option<String>,
    /// Classical movement name (`boma 0x0040`).
    pub movement_name: Option<String>,
    /// Classical movement number (`itma` offset 88..90).
    pub movement_number: u16,
    /// Classical total movements (`itma` offset 86..88).
    pub movement_total: u16,
    /// Sort title (`boma 0x001E`).
    pub sort_title: Option<String>,
    /// Sort album (`boma 0x001F`).
    pub sort_album: Option<String>,
    /// Sort artist (`boma 0x0020`).
    pub sort_artist: Option<String>,
    /// Sort album artist (`boma 0x0021`).
    pub sort_album_artist: Option<String>,
    /// Sort composer (`boma 0x0022`).
    pub sort_composer: Option<String>,
    /// Sort string generation modes (`itma` offset 66..71).
    pub sort_modes: TrackSortModes,
    /// Track number on disc (`itma` offset 160..162).
    pub track_number: u16,
    /// Total tracks on disc (`itma` offset 116..118).
    pub track_total: u16,
    /// Disc number (`itma` offset 84..86).
    pub disc_number: u16,
    /// Total discs (`itma` offset 90..92).
    pub disc_total: u16,
    /// Release year (`itma` offset 168..172).
    pub year: u32,
    /// Beats per minute (`itma` offset 82..84).
    pub bpm: u16,
    /// Duration in milliseconds (`TrackNumerics` offset 156..160).
    pub duration_ms: Option<u32>,
    /// File size in bytes (`TrackNumerics` offset 296..300).
    pub file_size: Option<u32>,
    /// Audio bit rate in kbps (`TrackNumerics` offset 88..92).
    pub bit_rate: Option<u32>,
    /// Whether the audio file uses Variable Bit Rate (`TrackNumerics` offset 27).
    pub is_vbr: Option<bool>,
    /// Audio sample rate in Hz (`TrackNumerics` offset 60..64).
    pub sample_rate: Option<f32>,
    /// 4-character file type code (e.g. `"M4A "`, `"MP3 "`).
    pub file_type_fourcc: Option<String>,
    /// 4-character audio codec code (e.g. `"mp4a"`, `"mpeg"`).
    pub codec_fourcc: Option<String>,
    /// Volume adjustment in `-255..=255` (`itma` offset 92..96).
    pub volume_adjustment: i32,
    /// Sound Check volume normalization (`iTunNORM`, `TrackNumerics` offset 132..136).
    pub normalization: Option<u32>,
    /// Gapless playback encoder delay in samples (`TrackNumerics` offset 108..112).
    pub gapless_encoder_delay: Option<u32>,
    /// Gapless playback encoder padding in samples (`TrackNumerics` offset 112..116).
    pub gapless_encoder_padding: Option<u32>,
    /// Gapless playback total sample count (`TrackNumerics` offset 256..264).
    pub gapless_total_samples: Option<u64>,
    /// Gapless playback byte offset to the 8th-from-last MPEG frame (`TrackNumerics` offset 272..280).
    pub gapless_last_eight_frames_offset: Option<u64>,
    /// Raw equalizer string (`boma 0x0007`).
    pub equalizer: Option<String>,
    /// Decoded equalizer preset (`boma 0x0007`).
    pub equalizer_preset: Option<EqualizerPreset>,
    /// Raw star rating (`0..=100` in steps of 20, `itma` offset 65).
    pub star_rating: u8,
    /// User suggestion / favorite flag (`itma` offset 62..64).
    pub suggestion_flag: SuggestionFlag,
    /// Whether the track is unchecked/disabled (`itma` offset 42).
    pub disabled: bool,
    /// "Skip when shuffling" checkbox (`itma` offset 30).
    pub skip_when_shuffling: bool,
    /// "Remember playback position" checkbox (`itma` offset 50).
    pub remember_playback_position: bool,
    /// "Show composer in all views" checkbox (`itma` offset 51).
    pub show_composer_in_all_views: bool,
    /// "Use work and movement" checkbox (`itma` offset 52).
    pub use_work_and_movement: bool,
    /// "Album is a compilation of songs by various artists" checkbox (`itma` offset 38).
    pub album_is_compilation: bool,
    /// "Part of a gapless album" checkbox (`itma` offset 49).
    pub part_of_gapless_album: bool,
    /// Whether the track exists only inside a subscribed/shared playlist and not in the main library (`itma` offset 39).
    pub playlist_only: bool,
    /// Whether the track was purchased from the iTunes Store.
    pub purchased: bool,
    /// Whether custom lyrics exist for this track (`itma` offset 255).
    pub has_custom_lyrics: bool,
    /// Whether custom lyrics were set locally (`itma` offset 44).
    pub has_local_custom_lyrics: bool,
    /// Whether local metadata edits are pending iCloud sync (`itma` offset 46).
    pub has_unsynced_local_changes: bool,
    /// Checksum/hash of custom lyrics (`TrackNumerics` offset 124..128).
    pub custom_lyrics_hash: Option<u32>,
    /// Whether the track is matched via iTunes Match (`TrackNumerics` offset 32..34).
    pub is_itunes_matched: Option<bool>,
    /// Whether the track is an Apple Music subscription catalog item (`TrackNumerics` offset 34).
    pub is_apple_music_catalog: Option<bool>,
    /// iCloud Music Library `sagaId` (`itma` offset 236..244).
    pub cloud_saga_id: Option<u64>,
    /// 64-bit Apple Music / iTunes Store track `adamId` (`TrackNumerics` offset 304..312).
    pub store_track_adam_id: Option<u64>,
    /// 64-bit Apple Music Store Album ID (`plID`, `TrackNumerics` offset 160..168).
    pub apple_music_store_album_id: Option<u64>,
    /// 64-bit Apple Music Store Artist ID (`atID`, `TrackNumerics` offset 168..176).
    pub apple_music_store_artist_id: Option<u64>,
    /// 64-bit Apple Music Store Album Artist ID (`TrackNumerics` offset 208..216).
    pub apple_music_store_album_artist_id: Option<u64>,
    /// Apple iTunes Storefront / Country ID (`sfID`, `TrackNumerics` offset 312..316).
    pub storefront_id: Option<u32>,
    /// Apple ID email of the purchaser (`boma 0x003B`).
    pub purchaser_email: Option<String>,
    /// Full name of the purchaser (`boma 0x003C`).
    pub purchaser_name: Option<String>,
    /// Local vs. cloud download state (`itma` offset 57).
    pub downloaded_state: DownloadedStatus,
    /// iCloud Music Library status (`itma` offset 58).
    pub cloud_status: CloudStatus,
    /// Cloud album identifier from `boma 0x0036` plist.
    pub cloud_album_id: Option<String>,
    /// Cloud artist identifier from `boma 0x0036` plist.
    pub cloud_artist_id: Option<String>,
    /// Cloud artwork token from `boma 0x0036` plist.
    pub cloud_artwork_token: Option<String>,
    /// Cloud lyrics token from `boma 0x0036` plist.
    pub cloud_lyrics_token: Option<String>,
    /// Content advisory rating (`itma` offset 59).
    pub content_advisory: ContentRating,
    /// Media kind (`itma` offset 108..112).
    pub media_type: MediaKind,
    /// Custom playback start offset in milliseconds (`itma` offset 148..152).
    pub playback_start_ms: u32,
    /// Custom playback stop offset in milliseconds (`itma` offset 152..156).
    pub playback_stop_ms: u32,
    /// User-visible play count (`TrackPlaysSkips` offset 12..16).
    pub play_count: Option<u32>,
    /// Cumulative play count unaffected by GUI resets (`TrackPlaysSkips` offset 16..20).
    pub true_play_count: Option<u32>,
    /// User-visible skip count (`TrackPlaysSkips` offset 32..36).
    pub skip_count: Option<u32>,
    /// Cumulative skip count (`TrackPlaysSkips` offset 36..40).
    pub true_skip_count: Option<u32>,
    /// Date added to library (`TrackNumerics` offset 92..96).
    pub date_added: Option<DateTime<Utc>>,
    /// Date modified (`TrackNumerics` offset 128..132).
    pub date_modified: Option<DateTime<Utc>>,
    /// Date last played (`TrackPlaysSkips` offset 8..12).
    pub date_last_played: Option<DateTime<Utc>>,
    /// Date first played in UTC (`TrackPlaysSkips` offset 20..24).
    pub date_first_played_utc: Option<DateTime<Utc>>,
    /// Date last skipped (`TrackPlaysSkips` offset 28..32).
    pub date_last_skipped: Option<DateTime<Utc>>,
    /// Release date (`TrackNumerics` offset 140..144).
    pub date_released: Option<DateTime<Utc>>,
    /// Purchase date (`TrackNumerics` offset 136..140).
    pub date_purchased: Option<DateTime<Utc>>,
    /// Cloud sync timestamp (`TrackNumerics` offset 356..360).
    pub date_cloud_synced: Option<DateTime<Utc>>,
    /// Local relative or `file://` URL (`boma 0x000B`).
    pub file_url: Option<String>,
    /// Legacy colon/slash file path (`boma 0x0043`).
    pub legacy_file_path: Option<String>,
    /// Extracted path from macOS `book` bookmark (`boma 0x0042`).
    pub book_path: Option<String>,
    /// Foreign key to [`Album::persistent_id`] (`itma` offset 172..180).
    pub album_persistent_id: Option<PersistentId>,
    /// Foreign key to [`Artist::persistent_id`] (`itma` offset 180..188).
    pub artist_persistent_id: Option<PersistentId>,
    /// Artwork UUID in `artwork.sqlite` (`itma` offset 256..272).
    pub artwork_uuid: Option<String>,
    /// Number of attached artworks (`TrackNumerics` offset 76..78).
    pub artwork_count: Option<u16>,
    /// Whether embedded artwork was scanned in the local audio file (`TrackNumerics` offset 80..82 == `-1`).
    pub has_embedded_artwork: Option<bool>,
    /// Total byte size of attached artworks (`TrackNumerics` offset 84..88).
    pub artwork_total_size_bytes: Option<u32>,
    /// File folder count (`TrackNumerics` offset 72..74).
    pub file_folder_count: Option<u16>,
    /// Library folder count (`TrackNumerics` offset 74..76).
    pub library_folder_count: Option<u16>,
    /// International Standard Recording Code (`boma 0x002B`).
    pub isrc: Option<String>,
    /// Copyright notice (`boma 0x002E`).
    pub copyright: Option<String>,
    /// Additional string `boma` subtypes preserved by key `"0xXXXX_name"`.
    pub extra_strings: BTreeMap<String, String>,
}

impl Track {
    /// Returns the star rating on a `0..=5` star scale (`star_rating / 20`).
    #[must_use]
    pub const fn stars(&self) -> u8 {
        self.star_rating / 20
    }
}

/// Decoded Playlist record (`lpma` + `boma` children + sidecar `Playlist_<ID>.plist`).
#[non_exhaustive]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Playlists have 4 orthogonal boolean flags (is_master, is_folder, is_smart, show_in_profile_and_search)"
)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Playlist {
    /// Playlist Persistent ID (`lpma` offset 30..38).
    pub persistent_id: PersistentId,
    /// Parent Folder Playlist Persistent ID (`lpma` offset 50..58).
    pub parent_persistent_id: Option<PersistentId>,
    /// Playlist title (`boma 0x00C8`).
    pub name: Option<String>,
    /// Playlist description from `boma 0x00CD` plist.
    pub description: Option<String>,
    /// Semantic playlist kind (`Master`, `Distinguished`, `Folder`, `Smart`, `SubscribedCatalog`, `Standard`).
    pub kind: PlaylistKind,
    /// Whether this is the internal Master playlist (`####!####`, `lpma` offset 44).
    pub is_master: bool,
    /// Whether this playlist is a Folder (`lpma` offset 49).
    pub is_folder: bool,
    /// Whether this playlist is a user/system Smart Playlist (`boma 0x00C9` / `0x00CA`).
    pub is_smart: bool,
    /// "Show in My Profile and in Search" checkbox (`lpma` offset 262).
    pub show_in_profile_and_search: bool,
    /// Distinguished system playlist kind (`lpma` offset 79..81).
    pub system_kind: DistinguishedPlaylistKind,
    /// User suggestion / favorite flag (`lpma` offset 223..225).
    pub suggestion_flag: SuggestionFlag,
    /// Creation timestamp (`lpma` offset 22..26).
    pub date_created: Option<DateTime<Utc>>,
    /// Modification timestamp (`lpma` offset 138..142).
    pub date_modified: Option<DateTime<Utc>>,
    /// Last played timestamp (`lpma` offset 182..186).
    pub date_last_played: Option<DateTime<Utc>>,
    /// Cloud publish / artwork generation timestamp (`lpma` offset 304..308).
    pub date_cloud_published: Option<DateTime<Utc>>,
    /// Cloud sync timestamp (`lpma` offset 324..328 / 356..360).
    pub date_cloud_synced: Option<DateTime<Utc>>,
    /// User-visible play count (`lpma` offset 174..178).
    pub play_count: u32,
    /// Cumulative play count (`lpma` offset 178..182).
    pub true_play_count: u32,
    /// Cloud refresh interval in seconds (`lpma` offset 256..260).
    pub refresh_interval_seconds: u32,
    /// Cloud share / visibility mode (`lpma` offset 308..312).
    pub share_mode: PlaylistShareMode,
    /// Cover artwork template kind (`lpma` offset 300..304).
    pub artwork_template_kind: u32,
    /// iCloud Music Library playlist `sagaId` (`lpma` offset 70..78).
    pub cloud_saga_id: Option<u64>,
    /// Universal library playlist ID (`pl.u-...`) from `boma 0x00CD`.
    pub universal_library_id: Option<String>,
    /// Cloud playlist UUID from `boma 0x00CD`.
    pub cloud_uuid: Option<String>,
    /// Subscribed container URL from `boma 0x00CD`.
    pub subscribed_container_url: Option<String>,
    /// Curator / owner display name from `boma 0x00CD`.
    pub owner_name: Option<String>,
    /// Curator / owner identifier from `boma 0x00CD`.
    pub owner_id: Option<String>,
    /// Foreign key to [`SocialProfile::persistent_id`] (`lpma` offset 340..348).
    pub author_social_profile_persistent_id: Option<PersistentId>,
    /// Cloud artwork URL from `boma 0x00CD`.
    pub cloud_artwork_url: Option<String>,
    /// Cloud artwork token from `boma 0x00CD`.
    pub cloud_artwork_token: Option<String>,
    /// Artwork UUID in `artwork.sqlite` (`lpma` offset 263..279).
    pub artwork_uuid: Option<String>,
    /// Declared track count in `lpma` header (`lpma` offset 16..20).
    pub declared_track_count: u32,
    /// Ordered list of track [`PersistentId`]s (`ipfa` / `IPfa` items).
    pub track_persistent_ids: Vec<PersistentId>,
    /// Direct child playlist [`PersistentId`]s when this playlist is a folder.
    pub child_playlist_persistent_ids: Vec<PersistentId>,
    /// Decoded Smart Playlist configuration (`boma 0x00CA` + `0x00C9` `SLst`).
    pub smart_playlist: Option<SmartPlaylistConfig>,
    /// View and sorting preferences loaded from `preferences/Playlist_<ID>.plist`.
    pub view_preferences: Option<PlaylistViewPreferences>,
}

/// Discovers the default Apple Music `.musiclibrary` bundle path in the user's Music directory if present.
#[must_use]
pub fn discover_default_library_path() -> Option<PathBuf> {
    let music_dir = default_user_music_dir()?;

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

fn default_user_music_dir() -> Option<PathBuf> {
    #[cfg(feature = "cli")]
    if let Some(user_dirs) = directories::UserDirs::new() {
        return Some(
            user_dirs
                .audio_dir()
                .map_or_else(|| user_dirs.home_dir().join("Music"), Path::to_path_buf),
        );
    }
    std::env::home_dir().map(|home| home.join("Music"))
}

impl Library {
    /// Opens the user's default Apple Music `.musiclibrary` bundle on the current system.
    ///
    /// # Errors
    ///
    /// Returns [`MusicDbError::LibraryNotFound`] if no default library is found, or [`MusicDbError`] if parsing fails.
    pub fn open_default() -> Result<Self> {
        let path = discover_default_library_path().ok_or(MusicDbError::LibraryNotFound)?;
        Self::open(path)
    }

    /// Opens a `.musicdb` file or `.musiclibrary` bundle directory and converts it to a high-level [`Library`].
    ///
    /// # Errors
    ///
    /// Returns [`MusicDbError`] if reading, decrypting, or parsing the `.musicdb` file fails.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p = path.as_ref();
        let raw = RawLibraryFile::open(p)?;
        let mut lib = Self::from(&raw);

        let bundle_dir = if p.is_dir() {
            Some(p)
        } else {
            p.parent().filter(|parent| {
                parent.extension().and_then(|e| e.to_str()) == Some("musiclibrary")
                    || parent.join("Preferences.plist").exists()
                    || parent.join("preferences").is_dir()
            })
        };

        if let Some(dir) = bundle_dir {
            lib.load_sidecar_preferences(dir);
        }

        Ok(lib)
    }

    /// Rebuilds the internal $O(1)$ [`PersistentId`] lookup indexes.
    pub fn rebuild_indexes(&mut self) {
        self.track_index = self
            .tracks
            .iter()
            .enumerate()
            .map(|(idx, t)| (t.persistent_id, idx))
            .collect();
        self.album_index = self
            .albums
            .iter()
            .enumerate()
            .map(|(idx, a)| (a.persistent_id, idx))
            .collect();
        self.artist_index = self
            .artists
            .iter()
            .enumerate()
            .map(|(idx, a)| (a.persistent_id, idx))
            .collect();
        self.playlist_index = self
            .playlists
            .iter()
            .enumerate()
            .map(|(idx, p)| (p.persistent_id, idx))
            .collect();
        self.social_profile_index = self
            .social_profiles
            .iter()
            .enumerate()
            .map(|(idx, sp)| (sp.persistent_id, idx))
            .collect();
    }

    /// Loads optional sidecar `.plist` files from a `.musiclibrary` bundle directory (`Preferences.plist` and `preferences/*.plist`).
    pub fn load_sidecar_preferences<P: AsRef<Path>>(&mut self, bundle_dir: P) {
        let dir = bundle_dir.as_ref();

        let prefs_plist = dir.join("Preferences.plist");
        if prefs_plist.is_file()
            && let Ok(plist::Value::Dictionary(root)) = plist::Value::from_file(&prefs_plist)
            && let Some(copy_flag) = root
                .get("com.apple.Music")
                .and_then(plist::Value::as_dictionary)
                .and_then(|d| {
                    d.get(" CopyFilesToMediaFolder")
                        .or_else(|| d.get("CopyFilesToMediaFolder"))
                })
                .or_else(|| root.get("Copy files to Music Media folder when adding to library"))
                .and_then(plist::Value::as_boolean)
        {
            self.metadata.copy_files_to_media_folder = Some(copy_flag);
        }

        let prefs_subdir = if dir.join("preferences").is_dir() {
            dir.join("preferences")
        } else {
            dir.join("Preferences")
        };
        if !prefs_subdir.is_dir() {
            return;
        }

        let songs_plist = prefs_subdir.join("Songs.plist");
        if songs_plist.is_file() {
            self.metadata.songs_view_preferences = parse_playlist_view_plist(&songs_plist);
        }

        for pl in &mut self.playlists {
            let pid_str = pl.persistent_id.to_string();
            let candidate_upper = prefs_subdir.join(format!("Playlist_{pid_str}.plist"));
            let candidate_lower =
                prefs_subdir.join(format!("Playlist_{}.plist", pid_str.to_lowercase()));
            if candidate_upper.is_file() {
                pl.view_preferences = parse_playlist_view_plist(&candidate_upper);
            } else if candidate_lower.is_file() {
                pl.view_preferences = parse_playlist_view_plist(&candidate_lower);
            }
        }
    }

    /// Looks up a track by [`PersistentId`] (or 16-char hex string) in $O(1)$ time.
    #[must_use]
    pub fn track_by_id(&self, id: impl IntoPersistentId) -> Option<&Track> {
        let pid = id.into_persistent_id()?;
        if let Some(&idx) = self.track_index.get(&pid) {
            self.tracks.get(idx)
        } else if self.track_index.is_empty() {
            self.tracks.iter().find(|t| t.persistent_id == pid)
        } else {
            None
        }
    }

    /// Looks up an album by [`PersistentId`] (or 16-char hex string) in $O(1)$ time.
    #[must_use]
    pub fn album_by_id(&self, id: impl IntoPersistentId) -> Option<&Album> {
        let pid = id.into_persistent_id()?;
        if let Some(&idx) = self.album_index.get(&pid) {
            self.albums.get(idx)
        } else if self.album_index.is_empty() {
            self.albums.iter().find(|a| a.persistent_id == pid)
        } else {
            None
        }
    }

    /// Looks up an artist by [`PersistentId`] (or 16-char hex string) in $O(1)$ time.
    #[must_use]
    pub fn artist_by_id(&self, id: impl IntoPersistentId) -> Option<&Artist> {
        let pid = id.into_persistent_id()?;
        if let Some(&idx) = self.artist_index.get(&pid) {
            self.artists.get(idx)
        } else if self.artist_index.is_empty() {
            self.artists.iter().find(|a| a.persistent_id == pid)
        } else {
            None
        }
    }

    /// Looks up a playlist by [`PersistentId`] (or 16-char hex string) in $O(1)$ time.
    #[must_use]
    pub fn playlist_by_id(&self, id: impl IntoPersistentId) -> Option<&Playlist> {
        let pid = id.into_persistent_id()?;
        if let Some(&idx) = self.playlist_index.get(&pid) {
            self.playlists.get(idx)
        } else if self.playlist_index.is_empty() {
            self.playlists.iter().find(|p| p.persistent_id == pid)
        } else {
            None
        }
    }

    /// Looks up an Apple Music social profile by [`PersistentId`] (or 16-char hex string) in $O(1)$ time.
    #[must_use]
    pub fn social_profile_by_id(&self, id: impl IntoPersistentId) -> Option<&SocialProfile> {
        let pid = id.into_persistent_id()?;
        if let Some(&idx) = self.social_profile_index.get(&pid) {
            self.social_profiles.get(idx)
        } else if self.social_profile_index.is_empty() {
            self.social_profiles
                .iter()
                .find(|sp| sp.persistent_id == pid)
        } else {
            None
        }
    }

    /// Looks up a playlist by exact name (with case-insensitive exact and substring fallback).
    #[must_use]
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

    /// Returns an iterator over all resolved [`Track`] records in the given [`Playlist`] in playlist order.
    pub fn playlist_tracks<'a>(
        &'a self,
        playlist: &'a Playlist,
    ) -> impl Iterator<Item = &'a Track> + 'a {
        playlist
            .track_persistent_ids
            .iter()
            .filter_map(|id| self.track_by_id(*id))
    }

    /// Returns an iterator over the direct child [`Playlist`] records of a folder playlist.
    pub fn playlist_children<'a>(
        &'a self,
        folder: &'a Playlist,
    ) -> impl Iterator<Item = &'a Playlist> + 'a {
        folder
            .child_playlist_persistent_ids
            .iter()
            .filter_map(|id| self.playlist_by_id(*id))
    }

    /// Returns an iterator over all [`Track`] records belonging to an [`Album`] by [`PersistentId`], sorted by disc and track number.
    pub fn album_tracks(
        &self,
        album_id: impl IntoPersistentId,
    ) -> impl Iterator<Item = &Track> + '_ {
        let pid = album_id.into_persistent_id();
        let mut out: Vec<&Track> = self
            .tracks
            .iter()
            .filter(move |t| pid.is_some() && t.album_persistent_id == pid)
            .collect();
        out.sort_by_key(|t| (t.disc_number, t.track_number));
        out.into_iter()
    }

    /// Returns an iterator over all [`Track`] records belonging to an [`Artist`] by [`PersistentId`].
    pub fn artist_tracks(
        &self,
        artist_id: impl IntoPersistentId,
    ) -> impl Iterator<Item = &Track> + '_ {
        let pid = artist_id.into_persistent_id();
        self.tracks
            .iter()
            .filter(move |t| pid.is_some() && t.artist_persistent_id == pid)
    }

    /// Serializes the library to a pretty-printed JSON string.
    ///
    /// # Errors
    ///
    /// Returns [`MusicDbError::Json`] if JSON serialization fails.
    #[cfg(any(feature = "sqlite", feature = "cli"))]
    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

impl From<&RawLibraryFile> for Library {
    fn from(raw: &RawLibraryFile) -> Self {
        let h = &raw.outer_header;
        let mut metadata = LibraryMetadata {
            persistent_id: PersistentId::new(h.library_persistent_id),
            itunes_persistent_id: (h.itunes_library_persistent_id != 0)
                .then_some(PersistentId::new(h.itunes_library_persistent_id)),
            revision_hash: PersistentId::new(h.revision_hash),
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
                    albums.extend(lama.albums.iter().map(Album::from));
                }
                HsmaChild::ArtistList(l_ama) => {
                    artists.extend(l_ama.artists.iter().map(Artist::from));
                }
                HsmaChild::TrackList(ltma) => {
                    tracks.extend(ltma.tracks.iter().map(Track::from));
                }
                HsmaChild::PlaylistList(l_pma) => {
                    playlists.extend(l_pma.playlists.iter().map(Playlist::from));
                }
                HsmaChild::SocialProfileList(lsma) => {
                    social_profiles.extend(lsma.profiles.iter().map(SocialProfile::from));
                }
                HsmaChild::InnerHfma(_)
                | HsmaChild::Padding(_)
                | HsmaChild::Unknown { .. }
                | HsmaChild::Skipped { .. } => {}
            }
        }

        link_playlist_hierarchy_and_refs(&mut playlists);

        let mut lib = Self {
            metadata,
            albums,
            artists,
            tracks,
            playlists,
            social_profiles,
            ..Self::default()
        };
        lib.rebuild_indexes();
        lib
    }
}

fn populate_plma_metadata(plma: &LibraryMasterSection, meta: &mut LibraryMetadata) {
    meta.show_song_list_checkboxes = Some(plma.header.checkbox_show_song_list_checkboxes != 0);
    meta.keep_media_folder_organized = Some(plma.header.checkbox_keep_media_folder_organized != 0);
    for boma in &plma.bomas {
        if let Some(s) = boma.as_str() {
            match boma.subtype {
                0x01F8 => meta.media_folder_uri = Some(s.to_owned()),
                0x01FC => meta.imported_itl_path = Some(s.to_owned()),
                0x01FD | 0x0200 => meta.media_folder_path = Some(s.to_owned()),
                _ => {}
            }
        }
    }
}

fn link_playlist_hierarchy_and_refs(playlists: &mut [Playlist]) {
    let mut parent_to_children: HashMap<PersistentId, Vec<PersistentId>> = HashMap::new();
    let mut playlist_names: HashMap<PersistentId, String> = HashMap::new();
    for pl in playlists.iter() {
        if let Some(parent_id) = pl.parent_persistent_id {
            parent_to_children
                .entry(parent_id)
                .or_default()
                .push(pl.persistent_id);
        }
        if let Some(name) = &pl.name {
            playlist_names.insert(pl.persistent_id, name.clone());
        }
    }
    for pl in playlists.iter_mut() {
        if let Some(children) = parent_to_children.remove(&pl.persistent_id) {
            pl.child_playlist_persistent_ids = children;
        }
        if let Some(smart) = &mut pl.smart_playlist
            && let Some(group) = &mut smart.rule_group
        {
            resolve_playlist_refs_in_group(group, &playlist_names);
        }
    }
}

impl From<&SocialProfileItem> for SocialProfile {
    fn from(isma: &SocialProfileItem) -> Self {
        let mut profile_id = None;
        let mut name = None;
        let mut handle = None;
        let mut avatar_id = None;
        let mut avatar_url = None;

        for boma in &isma.bomas {
            if let Some(s) = boma.as_str() {
                match boma.subtype {
                    0x0320 => profile_id = Some(s.to_owned()),
                    0x0321 => name = Some(s.to_owned()),
                    0x0322 => handle = Some(s.to_owned()),
                    0x0323 => avatar_id = Some(s.to_owned()),
                    0x0324 => avatar_url = Some(s.to_owned()),
                    _ => {}
                }
            }
        }

        Self {
            persistent_id: PersistentId::new(isma.header.profile_persistent_id),
            profile_id,
            name,
            handle,
            avatar_id,
            avatar_url,
        }
    }
}

impl From<&AlbumItem> for Album {
    fn from(iama: &AlbumItem) -> Self {
        let h = &iama.header;
        let mut title = None;
        let mut artist = None;
        let mut album_artist = None;
        let mut cloud_album_id = None;
        let mut editorial_notes = None;

        for boma in &iama.bomas {
            if let Some(s) = boma.as_str() {
                match boma.subtype {
                    0x012C => title = Some(s.to_owned()),
                    0x012D => artist = Some(s.to_owned()),
                    0x012E => album_artist = Some(s.to_owned()),
                    0x0133 => cloud_album_id = Some(s.to_owned()),
                    0x0134 => editorial_notes = Some(s.to_owned()),
                    _ => {}
                }
            }
        }

        Self {
            persistent_id: PersistentId::new(h.album_persistent_id),
            itunes_persistent_id: (h.album_persistent_id_2 != 0)
                .then_some(PersistentId::new(h.album_persistent_id_2)),
            first_track_persistent_id: (h.first_track_persistent_id != 0)
                .then_some(PersistentId::new(h.first_track_persistent_id)),
            title,
            artist,
            album_artist,
            cloud_album_id,
            editorial_notes,
            is_compilation: h.is_compilation != 0,
            star_rating: h.star_rating,
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

impl From<&ArtistItem> for Artist {
    fn from(i_ama: &ArtistItem) -> Self {
        let h = &i_ama.header;
        let mut name = None;
        let mut sort_name = None;
        let mut cloud_artist_id = None;
        let mut artwork_url_plist = None;

        for boma in &i_ama.bomas {
            if let Some(s) = boma.as_str() {
                match boma.subtype {
                    0x0190 | 0x0004 => name = Some(s.to_owned()),
                    0x0191 => sort_name = Some(s.to_owned()),
                    0x0192 => artwork_url_plist = Some(s.to_owned()),
                    0x0193 => cloud_artist_id = Some(s.to_owned()),
                    _ => {}
                }
            }
        }

        Self {
            persistent_id: PersistentId::new(h.artist_persistent_id),
            itunes_persistent_id: (h.artist_persistent_id_2 != 0)
                .then_some(PersistentId::new(h.artist_persistent_id_2)),
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

impl From<&TrackItem> for Track {
    fn from(itma: &TrackItem) -> Self {
        let h = &itma.header;
        let cloud_saga_id = (h.cloud_saga_id != 0 && h.cloud_saga_id != 0xFFFF_FFFF_FFFF_FFFD)
            .then_some(h.cloud_saga_id);

        let mut track = Self {
            persistent_id: PersistentId::new(h.track_persistent_id),
            movement_number: h.movement_number,
            movement_total: h.movement_total,
            sort_modes: TrackSortModes::from(h.auto_sort_flags),
            track_number: h.track_number,
            track_total: h.track_total,
            disc_number: h.disc_number,
            disc_total: h.disc_total,
            year: h.year,
            bpm: h.bpm,
            volume_adjustment: h.volume_adjustment,
            star_rating: h.star_rating,
            suggestion_flag: SuggestionFlag::from(h.suggestion_flag),
            disabled: h.checkbox_disabled != 0,
            skip_when_shuffling: h.checkbox_skip_when_shuffling != 0,
            remember_playback_position: h.checkbox_remember_playback_position != 0,
            show_composer_in_all_views: h.checkbox_show_composer_in_all_views != 0,
            use_work_and_movement: h.checkbox_use_work_and_movement != 0,
            album_is_compilation: h.checkbox_album_is_compilation != 0,
            part_of_gapless_album: h.checkbox_part_of_gapless_album != 0,
            playlist_only: h.playlist_only != 0,
            purchased: h.cloud_sync_state == 3 || *h.unk_96_purchased_album_group != 0,
            has_custom_lyrics: h.has_custom_lyrics != 0 || h.has_local_custom_lyrics != 0,
            has_local_custom_lyrics: h.has_local_custom_lyrics != 0,
            has_unsynced_local_changes: h.has_unsynced_local_changes != 0,
            cloud_saga_id,
            apple_music_store_artist_id: (h.apple_music_store_artist_id != 0)
                .then_some(u64::from(h.apple_music_store_artist_id)),
            downloaded_state: DownloadedStatus::from(h.downloaded_status),
            cloud_status: CloudStatus::from(h.cloud_status),
            content_advisory: ContentRating::from(h.content_rating),
            media_type: MediaKind::from(h.media_kind),
            playback_start_ms: h.playback_start_ms,
            playback_stop_ms: h.playback_stop_ms,
            album_persistent_id: (h.album_persistent_id != 0)
                .then_some(PersistentId::new(h.album_persistent_id)),
            artist_persistent_id: (h.artist_persistent_id != 0)
                .then_some(PersistentId::new(h.artist_persistent_id)),
            artwork_uuid: format_artwork_uuid(&h.artwork_uuid),
            ..Self::default()
        };

        for boma in &itma.bomas {
            apply_track_boma(&mut track, boma);
        }

        track
    }
}

fn apply_track_boma(track: &mut Track, boma: &BomaSection) {
    match &boma.payload {
        BomaPayload::TrackNumerics(num) => {
            if track.duration_ms.is_none() {
                apply_primary_track_numerics(track, num);
            } else {
                apply_secondary_track_numerics(track, num);
            }
        }
        BomaPayload::TrackPlaysSkips(ps) => {
            if track.play_count.is_none() {
                track.play_count = Some(ps.play_count);
                track.true_play_count = Some(ps.true_play_count);
                track.skip_count = Some(ps.skip_count);
                track.true_skip_count = Some(ps.true_skip_count);
                track.date_last_played = apple_timestamp_to_utc(ps.date_last_played);
                track.date_first_played_utc = apple_timestamp_to_utc(ps.date_first_played_utc);
                track.date_last_skipped = apple_timestamp_to_utc(ps.date_last_skipped);
            }
        }
        BomaPayload::Book { extracted_path, .. } => {
            if track.book_path.is_none() {
                track.book_path.clone_from(extracted_path);
            }
        }
        _ => {
            if let Some(s) = boma.as_str() {
                apply_track_string_boma(track, boma, s.to_owned());
            }
        }
    }
}

fn apply_primary_track_numerics(track: &mut Track, num: &TrackNumerics) {
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
    track.gapless_encoder_padding =
        (num.gapless_encoder_padding != 0).then_some(num.gapless_encoder_padding);
    track.gapless_total_samples =
        (num.gapless_total_samples != 0).then_some(num.gapless_total_samples);
    track.gapless_last_eight_frames_offset =
        (num.gapless_last_eight_frames_offset != 0).then_some(num.gapless_last_eight_frames_offset);
    track.store_genre_id = (num.store_genre_id != 0).then_some(num.store_genre_id);
    track.custom_lyrics_hash = (num.custom_lyrics_hash != 0).then_some(num.custom_lyrics_hash);
    track.date_added = apple_timestamp_to_utc(num.date_added);
    track.date_modified = apple_timestamp_to_utc(num.date_modified);
    track.date_purchased = apple_timestamp_to_utc(num.date_purchased);
    if num.date_purchased != 0
        || (num.subrecord_kind == 1
            && (num.store_track_adam_id != 0 || num.storefront_id != 0)
            && num.is_itunes_matched == [0, 0]
            && num.is_apple_music_catalog == 0)
    {
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
        track.apple_music_store_artist_id = Some(num.apple_music_store_artist_id);
    }
    if num.apple_music_store_album_artist_id != 0 {
        track.apple_music_store_album_artist_id = Some(num.apple_music_store_album_artist_id);
    }
    if num.storefront_id != 0 {
        track.storefront_id = Some(num.storefront_id);
    }
    track.artwork_count = Some(num.artwork_count);
    track.has_embedded_artwork = Some(num.has_embedded_artwork == -1);
    track.artwork_total_size_bytes = Some(num.artwork_total_size_bytes);
    track.file_folder_count = Some(num.file_folder_count);
    track.library_folder_count = Some(num.library_folder_count);
}

fn apply_secondary_track_numerics(track: &mut Track, num: &TrackNumerics) {
    if track.date_cloud_synced.is_none() {
        track.date_cloud_synced = apple_timestamp_to_utc(num.date_cloud_synced);
    }
    if track.date_released.is_none() {
        track.date_released = apple_timestamp_to_utc(num.date_released);
    }
    if track.date_purchased.is_none() && num.date_purchased != 0 {
        track.date_purchased = apple_timestamp_to_utc(num.date_purchased);
        track.purchased = true;
    }
    if track.artwork_count.unwrap_or(0) == 0 && num.artwork_count != 0 {
        track.artwork_count = Some(num.artwork_count);
    }
    if track.has_embedded_artwork != Some(true) && num.has_embedded_artwork == -1 {
        track.has_embedded_artwork = Some(true);
    }
    if track.artwork_total_size_bytes.unwrap_or(0) == 0 && num.artwork_total_size_bytes != 0 {
        track.artwork_total_size_bytes = Some(num.artwork_total_size_bytes);
    }
    if track.store_track_adam_id.is_none() && num.store_track_adam_id != 0 {
        track.store_track_adam_id = Some(num.store_track_adam_id);
    }
    if track.apple_music_store_album_id.is_none() && num.apple_music_store_album_id != 0 {
        track.apple_music_store_album_id = Some(num.apple_music_store_album_id);
    }
    if (track.apple_music_store_artist_id.is_none()
        || track.apple_music_store_artist_id == Some(num.apple_music_store_artist_id & 0xFFFF_FFFF))
        && num.apple_music_store_artist_id != 0
    {
        track.apple_music_store_artist_id = Some(num.apple_music_store_artist_id);
    }
    if track.apple_music_store_album_artist_id.is_none()
        && num.apple_music_store_album_artist_id != 0
    {
        track.apple_music_store_album_artist_id = Some(num.apple_music_store_album_artist_id);
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

fn apply_track_string_boma(track: &mut Track, boma: &BomaSection, val: String) {
    let target = match boma.subtype {
        0x0002 => &mut track.title,
        0x0003 => &mut track.album,
        0x0004 => &mut track.artist,
        0x0005 => &mut track.genre,
        0x0006 => &mut track.kind,
        0x0007 => {
            if track.equalizer.is_none() {
                track.equalizer_preset = Some(EqualizerPreset::from(val.as_str()));
                track.equalizer = Some(val);
            }
            return;
        }
        0x0008 => &mut track.comments,
        0x000B => &mut track.file_url,
        0x000C => &mut track.composer,
        0x000E => &mut track.grouping,
        0x001B => &mut track.album_artist,
        0x001E => &mut track.sort_title,
        0x001F => &mut track.sort_album,
        0x0020 => &mut track.sort_artist,
        0x0021 => &mut track.sort_album_artist,
        0x0022 => &mut track.sort_composer,
        0x002B => &mut track.isrc,
        0x002E => &mut track.copyright,
        0x003B => &mut track.purchaser_email,
        0x003C => &mut track.purchaser_name,
        0x003F => &mut track.work_name,
        0x0040 => &mut track.movement_name,
        0x0043 => &mut track.legacy_file_path,
        0x0036 => {
            apply_track_cloud_plist(track, boma, val);
            return;
        }
        other => {
            track
                .extra_strings
                .entry(format!("0x{other:04X}_{}", boma.subtype_name()))
                .or_insert(val);
            return;
        }
    };
    target.get_or_insert(val);
}

fn apply_track_cloud_plist(track: &mut Track, boma: &BomaSection, val: String) {
    if let BomaPayload::RawStringUtf8 { raw_bytes, .. } = &boma.payload
        && let Ok(plist::Value::Dictionary(dict)) =
            plist::Value::from_reader(std::io::Cursor::new(raw_bytes))
    {
        if let Some(v) = dict.get("cloud-album-id").and_then(plist::Value::as_string) {
            track.cloud_album_id.get_or_insert_with(|| v.to_owned());
        }
        if let Some(v) = dict
            .get("cloud-artist-id")
            .and_then(plist::Value::as_string)
        {
            track.cloud_artist_id.get_or_insert_with(|| v.to_owned());
        }
        if let Some(v) = dict
            .get("cloud-artwork-token")
            .and_then(plist::Value::as_string)
        {
            track
                .cloud_artwork_token
                .get_or_insert_with(|| v.to_owned());
        }
        if let Some(v) = dict
            .get("cloud-lyrics")
            .or_else(|| dict.get("cloud-lyrics-token"))
            .and_then(plist::Value::as_string)
        {
            track.cloud_lyrics_token.get_or_insert_with(|| v.to_owned());
        }
    }
    track
        .extra_strings
        .entry(format!("0x{:04X}_{}", boma.subtype, boma.subtype_name()))
        .or_insert(val);
}

impl From<&PlaylistItem> for Playlist {
    fn from(lpma: &PlaylistItem) -> Self {
        let h = &lpma.header;
        let mut raw_smart_rules: Option<&SlstSection> = None;
        let mut raw_smart_options: Option<&SmartPlaylistOptions> = None;
        let mut track_persistent_ids = Vec::new();

        let is_master = h.is_master_playlist != 0;
        let is_folder = h.is_folder != 0;
        let distinguished_kind =
            u16::from_le_bytes([h.distinguished_kind_bytes[1], h.distinguished_kind_bytes[2]]);
        let system_kind = DistinguishedPlaylistKind::from(distinguished_kind);
        let share_mode = PlaylistShareMode::from(h.cloud_share_mode);

        let mut pl = Self {
            persistent_id: PersistentId::new(h.playlist_persistent_id),
            parent_persistent_id: (h.parent_persistent_id != 0)
                .then_some(PersistentId::new(h.parent_persistent_id)),
            is_master,
            is_folder,
            show_in_profile_and_search: h.checkbox_show_in_profile_and_search != 0,
            system_kind,
            suggestion_flag: SuggestionFlag::from(h.suggestion_flag),
            date_created: apple_timestamp_to_utc(h.date_created),
            date_modified: apple_timestamp_to_utc(h.date_modified),
            date_last_played: apple_timestamp_to_utc(h.date_last_played),
            date_cloud_published: apple_timestamp_to_utc(h.date_cloud_published),
            date_cloud_synced: h
                .date_suggestion_flag_modified
                .or(h.date_cloud_synced_2)
                .and_then(apple_timestamp_to_utc),
            play_count: h.play_count,
            true_play_count: h.true_play_count,
            refresh_interval_seconds: h.refresh_interval_seconds,
            share_mode,
            artwork_template_kind: h.artwork_template_kind,
            cloud_saga_id: (h.cloud_playlist_saga_id_1 != 0).then_some(h.cloud_playlist_saga_id_1),
            author_social_profile_persistent_id: h
                .author_social_profile_persistent_id
                .filter(|&id| id != 0)
                .map(PersistentId::new),
            artwork_uuid: format_artwork_uuid(&h.artwork_uuid),
            declared_track_count: h.track_count,
            ..Self::default()
        };

        for boma in &lpma.bomas {
            apply_playlist_boma(
                &mut pl,
                boma,
                &mut track_persistent_ids,
                &mut raw_smart_rules,
                &mut raw_smart_options,
            );
        }

        pl.track_persistent_ids = track_persistent_ids;
        let has_smart_blocks = raw_smart_rules.is_some() || raw_smart_options.is_some();
        pl.is_smart = !is_folder && has_smart_blocks;
        pl.smart_playlist = pl
            .is_smart
            .then(|| decode_smart_playlist(raw_smart_options, raw_smart_rules));

        pl.kind = if is_master {
            PlaylistKind::Master
        } else if is_folder {
            PlaylistKind::Folder
        } else if distinguished_kind != 0 {
            PlaylistKind::Distinguished
        } else if pl.is_smart {
            PlaylistKind::Smart
        } else if share_mode == PlaylistShareMode::SubscribedCatalog
            || pl.subscribed_container_url.is_some()
        {
            PlaylistKind::SubscribedCatalog
        } else {
            PlaylistKind::Standard
        };

        pl
    }
}

fn apply_playlist_boma<'a>(
    pl: &mut Playlist,
    boma: &'a BomaSection,
    track_persistent_ids: &mut Vec<PersistentId>,
    raw_smart_rules: &mut Option<&'a SlstSection>,
    raw_smart_options: &mut Option<&'a SmartPlaylistOptions>,
) {
    match &boma.payload {
        BomaPayload::PlaylistItem(ipfa) => {
            track_persistent_ids.push(PersistentId::new(ipfa.track_persistent_id));
        }
        BomaPayload::PlaylistItemCompact(ipfa) => {
            track_persistent_ids.push(PersistentId::new(ipfa.track_persistent_id));
        }
        BomaPayload::SmartPlaylistRules(slst) => {
            *raw_smart_rules = Some(slst.as_ref());
        }
        BomaPayload::SmartPlaylistOptions(opts) => {
            *raw_smart_options = Some(opts.as_ref());
        }
        _ => {
            if boma.subtype == 0x00C8
                && let Some(s) = boma.as_str()
            {
                pl.name = Some(s.to_owned());
            } else if boma.subtype == 0x00CD
                && let BomaPayload::RawStringUtf8 { raw_bytes, .. } = &boma.payload
                && let Ok(plist::Value::Dictionary(dict)) =
                    plist::Value::from_reader(std::io::Cursor::new(raw_bytes))
            {
                apply_playlist_cloud_dict(pl, &dict);
            }
        }
    }
}

fn apply_playlist_cloud_dict(pl: &mut Playlist, dict: &plist::Dictionary) {
    pl.universal_library_id = dict
        .get("universal-library-id")
        .and_then(plist::Value::as_string)
        .map(str::to_owned);
    pl.cloud_uuid = dict
        .get("uuid")
        .and_then(plist::Value::as_string)
        .map(str::to_owned);
    pl.subscribed_container_url = dict
        .get("subscribed-container-url")
        .and_then(plist::Value::as_string)
        .map(str::to_owned);
    pl.owner_name = dict
        .get("ownerName")
        .and_then(plist::Value::as_string)
        .map(str::to_owned);
    pl.owner_id = dict
        .get("ownerID")
        .and_then(plist::Value::as_string)
        .map(str::to_owned);
    pl.description = dict
        .get("description")
        .and_then(plist::Value::as_string)
        .map(str::to_owned);
    pl.cloud_artwork_url = dict
        .get("cloud-artwork-url")
        .and_then(plist::Value::as_string)
        .map(str::to_owned);
    pl.cloud_artwork_token = dict
        .get("cloud-artwork-token")
        .and_then(plist::Value::as_string)
        .map(str::to_owned);
}

fn decode_smart_playlist(
    opts: Option<&SmartPlaylistOptions>,
    slst: Option<&SlstSection>,
) -> SmartPlaylistConfig {
    let (live_updating, match_rules, match_only_checked_items, limit) = if let Some(o) = opts {
        let lim = (o.checkbox_enable_limit != 0).then(|| SmartPlaylistLimit {
            count: o.limit_count,
            unit: SmartLimitUnit::from(o.limit_unit),
            selection: SmartSelectionMethod::from((
                o.limit_selection_method,
                o.negate_limit_selection_method,
            )),
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
        SmartRuleChild::Arguments(args) => decode_smart_rule_arguments(rule, field, operator, args),
        SmartRuleChild::Raw(bytes) => SmartRuleValue::Raw {
            hex: bytes_to_lower_hex(bytes),
        },
        SmartRuleChild::Nested(_) => SmartRuleValue::Raw { hex: String::new() },
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

fn decode_smart_rule_arguments(
    rule: &SmartPlaylistRule,
    field: SmartField,
    operator: SmartOperator,
    args: &crate::sections::SmartPlaylistRuleArguments,
) -> SmartRuleValue {
    if matches!(
        operator,
        SmartOperator::InTheLast | SmartOperator::NotInTheLast
    ) {
        let amount = u64::try_from((-args.arg_1).max(0)).unwrap_or(0);
        let unit = match args.arg_2 {
            86_400 | 1 => RelativeTimeUnit::Days,
            604_800 | 2 => RelativeTimeUnit::Weeks,
            2_628_000 | 3 => RelativeTimeUnit::Months,
            other => RelativeTimeUnit::Unknown(other),
        };
        return SmartRuleValue::RelativeTime {
            amount,
            unit,
            seconds_per_unit: args.arg_2,
        };
    }

    let is_range = matches!(operator, SmartOperator::InRange | SmartOperator::NotInRange);
    match field {
        SmartField::Compilation
        | SmartField::Checked
        | SmartField::AlbumArtwork
        | SmartField::Purchased
        | SmartField::HasVideo => SmartRuleValue::Boolean {
            value: rule.comparison_method == 0x0000_0001 && args.arg_0 != 0,
        },
        SmartField::Rating | SmartField::AlbumRating => decode_smart_rating_arg(args, is_range),
        SmartField::Playlist => SmartRuleValue::PlaylistRef {
            playlist_persistent_id: PersistentId::new(args.arg_0.cast_unsigned()),
            playlist_name: None,
        },
        SmartField::CloudStatus => SmartRuleValue::CloudStatus {
            status: CloudStatus::from(u8::try_from(args.arg_0).unwrap_or(u8::MAX)),
        },
        SmartField::Favorite | SmartField::AlbumFavorite => SmartRuleValue::SuggestionFlag {
            flag: SuggestionFlag::from(u16::try_from(args.arg_0).unwrap_or(u16::MAX)),
        },
        SmartField::MediaKind => SmartRuleValue::MediaKind {
            kind: MediaKind::from(u32::try_from(args.arg_0).unwrap_or(u32::MAX)),
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
                name: name.to_owned(),
            }
        }
        SmartField::DateAdded
        | SmartField::DateModified
        | SmartField::LastPlayed
        | SmartField::LastSkipped => decode_smart_date_arg(args, is_range),
        _ if is_range => SmartRuleValue::IntegerRange {
            min: args.arg_0,
            max: args.arg_3,
        },
        _ => SmartRuleValue::Integer { value: args.arg_0 },
    }
}

fn decode_smart_rating_arg(
    args: &crate::sections::SmartPlaylistRuleArguments,
    is_range: bool,
) -> SmartRuleValue {
    if is_range {
        SmartRuleValue::StarRatingRange {
            min_stars: u8::try_from(args.arg_0.clamp(0, 100) / 20).unwrap_or(0),
            max_stars: u8::try_from(args.arg_3.clamp(0, 100) / 20).unwrap_or(0),
            min_raw: args.arg_0,
            max_raw: args.arg_3,
        }
    } else {
        SmartRuleValue::StarRating {
            stars: u8::try_from(args.arg_0.clamp(0, 100) / 20).unwrap_or(0),
            raw_0_100: args.arg_0,
        }
    }
}

fn decode_smart_date_arg(
    args: &crate::sections::SmartPlaylistRuleArguments,
    is_range: bool,
) -> SmartRuleValue {
    if is_range {
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

fn format_smart_condition_description(rule: &SmartPlaylistRule, value: &SmartRuleValue) -> String {
    let fname = rule.field_name();
    let cmp = rule.comparison_description();
    let val_str = match value {
        SmartRuleValue::Text { text } => format!("{text:?}"),
        SmartRuleValue::Boolean { value: v } => v.to_string(),
        SmartRuleValue::Integer { value: v } => v.to_string(),
        SmartRuleValue::IntegerRange { min, max } => format!("{min} to {max}"),
        SmartRuleValue::StarRating { stars, .. } => format!("{stars} stars"),
        SmartRuleValue::StarRatingRange {
            min_stars,
            max_stars,
            ..
        } => format!("{min_stars} to {max_stars} stars"),
        SmartRuleValue::RelativeTime { amount, unit, .. } => format!("{amount} {unit}"),
        SmartRuleValue::Date {
            utc,
            apple_timestamp,
        } => utc.map_or_else(|| apple_timestamp.to_string(), |d| d.to_rfc3339()),
        SmartRuleValue::DateRange {
            start_utc,
            end_utc,
            start_apple,
            end_apple,
        } => format!(
            "{} to {}",
            start_utc.map_or_else(|| start_apple.to_string(), |d| d.to_rfc3339()),
            end_utc.map_or_else(|| end_apple.to_string(), |d| d.to_rfc3339())
        ),
        SmartRuleValue::PlaylistRef {
            playlist_persistent_id,
            playlist_name,
        } => match playlist_name {
            Some(n) => format!("{n:?} ({playlist_persistent_id})"),
            None => playlist_persistent_id.to_string(),
        },
        SmartRuleValue::CloudStatus { status } => status.to_string(),
        SmartRuleValue::SuggestionFlag { flag } => flag.to_string(),
        SmartRuleValue::MediaKind { kind } => kind.to_string(),
        SmartRuleValue::LocationKind { name, .. } => name.clone(),
        SmartRuleValue::Raw { hex } => format!("0x{hex}"),
    };
    format!("{fname} {cmp} {val_str}")
}

fn resolve_playlist_refs_in_group(
    group: &mut SmartRuleGroup,
    playlist_names: &HashMap<PersistentId, String>,
) {
    for node in &mut group.rules {
        match node {
            SmartRuleNode::Group(sub) => resolve_playlist_refs_in_group(sub, playlist_names),
            SmartRuleNode::Condition(cond) => {
                if let SmartRuleValue::PlaylistRef {
                    playlist_persistent_id,
                    playlist_name,
                } = &mut cond.value
                    && let Some(resolved) = playlist_names.get(playlist_persistent_id)
                {
                    *playlist_name = Some(resolved.clone());
                    cond.description = format!(
                        "Playlist {} {resolved:?} ({playlist_persistent_id})",
                        if cond.operator == SmartOperator::IsNot {
                            "is not"
                        } else {
                            "is"
                        },
                    );
                }
            }
        }
    }
}

fn parse_playlist_view_plist(path: &Path) -> Option<PlaylistViewPreferences> {
    let val = plist::Value::from_file(path).ok()?;
    let dict = val.as_dictionary()?;

    let mut view_as = dict
        .get("LibraryPage_ViewAs")
        .and_then(decode_view_as_value);

    let view_dict = if let Some(views) = dict
        .get("LibraryPage_Views")
        .and_then(plist::Value::as_dictionary)
    {
        if view_as.is_none() {
            view_as = views.keys().next().cloned();
        }
        view_as
            .as_deref()
            .and_then(|k| views.get(k))
            .or_else(|| views.values().next())
            .and_then(plist::Value::as_dictionary)
    } else {
        Some(dict)
    };

    let mut sort_field = None;
    let mut sort_ascending = None;
    let mut visible_columns = Vec::new();

    if let Some(vd) = view_dict {
        sort_field = vd
            .get("LibraryPage_View_SortingPrimary")
            .and_then(decode_sort_field_value);
        sort_ascending = vd
            .get("LibraryPage_View_SortingReverse")
            .and_then(plist::Value::as_boolean)
            .map(|rev| !rev);
        extract_visible_columns(vd, &mut sort_ascending, &mut visible_columns);
    }

    Some(PlaylistViewPreferences {
        view_as,
        sort_field,
        sort_ascending,
        visible_columns,
    })
}

fn decode_view_as_value(v: &plist::Value) -> Option<String> {
    if let Some(s) = v.as_string() {
        Some(s.to_owned())
    } else {
        v.as_signed_integer().map(|code| match code {
            1 => "Songs".to_owned(),
            7 => "Albums".to_owned(),
            11 => "Playlist".to_owned(),
            other => format!("ViewAs({other})"),
        })
    }
}

fn decode_sort_field_value(v: &plist::Value) -> Option<String> {
    if let Some(s) = v.as_string() {
        Some(s.to_owned())
    } else {
        v.as_signed_integer().map(|code| match code {
            0 => "PlaylistOrder".to_owned(),
            102 => "AlbumByArtist".to_owned(),
            104 => "Artist".to_owned(),
            106 => "Title".to_owned(),
            117 => "Album".to_owned(),
            1027 => "CloudOrArtist".to_owned(),
            25_001 => "Custom".to_owned(),
            other => format!("SortField({other})"),
        })
    }
}

fn extract_visible_columns(
    vd: &plist::Dictionary,
    sort_ascending: &mut Option<bool>,
    visible_columns: &mut Vec<String>,
) {
    if let Some(cols_arr) = vd
        .get("LibraryPage_View_ListColumns")
        .and_then(plist::Value::as_array)
    {
        for col in cols_arr {
            if let Some(cd) = col.as_dictionary() {
                if sort_ascending.is_none()
                    && let Some(asc) = cd
                        .get("LibraryPage_View_ListColumn_Ascending")
                        .and_then(plist::Value::as_boolean)
                {
                    *sort_ascending = Some(asc);
                }
                if let Some(kind) = cd
                    .get("LibraryPage_View_ListColumn_Kind")
                    .and_then(plist::Value::as_signed_integer)
                {
                    let col_name = match kind {
                        0 => "CheckBox".to_owned(),
                        1 => "Position".to_owned(),
                        3 => "Title".to_owned(),
                        4 => "Artist".to_owned(),
                        7 => "Album".to_owned(),
                        10 => "Genre".to_owned(),
                        11 => "Rating".to_owned(),
                        22 => "Plays".to_owned(),
                        27 => "CloudStatus".to_owned(),
                        30 => "Duration".to_owned(),
                        46 => "Favorite".to_owned(),
                        other => format!("Column({other})"),
                    };
                    visible_columns.push(col_name);
                }
            }
        }
    } else if let Some(cols_dict) = vd
        .get("LibraryPage_View_ListColumns")
        .and_then(plist::Value::as_dictionary)
    {
        for k in cols_dict.keys() {
            visible_columns.push(k.clone());
        }
    }
}

fn bytes_to_lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
