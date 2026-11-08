pub mod analysis;
pub mod crypto;
pub mod error;
pub mod model;
pub mod sections;
pub mod sqlite;

pub use crypto::{
    DEFAULT_MUSICDB_KEY, compress_and_encrypt_bytes, decrypt_and_decompress_bytes, load_raw_musicdb,
};
pub use error::{MusicDbError, Result};
pub use model::{
    Album, Artist, CloudStatus, ContentRating, DistinguishedPlaylistKind, DownloadedStatus,
    EqualizerPreset, Library, LibraryMetadata, MediaKind, MediaKindFlags, Playlist, PlaylistKind,
    PlaylistShareMode, PlaylistViewPreferences, RelativeTimeUnit, SmartCondition, SmartConjunction,
    SmartField, SmartLimitUnit, SmartOperator, SmartOperatorFlags, SmartPlaylistConfig,
    SmartPlaylistLimit, SmartRuleGroup, SmartRuleNode, SmartRuleValue, SmartSelectionMethod,
    SocialProfile, SortMode, SortModeFlags, SuggestionFlag, SuggestionFlags, Track, TrackSortModes,
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
        let base = std::env::var("MUSICDB_CORPUS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf()
            });
        base.join(rel)
    }

    fn make_utf16le_boma(subtype: u32, text: &str) -> BomaSection {
        let raw_string_bytes: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let string_byte_len = raw_string_bytes.len() as u32;
        let total_size = 20 + 16 + string_byte_len;
        BomaSection {
            header_size: 20,
            total_size,
            subtype,
            unk_16_end: vec![0; 4],
            payload: BomaPayload::StringSection(BomaStringSection {
                encoding: 1,
                string_byte_len,
                parent_subtype_counter: 1,
                unk_12: 0,
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
                unk_24_end: vec![0; 32],
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
                unk_24_end: vec![0; 32],
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
        let rock = EqualizerPreset::parse("#!#116#!#");
        assert_eq!(rock.preset_id, Some(116));
        assert_eq!(rock.name, "Rock");

        let acoustic = EqualizerPreset::parse("#!#100#!#");
        assert_eq!(acoustic.preset_id, Some(100));
        assert_eq!(acoustic.name, "Acoustic");

        let custom = EqualizerPreset::parse("My Custom EQ");
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
            unk_60: 20,
            unk_64_flags: [2, 1, 1, 0],
            track_count: 1,
            playlist_count: 1,
            album_count: 1,
            artist_count: 1,
            max_crypt_size: 102_400,
            timezone_offset_seconds: -10_800,
            apple_store_account_id: 0,
            date_modified: 3_800_000_000,
            unk_104: 1,
            itunes_library_persistent_id: 0xABCD_0123_4567_89EF,
            unk_116: [0x64, 0, 0, 0],
            revision_hash: 0x1122_3344_5566_7788,
            file_size_mirror: 0,
            unk_132_160: [0; 28],
            extra_bytes: Vec::new(),
        };

        let mut inner_hfma = outer_header.clone();
        inner_hfma.track_count = 0;
        inner_hfma.playlist_count = 0;
        inner_hfma.album_count = 0;
        inner_hfma.artist_count = 0;
        inner_hfma.max_crypt_size = 0;

        let album_boma = make_utf16le_boma(0x012C, "Synthetic Album");
        let iama = IamaSection {
            header: IamaHeader {
                header_size: 140,
                total_size: 140 + album_boma.total_size,
                boma_count: 1,
                album_persistent_id: 0xAA00_0000_0000_0001,
                unk_24: 2,
                is_compilation: 0,
                unk_26_28: [0, 1],
                unk_28: 1,
                first_track_persistent_id: 0xDD00_0000_0000_0004,
                star_rating: 80,
                star_rating_inheritance: 1,
                suggestion_flag: 2,
                unk_44_64: [0; 20],
                album_persistent_id_2: 0xAA00_0000_0000_0001,
                unk_72_96: [0; 24],
                date_suggestion_flag_modified: 3_800_000_000,
                date_last_played: 3_800_000_000,
                apple_music_store_album_id: Some(1_440_000_001),
                apple_music_store_album_id_2: Some(1_440_000_001),
                date_cloud_synced: Some(3_800_000_000),
                date_released: Some(3_700_000_000),
                unk_tail: vec![0; 12],
            },
            bomas: vec![album_boma],
        };
        let lama = LamaSection {
            header: LamaHeader {
                header_size: 48,
                album_count: 1,
                unk_12_end: vec![0; 36],
            },
            albums: vec![iama],
        };

        let artist_boma = make_utf16le_boma(0x0190, "Synthetic Artist");
        let i_ama = IAmaSection {
            header: IAmaHeader {
                header_size: 132,
                total_size: 132 + artist_boma.total_size,
                boma_count: 1,
                artist_persistent_id: 0xBB00_0000_0000_0002,
                unk_24: 2,
                unk_28: 1,
                unk_32_52: [0; 20],
                apple_music_store_artist_id: 900_001,
                date_suggestion_flag_modified: 0,
                artwork_uuid: [0x11; 16],
                artist_persistent_id_2: 0xBB00_0000_0000_0002,
                unk_88_96: [0; 8],
                artwork_source_kind: 45,
                unk_100: 5,
                suggestion_flag: 2,
                unk_103: 1,
                artwork_size_kind: 1,
                artwork_image_format: 1,
                apple_music_store_artist_id_2: 900_001,
                date_cloud_synced: Some(3_800_000_000),
                unk_tail: vec![0; 8],
            },
            bomas: vec![artist_boma],
        };
        let l_ama = LAmaSection {
            header: LAmaHeader {
                header_size: 100,
                artist_count: 1,
                unk_12_end: vec![0; 88],
            },
            artists: vec![i_ama],
        };

        let num_local = TrackNumerics {
            global_counter: 10,
            subrecord_kind: 1,
            track_data_id: 0x9000_0001,
            unk_16: 1,
            unk_20_flags: [0, 1, 1, 0],
            unk_24_27: [0; 3],
            is_vbr: 0,
            is_local_subrecord: 1,
            is_itunes_matched: [0, 0],
            is_apple_music_catalog: 0,
            unk_35_60: [0; 25],
            sample_rate: 44100.0,
            file_type_fourcc: 0x4D34_4120, // "M4A "
            unk_68_file_type: 0,
            file_folder_count: 5,
            library_folder_count: 1,
            artwork_count: 1,
            container_format_kind: 51,
            has_embedded_artwork: -1,
            local_artwork_state: 2,
            artwork_total_size_bytes: 120_000,
            bit_rate: 256,
            date_added: 3_800_000_000,
            unk_96_108: [0; 12],
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
            unk_152: 0,
            duration_ms: 245_000,
            apple_music_store_album_id: 0,
            apple_music_store_artist_id: 0,
            unk_176_200: [0; 24],
            unk_200_shared_val: 0,
            unk_204: 0,
            apple_music_store_album_artist_id: 0,
            unk_216_256: [0; 40],
            gapless_total_samples: 10_804_500,
            store_genre_id: 21,
            unk_268_272: 0,
            gapless_last_eight_frames_offset: 0,
            unk_280_shared_val: 0,
            unk_284_288: 0,
            cloud_saga_id_1: 0,
            file_size: 8_123_456,
            unk_300: 0,
            store_track_adam_id: 0,
            storefront_id: 143_441,
            unk_316_320: 0,
            store_track_adam_id_2: 0,
            cloud_saga_id_2: 0,
            unk_336_340: 0,
            store_track_adam_id_3: 0,
            store_track_adam_id_4: 0,
            date_cloud_synced: 2,
            unk_360: 0,
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
                unk_16_end: vec![0; 4],
                payload: BomaPayload::TrackNumerics(Box::new(num_local)),
            },
            BomaSection {
                header_size: 20,
                total_size: 20 + 364,
                subtype: 0x0001,
                unk_16_end: vec![0; 4],
                payload: BomaPayload::TrackNumerics(Box::new(num_cloud)),
            },
            BomaSection {
                header_size: 20,
                total_size: 20 + 52,
                subtype: 0x0017,
                unk_16_end: vec![0; 4],
                payload: BomaPayload::TrackPlaysSkips(TrackPlaysSkips {
                    track_persistent_id: 0xDD00_0000_0000_0004,
                    date_last_played: 3_800_000_000,
                    play_count: 12,
                    true_play_count: 15,
                    date_first_played_utc: 3_790_000_000,
                    unk_24: 0,
                    date_last_skipped: 0,
                    skip_count: 1,
                    true_skip_count: 1,
                    unk_40: 0,
                    unk_44_52: [0; 8],
                }),
            },
            make_utf16le_boma(0x0002, "Synthetic Song"),
            make_utf16le_boma(0x0003, "Synthetic Album"),
            make_utf16le_boma(0x0004, "Synthetic Artist"),
            make_utf16le_boma(0x0005, "Progressive Rock"),
            make_utf16le_boma(0x0007, "#!#116#!#"),
        ];
        let itma_boma_bytes: u32 = track_bomas.iter().map(|b| b.total_size).sum();
        let itma = ItmaSection {
            header: ItmaHeader {
                header_size: 376,
                total_size: 376 + itma_boma_bytes,
                boma_count: track_bomas.len() as u32,
                track_persistent_id: 0xDD00_0000_0000_0004,
                global_counter: 9,
                unk_28_30: [0; 2],
                checkbox_skip_when_shuffling: 0,
                unk_31_played_flag: 1,
                unk_32_38: [0; 6],
                checkbox_album_is_compilation: 0,
                playlist_only: 0,
                unk_40_42: [0; 2],
                checkbox_disabled: 0,
                unk_43: 0,
                has_local_custom_lyrics: 0,
                unk_45: 0,
                has_unsynced_local_changes: 0,
                unk_47_49: [0; 2],
                checkbox_part_of_gapless_album: 1,
                checkbox_remember_playback_position: 0,
                checkbox_show_composer_in_all_views: 0,
                checkbox_use_work_and_movement: 0,
                has_work_and_movement_active: 0,
                unk_54: 0,
                has_playback_or_bounds_activity: 1,
                unk_56: 0,
                downloaded_status: 1,
                cloud_status: 8,
                content_rating: 0,
                unk_60_62: [0; 2],
                suggestion_flag: 0x0102,
                unk_64_store_flag: 1,
                star_rating: 100,
                auto_sort_flags: [0x81, 0x80, 0x03, 0, 0, 0],
                unk_72_82: [0; 10],
                bpm: 120,
                disc_number: 1,
                movement_total: 0,
                movement_number: 0,
                disc_total: 1,
                volume_adjustment: 0,
                unk_96_purchased_album_group: 0,
                unk_100_108: [0; 8],
                media_kind: 1,
                unk_112: 1,
                track_total: 8,
                unk_118_148: [0; 30],
                playback_start_ms: 0,
                playback_stop_ms: 245_000,
                unk_156: 0,
                track_number: 1,
                unk_162_168: [0; 6],
                year: 2026,
                album_persistent_id: 0xAA00_0000_0000_0001,
                artist_persistent_id: 0xBB00_0000_0000_0002,
                apple_music_store_artist_id: 900_001,
                unk_192_220: [0; 28],
                cloud_saga_id_1: 0,
                itunes_store_matched_id: 0,
                cloud_saga_id: 0,
                unk_244_shared_val: 0,
                unk_248: 0,
                unk_252_254: 0,
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
                unk_312_320: 0,
                cloud_saga_id_2: 0,
                unk_328: 3,
                unk_332: 0,
                date_suggestion_flag_modified: 3_800_000_000,
                unk_340: 0,
                artwork_size_kind: 1,
                artwork_image_format: 1,
                date_added_most_recent_track: 0,
                unk_356_376: [0; 20],
                extra_bytes: Vec::new(),
            },
            bomas: track_bomas,
        };
        let ltma = LtmaSection {
            header: LtmaHeader {
                header_size: 92,
                track_count: 1,
                unk_12_end: vec![0; 80],
            },
            tracks: vec![itma],
        };

        let raw_file = RawLibraryFile {
            outer_header,
            sections: vec![
                wrap_hsma(3, HsmaChild::InnerHfma(inner_hfma)),
                wrap_hsma(4, HsmaChild::AlbumList(lama)),
                wrap_hsma(5, HsmaChild::ArtistList(l_ama)),
                wrap_hsma(1, HsmaChild::TrackList(ltma)),
            ],
            trailing_bytes: Vec::new(),
        };

        // Test full encrypt + compress -> decrypt + decompress -> parse round-trip
        let encrypted_musicdb = raw_file.to_encrypted_bytes().expect("encrypt synthetic");
        let decrypted_raw =
            decrypt_and_decompress_bytes(&encrypted_musicdb, DEFAULT_MUSICDB_KEY)
                .expect("decrypt synthetic");
        let parsed_raw =
            RawLibraryFile::from_raw_bytes(&decrypted_raw).expect("parse synthetic");
        assert_eq!(parsed_raw.to_raw_bytes().unwrap(), decrypted_raw);

        let lib = Library::from_raw(&parsed_raw);
        assert_eq!(lib.tracks.len(), 1);
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.artists.len(), 1);

        let t = &lib.tracks[0];
        assert_eq!(t.title.as_deref(), Some("Synthetic Song"));
        assert_eq!(t.file_type_fourcc.as_deref(), Some("M4A "));
        assert_eq!(t.codec_fourcc.as_deref(), Some("mp4a"));
        assert_eq!(t.suggestion_flag, SuggestionFlag::Favorite);
        assert_eq!(
            t.equalizer_preset.as_ref().map(|e| e.name.as_str()),
            Some("Rock")
        );

        let tmp_sqlite = std::env::temp_dir().join("musicdb_synthetic_test.sqlite");
        let _ = std::fs::remove_file(&tmp_sqlite);
        lib.export_sqlite(&tmp_sqlite).expect("export_sqlite");
        let _ = std::fs::remove_file(&tmp_sqlite);
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
        let bundle_2026 =
            corpus_path("library-data/Apple Music Library.musiclibrary.2026-11-01");
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
}
