use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

use anyhow::{Result, bail};
use serde::Serialize;

use musicdb::PersistentId;
use musicdb::model::{Album, Artist, Library, Playlist, SocialProfile, Track};
use musicdb::sections::{
    BomaPayload, BomaSection, HsmaChild, RawLibraryFile, Unknown, UnknownSpan, UnknownValue,
    UnknownVisitor, VisitUnknowns,
};

#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors independent CLI flags for query-unknowns"
)]
#[derive(Debug, Clone, Default)]
pub(crate) struct QueryUnknownsOptions {
    pub(crate) field: Option<String>,
    pub(crate) entity: Option<String>,
    pub(crate) value: Option<String>,
    pub(crate) nonzero: bool,
    pub(crate) include_zero: bool,
    pub(crate) examples: Option<usize>,
    pub(crate) all: bool,
    pub(crate) tracks_limit: usize,
    pub(crate) flat: bool,
    pub(crate) list_fields: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum EntityKey {
    Library,
    Album(PersistentId),
    Artist(PersistentId),
    Track(PersistentId),
    Playlist(PersistentId),
    PlaylistTrack {
        playlist_id: PersistentId,
        track_id: PersistentId,
        item_index: usize,
    },
    SocialProfile(PersistentId),
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct QueryUnknownsReport {
    pub(crate) zero_filter_mode: String,
    pub(crate) field_filter: Option<String>,
    pub(crate) entity_filter: Option<String>,
    pub(crate) value_filter: Option<String>,
    pub(crate) results: QueryResults,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "mode")]
pub(crate) enum QueryResults {
    FieldList {
        fields: Vec<FieldListing>,
    },
    GroupedByValue {
        fields: Vec<FieldQueryGroup>,
    },
    GroupedByEntity {
        entities: Vec<EntityQueryResult>,
    },
    Flat {
        total_matches: usize,
        shown: usize,
        truncated: usize,
        matches: Vec<FlatOccurrence>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FieldListing {
    pub(crate) section: String,
    pub(crate) field_name: String,
    pub(crate) span: String,
    pub(crate) struct_aliases: Vec<String>,
    pub(crate) total_instances: usize,
    pub(crate) nonzero_instances: usize,
    pub(crate) distinct_nonzero_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FieldQueryGroup {
    pub(crate) section: String,
    pub(crate) field_name: String,
    pub(crate) offset_info: String,
    pub(crate) struct_aliases: Vec<String>,
    pub(crate) total_instances: usize,
    pub(crate) nonzero_instances: usize,
    pub(crate) matched_instances: usize,
    pub(crate) distinct_values: usize,
    pub(crate) buckets: Vec<ValueBucket>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ValueBucket {
    pub(crate) value: String,
    pub(crate) is_zero: bool,
    pub(crate) count: usize,
    pub(crate) shown: usize,
    pub(crate) examples: Vec<ContextualOccurrence>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ContextualOccurrence {
    pub(crate) subcontext: Option<String>,
    pub(crate) entity: EntityContext,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EntityQueryResult {
    pub(crate) entity: EntityContext,
    pub(crate) fields: Vec<EntityFieldHit>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EntityFieldHit {
    pub(crate) section: String,
    pub(crate) field_name: String,
    pub(crate) offset_info: String,
    pub(crate) subcontext: Option<String>,
    pub(crate) value: String,
    pub(crate) is_zero: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FlatOccurrence {
    pub(crate) section: String,
    pub(crate) field_name: String,
    pub(crate) offset_info: String,
    pub(crate) subcontext: Option<String>,
    pub(crate) value: String,
    pub(crate) is_zero: bool,
    pub(crate) entity: EntityContext,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TrackRefSummary {
    pub(crate) persistent_id: String,
    pub(crate) disc_number: u16,
    pub(crate) track_number: u16,
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) playlist_only: bool,
    pub(crate) cloud_status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "entity_type")]
pub(crate) enum EntityContext {
    Library {
        persistent_id: String,
        app_version: String,
        format_version: String,
        tracks: usize,
        albums: usize,
        artists: usize,
        playlists: usize,
    },
    Album {
        persistent_id: String,
        title: Option<String>,
        artist: Option<String>,
        album_artist: Option<String>,
        is_compilation: bool,
        playlist_only: bool,
        star_rating: u8,
        first_track_persistent_id: Option<String>,
        apple_music_store_album_id: Option<u64>,
        track_count: usize,
        sample_tracks: Vec<TrackRefSummary>,
        tracks_truncated: usize,
    },
    Track {
        persistent_id: String,
        title: Option<String>,
        artist: Option<String>,
        album: Option<String>,
        album_artist: Option<String>,
        genre: Option<String>,
        year: u32,
        track_number: u16,
        disc_number: u16,
        duration_ms: Option<u32>,
        media_type: String,
        cloud_status: String,
        downloaded_state: String,
        playlist_only: bool,
        stars: u8,
        play_count: Option<u32>,
        skip_count: Option<u32>,
    },
    Artist {
        persistent_id: String,
        name: Option<String>,
        sort_name: Option<String>,
        apple_music_store_artist_id: u64,
        album_count: usize,
        track_count: usize,
        sample_tracks: Vec<TrackRefSummary>,
        tracks_truncated: usize,
    },
    Playlist {
        persistent_id: String,
        name: Option<String>,
        kind: String,
        share_mode: String,
        is_master: bool,
        is_folder: bool,
        is_smart: bool,
        track_count: usize,
        sample_tracks: Vec<TrackRefSummary>,
        tracks_truncated: usize,
    },
    PlaylistTrack {
        playlist_id: String,
        playlist_name: Option<String>,
        item_index: usize,
        track_id: String,
        track_title: Option<String>,
        track_artist: Option<String>,
        track_album: Option<String>,
    },
    SocialProfile {
        persistent_id: String,
        profile_id: Option<String>,
        name: Option<String>,
        handle: Option<String>,
    },
}

struct AliasMapping {
    alias: &'static str,
    sections: &'static [&'static str],
}

const CANONICAL_SECTIONS: &[&str] = &[
    "hfma(outer)",
    "hfma(inner)",
    "hsma",
    "LPma",
    "plma",
    "plma::0x01F6",
    "plma::0x01FF",
    "lama",
    "iama",
    "lAma",
    "iAma",
    "ltma",
    "itma",
    "TrackNumerics",
    "TrackPlaysSkips",
    "VideoInfo",
    "lPma",
    "lpma",
    "ipfa",
    "IPfa",
    "SmartPlaylistOptions",
    "SLst",
    "SmartPlaylistRule",
    "SmartPlaylistRuleArguments",
    "Lsma",
    "isma",
    "boma::str",
    "boma",
    "plma::boma",
    "iama::boma",
    "iAma::boma",
    "itma::boma",
    "lpma::boma",
    "isma::boma",
];

const STRUCT_ALIASES: &[AliasMapping] = &[
    AliasMapping {
        alias: "HfmaHeader",
        sections: &["hfma(outer)", "hfma(inner)"],
    },
    AliasMapping {
        alias: "hfma",
        sections: &["hfma(outer)", "hfma(inner)"],
    },
    AliasMapping {
        alias: "HsmaHeader",
        sections: &["hsma"],
    },
    AliasMapping {
        alias: "PaddingSection",
        sections: &["LPma"],
    },
    AliasMapping {
        alias: "LibraryMasterHeader",
        sections: &["plma"],
    },
    AliasMapping {
        alias: "LibraryMasterSection",
        sections: &["plma"],
    },
    AliasMapping {
        alias: "PlmaSub1F6",
        sections: &["plma::0x01F6"],
    },
    AliasMapping {
        alias: "PlmaSub1FF",
        sections: &["plma::0x01FF"],
    },
    AliasMapping {
        alias: "AlbumListHeader",
        sections: &["lama"],
    },
    AliasMapping {
        alias: "AlbumItemHeader",
        sections: &["iama"],
    },
    AliasMapping {
        alias: "AlbumItem",
        sections: &["iama"],
    },
    AliasMapping {
        alias: "ArtistListHeader",
        sections: &["lAma"],
    },
    AliasMapping {
        alias: "ArtistItemHeader",
        sections: &["iAma"],
    },
    AliasMapping {
        alias: "ArtistItem",
        sections: &["iAma"],
    },
    AliasMapping {
        alias: "TrackListHeader",
        sections: &["ltma"],
    },
    AliasMapping {
        alias: "TrackItemHeader",
        sections: &["itma"],
    },
    AliasMapping {
        alias: "TrackItem",
        sections: &["itma"],
    },
    AliasMapping {
        alias: "TrackNumerics",
        sections: &["TrackNumerics"],
    },
    AliasMapping {
        alias: "TrackPlaysSkips",
        sections: &["TrackPlaysSkips"],
    },
    AliasMapping {
        alias: "VideoInfo",
        sections: &["VideoInfo"],
    },
    AliasMapping {
        alias: "PlaylistListHeader",
        sections: &["lPma"],
    },
    AliasMapping {
        alias: "PlaylistItemHeader",
        sections: &["lpma"],
    },
    AliasMapping {
        alias: "PlaylistItem",
        sections: &["lpma"],
    },
    AliasMapping {
        alias: "PlaylistTrackItem",
        sections: &["ipfa"],
    },
    AliasMapping {
        alias: "CompactPlaylistTrackItem",
        sections: &["IPfa"],
    },
    AliasMapping {
        alias: "SmartPlaylistOptions",
        sections: &["SmartPlaylistOptions"],
    },
    AliasMapping {
        alias: "SlstSection",
        sections: &["SLst"],
    },
    AliasMapping {
        alias: "SmartPlaylistRule",
        sections: &["SmartPlaylistRule"],
    },
    AliasMapping {
        alias: "SmartPlaylistRuleArguments",
        sections: &["SmartPlaylistRuleArguments"],
    },
    AliasMapping {
        alias: "SocialProfileListHeader",
        sections: &["Lsma"],
    },
    AliasMapping {
        alias: "SocialProfileItemHeader",
        sections: &["isma"],
    },
    AliasMapping {
        alias: "SocialProfileItem",
        sections: &["isma"],
    },
    AliasMapping {
        alias: "BomaStringSection",
        sections: &["boma::str"],
    },
    AliasMapping {
        alias: "BomaSection",
        sections: &[
            "boma",
            "plma::boma",
            "iama::boma",
            "iAma::boma",
            "itma::boma",
            "lpma::boma",
            "isma::boma",
        ],
    },
    AliasMapping {
        alias: "*::boma",
        sections: &[
            "boma",
            "plma::boma",
            "iama::boma",
            "iAma::boma",
            "itma::boma",
            "lpma::boma",
            "isma::boma",
        ],
    },
];

fn struct_aliases_for_section(section: &str) -> Vec<String> {
    STRUCT_ALIASES
        .iter()
        .filter(|entry| entry.alias != "*::boma" && entry.sections.contains(&section))
        .map(|entry| entry.alias.to_string())
        .collect()
}

fn try_resolve_section_or_alias(name: &str) -> Result<Option<Vec<&'static str>>> {
    if let Some(&canonical) = CANONICAL_SECTIONS.iter().find(|&&s| s == name) {
        return Ok(Some(vec![canonical]));
    }
    if let Some(entry) = STRUCT_ALIASES.iter().find(|e| e.alias == name) {
        return Ok(Some(entry.sections.to_vec()));
    }

    let ci_canon: Vec<&'static str> = CANONICAL_SECTIONS
        .iter()
        .copied()
        .filter(|s| s.eq_ignore_ascii_case(name))
        .collect();
    if !ci_canon.is_empty() {
        bail!(
            "unknown section `{name}` (canonical section names are case-sensitive; did you mean {}?)",
            ci_canon.join(" or ")
        );
    }

    let ci_aliases: Vec<&AliasMapping> = STRUCT_ALIASES
        .iter()
        .filter(|e| e.alias.eq_ignore_ascii_case(name))
        .collect();
    if ci_aliases.len() == 1 {
        if let Some(first) = ci_aliases.first() {
            return Ok(Some(first.sections.to_vec()));
        }
    } else if ci_aliases.len() > 1 {
        let names: Vec<&str> = ci_aliases.iter().map(|e| e.alias).collect();
        bail!(
            "ambiguous section alias `{name}`; did you mean {}?",
            names.join(" or ")
        );
    }

    Ok(None)
}

fn resolve_section_or_alias(name: &str) -> Result<Vec<&'static str>> {
    match try_resolve_section_or_alias(name)? {
        Some(secs) => Ok(secs),
        None => bail!(
            "unknown section or struct alias `{name}`; run with `--list-fields` to see available sections"
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BomaSubtypeFilter {
    All,
    Exact(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FieldSelector {
    AllFields,
    SectionField {
        sections: Vec<&'static str>,
        field: String,
    },
    SectionAll {
        sections: Vec<&'static str>,
    },
    AnySectionField {
        field: String,
    },
    BareField {
        field: String,
    },
    BomaPayload {
        parent_sections: Option<Vec<&'static str>>,
        subtype: BomaSubtypeFilter,
    },
}

fn section_matches_parent_boma(section: &str, parent_section: &str) -> bool {
    section
        .strip_suffix("::boma")
        .is_some_and(|p| p == parent_section)
}

impl FieldSelector {
    fn is_single_concrete_field(&self) -> bool {
        matches!(
            self,
            Self::SectionField { .. }
                | Self::BareField { .. }
                | Self::BomaPayload {
                    subtype: BomaSubtypeFilter::Exact(_),
                    ..
                }
        )
    }

    fn matches_unknown_field(&self, section: &str, field_name: &str) -> bool {
        match self {
            Self::AllFields => true,
            Self::SectionField { sections, field } => {
                sections.contains(&section) && field == field_name
            }
            Self::SectionAll { sections } => sections.contains(&section),
            Self::AnySectionField { field } | Self::BareField { field } => field == field_name,
            Self::BomaPayload { .. } => false,
        }
    }

    fn matches_unhandled_boma(&self, parent_section: &str, subtype_val: u32) -> bool {
        match self {
            Self::AllFields => true,
            Self::SectionAll { sections } => sections
                .iter()
                .any(|s| section_matches_parent_boma(s, parent_section)),
            Self::BomaPayload {
                parent_sections,
                subtype,
            } => {
                let parent_ok = parent_sections
                    .as_ref()
                    .is_none_or(|ps| ps.contains(&parent_section));
                let subtype_ok = match subtype {
                    BomaSubtypeFilter::All => true,
                    BomaSubtypeFilter::Exact(s) => *s == subtype_val,
                };
                parent_ok && subtype_ok
            }
            _ => false,
        }
    }
}

fn parse_boma_subtype(raw: &str) -> Result<BomaSubtypeFilter> {
    let trimmed = raw.trim();
    if trimmed == "*" {
        return Ok(BomaSubtypeFilter::All);
    }
    if let Some(hex_str) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        let val = u32::from_str_radix(hex_str, 16)
            .map_err(|_| anyhow::anyhow!("invalid hex boma subtype `{trimmed}`"))?;
        return Ok(BomaSubtypeFilter::Exact(val));
    }
    if trimmed.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F')) {
        let val = u32::from_str_radix(trimmed, 16)
            .map_err(|_| anyhow::anyhow!("invalid hex boma subtype `{trimmed}`"))?;
        return Ok(BomaSubtypeFilter::Exact(val));
    }
    let val = trimmed
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("invalid boma subtype `{trimmed}`"))?;
    Ok(BomaSubtypeFilter::Exact(val))
}

fn find_boma_sep(raw: &str) -> Option<(usize, usize, char)> {
    for (idx, _) in raw.match_indices("::") {
        let after = raw.get(idx + 2..)?;
        if after.len() >= 5 && after.get(..4)?.eq_ignore_ascii_case("boma") {
            let delim = after.as_bytes().get(4).copied()?;
            if delim == b'.' || delim == b':' {
                return Some((idx, idx + 7, char::from(delim)));
            }
        }
    }
    None
}

fn parse_parent_boma_selector(raw: &str) -> Result<Option<FieldSelector>> {
    let Some((pos, rest_start, delim)) = find_boma_sep(raw) else {
        return Ok(None);
    };
    let parent_raw = raw.get(..pos).unwrap_or_default();
    let sub_raw = raw.get(rest_start..).unwrap_or_default().trim();
    // Keep `parent::boma.*` routed to `SectionAll` so it covers both `unk_16_end`
    // and unhandled `boma` payloads, while `parent::boma:*` scopes to payloads.
    if sub_raw == "*" && delim == '.' {
        return Ok(None);
    }
    let Ok(subtype) = parse_boma_subtype(sub_raw) else {
        return Ok(None);
    };
    let parent_sections = if parent_raw == "*" {
        None
    } else {
        Some(resolve_section_or_alias(parent_raw)?)
    };
    Ok(Some(FieldSelector::BomaPayload {
        parent_sections,
        subtype,
    }))
}

fn parse_field_selector(raw: &str) -> Result<FieldSelector> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("field selector cannot be empty");
    }
    if let Some(rest) = trimmed
        .get(..5)
        .filter(|p| p.eq_ignore_ascii_case("boma:"))
        .and_then(|_| trimmed.get(5..))
        .filter(|rest| !rest.starts_with(':'))
    {
        return Ok(FieldSelector::BomaPayload {
            parent_sections: None,
            subtype: parse_boma_subtype(rest)?,
        });
    }
    if let Some(sel) = parse_parent_boma_selector(trimmed)? {
        return Ok(sel);
    }
    if let Some(hex_str) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        && let Ok(sub) = u32::from_str_radix(hex_str, 16)
    {
        return Ok(FieldSelector::BomaPayload {
            parent_sections: None,
            subtype: BomaSubtypeFilter::Exact(sub),
        });
    }
    if let Some((sec_part, field_part)) = trimmed.rsplit_once('.') {
        if field_part.is_empty() {
            bail!("field selector `{trimmed}` is missing a field name after `.`");
        }
        if sec_part == "*" {
            return Ok(if field_part == "*" {
                FieldSelector::AllFields
            } else {
                FieldSelector::AnySectionField {
                    field: field_part.to_string(),
                }
            });
        }
        let sections = resolve_section_or_alias(sec_part)?;
        return Ok(if field_part == "*" {
            FieldSelector::SectionAll { sections }
        } else {
            FieldSelector::SectionField {
                sections,
                field: field_part.to_string(),
            }
        });
    }
    if trimmed == "*" {
        return Ok(FieldSelector::AllFields);
    }
    if let Some(sections) = try_resolve_section_or_alias(trimmed)? {
        return Ok(FieldSelector::SectionAll { sections });
    }
    if let Some((left, right)) = trimmed.rsplit_once("::")
        && !matches!(right, "str" | "boma" | "0x01F6" | "0x01FF")
        && let Some(sections) = try_resolve_section_or_alias(left)?
    {
        return Ok(if right == "*" {
            FieldSelector::SectionAll { sections }
        } else {
            FieldSelector::SectionField {
                sections,
                field: right.to_string(),
            }
        });
    }
    Ok(FieldSelector::BareField {
        field: trimmed.to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntityScope {
    Any,
    Library,
    Album,
    Artist,
    Track,
    Playlist,
    Profile,
}

struct EntityFilter {
    scope: EntityScope,
    include_library: bool,
    matching_albums: HashSet<PersistentId>,
    matching_artists: HashSet<PersistentId>,
    matching_tracks: HashSet<PersistentId>,
    matching_playlists: HashSet<PersistentId>,
    matching_profiles: HashSet<PersistentId>,
}

fn contains_ci(opt: Option<&str>, needle_lower: &str) -> bool {
    !needle_lower.is_empty() && opt.is_some_and(|s| s.to_lowercase().contains(needle_lower))
}

impl EntityFilter {
    fn from_query(raw: &str, lib: &Library) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("entity filter cannot be empty");
        }
        let (scope, query) = if trimmed.eq_ignore_ascii_case("library") {
            (EntityScope::Library, "")
        } else if let Some((prefix, rest)) = trimmed.split_once(':') {
            match prefix.to_ascii_lowercase().as_str() {
                "album" => (EntityScope::Album, rest),
                "artist" => (EntityScope::Artist, rest),
                "track" => (EntityScope::Track, rest),
                "playlist" => (EntityScope::Playlist, rest),
                "profile" => (EntityScope::Profile, rest),
                "library" => (EntityScope::Library, rest),
                _ => (EntityScope::Any, trimmed),
            }
        } else {
            (EntityScope::Any, trimmed)
        };

        let query_trimmed = query.trim();
        if scope != EntityScope::Library && query_trimmed.is_empty() {
            bail!("entity filter `{trimmed}` is missing a search query after `:`");
        }
        let pid = PersistentId::from_str(query_trimmed).ok();
        let needle_lower = query_trimmed.to_lowercase();

        let include_library = match scope {
            EntityScope::Library => {
                needle_lower.is_empty()
                    || needle_lower == "library"
                    || pid == Some(lib.metadata.persistent_id)
            }
            EntityScope::Any => {
                needle_lower == "library" || pid == Some(lib.metadata.persistent_id)
            }
            _ => false,
        };

        let mut matching_albums = HashSet::new();
        if matches!(scope, EntityScope::Any | EntityScope::Album) {
            matching_albums = Self::collect_albums(lib, pid, &needle_lower);
        }

        let mut matching_artists = HashSet::new();
        if matches!(scope, EntityScope::Any | EntityScope::Artist) {
            matching_artists = Self::collect_artists(lib, pid, &needle_lower);
        }

        let mut matching_tracks = HashSet::new();
        if matches!(scope, EntityScope::Any | EntityScope::Track) {
            matching_tracks = Self::collect_tracks(lib, pid, &needle_lower);
        }

        let mut matching_playlists = HashSet::new();
        if matches!(scope, EntityScope::Any | EntityScope::Playlist) {
            matching_playlists = Self::collect_playlists(lib, pid, &needle_lower);
        }

        let mut matching_profiles = HashSet::new();
        if matches!(scope, EntityScope::Any | EntityScope::Profile) {
            matching_profiles = Self::collect_profiles(lib, pid, &needle_lower);
        }

        Ok(Self {
            scope,
            include_library,
            matching_albums,
            matching_artists,
            matching_tracks,
            matching_playlists,
            matching_profiles,
        })
    }

    fn collect_albums(
        lib: &Library,
        pid: Option<PersistentId>,
        needle_lower: &str,
    ) -> HashSet<PersistentId> {
        lib.albums
            .iter()
            .filter(|a| {
                pid == Some(a.persistent_id)
                    || contains_ci(a.title.as_deref(), needle_lower)
                    || contains_ci(a.artist.as_deref(), needle_lower)
                    || contains_ci(a.album_artist.as_deref(), needle_lower)
            })
            .map(|a| a.persistent_id)
            .collect()
    }

    fn collect_artists(
        lib: &Library,
        pid: Option<PersistentId>,
        needle_lower: &str,
    ) -> HashSet<PersistentId> {
        lib.artists
            .iter()
            .filter(|a| {
                pid == Some(a.persistent_id)
                    || contains_ci(a.name.as_deref(), needle_lower)
                    || contains_ci(a.sort_name.as_deref(), needle_lower)
            })
            .map(|a| a.persistent_id)
            .collect()
    }

    fn collect_tracks(
        lib: &Library,
        pid: Option<PersistentId>,
        needle_lower: &str,
    ) -> HashSet<PersistentId> {
        lib.tracks
            .iter()
            .filter(|t| {
                pid == Some(t.persistent_id)
                    || contains_ci(t.title.as_deref(), needle_lower)
                    || contains_ci(t.artist.as_deref(), needle_lower)
                    || contains_ci(t.album.as_deref(), needle_lower)
            })
            .map(|t| t.persistent_id)
            .collect()
    }

    fn collect_playlists(
        lib: &Library,
        pid: Option<PersistentId>,
        needle_lower: &str,
    ) -> HashSet<PersistentId> {
        lib.playlists
            .iter()
            .filter(|p| {
                pid == Some(p.persistent_id) || contains_ci(p.name.as_deref(), needle_lower)
            })
            .map(|p| p.persistent_id)
            .collect()
    }

    fn collect_profiles(
        lib: &Library,
        pid: Option<PersistentId>,
        needle_lower: &str,
    ) -> HashSet<PersistentId> {
        lib.social_profiles
            .iter()
            .filter(|sp| {
                pid == Some(sp.persistent_id)
                    || contains_ci(sp.name.as_deref(), needle_lower)
                    || contains_ci(sp.handle.as_deref(), needle_lower)
                    || contains_ci(sp.profile_id.as_deref(), needle_lower)
            })
            .map(|sp| sp.persistent_id)
            .collect()
    }

    fn matches_key(&self, key: EntityKey) -> bool {
        match key {
            EntityKey::Library => self.include_library,
            EntityKey::Album(id) => self.matching_albums.contains(&id),
            EntityKey::Artist(id) => self.matching_artists.contains(&id),
            EntityKey::Track(id) => self.matching_tracks.contains(&id),
            EntityKey::Playlist(id) => self.matching_playlists.contains(&id),
            EntityKey::PlaylistTrack {
                playlist_id,
                track_id,
                ..
            } => {
                (matches!(self.scope, EntityScope::Any | EntityScope::Playlist)
                    && self.matching_playlists.contains(&playlist_id))
                    || (matches!(self.scope, EntityScope::Any | EntityScope::Track)
                        && self.matching_tracks.contains(&track_id))
            }
            EntityKey::SocialProfile(id) => self.matching_profiles.contains(&id),
        }
    }

    fn matches_playlist_section(&self, playlist_id: PersistentId, bomas: &[BomaSection]) -> bool {
        if self.matching_playlists.contains(&playlist_id) {
            return true;
        }
        matches!(self.scope, EntityScope::Any | EntityScope::Track)
            && !self.matching_tracks.is_empty()
            && bomas.iter().any(|b| match &b.payload {
                BomaPayload::PlaylistItem(ipfa) => self
                    .matching_tracks
                    .contains(&PersistentId(ipfa.track_persistent_id)),
                BomaPayload::PlaylistItemCompact(ipfa) => self
                    .matching_tracks
                    .contains(&PersistentId(ipfa.track_persistent_id)),
                _ => false,
            })
    }
}

fn parse_formatted_int(formatted: &str) -> (Option<i128>, Option<u128>) {
    if let Some((dec_part, rest)) = formatted.split_once(" (0x") {
        let dec = dec_part.trim().parse::<i128>().ok();
        let hex = rest
            .strip_suffix(')')
            .and_then(|h| u128::from_str_radix(h, 16).ok());
        return (dec, hex);
    }
    if let Some(hex_part) = formatted
        .strip_prefix("0x")
        .or_else(|| formatted.strip_prefix("0X"))
    {
        let val = u128::from_str_radix(hex_part, 16).ok();
        let dec = val.and_then(|v| i128::try_from(v).ok());
        return (dec, val);
    }
    (None, None)
}

fn matches_value_filter(formatted: &str, filter: &str) -> bool {
    let trimmed = filter.trim();
    if formatted.eq_ignore_ascii_case(trimmed) {
        return true;
    }

    let (fmt_dec, fmt_hex) = parse_formatted_int(formatted);
    if fmt_dec.is_some() || fmt_hex.is_some() {
        if let Some(hex_str) = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
        {
            if let Ok(target) = u128::from_str_radix(hex_str, 16) {
                if fmt_hex == Some(target) {
                    return true;
                }
                if let Some(d) = fmt_dec
                    && d >= 0
                    && u128::try_from(d).ok() == Some(target)
                {
                    return true;
                }
            }
        } else if let Ok(target) = trimmed.parse::<i128>() {
            if fmt_dec == Some(target) {
                return true;
            }
            if target >= 0
                && let Some(h) = fmt_hex
                && u128::try_from(target).ok() == Some(h)
            {
                return true;
            }
        }
        return false;
    }

    if formatted.bytes().all(|b| b.is_ascii_hexdigit()) {
        let stripped = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
            .unwrap_or(trimmed);
        let norm_filter: String = stripped
            .chars()
            .filter(|c| !c.is_ascii_whitespace() && *c != ':' && *c != '-')
            .collect::<String>()
            .to_ascii_lowercase();
        if !norm_filter.is_empty()
            && norm_filter.bytes().all(|b| b.is_ascii_hexdigit())
            && norm_filter == formatted.to_ascii_lowercase()
        {
            return true;
        }
    }

    false
}

struct LibraryContextBuilder<'a> {
    lib: &'a Library,
    album_tracks: HashMap<PersistentId, Vec<&'a Track>>,
    artist_tracks: HashMap<PersistentId, Vec<&'a Track>>,
    artist_album_counts: HashMap<PersistentId, usize>,
    album_playlist_only: HashMap<PersistentId, bool>,
    tracks_limit: Option<usize>,
    cache: HashMap<EntityKey, EntityContext>,
}

impl<'a> LibraryContextBuilder<'a> {
    fn new(raw: &'a RawLibraryFile, lib: &'a Library, tracks_limit: Option<usize>) -> Self {
        let mut album_tracks: HashMap<PersistentId, Vec<&'a Track>> = HashMap::new();
        let mut artist_tracks: HashMap<PersistentId, Vec<&'a Track>> = HashMap::new();
        let mut artist_albums: HashMap<PersistentId, HashSet<PersistentId>> = HashMap::new();

        for track in &lib.tracks {
            if let Some(album_id) = track.album_persistent_id {
                album_tracks.entry(album_id).or_default().push(track);
            }
            if let Some(artist_id) = track.artist_persistent_id {
                artist_tracks.entry(artist_id).or_default().push(track);
                if let Some(album_id) = track.album_persistent_id {
                    artist_albums.entry(artist_id).or_default().insert(album_id);
                }
            }
        }
        for tracks in album_tracks.values_mut() {
            tracks.sort_by_key(|t| (t.disc_number, t.track_number));
        }
        let artist_album_counts = artist_albums
            .into_iter()
            .map(|(k, v)| (k, v.len()))
            .collect();

        let mut album_playlist_only = HashMap::new();
        for hsma in &raw.sections {
            if let HsmaChild::AlbumList(lama) = &hsma.child {
                for album in &lama.albums {
                    album_playlist_only.insert(
                        PersistentId(album.header.album_persistent_id),
                        album.header.playlist_only != 0,
                    );
                }
            }
        }

        Self {
            lib,
            album_tracks,
            artist_tracks,
            artist_album_counts,
            album_playlist_only,
            tracks_limit,
            cache: HashMap::new(),
        }
    }

    fn track_summary(t: &Track) -> TrackRefSummary {
        TrackRefSummary {
            persistent_id: t.persistent_id.to_string(),
            disc_number: t.disc_number,
            track_number: t.track_number,
            title: t.title.clone(),
            artist: t.artist.clone(),
            playlist_only: t.playlist_only,
            cloud_status: t.cloud_status.to_string(),
        }
    }

    fn cap_tracks(&self, tracks: &[&Track]) -> (Vec<TrackRefSummary>, usize) {
        let total = tracks.len();
        let limit = self.tracks_limit.unwrap_or(total);
        let sample = tracks
            .iter()
            .take(limit)
            .map(|t| Self::track_summary(t))
            .collect();
        let truncated = total.saturating_sub(limit);
        (sample, truncated)
    }

    fn resolve(&mut self, key: EntityKey) -> EntityContext {
        if let Some(existing) = self.cache.get(&key) {
            return existing.clone();
        }
        let ctx = match key {
            EntityKey::Library => EntityContext::Library {
                persistent_id: self.lib.metadata.persistent_id.to_string(),
                app_version: self.lib.metadata.app_version.clone(),
                format_version: format!(
                    "{}.{}",
                    self.lib.metadata.format_major_version, self.lib.metadata.format_minor_version
                ),
                tracks: self.lib.tracks.len(),
                albums: self.lib.albums.len(),
                artists: self.lib.artists.len(),
                playlists: self.lib.playlists.len(),
            },
            EntityKey::Album(id) => self.resolve_album(id),
            EntityKey::Artist(id) => self.resolve_artist(id),
            EntityKey::Track(id) => self.resolve_track(id),
            EntityKey::Playlist(id) => self.resolve_playlist(id),
            EntityKey::PlaylistTrack {
                playlist_id,
                track_id,
                item_index,
            } => self.resolve_playlist_track(playlist_id, track_id, item_index),
            EntityKey::SocialProfile(id) => self.resolve_social_profile(id),
        };
        self.cache.insert(key, ctx.clone());
        ctx
    }

    fn resolve_album(&self, id: PersistentId) -> EntityContext {
        let album: Option<&Album> = self.lib.album_by_id(id);
        let tracks = self.album_tracks.get(&id).map_or(&[][..], Vec::as_slice);
        let (sample_tracks, tracks_truncated) = self.cap_tracks(tracks);
        EntityContext::Album {
            persistent_id: id.to_string(),
            title: album.and_then(|a| a.title.clone()),
            artist: album.and_then(|a| a.artist.clone()),
            album_artist: album.and_then(|a| a.album_artist.clone()),
            is_compilation: album.is_some_and(|a| a.is_compilation),
            playlist_only: self.album_playlist_only.get(&id).copied().unwrap_or(false),
            star_rating: album.map_or(0, |a| a.star_rating),
            first_track_persistent_id: album
                .and_then(|a| a.first_track_persistent_id)
                .map(|p| p.to_string()),
            apple_music_store_album_id: album.and_then(|a| a.apple_music_store_album_id),
            track_count: tracks.len(),
            sample_tracks,
            tracks_truncated,
        }
    }

    fn resolve_artist(&self, id: PersistentId) -> EntityContext {
        let artist: Option<&Artist> = self.lib.artist_by_id(id);
        let tracks = self.artist_tracks.get(&id).map_or(&[][..], Vec::as_slice);
        let (sample_tracks, tracks_truncated) = self.cap_tracks(tracks);
        EntityContext::Artist {
            persistent_id: id.to_string(),
            name: artist.and_then(|a| a.name.clone()),
            sort_name: artist.and_then(|a| a.sort_name.clone()),
            apple_music_store_artist_id: artist.map_or(0, |a| a.apple_music_store_artist_id),
            album_count: self.artist_album_counts.get(&id).copied().unwrap_or(0),
            track_count: tracks.len(),
            sample_tracks,
            tracks_truncated,
        }
    }

    fn resolve_track(&self, id: PersistentId) -> EntityContext {
        let track: Option<&Track> = self.lib.track_by_id(id);
        EntityContext::Track {
            persistent_id: id.to_string(),
            title: track.and_then(|t| t.title.clone()),
            artist: track.and_then(|t| t.artist.clone()),
            album: track.and_then(|t| t.album.clone()),
            album_artist: track.and_then(|t| t.album_artist.clone()),
            genre: track.and_then(|t| t.genre.clone()),
            year: track.map_or(0, |t| t.year),
            track_number: track.map_or(0, |t| t.track_number),
            disc_number: track.map_or(0, |t| t.disc_number),
            duration_ms: track.and_then(|t| t.duration_ms),
            media_type: track.map_or_else(String::new, |t| t.media_type.to_string()),
            cloud_status: track.map_or_else(String::new, |t| t.cloud_status.to_string()),
            downloaded_state: track.map_or_else(String::new, |t| t.downloaded_state.to_string()),
            playlist_only: track.is_some_and(|t| t.playlist_only),
            stars: track.map_or(0, Track::stars),
            play_count: track.and_then(|t| t.play_count),
            skip_count: track.and_then(|t| t.skip_count),
        }
    }

    fn resolve_playlist(&self, id: PersistentId) -> EntityContext {
        let pl: Option<&Playlist> = self.lib.playlist_by_id(id);
        let total = pl.map_or(0, |p| p.track_persistent_ids.len());
        let limit = self.tracks_limit.unwrap_or(total);
        let sample_tracks: Vec<TrackRefSummary> = pl.map_or_default(|p| {
            p.track_persistent_ids
                .iter()
                .filter_map(|&tid| self.lib.track_by_id(tid))
                .take(limit)
                .map(Self::track_summary)
                .collect()
        });
        let tracks_truncated = total.saturating_sub(sample_tracks.len());
        EntityContext::Playlist {
            persistent_id: id.to_string(),
            name: pl.and_then(|p| p.name.clone()),
            kind: pl.map_or_else(String::new, |p| p.kind.to_string()),
            share_mode: pl.map_or_else(String::new, |p| p.share_mode.to_string()),
            is_master: pl.is_some_and(|p| p.is_master),
            is_folder: pl.is_some_and(|p| p.is_folder),
            is_smart: pl.is_some_and(|p| p.is_smart),
            track_count: total,
            sample_tracks,
            tracks_truncated,
        }
    }

    fn resolve_playlist_track(
        &self,
        playlist_id: PersistentId,
        track_id: PersistentId,
        item_index: usize,
    ) -> EntityContext {
        let pl = self.lib.playlist_by_id(playlist_id);
        let tr = self.lib.track_by_id(track_id);
        EntityContext::PlaylistTrack {
            playlist_id: playlist_id.to_string(),
            playlist_name: pl.and_then(|p| p.name.clone()),
            item_index,
            track_id: track_id.to_string(),
            track_title: tr.and_then(|t| t.title.clone()),
            track_artist: tr.and_then(|t| t.artist.clone()),
            track_album: tr.and_then(|t| t.album.clone()),
        }
    }

    fn resolve_social_profile(&self, id: PersistentId) -> EntityContext {
        let sp: Option<&SocialProfile> = self.lib.social_profile_by_id(id);
        EntityContext::SocialProfile {
            persistent_id: id.to_string(),
            profile_id: sp.and_then(|s| s.profile_id.clone()),
            name: sp.and_then(|s| s.name.clone()),
            handle: sp.and_then(|s| s.handle.clone()),
        }
    }
}

#[derive(Debug, Clone)]
struct RawHit {
    section: String,
    field_name: String,
    span_start: usize,
    offset_info: String,
    entity_key: EntityKey,
    subcontext: Option<String>,
    value: String,
    is_zero: bool,
}

struct FieldStats {
    section: String,
    field_name: String,
    span_start: usize,
    span_str: String,
    min_payload_len: Option<usize>,
    max_payload_len: Option<usize>,
    total_instances: usize,
    nonzero_instances: usize,
    distinct_nonzero: HashSet<String>,
}

impl FieldStats {
    fn span_label(&self) -> String {
        match (self.min_payload_len, self.max_payload_len) {
            (Some(min), Some(max)) if min != max => format!("{min}..={max} bytes"),
            _ => self.span_str.clone(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CollectMode {
    Query,
    ListFields,
    ProbeSelector,
}

type StatsMap = BTreeMap<String, BTreeMap<String, FieldStats>>;

struct Collector<'a> {
    selector: Option<&'a FieldSelector>,
    entity_filter: Option<&'a EntityFilter>,
    value_filter: Option<&'a str>,
    exclude_zero: bool,
    mode: CollectMode,
    stats: StatsMap,
    hits: Vec<RawHit>,
}

#[derive(Clone, Copy)]
struct FieldVisitMeta<'a> {
    section: &'a str,
    field_name: &'a str,
    span: UnknownSpan,
    entity_key: EntityKey,
    subcontext: Option<&'a str>,
    is_zero: bool,
}

impl<'a> Collector<'a> {
    fn new(
        selector: Option<&'a FieldSelector>,
        entity_filter: Option<&'a EntityFilter>,
        value_filter: Option<&'a str>,
        exclude_zero: bool,
        mode: CollectMode,
    ) -> Self {
        Self {
            selector,
            entity_filter,
            value_filter,
            exclude_zero,
            mode,
            stats: BTreeMap::new(),
            hits: Vec::new(),
        }
    }

    fn record_stat<'s>(
        stats: &'s mut StatsMap,
        section: &str,
        field_name: &str,
        span_start: usize,
        make_span_str: impl FnOnce() -> String,
        payload_len: Option<usize>,
        is_zero: bool,
    ) -> Option<&'s mut FieldStats> {
        let already_exists = stats
            .get(section)
            .is_some_and(|m| m.contains_key(field_name));
        if !already_exists {
            stats.entry(section.to_string()).or_default().insert(
                field_name.to_string(),
                FieldStats {
                    section: section.to_string(),
                    field_name: field_name.to_string(),
                    span_start,
                    span_str: make_span_str(),
                    min_payload_len: payload_len,
                    max_payload_len: payload_len,
                    total_instances: 0,
                    nonzero_instances: 0,
                    distinct_nonzero: HashSet::new(),
                },
            );
        }
        let stat = stats.get_mut(section)?.get_mut(field_name)?;
        if let Some(len) = payload_len {
            stat.min_payload_len = Some(stat.min_payload_len.map_or(len, |m| m.min(len)));
            stat.max_payload_len = Some(stat.max_payload_len.map_or(len, |m| m.max(len)));
        }
        stat.total_instances += 1;
        if !is_zero {
            stat.nonzero_instances += 1;
        }
        Some(stat)
    }

    fn record_field(&mut self, meta: FieldVisitMeta<'_>, format_val: impl FnOnce() -> String) {
        let selector_matches = self
            .selector
            .is_none_or(|sel| sel.matches_unknown_field(meta.section, meta.field_name));
        if self.mode != CollectMode::ListFields && !selector_matches {
            return;
        }

        let stat = Self::record_stat(
            &mut self.stats,
            meta.section,
            meta.field_name,
            meta.span.start(),
            || meta.span.to_string(),
            None,
            meta.is_zero,
        );

        match self.mode {
            CollectMode::ProbeSelector => return,
            CollectMode::ListFields => {
                if !meta.is_zero
                    && let Some(stat) = stat
                {
                    stat.distinct_nonzero.insert(format_val());
                }
                return;
            }
            CollectMode::Query => {}
        }

        if self.exclude_zero && meta.is_zero {
            return;
        }
        if let Some(ef) = self.entity_filter
            && !ef.matches_key(meta.entity_key)
        {
            return;
        }
        let val_str = format_val();
        if let Some(vf) = self.value_filter
            && !matches_value_filter(&val_str, vf)
        {
            return;
        }

        self.hits.push(RawHit {
            section: meta.section.to_string(),
            field_name: meta.field_name.to_string(),
            span_start: meta.span.start(),
            offset_info: meta.span.to_string(),
            entity_key: meta.entity_key,
            subcontext: meta.subcontext.map(str::to_string),
            value: val_str,
            is_zero: meta.is_zero,
        });
    }

    fn record_unhandled_boma(
        &mut self,
        parent_section: &str,
        subtype: u32,
        entity_key: EntityKey,
        subcontext: &str,
        raw_bytes: &[u8],
    ) {
        let selector_matches = self
            .selector
            .is_none_or(|sel| sel.matches_unhandled_boma(parent_section, subtype));
        if self.mode != CollectMode::ListFields && !selector_matches {
            return;
        }

        let sec = format!("{parent_section}::boma");
        let field = format!("0x{subtype:04X}");
        let is_zero = raw_bytes.iter().all(|&b| b == 0);
        let len = raw_bytes.len();

        let stat = Self::record_stat(
            &mut self.stats,
            &sec,
            &field,
            0,
            || format!("0..{len}"),
            Some(len),
            is_zero,
        );

        match self.mode {
            CollectMode::ProbeSelector => return,
            CollectMode::ListFields => {
                if !is_zero && let Some(stat) = stat {
                    stat.distinct_nonzero.insert(hex::encode(raw_bytes));
                }
                return;
            }
            CollectMode::Query => {}
        }

        if self.exclude_zero && is_zero {
            return;
        }
        if let Some(ef) = self.entity_filter
            && !ef.matches_key(entity_key)
        {
            return;
        }
        let val_str = hex::encode(raw_bytes);
        if let Some(vf) = self.value_filter
            && !matches_value_filter(&val_str, vf)
        {
            return;
        }

        self.hits.push(RawHit {
            section: sec,
            field_name: field,
            span_start: 0,
            offset_info: format!("0..{len}"),
            entity_key,
            subcontext: Some(subcontext.to_string()),
            value: val_str,
            is_zero,
        });
    }
}

struct ScopedVisitor<'c, 'a> {
    collector: &'c mut Collector<'a>,
    entity_key: EntityKey,
    subcontext: Option<&'c str>,
}

impl UnknownVisitor for ScopedVisitor<'_, '_> {
    fn visit_unknown<T: UnknownValue>(
        &mut self,
        section: &str,
        field_name: &str,
        span: UnknownSpan,
        value: &Unknown<T>,
    ) {
        self.collector.record_field(
            FieldVisitMeta {
                section,
                field_name,
                span,
                entity_key: self.entity_key,
                subcontext: self.subcontext,
                is_zero: value.is_zero(),
            },
            || value.format_unknown(),
        );
    }
}

fn walk_bomas(
    bomas: &[BomaSection],
    parent_label: &str,
    entity_key: EntityKey,
    collector: &mut Collector<'_>,
) {
    for (i, boma) in bomas.iter().enumerate() {
        let subcontext = match &boma.payload {
            BomaPayload::TrackNumerics(n) => {
                format!(
                    "boma[{i}] TrackNumerics(subrecord_kind={})",
                    n.subrecord_kind
                )
            }
            _ => format!("boma[{i}] subtype={:#06x}", boma.subtype),
        };
        {
            let mut visitor = ScopedVisitor {
                collector,
                entity_key,
                subcontext: Some(&subcontext),
            };
            boma.visit_unknowns(parent_label, &mut visitor);
        }
        if let BomaPayload::Unknown { raw_bytes, .. } = &boma.payload {
            collector.record_unhandled_boma(
                parent_label,
                boma.subtype,
                entity_key,
                &subcontext,
                raw_bytes,
            );
        }
    }
}

fn visit_header_with_key<T: VisitUnknowns>(
    item: &T,
    label: &str,
    entity_key: EntityKey,
    subcontext: Option<&str>,
    collector: &mut Collector<'_>,
) {
    let mut visitor = ScopedVisitor {
        collector,
        entity_key,
        subcontext,
    };
    item.visit_unknowns(label, &mut visitor);
}

fn walk_album_list(
    lama: &musicdb::sections::AlbumListSection,
    lib_allowed: bool,
    collector: &mut Collector<'_>,
) {
    if lib_allowed {
        visit_header_with_key(&lama.header, "lama", EntityKey::Library, None, collector);
    }
    for album in &lama.albums {
        let key = EntityKey::Album(PersistentId(album.header.album_persistent_id));
        if collector
            .entity_filter
            .is_some_and(|ef| !ef.matches_key(key))
        {
            continue;
        }
        visit_header_with_key(&album.header, "iama", key, None, collector);
        walk_bomas(&album.bomas, "iama", key, collector);
    }
}

fn walk_artist_list(
    artists: &musicdb::sections::ArtistListSection,
    lib_allowed: bool,
    collector: &mut Collector<'_>,
) {
    if lib_allowed {
        visit_header_with_key(&artists.header, "lAma", EntityKey::Library, None, collector);
    }
    for artist in &artists.artists {
        let key = EntityKey::Artist(PersistentId(artist.header.artist_persistent_id));
        if collector
            .entity_filter
            .is_some_and(|ef| !ef.matches_key(key))
        {
            continue;
        }
        visit_header_with_key(&artist.header, "iAma", key, None, collector);
        walk_bomas(&artist.bomas, "iAma", key, collector);
    }
}

fn walk_track_list(
    ltma: &musicdb::sections::TrackListSection,
    lib_allowed: bool,
    collector: &mut Collector<'_>,
) {
    if lib_allowed {
        visit_header_with_key(&ltma.header, "ltma", EntityKey::Library, None, collector);
    }
    for track in &ltma.tracks {
        let key = EntityKey::Track(PersistentId(track.header.track_persistent_id));
        if collector
            .entity_filter
            .is_some_and(|ef| !ef.matches_key(key))
        {
            continue;
        }
        visit_header_with_key(&track.header, "itma", key, None, collector);
        walk_bomas(&track.bomas, "itma", key, collector);
    }
}

fn walk_playlist_list(
    playlists: &musicdb::sections::PlaylistListSection,
    lib_allowed: bool,
    collector: &mut Collector<'_>,
) {
    if lib_allowed {
        visit_header_with_key(
            &playlists.header,
            "lPma",
            EntityKey::Library,
            None,
            collector,
        );
    }
    for playlist in &playlists.playlists {
        let pl_id = PersistentId(playlist.header.playlist_persistent_id);
        if collector
            .entity_filter
            .is_some_and(|ef| !ef.matches_playlist_section(pl_id, &playlist.bomas))
        {
            continue;
        }
        if collector
            .entity_filter
            .is_none_or(|ef| ef.matches_key(EntityKey::Playlist(pl_id)))
        {
            visit_header_with_key(
                &playlist.header,
                "lpma",
                EntityKey::Playlist(pl_id),
                None,
                collector,
            );
        }
        for (i, boma) in playlist.bomas.iter().enumerate() {
            let key = match &boma.payload {
                BomaPayload::PlaylistItem(ipfa) => EntityKey::PlaylistTrack {
                    playlist_id: pl_id,
                    track_id: PersistentId(ipfa.track_persistent_id),
                    item_index: i,
                },
                BomaPayload::PlaylistItemCompact(ipfa) => EntityKey::PlaylistTrack {
                    playlist_id: pl_id,
                    track_id: PersistentId(ipfa.track_persistent_id),
                    item_index: i,
                },
                _ => EntityKey::Playlist(pl_id),
            };
            if collector
                .entity_filter
                .is_some_and(|ef| !ef.matches_key(key))
            {
                continue;
            }
            let subcontext = format!("boma[{i}] subtype={:#06x}", boma.subtype);
            visit_header_with_key(boma, "lpma", key, Some(&subcontext), collector);
            if let BomaPayload::Unknown { raw_bytes, .. } = &boma.payload {
                collector.record_unhandled_boma("lpma", boma.subtype, key, &subcontext, raw_bytes);
            }
        }
    }
}

fn walk_social_profile_list(
    lsma: &musicdb::sections::SocialProfileListSection,
    lib_allowed: bool,
    collector: &mut Collector<'_>,
) {
    if lib_allowed {
        visit_header_with_key(&lsma.header, "Lsma", EntityKey::Library, None, collector);
    }
    for profile in &lsma.profiles {
        let key = EntityKey::SocialProfile(PersistentId(profile.header.profile_persistent_id));
        if collector
            .entity_filter
            .is_some_and(|ef| !ef.matches_key(key))
        {
            continue;
        }
        visit_header_with_key(&profile.header, "isma", key, None, collector);
        walk_bomas(&profile.bomas, "isma", key, collector);
    }
}

fn walk_raw_library(raw: &RawLibraryFile, collector: &mut Collector<'_>) {
    let lib_allowed = collector
        .entity_filter
        .is_none_or(|ef| ef.matches_key(EntityKey::Library));

    if lib_allowed {
        visit_header_with_key(
            &raw.outer_header,
            "hfma(outer)",
            EntityKey::Library,
            None,
            collector,
        );
    }

    for (hsma_idx, hsma) in raw.sections.iter().enumerate() {
        if lib_allowed {
            let sub = format!("hsma[{hsma_idx}] subtype={}", hsma.header.kind());
            visit_header_with_key(
                &hsma.header,
                "hsma",
                EntityKey::Library,
                Some(&sub),
                collector,
            );
        }

        match &hsma.child {
            HsmaChild::InnerHfma(h) if lib_allowed => {
                visit_header_with_key(
                    h,
                    "hfma(inner)",
                    EntityKey::Library,
                    Some("hfma(inner)"),
                    collector,
                );
            }
            HsmaChild::Padding(p) if lib_allowed => {
                visit_header_with_key(p, "LPma", EntityKey::Library, None, collector);
            }
            HsmaChild::LibraryMaster(plma) if lib_allowed => {
                visit_header_with_key(&plma.header, "plma", EntityKey::Library, None, collector);
                walk_bomas(&plma.bomas, "plma", EntityKey::Library, collector);
            }
            HsmaChild::AlbumList(lama) => walk_album_list(lama, lib_allowed, collector),
            HsmaChild::ArtistList(artists) => walk_artist_list(artists, lib_allowed, collector),
            HsmaChild::TrackList(ltma) => walk_track_list(ltma, lib_allowed, collector),
            HsmaChild::PlaylistList(playlists) => {
                walk_playlist_list(playlists, lib_allowed, collector);
            }
            HsmaChild::SocialProfileList(lsma) => {
                walk_social_profile_list(lsma, lib_allowed, collector);
            }
            HsmaChild::Unknown {
                kind,
                signature,
                raw_bytes,
            } if lib_allowed => {
                let field_name = format!("unknown_child_{}", hsma.header.subtype);
                let sub = format!(
                    "hsma[{hsma_idx}] kind={kind} sig={}",
                    hex::encode(signature)
                );
                collector.record_field(
                    FieldVisitMeta {
                        section: "hsma",
                        field_name: &field_name,
                        span: UnknownSpan::Tail {
                            start: 0,
                            len: raw_bytes.len(),
                        },
                        entity_key: EntityKey::Library,
                        subcontext: Some(&sub),
                        is_zero: raw_bytes.iter().all(|&b| b == 0),
                    },
                    || format!("{} ({} bytes)", hex::encode(raw_bytes), raw_bytes.len()),
                );
            }
            _ => {}
        }
    }
}

fn stats_matching_keys<'a>(
    stats: &'a StatsMap,
    selector: &'a FieldSelector,
) -> Vec<(&'a str, &'a str)> {
    match selector {
        FieldSelector::BareField { field } | FieldSelector::AnySectionField { field } => stats
            .iter()
            .filter_map(|(s, fields)| {
                fields
                    .contains_key(field.as_str())
                    .then_some((s.as_str(), field.as_str()))
            })
            .collect(),
        FieldSelector::SectionField { sections, field } => sections
            .iter()
            .filter_map(|&s| {
                stats
                    .get(s)
                    .filter(|fields| fields.contains_key(field.as_str()))
                    .map(|_| (s, field.as_str()))
            })
            .collect(),
        FieldSelector::AllFields
        | FieldSelector::SectionAll { .. }
        | FieldSelector::BomaPayload { .. } => Vec::new(),
    }
}

fn check_selector_keys(selector: &FieldSelector, stats: &StatsMap) -> Result<()> {
    match selector {
        FieldSelector::BareField { field } => {
            let matching_sections: Vec<String> = stats_matching_keys(stats, selector)
                .into_iter()
                .map(|(s, f)| format!("{s}.{f}"))
                .collect();
            if matching_sections.is_empty() {
                bail!(
                    "no unknown field `{field}` found in this library; run with `--list-fields` to see available selectors"
                );
            }
            if matching_sections.len() > 1 {
                let first = matching_sections.first().map_or("", String::as_str);
                bail!(
                    "ambiguous field `{field}` matches multiple sections: {}; specify one (e.g. `-f {first}`) or use `-f '*.{field}'`",
                    matching_sections.join(", ")
                );
            }
        }
        FieldSelector::SectionField { sections, field } => {
            if stats_matching_keys(stats, selector).is_empty() {
                bail!(
                    "unknown field `{field}` not found in section(s) {}; run with `--list-fields` to see available selectors",
                    sections.join(", ")
                );
            }
        }
        FieldSelector::AnySectionField { field } => {
            if stats_matching_keys(stats, selector).is_empty() {
                bail!(
                    "no unknown field `{field}` found in any section; run with `--list-fields` to see available selectors"
                );
            }
        }
        FieldSelector::AllFields
        | FieldSelector::SectionAll { .. }
        | FieldSelector::BomaPayload { .. } => {}
    }
    Ok(())
}

fn validate_selector_matches(
    raw: &RawLibraryFile,
    selector: &FieldSelector,
    stats: &StatsMap,
    has_entity_filter: bool,
) -> Result<()> {
    if !has_entity_filter || !stats_matching_keys(stats, selector).is_empty() {
        return check_selector_keys(selector, stats);
    }
    // An active `--entity` filter may skip every item of the target section before
    // `stats` records the field. Probe `raw` without the entity filter so valid
    // combinations return empty matches while genuine field typos still error.
    let mut probe = Collector::new(
        Some(selector),
        None,
        None,
        false,
        CollectMode::ProbeSelector,
    );
    walk_raw_library(raw, &mut probe);
    check_selector_keys(selector, &probe.stats)
}

fn build_grouped_by_value(
    hits: Vec<RawHit>,
    stats: &StatsMap,
    ctx_builder: &mut LibraryContextBuilder<'_>,
    examples_limit: Option<usize>,
) -> Vec<FieldQueryGroup> {
    let mut by_field: BTreeMap<(String, String), Vec<RawHit>> = BTreeMap::new();
    for hit in hits {
        by_field
            .entry((hit.section.clone(), hit.field_name.clone()))
            .or_default()
            .push(hit);
    }

    let mut groups = Vec::new();
    for ((section, field_name), field_hits) in by_field {
        let stat = stats
            .get(section.as_str())
            .and_then(|sec_map| sec_map.get(field_name.as_str()));
        let offset_info = stat
            .map(FieldStats::span_label)
            .or_else(|| field_hits.first().map(|h| h.offset_info.clone()))
            .unwrap_or_default();
        let total_instances = stat.map_or(field_hits.len(), |s| s.total_instances);
        let nonzero_instances = stat.map_or(0, |s| s.nonzero_instances);
        let matched_instances = field_hits.len();

        let mut buckets_map: HashMap<(String, bool), Vec<RawHit>> = HashMap::new();
        for hit in field_hits {
            buckets_map
                .entry((hit.value.clone(), hit.is_zero))
                .or_default()
                .push(hit);
        }

        let distinct_values = buckets_map.len();
        let mut bucket_entries: Vec<((String, bool), Vec<RawHit>)> =
            buckets_map.into_iter().collect();
        bucket_entries.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.0.cmp(&b.0.0)));

        let buckets = bucket_entries
            .into_iter()
            .map(|((value, is_zero), items)| {
                let count = items.len();
                let take_n = examples_limit.unwrap_or(count);
                let examples: Vec<ContextualOccurrence> = items
                    .into_iter()
                    .take(take_n)
                    .map(|h| ContextualOccurrence {
                        subcontext: h.subcontext,
                        entity: ctx_builder.resolve(h.entity_key),
                    })
                    .collect();
                let shown = examples.len();
                ValueBucket {
                    value,
                    is_zero,
                    count,
                    shown,
                    examples,
                }
            })
            .collect();

        groups.push(FieldQueryGroup {
            struct_aliases: struct_aliases_for_section(&section),
            section,
            field_name,
            offset_info,
            total_instances,
            nonzero_instances,
            matched_instances,
            distinct_values,
            buckets,
        });
    }
    groups
}

fn build_grouped_by_entity(
    hits: Vec<RawHit>,
    ctx_builder: &mut LibraryContextBuilder<'_>,
) -> Vec<EntityQueryResult> {
    let mut by_entity: BTreeMap<EntityKey, Vec<RawHit>> = BTreeMap::new();
    for hit in hits {
        by_entity.entry(hit.entity_key).or_default().push(hit);
    }

    by_entity
        .into_iter()
        .map(|(key, mut entity_hits)| {
            entity_hits.sort_by(|a, b| {
                a.section
                    .cmp(&b.section)
                    .then_with(|| a.span_start.cmp(&b.span_start))
                    .then_with(|| a.field_name.cmp(&b.field_name))
                    .then_with(|| a.subcontext.cmp(&b.subcontext))
            });
            let fields = entity_hits
                .into_iter()
                .map(|h| EntityFieldHit {
                    section: h.section,
                    field_name: h.field_name,
                    offset_info: h.offset_info,
                    subcontext: h.subcontext,
                    value: h.value,
                    is_zero: h.is_zero,
                })
                .collect();
            EntityQueryResult {
                entity: ctx_builder.resolve(key),
                fields,
            }
        })
        .collect()
}

fn build_field_list(stats: StatsMap) -> QueryResults {
    let mut stats_vec: Vec<FieldStats> = stats
        .into_values()
        .flat_map(BTreeMap::into_values)
        .collect();
    stats_vec.sort_by(|a, b| {
        a.section
            .cmp(&b.section)
            .then_with(|| a.span_start.cmp(&b.span_start))
            .then_with(|| a.field_name.cmp(&b.field_name))
    });
    let fields = stats_vec
        .into_iter()
        .map(|s| FieldListing {
            struct_aliases: struct_aliases_for_section(&s.section),
            span: s.span_label(),
            section: s.section,
            field_name: s.field_name,
            total_instances: s.total_instances,
            nonzero_instances: s.nonzero_instances,
            distinct_nonzero_count: s.distinct_nonzero.len(),
        })
        .collect();
    QueryResults::FieldList { fields }
}

fn build_flat_results(
    hits: Vec<RawHit>,
    ctx_builder: &mut LibraryContextBuilder<'_>,
    options: &QueryUnknownsOptions,
) -> QueryResults {
    let total_matches = hits.len();
    let limit = if options.all {
        total_matches
    } else {
        options.examples.unwrap_or(50)
    };
    let matches: Vec<FlatOccurrence> = hits
        .into_iter()
        .take(limit)
        .map(|h| FlatOccurrence {
            section: h.section,
            field_name: h.field_name,
            offset_info: h.offset_info,
            subcontext: h.subcontext,
            value: h.value,
            is_zero: h.is_zero,
            entity: ctx_builder.resolve(h.entity_key),
        })
        .collect();
    let shown = matches.len();
    let truncated = total_matches.saturating_sub(shown);
    QueryResults::Flat {
        total_matches,
        shown,
        truncated,
        matches,
    }
}

fn resolve_zero_filter(
    options: &QueryUnknownsOptions,
    selector: Option<&FieldSelector>,
) -> (bool, String) {
    if options.list_fields {
        return (false, "not applicable (--list-fields)".to_string());
    }
    if options.nonzero {
        return (true, "exclude zeros (--nonzero)".to_string());
    }
    if options.include_zero {
        return (false, "include zeros (--include-zero)".to_string());
    }
    if selector.is_some_and(FieldSelector::is_single_concrete_field) {
        (false, "include zeros (single-field default)".to_string())
    } else {
        (
            true,
            "exclude zeros (multi-field/entity default)".to_string(),
        )
    }
}

pub(crate) fn query_unknowns(
    raw: &RawLibraryFile,
    options: &QueryUnknownsOptions,
) -> Result<QueryUnknownsReport> {
    if !options.list_fields && options.field.is_none() && options.entity.is_none() {
        bail!("at least one of --field, --entity, or --list-fields is required");
    }

    let selector = options
        .field
        .as_deref()
        .map(parse_field_selector)
        .transpose()?;
    let (exclude_zero, zero_filter_mode) = resolve_zero_filter(options, selector.as_ref());

    let lib = Library::from(raw);
    let entity_filter = options
        .entity
        .as_deref()
        .map(|q| EntityFilter::from_query(q, &lib))
        .transpose()?;

    let mode = if options.list_fields {
        CollectMode::ListFields
    } else {
        CollectMode::Query
    };
    let mut collector = Collector::new(
        selector.as_ref(),
        entity_filter.as_ref(),
        options.value.as_deref(),
        exclude_zero,
        mode,
    );
    walk_raw_library(raw, &mut collector);

    if options.list_fields {
        return Ok(QueryUnknownsReport {
            zero_filter_mode,
            field_filter: options.field.clone(),
            entity_filter: options.entity.clone(),
            value_filter: options.value.clone(),
            results: build_field_list(collector.stats),
        });
    }

    if let Some(sel) = &selector {
        validate_selector_matches(raw, sel, &collector.stats, entity_filter.is_some())?;
    }

    let tracks_limit = (!options.all).then_some(options.tracks_limit);
    let mut ctx_builder = LibraryContextBuilder::new(raw, &lib, tracks_limit);

    let results = if options.flat {
        build_flat_results(collector.hits, &mut ctx_builder, options)
    } else if selector.is_some() {
        let examples_limit = (!options.all).then(|| options.examples.unwrap_or(5));
        let fields = build_grouped_by_value(
            collector.hits,
            &collector.stats,
            &mut ctx_builder,
            examples_limit,
        );
        QueryResults::GroupedByValue { fields }
    } else {
        let entities = build_grouped_by_entity(collector.hits, &mut ctx_builder);
        QueryResults::GroupedByEntity { entities }
    };

    Ok(QueryUnknownsReport {
        zero_filter_mode,
        field_filter: options.field.clone(),
        entity_filter: options.entity.clone(),
        value_filter: options.value.clone(),
        results,
    })
}

impl fmt::Display for EntityContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Library {
                persistent_id,
                app_version,
                format_version,
                tracks,
                albums,
                artists,
                playlists,
            } => write!(
                f,
                "Library [{persistent_id}] app={app_version} fmt={format_version} ({tracks} tracks, {albums} albums, {artists} artists, {playlists} playlists)"
            ),
            Self::Album { .. } => self.fmt_album(f),
            Self::Track { .. } => self.fmt_track(f),
            Self::Artist { .. } => self.fmt_artist(f),
            Self::Playlist { .. } | Self::PlaylistTrack { .. } | Self::SocialProfile { .. } => {
                self.fmt_secondary(f)
            }
        }
    }
}

impl EntityContext {
    fn fmt_album(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::Album {
            persistent_id,
            title,
            artist,
            album_artist,
            is_compilation,
            playlist_only,
            star_rating,
            apple_music_store_album_id,
            track_count,
            ..
        } = self
        else {
            return Ok(());
        };
        let display_artist = artist
            .as_deref()
            .or(album_artist.as_deref())
            .unwrap_or("<unknown artist>");
        let aa_note = match (artist.as_deref(), album_artist.as_deref()) {
            (Some(a), Some(aa)) if a != aa => format!(", album_artist={aa:?}"),
            _ => String::new(),
        };
        let rating_note = if *star_rating > 0 {
            format!(", rating={star_rating}")
        } else {
            String::new()
        };
        let store_note =
            apple_music_store_album_id.map_or(String::new(), |id| format!(", store_id={id}"));
        write!(
            f,
            "Album [{persistent_id}] \"{}\" — {display_artist} ({track_count} tracks, compilation={is_compilation}, playlist_only={playlist_only}{rating_note}{store_note}{aa_note})",
            title.as_deref().unwrap_or("<untitled>"),
        )
    }

    fn fmt_track(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::Track {
            persistent_id,
            title,
            artist,
            album,
            genre,
            year,
            track_number,
            disc_number,
            media_type,
            cloud_status,
            downloaded_state,
            playlist_only,
            stars,
            play_count,
            skip_count,
            ..
        } = self
        else {
            return Ok(());
        };
        let year_note = if *year > 0 {
            format!(", {year}")
        } else {
            String::new()
        };
        let genre_note = genre
            .as_deref()
            .filter(|g| !g.is_empty())
            .map_or(String::new(), |g| format!(", genre={g:?}"));
        let plays = play_count.map_or_else(|| "-".to_string(), |n| n.to_string());
        let skips = skip_count.map_or_else(|| "-".to_string(), |n| n.to_string());
        write!(
            f,
            "Track [{persistent_id}] \"{}\" — {} (album: {}, D{disc_number}T{track_number}{year_note}{genre_note}) [media={media_type}, cloud={cloud_status}, local={downloaded_state}, playlist_only={playlist_only}, stars={stars}, plays={plays}, skips={skips}]",
            title.as_deref().unwrap_or("<untitled>"),
            artist.as_deref().unwrap_or("<unknown artist>"),
            album.as_deref().unwrap_or("<unknown album>"),
        )
    }

    fn fmt_artist(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::Artist {
            persistent_id,
            name,
            sort_name,
            apple_music_store_artist_id,
            album_count,
            track_count,
            ..
        } = self
        else {
            return Ok(());
        };
        let sort_note = match (name.as_deref(), sort_name.as_deref()) {
            (n, Some(s)) if n != Some(s) && !s.is_empty() => format!(" [sort: {s:?}]"),
            _ => String::new(),
        };
        let store_note = if *apple_music_store_artist_id > 0 {
            format!(", store_id={apple_music_store_artist_id}")
        } else {
            String::new()
        };
        write!(
            f,
            "Artist [{persistent_id}] \"{}\"{sort_note} ({album_count} albums, {track_count} tracks{store_note})",
            name.as_deref().unwrap_or("<unnamed>"),
        )
    }

    fn fmt_secondary(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Playlist {
                persistent_id,
                name,
                kind,
                share_mode,
                is_master,
                is_folder,
                is_smart,
                track_count,
                ..
            } => write!(
                f,
                "Playlist [{persistent_id}] \"{}\" ({kind}, {track_count} tracks, smart={is_smart}, folder={is_folder}, master={is_master}, share={share_mode})",
                name.as_deref().unwrap_or("<unnamed>"),
            ),
            Self::PlaylistTrack {
                playlist_id,
                playlist_name,
                item_index,
                track_id,
                track_title,
                track_artist,
                ..
            } => write!(
                f,
                "PlaylistTrack [{playlist_id} \"{}\" #{item_index}] -> [{track_id}] \"{}\" — {}",
                playlist_name.as_deref().unwrap_or("<unnamed>"),
                track_title.as_deref().unwrap_or("<untitled>"),
                track_artist.as_deref().unwrap_or("<unknown artist>"),
            ),
            Self::SocialProfile {
                persistent_id,
                profile_id,
                name,
                handle,
            } => write!(
                f,
                "SocialProfile [{persistent_id}] \"{}\" (@{}, id={})",
                name.as_deref().unwrap_or("<unnamed>"),
                handle.as_deref().unwrap_or("-"),
                profile_id.as_deref().unwrap_or("-"),
            ),
            _ => Ok(()),
        }
    }
}

fn fmt_sample_tracks(f: &mut fmt::Formatter<'_>, entity: &EntityContext) -> fmt::Result {
    let (sample_tracks, truncated) = match entity {
        EntityContext::Album {
            sample_tracks,
            tracks_truncated,
            ..
        }
        | EntityContext::Artist {
            sample_tracks,
            tracks_truncated,
            ..
        }
        | EntityContext::Playlist {
            sample_tracks,
            tracks_truncated,
            ..
        } => (sample_tracks.as_slice(), *tracks_truncated),
        _ => return Ok(()),
    };
    for t in sample_tracks {
        let pl_only = if t.playlist_only {
            ", playlist_only"
        } else {
            ""
        };
        writeln!(
            f,
            "      • [{}] D{}T{} \"{}\" — {} ({}{pl_only})",
            t.persistent_id,
            t.disc_number,
            t.track_number,
            t.title.as_deref().unwrap_or("<untitled>"),
            t.artist.as_deref().unwrap_or("<unknown>"),
            t.cloud_status
        )?;
    }
    if truncated > 0 {
        writeln!(f, "      • ... and {truncated} more track(s)")?;
    }
    Ok(())
}

fn fmt_field_list(f: &mut fmt::Formatter<'_>, fields: &[FieldListing]) -> fmt::Result {
    writeln!(f, "Observed unknown fields ({}):", fields.len())?;
    for field in fields {
        let aliases = if field.struct_aliases.is_empty() {
            String::new()
        } else {
            format!(" [{}]", field.struct_aliases.join(", "))
        };
        writeln!(
            f,
            "  {}.{} ({}){} — total: {}, non-zero: {}, distinct non-zero: {}",
            field.section,
            field.field_name,
            field.span,
            aliases,
            field.total_instances,
            field.nonzero_instances,
            field.distinct_nonzero_count
        )?;
    }
    Ok(())
}

fn fmt_grouped_by_value(f: &mut fmt::Formatter<'_>, fields: &[FieldQueryGroup]) -> fmt::Result {
    if fields.is_empty() {
        return writeln!(f, "No matching occurrences found.");
    }
    for group in fields {
        let aliases = if group.struct_aliases.is_empty() {
            String::new()
        } else {
            format!(" [{}]", group.struct_aliases.join(", "))
        };
        writeln!(
            f,
            "{}.{} (offset {}){} — {}/{} matched ({} non-zero, {} distinct):",
            group.section,
            group.field_name,
            group.offset_info,
            aliases,
            group.matched_instances,
            group.total_instances,
            group.nonzero_instances,
            group.distinct_values
        )?;
        for bucket in &group.buckets {
            let zero_tag = if bucket.is_zero { " [zero]" } else { "" };
            writeln!(
                f,
                "  Value: {}{} — {} occurrence(s) (showing {}):",
                bucket.value, zero_tag, bucket.count, bucket.shown
            )?;
            for ex in &bucket.examples {
                let sub = ex
                    .subcontext
                    .as_deref()
                    .map_or(String::new(), |s| format!(" [{s}]"));
                writeln!(f, "    - {}{sub}", ex.entity)?;
                fmt_sample_tracks(f, &ex.entity)?;
            }
        }
    }
    Ok(())
}

fn fmt_grouped_by_entity(
    f: &mut fmt::Formatter<'_>,
    entities: &[EntityQueryResult],
) -> fmt::Result {
    if entities.is_empty() {
        return writeln!(f, "No matching entities found.");
    }
    for res in entities {
        writeln!(f, "{}", res.entity)?;
        fmt_sample_tracks(f, &res.entity)?;
        for hit in &res.fields {
            let sub = hit
                .subcontext
                .as_deref()
                .map_or(String::new(), |s| format!(" [{s}]"));
            writeln!(
                f,
                "  {}.{} ({}){} = {}",
                hit.section, hit.field_name, hit.offset_info, sub, hit.value
            )?;
        }
    }
    Ok(())
}

fn fmt_flat(
    f: &mut fmt::Formatter<'_>,
    total_matches: usize,
    shown: usize,
    truncated: usize,
    matches: &[FlatOccurrence],
) -> fmt::Result {
    writeln!(f, "Matches: {shown} of {total_matches}:")?;
    for m in matches {
        let sub = m
            .subcontext
            .as_deref()
            .map_or(String::new(), |s| format!(" [{s}]"));
        writeln!(
            f,
            "  {}.{} ({}){} = {} | {}",
            m.section, m.field_name, m.offset_info, sub, m.value, m.entity
        )?;
    }
    if truncated > 0 {
        writeln!(
            f,
            "... {truncated} more match(es) omitted (use --examples <N> or --all to show more)."
        )?;
    }
    Ok(())
}

impl fmt::Display for QueryUnknownsReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== Unknowns Query ===")?;
        if !matches!(self.results, QueryResults::FieldList { .. }) {
            writeln!(f, "Zero filter: {}", self.zero_filter_mode)?;
        }
        if let Some(field) = &self.field_filter {
            writeln!(f, "Field filter: {field}")?;
        }
        if let Some(entity) = &self.entity_filter {
            writeln!(f, "Entity filter: {entity}")?;
        }
        if let Some(val) = &self.value_filter {
            writeln!(f, "Value filter: {val}")?;
        }
        writeln!(f)?;

        match &self.results {
            QueryResults::FieldList { fields } => fmt_field_list(f, fields),
            QueryResults::GroupedByValue { fields } => fmt_grouped_by_value(f, fields),
            QueryResults::GroupedByEntity { entities } => fmt_grouped_by_entity(f, entities),
            QueryResults::Flat {
                total_matches,
                shown,
                truncated,
                matches,
            } => fmt_flat(f, *total_matches, *shown, *truncated, matches),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u16(buf: &mut [u8], off: usize, val: u16) {
        buf[off..off + 2].copy_from_slice(&val.to_le_bytes());
    }

    fn put_u32(buf: &mut [u8], off: usize, val: u32) {
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
    }

    fn put_u64(buf: &mut [u8], off: usize, val: u64) {
        buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
    }

    fn make_hfma(
        track_count: u32,
        playlist_count: u32,
        album_count: u32,
        artist_count: u32,
        lib_pid: u64,
        unk_60: u32,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; 160];
        buf[0..4].copy_from_slice(b"hfma");
        put_u32(&mut buf, 4, 160);
        put_u16(&mut buf, 12, 23);
        put_u16(&mut buf, 14, 1);
        buf[16..25].copy_from_slice(b"1.7.0.156");
        put_u64(&mut buf, 48, lib_pid);
        put_u32(&mut buf, 56, 9);
        put_u32(&mut buf, 60, unk_60);
        put_u32(&mut buf, 68, track_count);
        put_u32(&mut buf, 72, playlist_count);
        put_u32(&mut buf, 76, album_count);
        put_u32(&mut buf, 80, artist_count);
        put_u32(&mut buf, 84, 102_400);
        buf
    }

    fn wrap_hsma(subtype: u32, child: &[u8]) -> Vec<u8> {
        let mut hdr = vec![0u8; 56];
        hdr[0..4].copy_from_slice(b"hsma");
        put_u32(&mut hdr, 4, 56);
        let total = 56u32 + u32::try_from(child.len()).unwrap();
        put_u32(&mut hdr, 8, total);
        put_u32(&mut hdr, 12, subtype);
        hdr.extend_from_slice(child);
        hdr
    }

    fn make_str_boma(subtype: u32, text: &str, unk_12: u32) -> Vec<u8> {
        let mut utf16 = Vec::new();
        for u in text.encode_utf16() {
            utf16.extend_from_slice(&u.to_le_bytes());
        }
        let total = 20u32 + 16 + u32::try_from(utf16.len()).unwrap();
        let mut buf = vec![0u8; 36];
        buf[0..4].copy_from_slice(b"boma");
        put_u32(&mut buf, 4, 20);
        put_u32(&mut buf, 8, total);
        put_u32(&mut buf, 12, subtype);
        put_u32(&mut buf, 20, 1); // UTF-16LE
        put_u32(&mut buf, 24, u32::try_from(utf16.len()).unwrap());
        put_u32(&mut buf, 32, unk_12);
        buf.extend_from_slice(&utf16);
        buf
    }

    fn make_raw_boma(subtype: u32, payload: &[u8]) -> Vec<u8> {
        let total = 20u32 + u32::try_from(payload.len()).unwrap();
        let mut buf = vec![0u8; 20];
        buf[0..4].copy_from_slice(b"boma");
        put_u32(&mut buf, 4, 20);
        put_u32(&mut buf, 8, total);
        put_u32(&mut buf, 12, subtype);
        buf.extend_from_slice(payload);
        buf
    }

    fn make_track_numerics_boma(subrecord_kind: u32, unk_300_val: i32) -> Vec<u8> {
        let mut payload = vec![0u8; 364];
        put_u32(&mut payload, 4, subrecord_kind);
        payload[300..304].copy_from_slice(&unk_300_val.to_le_bytes());
        make_raw_boma(0x0001, &payload)
    }

    fn make_album_item(
        pid: u64,
        unk_24: u8,
        unk_28: u32,
        title: &str,
        artist: &str,
        extra_bomas: &[Vec<u8>],
    ) -> Vec<u8> {
        let mut bomas = Vec::new();
        bomas.extend_from_slice(&make_str_boma(0x012C, title, 0));
        bomas.extend_from_slice(&make_str_boma(0x012D, artist, 0));
        for b in extra_bomas {
            bomas.extend_from_slice(b);
        }
        let boma_count = 2 + u32::try_from(extra_bomas.len()).unwrap();
        let mut hdr = vec![0u8; 140];
        hdr[0..4].copy_from_slice(b"iama");
        put_u32(&mut hdr, 4, 140);
        put_u32(&mut hdr, 8, 140 + u32::try_from(bomas.len()).unwrap());
        put_u32(&mut hdr, 12, boma_count);
        put_u64(&mut hdr, 16, pid);
        hdr[24] = unk_24;
        put_u32(&mut hdr, 28, unk_28);
        put_u64(&mut hdr, 64, pid);
        hdr.extend_from_slice(&bomas);
        hdr
    }

    fn make_artist_item(pid: u64, unk_24: u32, name: &str) -> Vec<u8> {
        let boma = make_str_boma(0x0190, name, 0);
        let mut hdr = vec![0u8; 140];
        hdr[0..4].copy_from_slice(b"iAma");
        put_u32(&mut hdr, 4, 140);
        put_u32(&mut hdr, 8, 140 + u32::try_from(boma.len()).unwrap());
        put_u32(&mut hdr, 12, 1);
        put_u64(&mut hdr, 16, pid);
        put_u32(&mut hdr, 24, unk_24);
        hdr.extend_from_slice(&boma);
        hdr
    }

    struct TrackSpec<'a> {
        pid: u64,
        album_pid: u64,
        artist_pid: u64,
        disc: u16,
        track_num: u16,
        title: &'a str,
        artist: &'a str,
        album: &'a str,
    }

    fn make_track_item(spec: &TrackSpec<'_>, extra_bomas: &[Vec<u8>]) -> Vec<u8> {
        let mut bomas = Vec::new();
        bomas.extend_from_slice(&make_str_boma(0x0002, spec.title, 0));
        bomas.extend_from_slice(&make_str_boma(0x0003, spec.album, 0));
        bomas.extend_from_slice(&make_str_boma(0x0004, spec.artist, 0));
        for b in extra_bomas {
            bomas.extend_from_slice(b);
        }
        let boma_count = 3 + u32::try_from(extra_bomas.len()).unwrap();
        let mut hdr = vec![0u8; 376];
        hdr[0..4].copy_from_slice(b"itma");
        put_u32(&mut hdr, 4, 376);
        put_u32(&mut hdr, 8, 376 + u32::try_from(bomas.len()).unwrap());
        put_u32(&mut hdr, 12, boma_count);
        put_u64(&mut hdr, 16, spec.pid);
        put_u16(&mut hdr, 84, spec.disc);
        put_u16(&mut hdr, 160, spec.track_num);
        put_u64(&mut hdr, 172, spec.album_pid);
        put_u64(&mut hdr, 180, spec.artist_pid);
        hdr.extend_from_slice(&bomas);
        hdr
    }

    fn make_ipfa_boma(track_pid: u64, unk_28_val: u32) -> Vec<u8> {
        let mut payload = vec![0u8; 68];
        payload[0..4].copy_from_slice(b"ipfa");
        put_u32(&mut payload, 4, 68);
        put_u64(&mut payload, 20, track_pid);
        put_u32(&mut payload, 28, unk_28_val);
        make_raw_boma(0x00CE, &payload)
    }

    fn make_compact_ipfa_boma(track_pid: u64, unk_24_val: u64) -> Vec<u8> {
        let mut payload = vec![0u8; 32];
        payload[0..4].copy_from_slice(b"IPfa");
        put_u32(&mut payload, 4, 32);
        put_u64(&mut payload, 16, track_pid);
        put_u64(&mut payload, 24, unk_24_val);
        make_raw_boma(0x00DA, &payload)
    }

    fn make_playlist_item(pid: u64, name: &str, extra_bomas: &[Vec<u8>]) -> Vec<u8> {
        let mut bomas = Vec::new();
        bomas.extend_from_slice(&make_str_boma(0x00C8, name, 0));
        for b in extra_bomas {
            bomas.extend_from_slice(b);
        }
        let boma_count = 1 + u32::try_from(extra_bomas.len()).unwrap();
        let mut hdr = vec![0u8; 360];
        hdr[0..4].copy_from_slice(b"lpma");
        put_u32(&mut hdr, 4, 360);
        put_u32(&mut hdr, 8, 360 + u32::try_from(bomas.len()).unwrap());
        put_u32(&mut hdr, 12, boma_count);
        put_u64(&mut hdr, 30, pid);
        hdr.extend_from_slice(&bomas);
        hdr
    }

    fn make_social_profile_item(pid: u64, unk_52: u32, name: &str, handle: &str) -> Vec<u8> {
        let mut bomas = Vec::new();
        bomas.extend_from_slice(&make_str_boma(0x0320, "p.SYNTH01", 0));
        bomas.extend_from_slice(&make_str_boma(0x0321, name, 0));
        bomas.extend_from_slice(&make_str_boma(0x0322, handle, 0));
        let mut hdr = vec![0u8; 136];
        hdr[0..4].copy_from_slice(b"isma");
        put_u32(&mut hdr, 4, 136);
        put_u32(&mut hdr, 8, 136 + u32::try_from(bomas.len()).unwrap());
        put_u32(&mut hdr, 12, 3);
        put_u64(&mut hdr, 16, pid);
        put_u32(&mut hdr, 52, unk_52);
        hdr.extend_from_slice(&bomas);
        hdr
    }

    fn make_list_section(magic: [u8; 4], items: &[Vec<u8>]) -> Vec<u8> {
        let header_size = if magic == *b"Lsma" { 92 } else { 48 };
        let mut hdr = vec![0u8; header_size];
        hdr[0..4].copy_from_slice(&magic);
        put_u32(&mut hdr, 4, u32::try_from(header_size).unwrap());
        put_u32(&mut hdr, 8, u32::try_from(items.len()).unwrap());
        for item in items {
            hdr.extend_from_slice(item);
        }
        hdr
    }

    fn build_synthetic_raw_library() -> RawLibraryFile {
        let album_a = make_album_item(
            0xAA01,
            2,
            0,
            "Moving Pictures",
            "Rush",
            &[make_raw_boma(0x004A, &[0xAA, 0xBB, 0xCC])],
        );
        let album_b = make_album_item(
            0xAA02,
            2,
            1,
            "Permanent Waves",
            "Rush",
            &[make_raw_boma(0x004A, &[0x11, 0x22, 0x33, 0x44, 0x55])],
        );
        let album_c = make_album_item(0xAA03, 2, 2, "Hemispheres", "Rush", &[]);
        let album_list = make_list_section(*b"lama", &[album_a, album_b, album_c]);

        let artist_1 = make_artist_item(0xBB01, 77, "Rush");
        let artist_list = make_list_section(*b"lAma", &[artist_1]);

        let track_1 = make_track_item(
            &TrackSpec {
                pid: 0xCC01,
                album_pid: 0xAA01,
                artist_pid: 0xBB01,
                disc: 1,
                track_num: 2,
                title: "Red Barchetta",
                artist: "Rush",
                album: "Moving Pictures",
            },
            &[],
        );
        let track_2 = make_track_item(
            &TrackSpec {
                pid: 0xCC02,
                album_pid: 0xAA01,
                artist_pid: 0xBB01,
                disc: 1,
                track_num: 1,
                title: "Tom Sawyer",
                artist: "Rush",
                album: "Moving Pictures",
            },
            &[
                make_track_numerics_boma(1, 100),
                make_track_numerics_boma(3, -1),
            ],
        );
        let track_list = make_list_section(*b"ltma", &[track_1, track_2]);

        let playlist_1 = make_playlist_item(
            0xDD01,
            "2112 Essentials",
            &[
                make_ipfa_boma(0xCC02, 42),
                make_compact_ipfa_boma(0xCC01, 99),
            ],
        );
        let playlist_list = make_list_section(*b"lPma", &[playlist_1]);

        let profile_1 = make_social_profile_item(0xEE01, 5, "Rush Überfan", "rush2112");
        let social_list = make_list_section(*b"Lsma", &[profile_1]);

        let mut plma = vec![0u8; 160];
        plma[0..4].copy_from_slice(b"plma");
        put_u32(&mut plma, 4, 160);
        put_u32(&mut plma, 8, 0);

        let mut raw_bytes = make_hfma(2, 1, 3, 1, 0x1111_2222_3333_4444, 20);
        raw_bytes.extend_from_slice(&wrap_hsma(6, &plma));
        raw_bytes.extend_from_slice(&wrap_hsma(4, &album_list));
        raw_bytes.extend_from_slice(&wrap_hsma(5, &artist_list));
        raw_bytes.extend_from_slice(&wrap_hsma(1, &track_list));
        raw_bytes.extend_from_slice(&wrap_hsma(2, &playlist_list));
        raw_bytes.extend_from_slice(&wrap_hsma(15, &social_list));

        RawLibraryFile::from_raw_bytes(&raw_bytes).expect("synthetic raw library should parse")
    }

    #[test]
    fn alias_and_case_sensitivity_resolution() {
        let raw = build_synthetic_raw_library();

        let sel_album = parse_field_selector("AlbumItemHeader.unk_24").unwrap();
        assert!(sel_album.matches_unknown_field("iama", "unk_24"));
        assert!(!sel_album.matches_unknown_field("iAma", "unk_24"));

        let sel_artist = parse_field_selector("ArtistItemHeader.unk_24").unwrap();
        assert!(sel_artist.matches_unknown_field("iAma", "unk_24"));
        assert!(!sel_artist.matches_unknown_field("iama", "unk_24"));

        let sel_boma_str = parse_field_selector("boma::str.unk_12").unwrap();
        assert!(sel_boma_str.matches_unknown_field("boma::str", "unk_12"));

        let sel_plma_sub = parse_field_selector("plma::0x01F6.unk_0").unwrap();
        assert!(sel_plma_sub.matches_unknown_field("plma::0x01F6", "unk_0"));

        let err_ambig = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("unk_24".into()),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(err_ambig.contains("ambiguous field `unk_24`"));
        assert!(err_ambig.contains("iama.unk_24"));
        assert!(err_ambig.contains("iAma.unk_24"));

        let unique_res = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("unk_300".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(matches!(
            unique_res.results,
            QueryResults::GroupedByValue { .. }
        ));

        assert!(
            query_unknowns(
                &raw,
                &QueryUnknownsOptions {
                    field: Some("iama.nonexistent_field".into()),
                    ..Default::default()
                },
            )
            .is_err()
        );
    }

    #[test]
    fn value_bucketing_and_album_context() {
        let raw = build_synthetic_raw_library();
        let report = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("iama.unk_28".into()),
                tracks_limit: 5,
                ..Default::default()
            },
        )
        .unwrap();

        let QueryResults::GroupedByValue { fields } = report.results else {
            panic!("expected GroupedByValue");
        };
        assert_eq!(fields.len(), 1);
        let group = &fields[0];
        assert_eq!(group.total_instances, 3);
        assert_eq!(group.nonzero_instances, 2);
        assert_eq!(group.distinct_values, 3);

        let moving_pictures = group
            .buckets
            .iter()
            .find(|b| b.is_zero)
            .expect("zero bucket should be included by default on single-field query");
        let EntityContext::Album {
            title,
            artist,
            track_count,
            sample_tracks,
            ..
        } = &moving_pictures.examples[0].entity
        else {
            panic!("expected Album context");
        };
        assert_eq!(title.as_deref(), Some("Moving Pictures"));
        assert_eq!(artist.as_deref(), Some("Rush"));
        assert_eq!(*track_count, 2);
        assert_eq!(sample_tracks[0].title.as_deref(), Some("Tom Sawyer"));
        assert_eq!(sample_tracks[1].title.as_deref(), Some("Red Barchetta"));

        let nonzero_report = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("iama.unk_28".into()),
                nonzero: true,
                tracks_limit: 5,
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByValue { fields } = nonzero_report.results else {
            panic!("expected GroupedByValue");
        };
        assert_eq!(fields[0].matched_instances, 2);
        assert!(fields[0].buckets.iter().all(|b| !b.is_zero));
    }

    #[test]
    fn track_numerics_multi_subrecord_attribution() {
        let raw = build_synthetic_raw_library();
        let report = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("TrackNumerics.unk_300".into()),
                flat: true,
                ..Default::default()
            },
        )
        .unwrap();

        let QueryResults::Flat { matches, .. } = report.results else {
            panic!("expected Flat");
        };
        assert_eq!(matches.len(), 2);
        let subcontexts: Vec<&str> = matches
            .iter()
            .filter_map(|m| m.subcontext.as_deref())
            .collect();
        assert!(
            subcontexts
                .iter()
                .any(|s| s.contains("TrackNumerics(subrecord_kind=1)"))
        );
        assert!(
            subcontexts
                .iter()
                .any(|s| s.contains("TrackNumerics(subrecord_kind=3)"))
        );
        assert!(matches.iter().all(|m| matches!(
            &m.entity,
            EntityContext::Track { title: Some(t), .. } if t == "Tom Sawyer"
        )));
    }

    #[test]
    fn playlist_ipfa_and_compact_ipfa_attribution() {
        let raw = build_synthetic_raw_library();
        let ipfa_report = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("PlaylistTrackItem.unk_28_40".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByValue { fields } = ipfa_report.results else {
            panic!("expected GroupedByValue");
        };
        assert_eq!(fields[0].section, "ipfa");
        assert_eq!(fields[0].matched_instances, 1);
        let EntityContext::PlaylistTrack {
            playlist_name,
            track_title,
            track_artist,
            ..
        } = &fields[0].buckets[0].examples[0].entity
        else {
            panic!("expected PlaylistTrack");
        };
        assert_eq!(playlist_name.as_deref(), Some("2112 Essentials"));
        assert_eq!(track_title.as_deref(), Some("Tom Sawyer"));
        assert_eq!(track_artist.as_deref(), Some("Rush"));

        let compact_report = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("IPfa.unk_24_end".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByValue { fields } = compact_report.results else {
            panic!("expected GroupedByValue");
        };
        assert_eq!(fields[0].section, "IPfa");
        assert_eq!(fields[0].matched_instances, 1);
    }

    #[test]
    fn entity_only_queries_and_zero_filtering() {
        let raw = build_synthetic_raw_library();
        let default_res = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                entity: Some("album:Moving Pictures".into()),
                tracks_limit: 5,
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByEntity { entities } = default_res.results else {
            panic!("expected GroupedByEntity");
        };
        assert_eq!(entities.len(), 1);
        assert!(entities[0].fields.iter().all(|f| !f.is_zero));

        let with_zero = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                entity: Some("album:Moving Pictures".into()),
                include_zero: true,
                tracks_limit: 5,
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByEntity { entities } = with_zero.results else {
            panic!("expected GroupedByEntity");
        };
        assert!(entities[0].fields.iter().any(|f| f.is_zero));

        let lib_res = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                entity: Some("library".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByEntity { entities } = lib_res.results else {
            panic!("expected GroupedByEntity");
        };
        assert_eq!(entities.len(), 1);
        assert!(matches!(entities[0].entity, EntityContext::Library { .. }));
        assert!(
            entities[0]
                .fields
                .iter()
                .any(|f| f.section == "hfma(outer)")
        );
    }

    #[test]
    fn unhandled_boma_and_value_matching() {
        let raw = build_synthetic_raw_library();
        let boma_res = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("boma:0x004A".into()),
                value: Some("aa bb cc".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByValue { fields } = boma_res.results else {
            panic!("expected GroupedByValue");
        };
        assert_eq!(fields[0].section, "iama::boma");
        assert_eq!(fields[0].buckets[0].value, "aabbcc");

        assert!(matches_value_filter("2 (0x00000002)", "2"));
        assert!(matches_value_filter("2 (0x00000002)", "0x2"));
        assert!(matches_value_filter("2 (0x00000002)", "0x00000002"));
        assert!(matches_value_filter("-1 (0xFFFFFFFF)", "-1"));
        assert!(matches_value_filter("-1 (0xFFFFFFFF)", "0xFFFFFFFF"));
        assert!(matches_value_filter("aabbcc", "AABBCC"));
        assert!(matches_value_filter("aabbcc", "aa bb cc"));
        assert!(!matches_value_filter("2 (0x00000002)", "3"));
    }

    #[test]
    fn list_fields_and_json_serialization() {
        let raw = build_synthetic_raw_library();
        let report = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                list_fields: true,
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::FieldList { ref fields } = report.results else {
            panic!("expected FieldList");
        };
        assert!(
            fields.iter().any(|f| f.section == "iama"
                && f.field_name == "unk_28"
                && f.nonzero_instances == 2)
        );
        assert!(
            fields
                .iter()
                .any(|f| f.section == "iama::boma" && f.field_name == "0x004A")
        );

        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("FieldList"));
        assert!(json.contains("0x004A"));
        let list_text = report.to_string();
        assert!(!list_text.contains("Zero filter:"));
        assert!(list_text.contains("3..=5 bytes"));
    }

    #[test]
    fn field_and_disjoint_or_missing_entity_returns_empty_not_error() {
        let raw = build_synthetic_raw_library();

        let no_such_album = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("iama.unk_28".into()),
                entity: Some("ZZZ_NO_SUCH_ALBUM".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByValue { fields } = &no_such_album.results else {
            panic!("expected GroupedByValue");
        };
        assert!(fields.is_empty());
        assert!(
            no_such_album
                .to_string()
                .contains("No matching occurrences")
        );

        let disjoint_track = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("iama.unk_28".into()),
                entity: Some("track:Tom Sawyer".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(matches!(
            disjoint_track.results,
            QueryResults::GroupedByValue { ref fields } if fields.is_empty()
        ));

        let genuine_typo = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("iama.typo_field".into()),
                entity: Some("album:Moving Pictures".into()),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(genuine_typo.contains("unknown field `typo_field`"));
    }

    #[test]
    fn boma_wildcard_and_hex_without_prefix_selectors() {
        let raw = build_synthetic_raw_library();

        let iama_boma_all = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("iama::boma.*".into()),
                include_zero: true,
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByValue { fields } = &iama_boma_all.results else {
            panic!("expected GroupedByValue");
        };
        assert!(fields.iter().any(|f| f.field_name == "unk_16_end"));
        let boma_004a = fields.iter().find(|f| f.field_name == "0x004A").unwrap();
        assert_eq!(boma_004a.offset_info, "3..=5 bytes");

        let all_parent_boma = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("*::boma.*".into()),
                include_zero: true,
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByValue {
            fields: star_fields,
        } = &all_parent_boma.results
        else {
            panic!("expected GroupedByValue");
        };
        assert!(star_fields.iter().any(|f| f.section == "itma::boma"));
        assert!(star_fields.iter().any(|f| f.section == "iama::boma"));

        let no_0x = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("iama::boma:4A".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByValue { fields: hex_fields } = &no_0x.results else {
            panic!("expected GroupedByValue");
        };
        assert_eq!(hex_fields.len(), 1);
        assert_eq!(hex_fields[0].matched_instances, 2);
    }

    #[test]
    fn artist_social_profile_unicode_and_display_details() {
        let raw = build_synthetic_raw_library();

        let album_entity = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                entity: Some("album:Moving Pictures".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByEntity { entities } = &album_entity.results else {
            panic!("expected GroupedByEntity");
        };
        assert!(
            entities[0]
                .fields
                .iter()
                .any(|f| f.section == "iama::boma" && f.field_name == "0x004A")
        );

        let artist_res = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                field: Some("ArtistItemHeader.unk_24".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let artist_text = artist_res.to_string();
        assert!(artist_text.contains("[ArtistItemHeader, ArtistItem]"));
        assert!(artist_text.contains("Rush"));

        let track_res = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                entity: Some("track:Tom Sawyer".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let track_text = track_res.to_string();
        assert!(track_text.contains("media="));
        assert!(track_text.contains("cloud="));
        assert!(track_text.contains("plays="));

        let profile_res = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                entity: Some("profile:ÜBER".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let QueryResults::GroupedByEntity { entities } = &profile_res.results else {
            panic!("expected GroupedByEntity");
        };
        assert_eq!(entities.len(), 1);
        assert!(matches!(
            &entities[0].entity,
            EntityContext::SocialProfile { name: Some(n), .. } if n == "Rush Überfan"
        ));

        let empty_err = query_unknowns(
            &raw,
            &QueryUnknownsOptions {
                entity: Some("album:".into()),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(empty_err.contains("missing a search query"));
    }
}
