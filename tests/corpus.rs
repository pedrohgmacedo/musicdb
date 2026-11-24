//! Integration tests against the private local real-library corpus (`MUSICDB_CORPUS_DIR`).
//!
//! Each test returns early when its fixture is not present on disk (e.g. in CI).

use std::path::{Path, PathBuf};

use musicdb::{
    CloudStatus, DistinguishedPlaylistKind, Library, SmartLimitUnit, SmartSelectionMethod,
};

/// Resolves a path inside the local test corpus directory (`MUSICDB_CORPUS_DIR` if set,
/// otherwise the parent workspace directory of `CARGO_MANIFEST_DIR`).
fn corpus_path(rel: &str) -> PathBuf {
    let base = std::env::var("MUSICDB_CORPUS_DIR").map_or_else(
        |_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("corpus")
        },
        PathBuf::from,
    );
    base.join(rel)
}

#[test]
fn test_snapshot_library_and_smart_playlists() {
    let snap =
        corpus_path("snapshots/2026-11-07-initial-bitflags/03_smart_playlists/Library.musicdb");
    if !snap.exists() {
        return;
    }
    let lib = Library::open(&snap).expect("open snapshot 03_smart_playlists");
    assert_eq!(lib.tracks.len(), 33891);
    assert_eq!(lib.albums.len(), 11806);
    assert_eq!(lib.artists.len(), 6480);

    let f1 = lib
        .playlist_by_name("TEST_SMART_FIELDS_1")
        .expect("TEST_SMART_FIELDS_1");
    assert!(f1.is_smart);
    let f2 = lib
        .playlist_by_name("TEST_SMART_FIELDS_2")
        .expect("TEST_SMART_FIELDS_2");
    assert!(f2.is_smart);
    let fav_songs = lib
        .playlist_by_name("Favourite Songs")
        .expect("Favourite Songs");
    assert_eq!(
        fav_songs.system_kind,
        DistinguishedPlaylistKind::FavoriteSongs
    );
    assert_eq!(fav_songs.system_kind.raw(), 61);
    let hidden_cloud = lib
        .playlist_by_name("Hidden Cloud PlaylistOnly Tracks")
        .expect("Hidden Cloud PlaylistOnly Tracks");
    assert_eq!(
        hidden_cloud.system_kind,
        DistinguishedPlaylistKind::HiddenCloudPlaylistOnlyTracks
    );
    let opts = lib
        .playlist_by_name("TEST_SMART_OPTIONS")
        .expect("TEST_SMART_OPTIONS");
    let cfg = opts.smart_playlist.as_ref().expect("smart_playlist config");
    assert!(!cfg.live_updating);
    assert!(cfg.match_rules);
    assert!(cfg.match_only_checked_items);
    let lim = cfg.limit.as_ref().expect("limit enabled");
    assert_eq!(lim.count, 40);
    assert_eq!(lim.unit, SmartLimitUnit::Megabytes);
    assert_eq!(lim.selection, SmartSelectionMethod::MostRecentlyPlayed);
}

#[test]
fn test_historical_bundles_and_sidecar_plists() {
    let bundle_2026 = corpus_path("library-data/2026-11-01/Apple Music Library.musiclibrary");
    if !bundle_2026.exists() {
        return;
    }
    let lib = Library::open(&bundle_2026).expect("open 2026-11-01 bundle");
    assert_eq!(lib.tracks.len(), 33852);
    assert_eq!(lib.albums.len(), 11777);
    assert_eq!(lib.artists.len(), 6465);
    assert_eq!(lib.playlists.len(), 239);

    let songs_vp = lib
        .metadata
        .songs_view_preferences
        .as_ref()
        .expect("Songs.plist loaded");
    assert_eq!(songs_vp.view_as.as_deref(), Some("List"));
    assert_eq!(songs_vp.sort_field.as_deref(), Some("CloudOrArtist"));
    assert_eq!(songs_vp.sort_ascending, Some(true));
    assert!(songs_vp.visible_columns.contains(&"Title".to_string()));
    assert!(songs_vp.visible_columns.contains(&"Artist".to_string()));

    let rush_studio = lib
        .playlist_by_id("120CFE21FE026F37")
        .expect("Rush: Studio playlist");
    let r_vp = rush_studio
        .view_preferences
        .as_ref()
        .expect("Playlist_120cfe21fe026f37.plist loaded");
    assert_eq!(r_vp.view_as.as_deref(), Some("Albums"));
    assert_eq!(r_vp.sort_field.as_deref(), Some("Album"));
}

#[test]
fn test_macos_monterey_and_sequoia_samples() {
    let monterey = corpus_path("libraries-music-samples/Monterey/SmallerLibrary.musicdb");
    if monterey.exists() {
        let lib = Library::open(&monterey).expect("open Monterey SmallerLibrary.musicdb");
        assert_eq!(lib.tracks.len(), 745);
        assert_eq!(lib.albums.len(), 606);
        assert_eq!(lib.artists.len(), 421);
        assert_eq!(lib.playlists.len(), 6);
        assert_eq!(lib.tracks[0].cloud_status, CloudStatus::LocalOnly);
    }

    let sequoia = corpus_path("libraries-music-samples/Sequoia/MiniLibraryV5.musicdb");
    if sequoia.exists() {
        let lib = Library::open(&sequoia).expect("open Sequoia MiniLibraryV5.musicdb");
        assert_eq!(lib.tracks.len(), 6);
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.artists.len(), 1);
        assert_eq!(lib.playlists.len(), 12);
        let genius = lib.playlist_by_name("Genius").expect("Genius playlist");
        assert_eq!(genius.system_kind, DistinguishedPlaylistKind::Genius);
        let tv = lib
            .playlist_by_name("Séries et films")
            .expect("Séries et films playlist");
        assert_eq!(tv.system_kind, DistinguishedPlaylistKind::TvAndMovies);
    }
}

#[test]
fn test_2026_11_23_offline_local_probes() {
    let snap = corpus_path(
        "snapshots/2026-11-23-offline-local-probes/03_local_video_and_lossless_audio/Apple Music Library.musiclibrary/Library.musicdb",
    );
    if !snap.exists() {
        return;
    }
    let raw =
        musicdb::RawLibraryFile::open(&snap).expect("parse 03_local_video_and_lossless_audio");
    let tracks = raw
        .sections
        .iter()
        .find_map(|s| match &s.child {
            musicdb::sections::HsmaChild::TrackList(tl) => Some(tl.tracks.as_slice()),
            _ => None,
        })
        .expect("TrackList section");

    // 1. itma[54] (has_asset_info_plist) is an exact bijection with boma 0x001D across all 33,980 tracks
    let mut with_1d = 0usize;
    for t in tracks {
        let has_1d = t.bomas.iter().any(|b| b.subtype == 0x001D);
        assert_eq!(
            t.header.has_asset_info_plist != 0,
            has_1d,
            "itma[54] mismatch on track {:016X}",
            t.header.track_persistent_id,
        );
        if has_1d {
            with_1d += 1;
        }
    }
    assert_eq!(with_1d, 15);

    // 2. Local 720p 29.97fps MP4 video: is_local_video == 1 and VideoInfo matches avcC/stts
    let local_vid = tracks
        .iter()
        .find(|t| t.header.track_persistent_id == 0xAC4A_A560_6D9B_880C)
        .expect("local 720p video track");
    let vid_num = local_vid
        .bomas
        .iter()
        .find_map(|b| match &b.payload {
            musicdb::sections::BomaPayload::TrackNumerics(n) => Some(n),
            _ => None,
        })
        .expect("local video TrackNumerics");
    assert_eq!(vid_num.is_local_video, 1);
    assert_eq!(vid_num.file_type_fourcc_str().as_deref(), Some("MP4 "));
    assert_eq!(vid_num.container_format_kind, 51);
    assert_eq!(vid_num.has_embedded_artwork, 128);

    let vi = local_vid
        .bomas
        .iter()
        .find_map(|b| match &b.payload {
            musicdb::sections::BomaPayload::VideoInfo(v) => Some(v),
            _ => None,
        })
        .expect("local video VideoInfo");
    assert_eq!((vi.width, vi.height), (1280, 720));
    assert_eq!(vi.avc_profile_idc, 100);
    assert_eq!(vi.avc_level_idc, 31);
    assert_eq!(vi.frame_rate_milli, 29_970);
    assert_eq!(vi.color_bit_depth, 24);

    // 3. ALAC (fmt=60), WAV (fmt=70), AIFF (fmt=70) format codes
    for (pid, expected_fcc, expected_fmt) in [
        (0xD98A_C926_FB1A_C81B, "M4A ", 60u16),
        (0x8B60_5B7E_8111_8CE6, "WAV ", 70u16),
        (0x989C_F90D_6A53_71D8, "AIFF", 70u16),
    ] {
        let track = tracks
            .iter()
            .find(|t| t.header.track_persistent_id == pid)
            .expect("local format probe track");
        let num = track
            .bomas
            .iter()
            .find_map(|b| match &b.payload {
                musicdb::sections::BomaPayload::TrackNumerics(n) => Some(n),
                _ => None,
            })
            .expect("local format TrackNumerics");
        assert_eq!(num.is_local_video, 0);
        assert_eq!(num.file_type_fourcc_str().as_deref(), Some(expected_fcc));
        assert_eq!(num.container_format_kind, expected_fmt);
    }
}
