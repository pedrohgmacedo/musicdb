#![doc = include_str!("../README.md")]

pub mod crypto;
pub mod error;
/// High-level domain model for Apple Music libraries (`Library`, `Track`, `Album`, `Artist`, `Playlist`).
pub mod model;
pub mod sections;
#[cfg(feature = "sqlite")]
pub mod sqlite;

pub use crypto::{
    DEFAULT_MUSICDB_KEY, compress_and_encrypt_bytes, decrypt_and_decompress_bytes, load_raw_musicdb,
};
pub use error::{MusicDbError, Result};
pub use model::{
    Album, Artist, CloudStatus, ContentRating, DistinguishedPlaylistKind, DownloadedStatus,
    EqualizerPreset, IntoPersistentId, Library, LibraryMetadata, MediaKind, MediaKindFlags,
    ParsePersistentIdError, PersistentId, Playlist, PlaylistKind, PlaylistShareMode,
    PlaylistViewPreferences, RelativeTimeUnit, SmartCondition, SmartConjunction, SmartField,
    SmartLimitUnit, SmartOperator, SmartOperatorFlags, SmartPlaylistConfig, SmartPlaylistLimit,
    SmartRuleGroup, SmartRuleNode, SmartRuleValue, SmartSelectionMethod, SocialProfile, SortMode,
    SortModeFlags, SuggestionFlag, SuggestionFlags, Track, TrackSortModes,
    discover_default_library_path,
};
pub use sections::RawLibraryFile;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{apple_timestamp_to_utc, format_persistent_id};
    use crate::sections::*;
    use std::path::{Path, PathBuf};

    /// Resolves a path inside the local test corpus directory (`MUSICDB_CORPUS_DIR` if set,
    /// otherwise the parent workspace directory of `CARGO_MANIFEST_DIR`).
    fn corpus_path(rel: &str) -> PathBuf {
        let base = std::env::var("MUSICDB_CORPUS_DIR").map_or_else(
            |_| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf()
            },
            PathBuf::from,
        );
        base.join(rel)
    }

    fn make_utf16le_boma(subtype: u32, text: &str) -> BomaSection {
        let raw_string_bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let string_byte_len = u32::try_from(raw_string_bytes.len()).unwrap();
        let total_size = 20 + 16 + string_byte_len;
        BomaSection {
            header_size: 20,
            total_size,
            subtype,
            unk_16_end: Unknown(vec![0; 4]),
            payload: BomaPayload::StringSection(BomaStringSection {
                encoding: 1,
                string_byte_len,
                parent_subtype_counter: 1,
                unk_12: Unknown(0),
                text: text.to_string(),
                raw_string_bytes,
            }),
        }
    }

    fn wrap_hsma(subtype: u32, child: HsmaChild) -> HsmaSection {
        let mut cur = std::io::Cursor::new(Vec::new());
        let dummy = HsmaSection {
            header: HsmaHeader {
                header_size: 56,
                total_size: 0,
                subtype,
                total_size_64: 0,
                unk_24_end: Unknown(vec![0; 32]),
            },
            child,
        };
        binrw::BinWrite::write_le(&dummy, &mut cur).unwrap();
        let total_bytes = cur.into_inner().len() as u64;
        HsmaSection {
            header: HsmaHeader {
                header_size: 56,
                total_size: 0,
                subtype,
                total_size_64: total_bytes,
                unk_24_end: Unknown(vec![0; 32]),
            },
            child: dummy.child,
        }
    }

    #[test]
    fn test_apple_timestamp_and_persistent_id() {
        assert_eq!(apple_timestamp_to_utc(0), None);
        // 2_082_844_800 is 1970-01-01T00:00:00Z in Apple HFS+ epoch (1904-01-01)
        let epoch = apple_timestamp_to_utc(2_082_844_800).unwrap();
        assert_eq!(epoch.timestamp(), 0);
        assert_eq!(
            format_persistent_id(0x0123_4567_89AB_CDEF),
            "0123456789ABCDEF"
        );
    }

    #[test]
    fn test_equalizer_preset_decoding() {
        let rock = EqualizerPreset::from("#!#116#!#");
        assert_eq!(rock.preset_id, Some(116));
        assert_eq!(rock.name, "Rock");

        let acoustic = EqualizerPreset::from("#!#100#!#");
        assert_eq!(acoustic.preset_id, Some(100));
        assert_eq!(acoustic.name, "Acoustic");

        let custom = EqualizerPreset::from("My Custom EQ");
        assert_eq!(custom.preset_id, None);
        assert_eq!(custom.name, "My Custom EQ");
    }

    #[test]
    fn test_suggestion_flag_and_smart_mappings() {
        assert_eq!(SuggestionFlag::from(0x0000), SuggestionFlag::Neutral);
        assert_eq!(SuggestionFlag::from(0x0001), SuggestionFlag::Removed);
        assert_eq!(SuggestionFlag::from(0x0002), SuggestionFlag::Favorite);
        assert_eq!(SuggestionFlag::from(0x0003), SuggestionFlag::SuggestLess);
        assert_eq!(SuggestionFlag::from(0x0101), SuggestionFlag::SuggestLess);
        assert_eq!(SuggestionFlag::from(0x0102), SuggestionFlag::Favorite);
        assert!(SuggestionFlag::is_locally_modified(0x0102));
        assert!(!SuggestionFlag::is_locally_modified(0x0002));

        assert_eq!(SmartField::from(0x9F), SmartField::Work);
        assert_eq!(SmartField::from(0x5A), SmartField::AlbumRating);
        assert_eq!(SmartOperator::from(0x0000_0400), SmartOperator::Is);
        assert_eq!(SmartOperator::from(0x0200_0400), SmartOperator::IsNot);

        let mk_flags = MediaKind::flags(0x0010_0021);
        assert!(mk_flags.contains(MediaKindFlags::MUSIC));
        assert!(mk_flags.contains(MediaKindFlags::MUSIC_VIDEO));
        assert!(mk_flags.contains(MediaKindFlags::VOICE_MEMO));
        assert_eq!(MediaKind::from(0x0000_0042), MediaKind::MovieOrTvShow);

        let op_flags = SmartOperator::flags(0x0300_0002);
        assert!(op_flags.contains(SmartOperatorFlags::CONTAINS));
        assert!(op_flags.contains(SmartOperatorFlags::STRING_MODE));
        assert!(op_flags.contains(SmartOperatorFlags::NEGATE));

        assert_eq!(SortMode::from(0x80), SortMode::AutoGenerated);
        assert_eq!(SortMode::from(0x81), SortMode::AutoGenerated);
        assert_eq!(SortMode::from(0x01), SortMode::CustomUserOverride);
        assert_eq!(SortMode::from(0x03), SortMode::CustomUserOverride);
        assert!(SortMode::flags(0x81).contains(SortModeFlags::AUTO_GENERATED));
        assert!(SortMode::flags(0x81).contains(SortModeFlags::HAS_SORT_STRING));
    }

    #[expect(
        clippy::too_many_lines,
        reason = "synthetic binary fixture builder covering all section types"
    )]
    #[test]
    fn test_synthetic_musicdb_roundtrip_and_sqlite_export() {
        let mut version_bytes = [0u8; 32];
        version_bytes[..9].copy_from_slice(b"1.7.0.156");
        let outer_header = HfmaHeader {
            header_size: 160,
            file_size: 0,
            major_version: 23,
            minor_version: 1,
            version_bytes,
            library_persistent_id: 0xABCD_0123_4567_89EF,
            musicdb_file_type: 9,
            unk_60: Unknown(20),
            unk_64_flags: Unknown([2, 1, 1, 0]),
            track_count: 1,
            playlist_count: 1,
            album_count: 1,
            artist_count: 1,
            max_crypt_size: 102_400,
            timezone_offset_seconds: -10_800,
            apple_store_account_id: 0,
            date_modified: 3_800_000_000,
            unk_104: Unknown(1),
            itunes_library_persistent_id: 0xABCD_0123_4567_89EF,
            unk_116: Unknown([0x64, 0, 0, 0]),
            revision_hash: 0x1122_3344_5566_7788,
            file_size_mirror: 0,
            unk_132_160: Unknown([0; 28]),
            extra_bytes: Unknown(Vec::new()),
        };

        let mut inner_hfma = outer_header.clone();
        inner_hfma.track_count = 0;
        inner_hfma.playlist_count = 0;
        inner_hfma.album_count = 0;
        inner_hfma.artist_count = 0;
        inner_hfma.max_crypt_size = 0;

        let album_boma = make_utf16le_boma(0x012C, "Synthetic Album");
        let album_item = AlbumItem {
            header: AlbumItemHeader {
                header_size: 140,
                total_size: 140 + album_boma.total_size,
                boma_count: 1,
                album_persistent_id: 0xAA00_0000_0000_0001,
                unk_24: Unknown(2),
                is_compilation: 0,
                playlist_only: 0,
                unk_27: Unknown(1),
                unk_28: Unknown(1),
                first_track_persistent_id: 0xDD00_0000_0000_0004,
                star_rating: 80,
                star_rating_inheritance: 1,
                suggestion_flag: 2,
                unk_44_64: Unknown([0; 20]),
                album_persistent_id_2: 0xAA00_0000_0000_0001,
                unk_72_96: Unknown([0; 24]),
                date_suggestion_flag_modified: 3_800_000_000,
                date_last_played: 3_800_000_000,
                apple_music_store_album_id: Some(1_440_000_001),
                apple_music_store_album_id_2: Some(1_440_000_001),
                date_cloud_synced: Some(3_800_000_000),
                date_released: Some(3_700_000_000),
                unk_tail: Unknown(vec![0; 12]),
            },
            bomas: vec![album_boma],
        };
        let album_list = AlbumListSection {
            header: AlbumListHeader {
                header_size: 48,
                album_count: 1,
                unk_12_end: Unknown(vec![0; 36]),
            },
            albums: vec![album_item],
        };

        let artist_boma = make_utf16le_boma(0x0190, "Synthetic Artist");
        let artist_item = ArtistItem {
            header: ArtistItemHeader {
                header_size: 132,
                total_size: 132 + artist_boma.total_size,
                boma_count: 1,
                artist_persistent_id: 0xBB00_0000_0000_0002,
                unk_24: Unknown(2),
                unk_28: Unknown(1),
                unk_32_52: Unknown([0; 20]),
                apple_music_store_artist_id: 900_001,
                date_suggestion_flag_modified: 0,
                artwork_uuid: [0x11; 16],
                artist_persistent_id_2: 0xBB00_0000_0000_0002,
                unk_88_96: Unknown([0; 8]),
                artwork_source_kind: 45,
                unk_100: Unknown(5),
                suggestion_flag: 2,
                unk_103: Unknown(1),
                artwork_size_kind: 1,
                artwork_image_format: 1,
                apple_music_store_artist_id_2: 900_001,
                date_cloud_synced: Some(3_800_000_000),
                unk_tail: Unknown(vec![0; 8]),
            },
            bomas: vec![artist_boma],
        };
        let artist_list = ArtistListSection {
            header: ArtistListHeader {
                header_size: 100,
                artist_count: 1,
                unk_12_end: Unknown(vec![0; 88]),
            },
            artists: vec![artist_item],
        };

        let num_local = TrackNumerics {
            global_counter: 10,
            subrecord_kind: 1,
            track_data_id: 0x9000_0001,
            unk_16: Unknown::new(1),
            unk_20_flags: Unknown::new([0, 1, 1, 0]),
            unk_24_27: Unknown::new([0; 3]),
            is_vbr: 0,
            is_local_subrecord: 1,
            is_itunes_matched: [0, 0],
            is_apple_music_catalog: 0,
            unk_35_60: Unknown::new([0; 25]),
            sample_rate: 44100.0,
            file_type_fourcc: 0x4D34_4120, // "M4A "
            unk_68_file_type: Unknown::new(0),
            file_folder_count: 5,
            library_folder_count: 1,
            artwork_count: 1,
            container_format_kind: 51,
            has_embedded_artwork: -1,
            local_artwork_state: 2,
            artwork_total_size_bytes: 120_000,
            bit_rate: 256,
            date_added: 3_800_000_000,
            unk_96_108: Unknown::new([0; 12]),
            gapless_encoder_delay: 2112,
            gapless_encoder_padding: 576,
            gapless_heuristic_info: 0,
            gapless_playback_flags: 1,
            custom_lyrics_hash: 0,
            date_modified: 3_800_000_000,
            normalization: 1500,
            date_purchased: 0,
            date_released: 3_700_000_000,
            cloud_codec_kind: 0,
            codec_fourcc: 0,
            unk_152: Unknown::new(0),
            duration_ms: 245_000,
            apple_music_store_album_id: 0,
            apple_music_store_artist_id: 0,
            unk_176_200: Unknown::new([0; 24]),
            unk_200_shared_val: Unknown::new(0),
            unk_204: Unknown::new(0),
            apple_music_store_album_artist_id: 0,
            unk_216_256: Unknown::new([0; 40]),
            gapless_total_samples: 10_804_500,
            store_genre_id: 21,
            unk_268_272: Unknown::new(0),
            gapless_last_eight_frames_offset: 0,
            unk_280_shared_val: Unknown::new(0),
            unk_284_288: Unknown::new(0),
            cloud_saga_id_1: 0,
            file_size: 8_123_456,
            unk_300: Unknown::new(0),
            store_track_adam_id: 0,
            storefront_id: 143_441,
            unk_316_320: Unknown::new(0),
            store_track_adam_id_2: 0,
            cloud_saga_id_2: 0,
            unk_336_340: Unknown::new(0),
            store_track_adam_id_3: 0,
            store_track_adam_id_4: 0,
            date_cloud_synced: 2,
            unk_360: Unknown::new(0),
        };
        let mut num_cloud = num_local.clone();
        num_cloud.global_counter = 11;
        num_cloud.subrecord_kind = 3;
        num_cloud.track_data_id = 0x9000_0002;
        num_cloud.is_local_subrecord = 0;
        num_cloud.local_artwork_state = 0;
        num_cloud.is_apple_music_catalog = 1;
        num_cloud.cloud_codec_kind = 2;
        num_cloud.codec_fourcc = 0x6D70_3461; // "mp4a"
        num_cloud.date_cloud_synced = 3_800_000_000;

        let track_bomas = vec![
            BomaSection {
                header_size: 20,
                total_size: 20 + 364,
                subtype: 0x0001,
                unk_16_end: Unknown::new(vec![0; 4]),
                payload: BomaPayload::TrackNumerics(Box::new(num_local)),
            },
            BomaSection {
                header_size: 20,
                total_size: 20 + 364,
                subtype: 0x0001,
                unk_16_end: Unknown::new(vec![0; 4]),
                payload: BomaPayload::TrackNumerics(Box::new(num_cloud)),
            },
            BomaSection {
                header_size: 20,
                total_size: 20 + 52,
                subtype: 0x0017,
                unk_16_end: Unknown::new(vec![0; 4]),
                payload: BomaPayload::TrackPlaysSkips(TrackPlaysSkips {
                    track_persistent_id: 0xDD00_0000_0000_0004,
                    date_last_played: 3_800_000_000,
                    play_count: 12,
                    true_play_count: 15,
                    date_first_played_utc: 3_790_000_000,
                    unk_24: Unknown::new(0),
                    date_last_skipped: 0,
                    skip_count: 1,
                    true_skip_count: 1,
                    unk_40: Unknown::new(0),
                    unk_44_52: Unknown::new([0; 8]),
                }),
            },
            make_utf16le_boma(0x0002, "Synthetic Song"),
            make_utf16le_boma(0x0003, "Synthetic Album"),
            make_utf16le_boma(0x0004, "Synthetic Artist"),
            make_utf16le_boma(0x0005, "Progressive Rock"),
            make_utf16le_boma(0x0007, "#!#116#!#"),
        ];
        let itma_boma_bytes: u32 = track_bomas.iter().map(|b| b.total_size).sum();
        let track_item = TrackItem {
            header: TrackItemHeader {
                header_size: 376,
                total_size: 376 + itma_boma_bytes,
                boma_count: u32::try_from(track_bomas.len()).unwrap(),
                track_persistent_id: 0xDD00_0000_0000_0004,
                global_counter: 9,
                unk_28_30: Unknown::new([0; 2]),
                checkbox_skip_when_shuffling: 0,
                unk_31_played_flag: Unknown::new(1),
                unk_32_38: Unknown::new([0; 6]),
                checkbox_album_is_compilation: 0,
                playlist_only: 0,
                unk_40_42: Unknown::new([0; 2]),
                checkbox_disabled: 0,
                unk_43: Unknown::new(0),
                has_local_custom_lyrics: 0,
                unk_45: Unknown::new(0),
                has_unsynced_local_changes: 0,
                unk_47_49: Unknown::new([0; 2]),
                checkbox_part_of_gapless_album: 1,
                checkbox_remember_playback_position: 0,
                checkbox_show_composer_in_all_views: 0,
                checkbox_use_work_and_movement: 0,
                has_work_and_movement_active: 0,
                unk_54: Unknown::new(0),
                has_playback_or_bounds_activity: 1,
                unk_56: Unknown::new(0),
                downloaded_status: 1,
                cloud_status: 8,
                content_rating: 0,
                unk_60_62: Unknown::new([0; 2]),
                suggestion_flag: 0x0102,
                unk_64_store_flag: Unknown::new(1),
                star_rating: 100,
                auto_sort_flags: [0x81, 0x80, 0x03, 0, 0, 0],
                unk_72_82: Unknown::new([0; 10]),
                bpm: 120,
                disc_number: 1,
                movement_total: 0,
                movement_number: 0,
                disc_total: 1,
                volume_adjustment: 0,
                unk_96_purchased_album_group: Unknown::new(0),
                unk_100_108: Unknown::new([0; 8]),
                media_kind: 1,
                unk_112: Unknown::new(1),
                track_total: 8,
                unk_118_148: Unknown::new([0; 30]),
                playback_start_ms: 0,
                playback_stop_ms: 245_000,
                unk_156: Unknown::new(0),
                track_number: 1,
                unk_162_168: Unknown::new([0; 6]),
                year: 2026,
                album_persistent_id: 0xAA00_0000_0000_0001,
                artist_persistent_id: 0xBB00_0000_0000_0002,
                apple_music_store_artist_id: 900_001,
                unk_192_220: Unknown::new([0; 28]),
                cloud_saga_id_1: 0,
                itunes_store_matched_id: 0,
                cloud_saga_id: 0,
                unk_244_shared_val: Unknown::new(0),
                unk_248: Unknown::new(0),
                unk_252_254: Unknown::new(0),
                cloud_sync_state: 2,
                has_custom_lyrics: 0,
                artwork_uuid: [0x22; 16],
                track_persistent_id_2: 0xDD00_0000_0000_0004,
                title_sort_order: 1000,
                album_sort_order: 1000,
                artist_sort_order: 1000,
                genre_sort_order: 1000,
                composer_sort_order: 1000,
                album_artist_sort_order: 1000,
                album_artist_or_artist_sort_order: 1000,
                artwork_source_kind: 12,
                unk_312_320: Unknown::new(0),
                cloud_saga_id_2: 0,
                unk_328: Unknown::new(3),
                unk_332: Unknown::new(0),
                date_suggestion_flag_modified: 3_800_000_000,
                unk_340: Unknown::new(0),
                artwork_size_kind: 1,
                artwork_image_format: 1,
                date_added_most_recent_track: 0,
                unk_356_376: Unknown::new([0; 20]),
                extra_bytes: Unknown::new(Vec::new()),
            },
            bomas: track_bomas,
        };
        let track_list = TrackListSection {
            header: TrackListHeader {
                header_size: 92,
                track_count: 1,
                unk_12_end: Unknown::new(vec![0; 80]),
            },
            tracks: vec![track_item],
        };

        let raw_file = RawLibraryFile {
            outer_header,
            sections: vec![
                wrap_hsma(3, HsmaChild::InnerHfma(inner_hfma)),
                wrap_hsma(4, HsmaChild::AlbumList(album_list)),
                wrap_hsma(5, HsmaChild::ArtistList(artist_list)),
                wrap_hsma(1, HsmaChild::TrackList(track_list)),
            ],
            trailing_bytes: Vec::new(),
        };

        // Test full encrypt + compress -> decrypt + decompress -> parse round-trip
        let encrypted_musicdb = raw_file.to_encrypted_bytes().expect("encrypt synthetic");
        let decrypted_raw = decrypt_and_decompress_bytes(&encrypted_musicdb, DEFAULT_MUSICDB_KEY)
            .expect("decrypt synthetic");
        let parsed_raw = RawLibraryFile::from_raw_bytes(&decrypted_raw).expect("parse synthetic");
        assert_eq!(parsed_raw.to_raw_bytes().unwrap(), decrypted_raw);

        // Verify filtered parsing preserves 100% lossless round-trip via HsmaChild::Skipped
        let parsed_filtered =
            RawLibraryFile::from_raw_bytes_filtered(&decrypted_raw, |subtype| subtype != 1)
                .expect("parse synthetic filtered");
        assert!(
            parsed_filtered
                .sections
                .iter()
                .any(|s| matches!(s.child, HsmaChild::Skipped { subtype: 1, .. }))
        );
        assert_eq!(parsed_filtered.to_raw_bytes().unwrap(), decrypted_raw);
        assert_eq!(
            parsed_filtered.to_encrypted_bytes().unwrap(),
            encrypted_musicdb
        );

        let lib = Library::from(&parsed_raw);
        assert_eq!(lib.tracks.len(), 1);
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.artists.len(), 1);

        let t = &lib.tracks[0];
        assert_eq!(t.title.as_deref(), Some("Synthetic Song"));
        assert_eq!(t.file_type_fourcc.as_deref(), Some("M4A "));
        assert_eq!(t.codec_fourcc.as_deref(), Some("mp4a"));
        assert_eq!(t.suggestion_flag, SuggestionFlag::Favorite);
        assert_eq!(t.has_embedded_artwork, Some(true));
        assert!(t.purchased);
        assert_eq!(
            t.equalizer_preset.as_ref().map(|e| e.name.as_str()),
            Some("Rock")
        );

        #[cfg(feature = "sqlite")]
        {
            let tmp_dir = tempfile::tempdir().expect("create tempdir");
            let tmp_sqlite = tmp_dir.path().join("musicdb_synthetic_test.sqlite");
            lib.export_sqlite(&tmp_sqlite).expect("export_sqlite");
            assert!(tmp_sqlite.is_file());
        }
    }

    #[test]
    fn test_snapshot_library_and_smart_playlists() {
        let snap = corpus_path("snapshots/03_smart_playlists/Library.musicdb");
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
        let bundle_2026 = corpus_path("library-data/Apple Music Library.musiclibrary.2026-11-01");
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
        let monterey = corpus_path(
            "existing-tools/AppMusicLibParser/libraries-music-samples/Monterey/SmallerLibrary.musicdb",
        );
        if monterey.exists() {
            let lib = Library::open(&monterey).expect("open Monterey SmallerLibrary.musicdb");
            assert_eq!(lib.tracks.len(), 745);
            assert_eq!(lib.albums.len(), 606);
            assert_eq!(lib.artists.len(), 421);
            assert_eq!(lib.playlists.len(), 6);
            assert_eq!(lib.tracks[0].cloud_status, CloudStatus::LocalOnly);
        }

        let sequoia = corpus_path(
            "existing-tools/AppMusicLibParser/libraries-music-samples/Sequoia/MiniLibraryV5.musicdb",
        );
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
    fn test_malformed_inputs_do_not_panic_or_abort() {
        // Probe 1: compress_and_encrypt_bytes with header_size = 5000 on a 160-byte buffer
        let mut bad_header = vec![0u8; 160];
        bad_header[0..4].copy_from_slice(b"hfma");
        bad_header[4..8].copy_from_slice(&5000u32.to_le_bytes());
        assert!(matches!(
            compress_and_encrypt_bytes(&bad_header, DEFAULT_MUSICDB_KEY),
            Err(MusicDbError::HeaderTooLarge {
                header_size: 5000,
                file_size: 160
            })
        ));

        // Probe 1b: header_size below minimum (< 92) in both decrypt and encrypt
        let mut small_header = vec![0u8; 160];
        small_header[0..4].copy_from_slice(b"hfma");
        small_header[4..8].copy_from_slice(&16u32.to_le_bytes());
        small_header[8..12].copy_from_slice(&160u32.to_le_bytes());
        assert!(matches!(
            decrypt_and_decompress_bytes(&small_header, DEFAULT_MUSICDB_KEY),
            Err(MusicDbError::InvalidHeaderSize {
                section: "hfma",
                header_size: 16,
                min_size: 92
            })
        ));
        assert!(matches!(
            compress_and_encrypt_bytes(&small_header, DEFAULT_MUSICDB_KEY),
            Err(MusicDbError::InvalidHeaderSize {
                section: "hfma",
                header_size: 16,
                min_size: 92
            })
        ));

        // Probe 2: RawLibraryFile::from_raw_bytes with hsma.total_size_64 = u64::MAX / 2
        let mut huge_hsma = vec![0u8; 160 + 56];
        huge_hsma[0..4].copy_from_slice(b"hfma");
        huge_hsma[4..8].copy_from_slice(&160u32.to_le_bytes());
        huge_hsma[160..164].copy_from_slice(b"hsma");
        huge_hsma[164..168].copy_from_slice(&56u32.to_le_bytes());
        huge_hsma[168..172].copy_from_slice(&0u32.to_le_bytes());
        huge_hsma[172..176].copy_from_slice(&1u32.to_le_bytes());
        huge_hsma[176..184].copy_from_slice(&(u64::MAX / 2).to_le_bytes());
        assert!(RawLibraryFile::from_raw_bytes(&huge_hsma).is_err());

        // Probe 3: HfmaHeader with header_size = 100 (< 160 minimum)
        let mut short_hfma = vec![0u8; 160];
        short_hfma[0..4].copy_from_slice(b"hfma");
        short_hfma[4..8].copy_from_slice(&100u32.to_le_bytes());
        assert!(RawLibraryFile::from_raw_bytes(&short_hfma).is_err());

        // Probe 4: ltma section claiming u32::MAX tracks inside a small hsma buffer
        let mut huge_track_count = vec![0u8; 160 + 56 + 92];
        huge_track_count[0..4].copy_from_slice(b"hfma");
        huge_track_count[4..8].copy_from_slice(&160u32.to_le_bytes());
        huge_track_count[160..164].copy_from_slice(b"hsma");
        huge_track_count[164..168].copy_from_slice(&56u32.to_le_bytes());
        huge_track_count[168..172].copy_from_slice(&(56u32 + 92u32).to_le_bytes());
        huge_track_count[172..176].copy_from_slice(&1u32.to_le_bytes());
        huge_track_count[216..220].copy_from_slice(b"ltma");
        huge_track_count[220..224].copy_from_slice(&92u32.to_le_bytes());
        huge_track_count[224..228].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(RawLibraryFile::from_raw_bytes(&huge_track_count).is_err());
    }

    struct SpanInvariantChecker {
        visited: usize,
        enforce_order: bool,
        min_start: usize,
        prev_end: usize,
    }

    impl SpanInvariantChecker {
        fn for_struct(magic_prefixed: bool) -> Self {
            Self {
                visited: 0,
                enforce_order: true,
                min_start: if magic_prefixed { 4 } else { 0 },
                prev_end: 0,
            }
        }

        fn for_tree() -> Self {
            Self {
                visited: 0,
                enforce_order: false,
                min_start: 0,
                prev_end: 0,
            }
        }
    }

    impl crate::sections::unknown::UnknownVisitor for SpanInvariantChecker {
        fn visit_unknown<T: crate::sections::unknown::UnknownValue>(
            &mut self,
            section: &str,
            field_name: &str,
            span: crate::sections::unknown::UnknownSpan,
            value: &Unknown<T>,
        ) {
            self.visited += 1;
            let start = span.start();
            assert!(
                start >= self.min_start,
                "span start {start} < {} for {section}.{field_name}",
                self.min_start
            );
            if self.enforce_order {
                assert!(
                    self.prev_end <= start,
                    "overlapping or out-of-order span for {section}.{field_name}: prev_end={}, start={start}",
                    self.prev_end
                );
            }
            match span {
                crate::sections::unknown::UnknownSpan::Fixed { start, end } => {
                    let expected_len = T::WIRE_LEN
                        .expect("Fixed span must only be used with fixed-width UnknownValue");
                    assert_eq!(
                        end - start,
                        expected_len,
                        "span length mismatch for {section}.{field_name}"
                    );
                    self.prev_end = end;
                }
                crate::sections::unknown::UnknownSpan::Tail { start, len } => {
                    assert!(
                        start > 0,
                        "tail span start should be > 0 for {section}.{field_name}"
                    );
                    self.prev_end = start.saturating_add(len);
                }
            }
            let _ = value.is_zero();
        }
    }

    fn check_struct_spans<V: crate::sections::unknown::VisitUnknowns>(
        item: &V,
        section: &str,
        magic_prefixed: bool,
    ) {
        let mut checker = SpanInvariantChecker::for_struct(magic_prefixed);
        item.visit_unknowns(section, &mut checker);
        assert!(checker.visited > 0, "expected unknown fields in {section}");
    }

    fn wrap_boma_section(subtype: u32, total_size: u32, payload: BomaPayload) -> BomaSection {
        BomaSection {
            header_size: 20,
            total_size,
            subtype,
            unk_16_end: Unknown::new(Vec::new()),
            payload,
        }
    }

    fn build_span_test_master_and_lists(outer_header: &HfmaHeader) -> Vec<HsmaSection> {
        use crate::sections::PaddingSection;
        use crate::sections::boma::BomaStringSection;
        use crate::sections::library_master::{
            LibraryMasterHeader, LibraryMasterSection, PlmaSub1F6, PlmaSub1FF,
        };
        use crate::sections::social::{
            SocialProfileItem, SocialProfileItemHeader, SocialProfileListHeader,
            SocialProfileListSection,
        };

        let str_boma = wrap_boma_section(
            0x0002,
            36,
            BomaPayload::StringSection(BomaStringSection {
                encoding: 2,
                string_byte_len: 0,
                parent_subtype_counter: 0,
                unk_12: Unknown::new(0),
                text: String::new(),
                raw_string_bytes: Vec::new(),
            }),
        );
        let hsma = |child| HsmaSection {
            header: HsmaHeader::default(),
            child,
        };
        vec![
            HsmaSection {
                header: HsmaHeader {
                    header_size: 56,
                    ..Default::default()
                },
                child: HsmaChild::InnerHfma(outer_header.clone()),
            },
            hsma(HsmaChild::LibraryMaster(LibraryMasterSection {
                header: LibraryMasterHeader {
                    header_size: 160,
                    ..Default::default()
                },
                bomas: vec![
                    wrap_boma_section(0x01F6, 20, BomaPayload::Plma1F6(PlmaSub1F6::default())),
                    wrap_boma_section(0x01FF, 44, BomaPayload::Plma1FF(PlmaSub1FF::default())),
                    str_boma,
                ],
            })),
            hsma(HsmaChild::AlbumList(AlbumListSection {
                header: AlbumListHeader::default(),
                albums: vec![AlbumItem {
                    header: AlbumItemHeader::default(),
                    bomas: Vec::new(),
                }],
            })),
            hsma(HsmaChild::ArtistList(ArtistListSection {
                header: ArtistListHeader::default(),
                artists: vec![ArtistItem {
                    header: ArtistItemHeader::default(),
                    bomas: Vec::new(),
                }],
            })),
            hsma(HsmaChild::SocialProfileList(SocialProfileListSection {
                header: SocialProfileListHeader::default(),
                profiles: vec![SocialProfileItem {
                    header: SocialProfileItemHeader::default(),
                    bomas: Vec::new(),
                }],
            })),
            hsma(HsmaChild::Padding(PaddingSection {
                section_size: 8,
                raw_bytes: Unknown::new(vec![0; 8]),
            })),
        ]
    }

    fn build_span_test_track_bomas() -> Vec<BomaSection> {
        vec![
            BomaSection {
                header_size: 20,
                total_size: 384,
                subtype: 0x0001,
                unk_16_end: Unknown::new(Vec::new()),
                payload: BomaPayload::TrackNumerics(Box::default()),
            },
            BomaSection {
                header_size: 20,
                total_size: 72,
                subtype: 0x0017,
                unk_16_end: Unknown::new(Vec::new()),
                payload: BomaPayload::TrackPlaysSkips(TrackPlaysSkips::default()),
            },
            BomaSection {
                header_size: 20,
                total_size: 104,
                subtype: 0x0024,
                unk_16_end: Unknown::new(Vec::new()),
                payload: BomaPayload::VideoInfo(Box::default()),
            },
        ]
    }

    fn build_span_test_tracks_and_playlists() -> Vec<HsmaSection> {
        use crate::sections::playlist::{
            CompactPlaylistTrackItem, PlaylistItem, PlaylistItemHeader, PlaylistListHeader,
            PlaylistListSection, PlaylistTrackItem, SlstSection, SmartPlaylistRule,
            SmartPlaylistRuleArguments, SmartRuleChild,
        };

        let lpma_header = PlaylistItemHeader {
            header_size: 368,
            unk_328_340: Some(Unknown::new([1; 12])),
            unk_348_356: Some(Unknown::new(2)),
            unk_360_368: Some(Unknown::new(3)),
            ..Default::default()
        };
        let slst = SlstSection {
            rule_count: 1,
            rules: vec![SmartPlaylistRule {
                field_id: 0,
                comparison_method: 0,
                unk_8: Unknown::new(1),
                unk_12_44: Unknown::new([0; 32]),
                unk_44: Unknown::new(0),
                unk_48: Unknown::new(0),
                child_size: 260,
                child: SmartRuleChild::Nested(Box::new(SlstSection {
                    rule_count: 1,
                    rules: vec![SmartPlaylistRule {
                        field_id: 2,
                        comparison_method: 1,
                        unk_8: Unknown::new(0),
                        unk_12_44: Unknown::new([0; 32]),
                        unk_44: Unknown::new(0),
                        unk_48: Unknown::new(0),
                        child_size: 68,
                        child: SmartRuleChild::Arguments(SmartPlaylistRuleArguments::default()),
                    }],
                    ..Default::default()
                })),
            }],
            ..Default::default()
        };
        let playlist_bomas = vec![
            wrap_boma_section(
                0x0064,
                132,
                BomaPayload::SmartPlaylistOptions(Box::default()),
            ),
            wrap_boma_section(0x0065, 156, BomaPayload::SmartPlaylistRules(Box::new(slst))),
            wrap_boma_section(
                0x00CE,
                88,
                BomaPayload::PlaylistItem(PlaylistTrackItem::default()),
            ),
            wrap_boma_section(
                0x00DA,
                52,
                BomaPayload::PlaylistItemCompact(CompactPlaylistTrackItem::default()),
            ),
        ];
        vec![
            HsmaSection {
                header: HsmaHeader::default(),
                child: HsmaChild::TrackList(TrackListSection {
                    header: TrackListHeader::default(),
                    tracks: vec![TrackItem {
                        header: TrackItemHeader::default(),
                        bomas: build_span_test_track_bomas(),
                    }],
                }),
            },
            HsmaSection {
                header: HsmaHeader::default(),
                child: HsmaChild::PlaylistList(PlaylistListSection {
                    header: PlaylistListHeader::default(),
                    playlists: vec![PlaylistItem {
                        header: lpma_header,
                        bomas: playlist_bomas,
                    }],
                }),
            },
        ]
    }

    fn build_full_span_test_file() -> RawLibraryFile {
        let outer_header = HfmaHeader {
            header_size: 164,
            extra_bytes: Unknown::new(vec![0x11, 0x22, 0x33, 0x44]),
            ..Default::default()
        };
        let mut sections = build_span_test_master_and_lists(&outer_header);
        sections.extend(build_span_test_tracks_and_playlists());
        RawLibraryFile {
            outer_header,
            sections,
            trailing_bytes: Vec::new(),
        }
    }

    fn check_payload_spans() {
        use crate::sections::boma::BomaStringSection;
        use crate::sections::library_master::{PlmaSub1F6, PlmaSub1FF};
        use crate::sections::playlist::{
            SmartPlaylistOptions, SmartPlaylistRule, SmartPlaylistRuleArguments, SmartRuleChild,
        };

        check_struct_spans(&TrackNumerics::default(), "TrackNumerics", false);
        check_struct_spans(&TrackPlaysSkips::default(), "TrackPlaysSkips", false);
        check_struct_spans(&VideoInfo::default(), "VideoInfo", false);
        check_struct_spans(
            &SmartPlaylistOptions::default(),
            "SmartPlaylistOptions",
            false,
        );
        check_struct_spans(&PlmaSub1F6::default(), "plma::0x01F6", false);
        check_struct_spans(&PlmaSub1FF::default(), "plma::0x01FF", false);
        check_struct_spans(
            &BomaStringSection {
                encoding: 2,
                string_byte_len: 0,
                parent_subtype_counter: 0,
                unk_12: Unknown::new(0),
                text: String::new(),
                raw_string_bytes: Vec::new(),
            },
            "boma::str",
            false,
        );
        check_struct_spans(
            &SmartPlaylistRule {
                field_id: 2,
                comparison_method: 1,
                unk_8: Unknown::new(0),
                unk_12_44: Unknown::new([0; 32]),
                unk_44: Unknown::new(0),
                unk_48: Unknown::new(0),
                child_size: 0,
                child: SmartRuleChild::Raw(Vec::new()),
            },
            "SmartPlaylistRule",
            false,
        );
        check_struct_spans(
            &SmartPlaylistRuleArguments::default(),
            "SmartPlaylistRuleArguments",
            false,
        );
    }

    #[test]
    fn test_unknown_span_invariants() {
        use crate::sections::PaddingSection;
        use crate::sections::library_master::LibraryMasterHeader;
        use crate::sections::playlist::{
            CompactPlaylistTrackItem, PlaylistItemHeader, PlaylistListHeader, PlaylistTrackItem,
            SlstSection,
        };
        use crate::sections::social::{SocialProfileItemHeader, SocialProfileListHeader};
        use crate::sections::unknown::VisitUnknowns;

        let raw = build_full_span_test_file();

        // Check strict per-struct ordering and start >= 4 on magic-prefixed headers.
        check_struct_spans(&raw.outer_header, "hfma", true);
        check_struct_spans(&HsmaHeader::default(), "hsma", true);
        check_struct_spans(&LibraryMasterHeader::default(), "plma", true);
        check_struct_spans(&AlbumListHeader::default(), "lama", true);
        check_struct_spans(&AlbumItemHeader::default(), "iama", true);
        check_struct_spans(&ArtistListHeader::default(), "lAma", true);
        check_struct_spans(&ArtistItemHeader::default(), "iAma", true);
        check_struct_spans(&TrackListHeader::default(), "ltma", true);
        check_struct_spans(&TrackItemHeader::default(), "itma", true);
        check_struct_spans(&PlaylistListHeader::default(), "lPma", true);
        let lpma_368 = PlaylistItemHeader {
            header_size: 368,
            unk_328_340: Some(Unknown::new([1; 12])),
            unk_348_356: Some(Unknown::new(2)),
            unk_360_368: Some(Unknown::new(3)),
            ..Default::default()
        };
        check_struct_spans(&lpma_368, "lpma", true);
        check_struct_spans(&SocialProfileListHeader::default(), "Lsma", true);
        check_struct_spans(&SocialProfileItemHeader::default(), "isma", true);
        check_struct_spans(&PlaylistTrackItem::default(), "ipfa", true);
        check_struct_spans(&CompactPlaylistTrackItem::default(), "IPfa", true);
        check_struct_spans(
            &PaddingSection {
                section_size: 8,
                raw_bytes: Unknown::new(vec![0; 4]),
            },
            "LPma",
            true,
        );
        check_struct_spans(&SlstSection::default(), "SLst", true);
        check_payload_spans();

        let mut tree_checker = SpanInvariantChecker::for_tree();
        raw.visit_unknowns("", &mut tree_checker);
        assert!(tree_checker.visited > 80);
    }

    #[test]
    fn test_smart_rule_be_unknown_roundtrip() {
        use std::io::Cursor;

        use binrw::{BinRead, BinWrite};

        use crate::sections::playlist::{
            SlstSection, SmartPlaylistRule, SmartPlaylistRuleArguments, SmartRuleChild,
        };
        use crate::sections::unknown::{
            Unknown, UnknownSpan, UnknownValue, UnknownVisitor, VisitUnknowns,
        };

        struct CountVisitor(usize);
        impl UnknownVisitor for CountVisitor {
            fn visit_unknown<T: UnknownValue>(
                &mut self,
                _section: &str,
                _field_name: &str,
                span: UnknownSpan,
                _value: &Unknown<T>,
            ) {
                self.0 += 1;
                if let UnknownSpan::Fixed { start, end } = span {
                    assert_eq!(end - start, T::WIRE_LEN.unwrap());
                }
            }
        }

        let rule = SmartPlaylistRule {
            field_id: 2,
            comparison_method: 1,
            unk_8: Unknown::new(0x1122_3344),
            unk_12_44: Unknown::new([0x55; 32]),
            unk_44: Unknown::new(0x6677_8899),
            unk_48: Unknown::new(0xAABB_CCDD),
            child_size: 68,
            child: SmartRuleChild::Arguments(SmartPlaylistRuleArguments {
                arg_0: 1,
                arg_1: 10,
                arg_2: 20,
                arg_3: 30,
                arg_4: 40,
                arg_5: 50,
                arg_6: 60,
                arg_7: 70,
                unk_64: Unknown::new(0xDEAD_BEEF),
            }),
        };

        let slst = SlstSection {
            unk_4: Unknown::new(0x0102_0304),
            rule_count: 1,
            conjunction: 0,
            unk_16_136: Unknown::new([0x07; 120]),
            rules: vec![rule],
        };

        let mut buf = Vec::new();
        slst.write_be(&mut Cursor::new(&mut buf))
            .expect("write_be SlstSection");
        let parsed = SlstSection::read_be(&mut Cursor::new(&buf)).expect("read_be SlstSection");
        assert_eq!(parsed, slst);
        let mut visitor = CountVisitor(0);
        parsed.visit_unknowns("slst", &mut visitor);
        assert_eq!(visitor.0, 7);
    }
}
