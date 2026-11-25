//! Normalized SQLite database exporter (`Library::export_sqlite`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, Transaction, params};

use crate::error::{MusicDbError, Result};
use crate::model::{Library, SmartRuleGroup, SmartRuleNode};

/// Reinterprets a 64-bit unsigned integer into SQLite's two's-complement 64-bit `INTEGER` (`i64`),
/// preserving sentinel bit patterns such as `0xFFFF_FFFF_FFFF_FFFD` (`-3` on Apple Music catalog `sagaId`s).
const fn u64_to_sqlite_i64(v: u64) -> i64 {
    i64::from_ne_bytes(v.to_ne_bytes())
}

fn temp_sibling_sqlite_path(target: &Path) -> PathBuf {
    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("library.sqlite");
    let pid = std::process::id();
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(".{file_name}.{pid}.{seq}.tmp"))
}

impl Library {
    /// Exports the entire [`Library`] domain model into a normalized, indexed SQLite database file at `path`.
    ///
    /// Writes to a temporary file in the destination directory first and atomically renames it to `path`
    /// on completion so an existing file is never left half-written on failure.
    ///
    /// # Errors
    /// Returns [`MusicDbError`] if creating, populating, or renaming the SQLite database fails.
    pub fn export_sqlite(&self, path: impl AsRef<Path>) -> Result<()> {
        let final_path = path.as_ref();
        let temp_path = temp_sibling_sqlite_path(final_path);
        // Best-effort cleanup of any leftover temporary file from an interrupted process with the same PID.
        let _ = fs::remove_file(&temp_path);

        let write_res = self.write_sqlite_to_path(&temp_path);
        if let Err(err) = write_res {
            let _ = fs::remove_file(&temp_path);
            return Err(err);
        }

        if let Err(err) = fs::rename(&temp_path, final_path) {
            let _ = fs::remove_file(&temp_path);
            return Err(MusicDbError::Io(err));
        }

        Ok(())
    }

    fn write_sqlite_to_path(&self, path: &Path) -> Result<()> {
        let mut conn = Connection::open(path)?;
        create_tables(&conn)?;

        let tx = conn.transaction()?;
        insert_library_metadata(&tx, self)?;
        insert_albums(&tx, self)?;
        insert_artists(&tx, self)?;
        insert_tracks(&tx, self)?;
        insert_playlists(&tx, self)?;
        insert_social_profiles(&tx, self)?;
        insert_pending_cloud_deletions(&tx, self)?;
        tx.commit()?;

        create_indexes_and_views(&conn)?;
        conn.close().map_err(|(_, err)| MusicDbError::Sqlite(err))?;
        Ok(())
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "declarative SQLite DDL schema definition for all 8 normalized tables"
)]
fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        PRAGMA synchronous = OFF;
        PRAGMA journal_mode = MEMORY;
        PRAGMA foreign_keys = OFF;

        CREATE TABLE library_metadata (
            persistent_id TEXT PRIMARY KEY,
            itunes_persistent_id TEXT,
            revision_hash TEXT NOT NULL,
            file_size_mirror INTEGER NOT NULL,
            app_version TEXT NOT NULL,
            format_major_version INTEGER NOT NULL,
            format_minor_version INTEGER NOT NULL,
            hsma_count INTEGER NOT NULL,
            declared_track_count INTEGER NOT NULL,
            declared_album_count INTEGER NOT NULL,
            declared_artist_count INTEGER NOT NULL,
            declared_playlist_count INTEGER NOT NULL,
            timezone_offset_seconds INTEGER NOT NULL,
            apple_store_account_id INTEGER,
            date_modified_utc TEXT,
            date_cloud_synced_utc TEXT,
            date_last_modified_utc TEXT,
            cloud_database_revision INTEGER,
            date_cloud_lease_expires_utc TEXT,
            date_local_lease_expires_utc TEXT,
            show_song_list_checkboxes INTEGER,
            keep_media_folder_organized INTEGER,
            copy_files_to_media_folder INTEGER,
            media_folder_uri TEXT,
            media_folder_path TEXT,
            imported_itl_path TEXT,
            songs_view_preferences_json TEXT
        );

        CREATE TABLE albums (
            persistent_id TEXT PRIMARY KEY,
            itunes_persistent_id TEXT,
            first_track_persistent_id TEXT,
            title TEXT,
            artist TEXT,
            album_artist TEXT,
            cloud_album_id TEXT,
            editorial_notes TEXT,
            catalog_genre TEXT,
            motion_artwork_url TEXT,
            motion_preview_frame_url TEXT,
            motion_artwork_token TEXT,
            is_compilation INTEGER NOT NULL,
            playlist_only INTEGER NOT NULL,
            catalog_album_type_raw INTEGER NOT NULL,
            catalog_album_type TEXT NOT NULL,
            star_rating INTEGER NOT NULL,
            stars INTEGER NOT NULL,
            star_rating_is_computed INTEGER NOT NULL,
            suggestion_flag TEXT NOT NULL,
            suggestion_locally_modified INTEGER NOT NULL,
            date_suggestion_flag_modified_utc TEXT,
            date_last_played_utc TEXT,
            date_cloud_synced_utc TEXT,
            date_released_utc TEXT,
            apple_music_store_album_id INTEGER
        );

        CREATE TABLE artists (
            persistent_id TEXT PRIMARY KEY,
            itunes_persistent_id TEXT,
            name TEXT,
            sort_name TEXT,
            cloud_artist_id TEXT,
            playlist_only INTEGER NOT NULL,
            suggestion_flag TEXT NOT NULL,
            suggestion_locally_modified INTEGER NOT NULL,
            date_suggestion_flag_modified_utc TEXT,
            date_cloud_synced_utc TEXT,
            apple_music_store_artist_id INTEGER,
            artwork_uuid TEXT,
            artwork_url TEXT
        );

        CREATE TABLE tracks (
            persistent_id TEXT PRIMARY KEY,
            title TEXT,
            album TEXT,
            artist TEXT,
            album_artist TEXT,
            composer TEXT,
            genre TEXT,
            store_genre_id INTEGER,
            kind TEXT,
            comments TEXT,
            grouping TEXT,
            work_name TEXT,
            movement_name TEXT,
            movement_number INTEGER NOT NULL,
            movement_total INTEGER NOT NULL,
            sort_title TEXT,
            sort_album TEXT,
            sort_artist TEXT,
            sort_album_artist TEXT,
            sort_composer TEXT,
            sort_show TEXT,
            sort_title_mode TEXT NOT NULL,
            sort_album_mode TEXT NOT NULL,
            sort_artist_mode TEXT NOT NULL,
            sort_album_artist_mode TEXT NOT NULL,
            sort_composer_mode TEXT NOT NULL,
            sort_show_mode TEXT NOT NULL,
            track_number INTEGER NOT NULL,
            track_total INTEGER NOT NULL,
            disc_number INTEGER NOT NULL,
            disc_total INTEGER NOT NULL,
            year INTEGER NOT NULL,
            bpm INTEGER NOT NULL,
            duration_ms INTEGER,
            file_size INTEGER,
            bit_rate INTEGER,
            is_vbr INTEGER,
            sample_rate REAL,
            file_type_fourcc TEXT,
            codec_fourcc TEXT,
            video_width INTEGER,
            video_height INTEGER,
            video_codec_fourcc TEXT,
            video_bit_rate_kbps INTEGER,
            video_avc_profile_idc INTEGER,
            video_avc_level_idc INTEGER,
            video_frame_rate_milli INTEGER,
            video_color_bit_depth INTEGER,
            volume_adjustment INTEGER NOT NULL,
            normalization INTEGER,
            gapless_encoder_delay INTEGER,
            gapless_encoder_padding INTEGER,
            gapless_total_samples INTEGER,
            gapless_last_eight_frames_offset INTEGER,
            equalizer_raw TEXT,
            equalizer_preset_id INTEGER,
            equalizer_name TEXT,
            star_rating INTEGER NOT NULL,
            stars INTEGER NOT NULL,
            suggestion_flag TEXT NOT NULL,
            suggestion_locally_modified INTEGER NOT NULL,
            date_suggestion_flag_modified_utc TEXT,
            disabled INTEGER NOT NULL,
            skip_when_shuffling INTEGER NOT NULL,
            has_ever_been_played INTEGER NOT NULL,
            remember_playback_position INTEGER NOT NULL,
            show_composer_in_all_views INTEGER NOT NULL,
            use_work_and_movement INTEGER NOT NULL,
            album_is_compilation INTEGER NOT NULL,
            part_of_gapless_album INTEGER NOT NULL,
            playlist_only INTEGER NOT NULL,
            purchased INTEGER NOT NULL,
            has_custom_lyrics INTEGER NOT NULL,
            has_local_custom_lyrics INTEGER NOT NULL,
            has_unsynced_local_changes INTEGER NOT NULL,
            custom_lyrics_hash INTEGER,
            has_catalog_synced_lyrics INTEGER,
            is_itunes_matched INTEGER,
            is_apple_music_catalog INTEGER,
            is_apple_digital_master INTEGER,
            is_cloud_unavailable INTEGER,
            is_available_in_catalog INTEGER,
            audio_traits_raw INTEGER NOT NULL,
            audio_traits TEXT NOT NULL,
            downloaded_audio_traits_raw INTEGER NOT NULL,
            downloaded_audio_traits TEXT NOT NULL,
            purchaser_account_id INTEGER,
            drm_account_id INTEGER,
            cloud_saga_id INTEGER,
            store_track_adam_id INTEGER,
            apple_music_store_album_id INTEGER,
            apple_music_store_artist_id INTEGER,
            apple_music_store_album_artist_id INTEGER,
            storefront_id INTEGER,
            purchaser_email TEXT,
            purchaser_name TEXT,
            downloaded_status INTEGER NOT NULL,
            downloaded_state TEXT NOT NULL,
            cloud_status TEXT NOT NULL,
            cloud_status_raw INTEGER NOT NULL,
            cloud_album_id TEXT,
            cloud_artist_id TEXT,
            cloud_artwork_token TEXT,
            cloud_lyrics_token TEXT,
            content_rating INTEGER NOT NULL,
            content_advisory TEXT NOT NULL,
            media_kind INTEGER NOT NULL,
            media_type TEXT NOT NULL,
            playback_start_ms INTEGER NOT NULL,
            playback_stop_ms INTEGER NOT NULL,
            play_count INTEGER,
            true_play_count INTEGER,
            skip_count INTEGER,
            true_skip_count INTEGER,
            date_added_utc TEXT,
            date_modified_utc TEXT,
            date_last_played_utc TEXT,
            date_first_played_utc TEXT,
            date_last_skipped_utc TEXT,
            date_released_utc TEXT,
            date_purchased_utc TEXT,
            date_cloud_synced_utc TEXT,
            file_url TEXT,
            legacy_file_path TEXT,
            book_path TEXT,
            album_persistent_id TEXT,
            artist_persistent_id TEXT,
            artwork_uuid TEXT,
            artwork_count INTEGER,
            embedded_artwork_raw INTEGER,
            embedded_artwork TEXT,
            artwork_total_size_bytes INTEGER,
            file_folder_count INTEGER,
            library_folder_count INTEGER,
            isrc TEXT,
            copyright TEXT,
            extra_strings_json TEXT
        );

        CREATE TABLE playlists (
            persistent_id TEXT PRIMARY KEY,
            parent_persistent_id TEXT,
            name TEXT,
            description TEXT,
            kind TEXT NOT NULL,
            is_master INTEGER NOT NULL,
            is_folder INTEGER NOT NULL,
            is_smart INTEGER NOT NULL,
            is_subscribed INTEGER NOT NULL,
            show_in_profile_and_search INTEGER NOT NULL,
            distinguished_kind INTEGER NOT NULL,
            system_kind TEXT NOT NULL,
            suggestion_flag TEXT NOT NULL,
            suggestion_locally_modified INTEGER NOT NULL,
            date_suggestion_flag_modified_utc TEXT,
            date_created_utc TEXT,
            date_modified_utc TEXT,
            date_last_played_utc TEXT,
            date_cloud_published_utc TEXT,
            date_cloud_synced_utc TEXT,
            play_count INTEGER NOT NULL,
            true_play_count INTEGER NOT NULL,
            refresh_interval_seconds INTEGER NOT NULL,
            cloud_share_mode INTEGER NOT NULL,
            share_mode TEXT NOT NULL,
            artwork_cache_state INTEGER NOT NULL,
            cloud_saga_id INTEGER,
            universal_library_id TEXT,
            cloud_uuid TEXT,
            subscribed_container_url TEXT,
            owner_name TEXT,
            owner_id TEXT,
            author_social_profile_persistent_id TEXT,
            cloud_artwork_url TEXT,
            cloud_artwork_token TEXT,
            artwork_uuid TEXT,
            declared_track_count INTEGER NOT NULL,
            smart_live_updating INTEGER,
            smart_match_rules INTEGER,
            smart_match_only_checked_items INTEGER,
            smart_limit_count INTEGER,
            smart_limit_unit TEXT,
            smart_limit_selection TEXT,
            smart_conjunction TEXT,
            smart_playlist_json TEXT,
            view_as TEXT,
            sort_field TEXT,
            sort_ascending INTEGER,
            view_preferences_json TEXT
        );

        CREATE TABLE playlist_items (
            playlist_persistent_id TEXT NOT NULL,
            position INTEGER NOT NULL,
            track_persistent_id TEXT NOT NULL,
            PRIMARY KEY (playlist_persistent_id, position)
        );

        CREATE TABLE smart_playlist_rules (
            playlist_persistent_id TEXT NOT NULL,
            rule_index INTEGER NOT NULL,
            depth INTEGER NOT NULL,
            conjunction TEXT NOT NULL,
            group_negated INTEGER NOT NULL,
            field_id INTEGER NOT NULL,
            field TEXT NOT NULL,
            operator TEXT NOT NULL,
            comparison_raw INTEGER NOT NULL,
            description TEXT NOT NULL,
            value_json TEXT NOT NULL,
            PRIMARY KEY (playlist_persistent_id, rule_index)
        );

        CREATE TABLE social_profiles (
            persistent_id TEXT PRIMARY KEY,
            profile_id TEXT,
            name TEXT,
            handle TEXT,
            avatar_id TEXT,
            avatar_url TEXT,
            artwork_uuid TEXT
        );

        CREATE TABLE pending_cloud_deletions (
            cloud_saga_id INTEGER NOT NULL,
            deletion_kind_raw INTEGER NOT NULL,
            deletion_kind TEXT NOT NULL
        );
        ",
    )?;
    Ok(())
}

fn insert_library_metadata(tx: &Transaction<'_>, lib: &Library) -> Result<()> {
    let m = &lib.metadata;
    let songs_vp_json = m
        .songs_view_preferences
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    tx.execute(
        "INSERT INTO library_metadata (
            persistent_id, itunes_persistent_id, revision_hash, file_size_mirror,
            app_version, format_major_version, format_minor_version, hsma_count,
            declared_track_count, declared_album_count, declared_artist_count,
            declared_playlist_count, timezone_offset_seconds, apple_store_account_id,
            date_modified_utc, date_cloud_synced_utc, date_last_modified_utc,
            cloud_database_revision, date_cloud_lease_expires_utc,
            date_local_lease_expires_utc, show_song_list_checkboxes,
            keep_media_folder_organized, copy_files_to_media_folder,
            media_folder_uri, media_folder_path, imported_itl_path,
            songs_view_preferences_json
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27
        )",
        params![
            m.persistent_id.to_string(),
            m.itunes_persistent_id.map(|id| id.to_string()),
            m.revision_hash.to_string(),
            m.file_size_mirror,
            m.app_version,
            m.format_major_version,
            m.format_minor_version,
            m.hsma_count,
            m.declared_track_count,
            m.declared_album_count,
            m.declared_artist_count,
            m.declared_playlist_count,
            m.timezone_offset_seconds,
            m.apple_store_account_id.map(u64_to_sqlite_i64),
            m.date_modified.map(|d| d.to_string()),
            m.date_cloud_synced.map(|d| d.to_string()),
            m.date_last_modified.map(|d| d.to_string()),
            m.cloud_database_revision,
            m.date_cloud_lease_expires.map(|d| d.to_string()),
            m.date_local_lease_expires.map(|d| d.to_string()),
            m.show_song_list_checkboxes.map(i32::from),
            m.keep_media_folder_organized.map(i32::from),
            m.copy_files_to_media_folder.map(i32::from),
            m.media_folder_uri,
            m.media_folder_path,
            m.imported_itl_path,
            songs_vp_json,
        ],
    )?;
    Ok(())
}

fn insert_albums(tx: &Transaction<'_>, lib: &Library) -> Result<()> {
    let mut stmt = tx.prepare(
        "INSERT INTO albums (
            persistent_id, itunes_persistent_id, first_track_persistent_id,
            title, artist, album_artist, cloud_album_id, editorial_notes,
            catalog_genre, motion_artwork_url, motion_preview_frame_url,
            motion_artwork_token, is_compilation, playlist_only,
            catalog_album_type_raw, catalog_album_type, star_rating, stars,
            star_rating_is_computed, suggestion_flag, suggestion_locally_modified,
            date_suggestion_flag_modified_utc, date_last_played_utc,
            date_cloud_synced_utc, date_released_utc, apple_music_store_album_id
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26
        )",
    )?;
    for a in &lib.albums {
        stmt.execute(params![
            a.persistent_id.to_string(),
            a.itunes_persistent_id.map(|id| id.to_string()),
            a.first_track_persistent_id.map(|id| id.to_string()),
            a.title,
            a.artist,
            a.album_artist,
            a.cloud_album_id,
            a.editorial_notes,
            a.catalog_genre,
            a.motion_artwork_url,
            a.motion_preview_frame_url,
            a.motion_artwork_token,
            i32::from(a.is_compilation),
            i32::from(a.playlist_only),
            a.catalog_album_type.raw(),
            a.catalog_album_type.to_string(),
            a.star_rating,
            a.stars(),
            i32::from(a.star_rating_is_computed),
            a.suggestion_flag.to_string(),
            i32::from(a.suggestion_locally_modified),
            a.date_suggestion_flag_modified.map(|d| d.to_string()),
            a.date_last_played.map(|d| d.to_string()),
            a.date_cloud_synced.map(|d| d.to_string()),
            a.date_released.map(|d| d.to_string()),
            a.apple_music_store_album_id.map(u64_to_sqlite_i64),
        ])?;
    }
    Ok(())
}

fn insert_artists(tx: &Transaction<'_>, lib: &Library) -> Result<()> {
    let mut stmt = tx.prepare(
        "INSERT INTO artists (
            persistent_id, itunes_persistent_id, name, sort_name,
            cloud_artist_id, playlist_only, suggestion_flag,
            suggestion_locally_modified, date_suggestion_flag_modified_utc,
            date_cloud_synced_utc, apple_music_store_artist_id, artwork_uuid,
            artwork_url
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;
    for a in &lib.artists {
        stmt.execute(params![
            a.persistent_id.to_string(),
            a.itunes_persistent_id.map(|id| id.to_string()),
            a.name,
            a.sort_name,
            a.cloud_artist_id,
            i32::from(a.playlist_only),
            a.suggestion_flag.to_string(),
            i32::from(a.suggestion_locally_modified),
            a.date_suggestion_flag_modified.map(|d| d.to_string()),
            a.date_cloud_synced.map(|d| d.to_string()),
            a.apple_music_store_artist_id.map(u64_to_sqlite_i64),
            a.artwork_uuid,
            a.artwork_url,
        ])?;
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "full-row 137-column SQLite track binding"
)]
fn insert_tracks(tx: &Transaction<'_>, lib: &Library) -> Result<()> {
    let placeholders: Vec<String> = (1..=137).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "INSERT INTO tracks (
            persistent_id, title, album, artist, album_artist, composer, genre,
            store_genre_id, kind, comments, grouping, work_name, movement_name,
            movement_number, movement_total, sort_title, sort_album, sort_artist,
            sort_album_artist, sort_composer, sort_show, sort_title_mode,
            sort_album_mode, sort_artist_mode, sort_album_artist_mode,
            sort_composer_mode, sort_show_mode, track_number, track_total,
            disc_number, disc_total, year, bpm, duration_ms, file_size, bit_rate,
            is_vbr, sample_rate, file_type_fourcc, codec_fourcc, video_width,
            video_height, video_codec_fourcc, video_bit_rate_kbps,
            video_avc_profile_idc, video_avc_level_idc, video_frame_rate_milli,
            video_color_bit_depth, volume_adjustment, normalization,
            gapless_encoder_delay, gapless_encoder_padding, gapless_total_samples,
            gapless_last_eight_frames_offset, equalizer_raw, equalizer_preset_id,
            equalizer_name, star_rating, stars, suggestion_flag,
            suggestion_locally_modified, date_suggestion_flag_modified_utc,
            disabled, skip_when_shuffling,
            has_ever_been_played, remember_playback_position,
            show_composer_in_all_views, use_work_and_movement,
            album_is_compilation, part_of_gapless_album, playlist_only, purchased,
            has_custom_lyrics, has_local_custom_lyrics,
            has_unsynced_local_changes, custom_lyrics_hash,
            has_catalog_synced_lyrics, is_itunes_matched,
            is_apple_music_catalog, is_apple_digital_master,
            is_cloud_unavailable, is_available_in_catalog, audio_traits_raw,
            audio_traits, downloaded_audio_traits_raw, downloaded_audio_traits,
            purchaser_account_id, drm_account_id, cloud_saga_id,
            store_track_adam_id, apple_music_store_album_id,
            apple_music_store_artist_id, apple_music_store_album_artist_id,
            storefront_id, purchaser_email, purchaser_name, downloaded_status,
            downloaded_state, cloud_status, cloud_status_raw, cloud_album_id,
            cloud_artist_id, cloud_artwork_token, cloud_lyrics_token,
            content_rating, content_advisory, media_kind, media_type,
            playback_start_ms, playback_stop_ms, play_count, true_play_count,
            skip_count, true_skip_count, date_added_utc, date_modified_utc,
            date_last_played_utc, date_first_played_utc, date_last_skipped_utc,
            date_released_utc, date_purchased_utc, date_cloud_synced_utc,
            file_url, legacy_file_path, book_path, album_persistent_id,
            artist_persistent_id, artwork_uuid, artwork_count,
            embedded_artwork_raw, embedded_artwork, artwork_total_size_bytes,
            file_folder_count, library_folder_count, isrc, copyright,
            extra_strings_json
        ) VALUES ({})",
        placeholders.join(", ")
    );
    let mut stmt = tx.prepare(&sql)?;
    for t in &lib.tracks {
        let extra_json = if t.extra_strings.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&t.extra_strings)?)
        };
        let vi = t.video_info.as_ref();
        stmt.execute(params![
            t.persistent_id.to_string(),
            t.title,
            t.album,
            t.artist,
            t.album_artist,
            t.composer,
            t.genre,
            t.store_genre_id,
            t.kind,
            t.comments,
            t.grouping,
            t.work_name,
            t.movement_name,
            t.movement_number,
            t.movement_total,
            t.sort_title,
            t.sort_album,
            t.sort_artist,
            t.sort_album_artist,
            t.sort_composer,
            t.sort_show,
            t.sort_modes.title.to_string(),
            t.sort_modes.album.to_string(),
            t.sort_modes.artist.to_string(),
            t.sort_modes.album_artist.to_string(),
            t.sort_modes.composer.to_string(),
            t.sort_modes.show.to_string(),
            t.track_number,
            t.track_total,
            t.disc_number,
            t.disc_total,
            t.year,
            t.bpm,
            t.duration_ms,
            t.file_size,
            t.bit_rate,
            t.is_vbr.map(i32::from),
            t.sample_rate.map(f64::from),
            t.file_type_fourcc,
            t.codec_fourcc,
            vi.map(|v| v.width),
            vi.map(|v| v.height),
            vi.and_then(|v| v.codec_fourcc.as_deref()),
            vi.map(|v| v.video_bit_rate_kbps),
            vi.map(|v| v.avc_profile_idc),
            vi.map(|v| v.avc_level_idc),
            vi.map(|v| v.frame_rate_milli),
            vi.map(|v| v.color_bit_depth),
            t.volume_adjustment,
            t.normalization,
            t.gapless_encoder_delay,
            t.gapless_encoder_padding,
            t.gapless_total_samples.map(u64_to_sqlite_i64),
            t.gapless_last_eight_frames_offset.map(u64_to_sqlite_i64),
            t.equalizer_preset.as_ref().map(|e| e.raw.as_str()),
            t.equalizer_preset.as_ref().and_then(|e| e.preset_id),
            t.equalizer_preset.as_ref().map(|e| e.name.as_str()),
            t.star_rating,
            t.stars(),
            t.suggestion_flag.to_string(),
            i32::from(t.suggestion_locally_modified),
            t.date_suggestion_flag_modified.map(|d| d.to_string()),
            i32::from(t.disabled),
            i32::from(t.skip_when_shuffling),
            i32::from(t.has_ever_been_played),
            i32::from(t.remember_playback_position),
            i32::from(t.show_composer_in_all_views),
            i32::from(t.use_work_and_movement),
            i32::from(t.album_is_compilation),
            i32::from(t.part_of_gapless_album),
            i32::from(t.playlist_only),
            i32::from(t.purchased),
            i32::from(t.has_custom_lyrics),
            i32::from(t.has_local_custom_lyrics),
            i32::from(t.has_unsynced_local_changes),
            t.custom_lyrics_hash,
            t.has_catalog_synced_lyrics.map(i32::from),
            t.is_itunes_matched.map(i32::from),
            t.is_apple_music_catalog.map(i32::from),
            t.is_apple_digital_master.map(i32::from),
            t.is_cloud_unavailable.map(i32::from),
            t.is_available_in_catalog.map(i32::from),
            t.audio_traits.bits(),
            t.audio_traits.to_string(),
            t.downloaded_audio_traits.bits(),
            t.downloaded_audio_traits.to_string(),
            t.purchaser_account_id.map(u64_to_sqlite_i64),
            t.drm_account_id.map(u64_to_sqlite_i64),
            t.cloud_saga_id.map(u64_to_sqlite_i64),
            t.store_track_adam_id.map(u64_to_sqlite_i64),
            t.apple_music_store_album_id.map(u64_to_sqlite_i64),
            t.apple_music_store_artist_id.map(u64_to_sqlite_i64),
            t.apple_music_store_album_artist_id.map(u64_to_sqlite_i64),
            t.storefront_id,
            t.purchaser_email,
            t.purchaser_name,
            t.downloaded_state.raw(),
            t.downloaded_state.to_string(),
            t.cloud_status.to_string(),
            t.cloud_status.raw(),
            t.cloud_album_id,
            t.cloud_artist_id,
            t.cloud_artwork_token,
            t.cloud_lyrics_token,
            t.content_advisory.raw(),
            t.content_advisory.to_string(),
            t.media_type.raw(),
            t.media_type.to_string(),
            t.playback_start_ms,
            t.playback_stop_ms,
            t.play_count,
            t.true_play_count,
            t.skip_count,
            t.true_skip_count,
            t.date_added.map(|d| d.to_string()),
            t.date_modified.map(|d| d.to_string()),
            t.date_last_played.map(|d| d.to_string()),
            t.date_first_played_utc.map(|d| d.to_string()),
            t.date_last_skipped.map(|d| d.to_string()),
            t.date_released.map(|d| d.to_string()),
            t.date_purchased.map(|d| d.to_string()),
            t.date_cloud_synced.map(|d| d.to_string()),
            t.file_url,
            t.legacy_file_path,
            t.book_path,
            t.album_persistent_id.map(|id| id.to_string()),
            t.artist_persistent_id.map(|id| id.to_string()),
            t.artwork_uuid,
            t.artwork_count,
            t.embedded_artwork.map(|k| i32::from(k.raw())),
            t.embedded_artwork.map(|k| k.to_string()),
            t.artwork_total_size_bytes,
            t.file_folder_count,
            t.library_folder_count,
            t.isrc,
            t.copyright,
            extra_json,
        ])?;
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "full-row 49-column SQLite playlist binding"
)]
fn insert_playlists(tx: &Transaction<'_>, lib: &Library) -> Result<()> {
    let placeholders: Vec<String> = (1..=49).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "INSERT INTO playlists (
            persistent_id, parent_persistent_id, name, description, kind,
            is_master, is_folder, is_smart, is_subscribed,
            show_in_profile_and_search, distinguished_kind, system_kind,
            suggestion_flag, suggestion_locally_modified,
            date_suggestion_flag_modified_utc, date_created_utc,
            date_modified_utc, date_last_played_utc, date_cloud_published_utc,
            date_cloud_synced_utc, play_count, true_play_count,
            refresh_interval_seconds, cloud_share_mode, share_mode,
            artwork_cache_state, cloud_saga_id, universal_library_id,
            cloud_uuid, subscribed_container_url, owner_name, owner_id,
            author_social_profile_persistent_id, cloud_artwork_url,
            cloud_artwork_token, artwork_uuid, declared_track_count,
            smart_live_updating, smart_match_rules,
            smart_match_only_checked_items, smart_limit_count,
            smart_limit_unit, smart_limit_selection, smart_conjunction,
            smart_playlist_json, view_as, sort_field, sort_ascending,
            view_preferences_json
        ) VALUES ({})",
        placeholders.join(", ")
    );
    let mut pl_stmt = tx.prepare(&sql)?;
    let mut item_stmt = tx.prepare(
        "INSERT INTO playlist_items (
            playlist_persistent_id, position, track_persistent_id
        ) VALUES (?1, ?2, ?3)",
    )?;
    let mut rule_stmt = tx.prepare(
        "INSERT INTO smart_playlist_rules (
            playlist_persistent_id, rule_index, depth, conjunction, group_negated,
            field_id, field, operator, comparison_raw, description, value_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;

    for p in &lib.playlists {
        let sp = p.smart_playlist.as_ref();
        let sp_lim = sp.and_then(|s| s.limit.as_ref());
        let sp_conj = sp
            .and_then(|s| s.rule_group.as_ref())
            .map(|g| g.conjunction.to_string());
        let sp_json = sp.map(serde_json::to_string).transpose()?;
        let vp = p.view_preferences.as_ref();
        let vp_json = vp.map(serde_json::to_string).transpose()?;

        let p_pid_str = p.persistent_id.to_string();
        pl_stmt.execute(params![
            &p_pid_str,
            p.parent_persistent_id.map(|id| id.to_string()),
            p.name,
            p.description,
            p.kind.to_string(),
            i32::from(p.is_master),
            i32::from(p.is_folder),
            i32::from(p.is_smart),
            i32::from(p.is_subscribed),
            i32::from(p.show_in_profile_and_search),
            p.system_kind.raw(),
            p.system_kind.to_string(),
            p.suggestion_flag.to_string(),
            i32::from(p.suggestion_locally_modified),
            p.date_suggestion_flag_modified.map(|d| d.to_string()),
            p.date_created.map(|d| d.to_string()),
            p.date_modified.map(|d| d.to_string()),
            p.date_last_played.map(|d| d.to_string()),
            p.date_cloud_published.map(|d| d.to_string()),
            p.date_cloud_synced.map(|d| d.to_string()),
            p.play_count,
            p.true_play_count,
            p.refresh_interval_seconds,
            p.share_mode.raw(),
            p.share_mode.to_string(),
            p.artwork_cache_state,
            p.cloud_saga_id.map(u64_to_sqlite_i64),
            p.universal_library_id,
            p.cloud_uuid,
            p.subscribed_container_url,
            p.owner_name,
            p.owner_id,
            p.author_social_profile_persistent_id
                .map(|id| id.to_string()),
            p.cloud_artwork_url,
            p.cloud_artwork_token,
            p.artwork_uuid,
            p.declared_track_count,
            sp.map(|s| i32::from(s.live_updating)),
            sp.map(|s| i32::from(s.match_rules)),
            sp.map(|s| i32::from(s.match_only_checked_items)),
            sp_lim.map(|l| l.count),
            sp_lim.map(|l| l.unit.to_string()),
            sp_lim.map(|l| l.selection.to_string()),
            sp_conj,
            sp_json,
            vp.and_then(|v| v.view_as.as_deref()),
            vp.and_then(|v| v.sort_field.as_deref()),
            vp.and_then(|v| v.sort_ascending.map(i32::from)),
            vp_json,
        ])?;

        for (pos, tpid) in p.track_persistent_ids.iter().enumerate() {
            let pos_i64 = i64::try_from(pos).unwrap_or(i64::MAX);
            item_stmt.execute(params![&p_pid_str, pos_i64, tpid.to_string()])?;
        }

        if let Some(smart) = sp
            && let Some(group) = &smart.rule_group
        {
            let mut rule_index = 0usize;
            insert_smart_rules_recursive(&mut rule_stmt, &p_pid_str, group, 0, &mut rule_index)?;
        }
    }
    Ok(())
}

fn insert_social_profiles(tx: &Transaction<'_>, lib: &Library) -> Result<()> {
    let mut stmt = tx.prepare(
        "INSERT INTO social_profiles (
            persistent_id, profile_id, name, handle, avatar_id, avatar_url,
            artwork_uuid
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for s in &lib.social_profiles {
        stmt.execute(params![
            s.persistent_id.to_string(),
            s.profile_id,
            s.name,
            s.handle,
            s.avatar_id,
            s.avatar_url,
            s.artwork_uuid,
        ])?;
    }
    Ok(())
}

fn insert_pending_cloud_deletions(tx: &Transaction<'_>, lib: &Library) -> Result<()> {
    let mut stmt = tx.prepare(
        "INSERT INTO pending_cloud_deletions (
            cloud_saga_id, deletion_kind_raw, deletion_kind
        ) VALUES (?1, ?2, ?3)",
    )?;
    for d in &lib.pending_cloud_deletions {
        stmt.execute(params![
            u64_to_sqlite_i64(d.cloud_saga_id),
            d.kind.raw(),
            d.kind.to_string(),
        ])?;
    }
    Ok(())
}

fn create_indexes_and_views(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE INDEX idx_tracks_album_pid ON tracks(album_persistent_id);
        CREATE INDEX idx_tracks_artist_pid ON tracks(artist_persistent_id);
        CREATE INDEX idx_tracks_title ON tracks(title);
        CREATE INDEX idx_tracks_artist ON tracks(artist);
        CREATE INDEX idx_tracks_album ON tracks(album);
        CREATE INDEX idx_tracks_cloud_status ON tracks(cloud_status);
        CREATE INDEX idx_tracks_suggestion ON tracks(suggestion_flag);
        CREATE INDEX idx_tracks_unsynced ON tracks(has_unsynced_local_changes);
        CREATE INDEX idx_albums_artist ON albums(artist);
        CREATE INDEX idx_albums_title ON albums(title);
        CREATE INDEX idx_artists_name ON artists(name);
        CREATE INDEX idx_playlists_parent ON playlists(parent_persistent_id);
        CREATE INDEX idx_playlists_name ON playlists(name);
        CREATE INDEX idx_playlists_kind ON playlists(kind);
        CREATE INDEX idx_playlist_items_track ON playlist_items(track_persistent_id);

        CREATE VIEW v_playlist_tracks AS
        SELECT
            p.persistent_id AS playlist_persistent_id,
            p.name AS playlist_name,
            p.kind AS playlist_kind,
            p.system_kind AS playlist_system_kind,
            pi.position AS position,
            t.persistent_id AS track_persistent_id,
            t.title AS track_title,
            t.artist AS track_artist,
            t.album AS track_album,
            t.duration_ms AS duration_ms,
            t.playlist_only AS track_playlist_only,
            t.store_track_adam_id AS store_track_adam_id
        FROM playlist_items pi
        JOIN playlists p ON p.persistent_id = pi.playlist_persistent_id
        LEFT JOIN tracks t ON t.persistent_id = pi.track_persistent_id
        WHERE p.is_master = 0
          AND p.is_folder = 0
          AND p.distinguished_kind != 63
        ORDER BY pi.playlist_persistent_id, pi.position;

        CREATE VIEW v_tracks_enriched AS
        SELECT
            t.*,
            al.title AS linked_album_title,
            al.catalog_genre AS album_catalog_genre,
            al.suggestion_flag AS album_suggestion_flag,
            ar.name AS linked_artist_name,
            ar.suggestion_flag AS artist_suggestion_flag
        FROM tracks t
        LEFT JOIN albums al ON al.persistent_id = t.album_persistent_id
        LEFT JOIN artists ar ON ar.persistent_id = t.artist_persistent_id;
        ",
    )?;
    Ok(())
}

fn insert_smart_rules_recursive(
    stmt: &mut rusqlite::Statement<'_>,
    playlist_pid: &str,
    group: &SmartRuleGroup,
    depth: usize,
    rule_index: &mut usize,
) -> Result<()> {
    let conj_str = group.conjunction.to_string();
    let depth_i64 = i64::try_from(depth).unwrap_or(i64::MAX);
    for node in &group.rules {
        match node {
            SmartRuleNode::Group(sub) => {
                insert_smart_rules_recursive(
                    stmt,
                    playlist_pid,
                    sub,
                    depth.saturating_add(1),
                    rule_index,
                )?;
            }
            SmartRuleNode::Condition(cond) => {
                let val_json = serde_json::to_string(&cond.value)?;
                let idx_i64 = i64::try_from(*rule_index).unwrap_or(i64::MAX);
                stmt.execute(params![
                    playlist_pid,
                    idx_i64,
                    depth_i64,
                    conj_str,
                    i32::from(group.negated),
                    cond.field_id,
                    cond.field.to_string(),
                    cond.operator.to_string(),
                    cond.comparison_raw,
                    cond.description,
                    val_json,
                ])?;
                *rule_index = rule_index.saturating_add(1);
            }
        }
    }
    Ok(())
}
