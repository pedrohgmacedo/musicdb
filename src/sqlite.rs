use std::path::Path;

use rusqlite::{Connection, params};

use crate::error::Result;
use crate::model::{Library, SmartRuleGroup, SmartRuleNode};

impl Library {
    /// Exports the entire `Library` domain model into a normalized, indexed SQLite database file at `path`.
    /// If `path` already exists, it is overwritten cleanly.
    pub fn export_sqlite(&self, path: impl AsRef<Path>) -> Result<()> {
        let p = path.as_ref();
        if p.exists() {
            std::fs::remove_file(p)?;
        }

        let mut conn = Connection::open(p)?;
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
                file_type INTEGER NOT NULL,
                declared_track_count INTEGER NOT NULL,
                declared_album_count INTEGER NOT NULL,
                declared_artist_count INTEGER NOT NULL,
                declared_playlist_count INTEGER NOT NULL,
                timezone_offset_seconds INTEGER NOT NULL,
                apple_store_account_id INTEGER NOT NULL,
                date_modified_utc TEXT,
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
                is_compilation INTEGER NOT NULL,
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
                suggestion_flag TEXT NOT NULL,
                suggestion_locally_modified INTEGER NOT NULL,
                date_suggestion_flag_modified_utc TEXT,
                date_cloud_synced_utc TEXT,
                apple_music_store_artist_id INTEGER NOT NULL,
                artwork_uuid TEXT,
                artwork_url_plist TEXT
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
                sort_title_mode TEXT NOT NULL,
                sort_album_mode TEXT NOT NULL,
                sort_artist_mode TEXT NOT NULL,
                sort_album_artist_mode TEXT NOT NULL,
                sort_composer_mode TEXT NOT NULL,
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
                disabled INTEGER NOT NULL,
                skip_when_shuffling INTEGER NOT NULL,
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
                is_itunes_matched INTEGER,
                is_apple_music_catalog INTEGER,
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
                has_embedded_artwork INTEGER,
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
                show_in_profile_and_search INTEGER NOT NULL,
                distinguished_kind INTEGER NOT NULL,
                system_kind TEXT NOT NULL,
                suggestion_flag TEXT NOT NULL,
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
                artwork_template_kind INTEGER NOT NULL,
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
                avatar_url TEXT
            );
            ",
        )?;

        let tx = conn.transaction()?;

        {
            let m = &self.metadata;
            let songs_vp_json = m
                .songs_view_preferences
                .as_ref()
                .and_then(|vp| serde_json::to_string(vp).ok());
            tx.execute(
                "INSERT INTO library_metadata VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                params![
                    m.persistent_id,
                    m.itunes_persistent_id,
                    m.revision_hash,
                    m.file_size_mirror,
                    m.app_version,
                    m.format_major_version,
                    m.format_minor_version,
                    m.file_type,
                    m.declared_track_count,
                    m.declared_album_count,
                    m.declared_artist_count,
                    m.declared_playlist_count,
                    m.timezone_offset_seconds,
                    m.apple_store_account_id as i64,
                    m.date_modified.map(|d| d.to_rfc3339()),
                    m.show_song_list_checkboxes.map(i32::from),
                    m.keep_media_folder_organized.map(i32::from),
                    m.copy_files_to_media_folder.map(i32::from),
                    m.media_folder_uri,
                    m.media_folder_path,
                    m.imported_itl_path,
                    songs_vp_json,
                ],
            )?;
        }

        {
            let mut stmt = tx.prepare(
                "INSERT INTO albums VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            )?;
            for a in &self.albums {
                stmt.execute(params![
                    a.persistent_id,
                    a.itunes_persistent_id,
                    a.first_track_persistent_id,
                    a.title,
                    a.artist,
                    a.album_artist,
                    a.cloud_album_id,
                    a.editorial_notes,
                    i32::from(a.is_compilation),
                    a.star_rating,
                    a.stars,
                    i32::from(a.star_rating_is_computed),
                    format!("{:?}", a.suggestion_flag),
                    i32::from(a.suggestion_locally_modified),
                    a.date_suggestion_flag_modified.map(|d| d.to_rfc3339()),
                    a.date_last_played.map(|d| d.to_rfc3339()),
                    a.date_cloud_synced.map(|d| d.to_rfc3339()),
                    a.date_released.map(|d| d.to_rfc3339()),
                    a.apple_music_store_album_id.map(|id| id as i64),
                ])?;
            }
        }

        {
            let mut stmt = tx.prepare(
                "INSERT INTO artists VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            for a in &self.artists {
                stmt.execute(params![
                    a.persistent_id,
                    a.itunes_persistent_id,
                    a.name,
                    a.sort_name,
                    a.cloud_artist_id,
                    format!("{:?}", a.suggestion_flag),
                    i32::from(a.suggestion_locally_modified),
                    a.date_suggestion_flag_modified.map(|d| d.to_rfc3339()),
                    a.date_cloud_synced.map(|d| d.to_rfc3339()),
                    a.apple_music_store_artist_id as i64,
                    a.artwork_uuid,
                    a.artwork_url_plist,
                ])?;
            }
        }

        {
            let placeholders: Vec<String> = (1..=113).map(|i| format!("?{}", i)).collect();
            let sql = format!("INSERT INTO tracks VALUES ({})", placeholders.join(", "));
            let mut stmt = tx.prepare(&sql)?;
            for t in &self.tracks {
                let extra_json = if t.extra_strings.is_empty() {
                    None
                } else {
                    serde_json::to_string(&t.extra_strings).ok()
                };
                stmt.execute(params![
                    t.persistent_id,
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
                    format!("{:?}", t.sort_modes.title),
                    format!("{:?}", t.sort_modes.album),
                    format!("{:?}", t.sort_modes.artist),
                    format!("{:?}", t.sort_modes.album_artist),
                    format!("{:?}", t.sort_modes.composer),
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
                    t.volume_adjustment,
                    t.normalization,
                    t.gapless_encoder_delay,
                    t.gapless_encoder_padding,
                    t.gapless_total_samples.map(|v| v as i64),
                    t.gapless_last_eight_frames_offset.map(|v| v as i64),
                    t.equalizer,
                    t.equalizer_preset.as_ref().and_then(|e| e.preset_id),
                    t.equalizer_preset.as_ref().map(|e| e.name.as_str()),
                    t.star_rating,
                    t.stars,
                    format!("{:?}", t.suggestion_flag),
                    i32::from(t.disabled),
                    i32::from(t.skip_when_shuffling),
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
                    t.is_itunes_matched.map(i32::from),
                    t.is_apple_music_catalog.map(i32::from),
                    t.cloud_saga_id.map(|v| v as i64),
                    t.store_track_adam_id.map(|v| v as i64),
                    t.apple_music_store_album_id.map(|v| v as i64),
                    t.apple_music_store_artist_id.map(|v| v as i64),
                    t.apple_music_store_album_artist_id.map(|v| v as i64),
                    t.storefront_id,
                    t.purchaser_email,
                    t.purchaser_name,
                    t.downloaded_status,
                    format!("{:?}", t.downloaded_state),
                    format!("{:?}", t.cloud_status),
                    t.cloud_status_raw,
                    t.cloud_album_id,
                    t.cloud_artist_id,
                    t.cloud_artwork_token,
                    t.cloud_lyrics_token,
                    t.content_rating,
                    format!("{:?}", t.content_advisory),
                    t.media_kind,
                    format!("{:?}", t.media_type),
                    t.playback_start_ms,
                    t.playback_stop_ms,
                    t.play_count,
                    t.true_play_count,
                    t.skip_count,
                    t.true_skip_count,
                    t.date_added.map(|d| d.to_rfc3339()),
                    t.date_modified.map(|d| d.to_rfc3339()),
                    t.date_last_played.map(|d| d.to_rfc3339()),
                    t.date_first_played_utc.map(|d| d.to_rfc3339()),
                    t.date_last_skipped.map(|d| d.to_rfc3339()),
                    t.date_released.map(|d| d.to_rfc3339()),
                    t.date_purchased.map(|d| d.to_rfc3339()),
                    t.date_cloud_synced.map(|d| d.to_rfc3339()),
                    t.file_url,
                    t.legacy_file_path,
                    t.book_path,
                    t.album_persistent_id,
                    t.artist_persistent_id,
                    t.artwork_uuid,
                    t.artwork_count,
                    t.has_embedded_artwork.map(i32::from),
                    t.artwork_total_size_bytes,
                    t.file_folder_count,
                    t.library_folder_count,
                    t.isrc,
                    t.copyright,
                    extra_json,
                ])?;
            }
        }

        {
            let placeholders: Vec<String> = (1..=46).map(|i| format!("?{}", i)).collect();
            let sql = format!("INSERT INTO playlists VALUES ({})", placeholders.join(", "));
            let mut pl_stmt = tx.prepare(&sql)?;
            let mut item_stmt = tx.prepare("INSERT INTO playlist_items VALUES (?1, ?2, ?3)")?;
            let mut rule_stmt = tx.prepare(
                "INSERT INTO smart_playlist_rules VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;

            for p in &self.playlists {
                let sp = p.smart_playlist.as_ref();
                let sp_lim = sp.and_then(|s| s.limit.as_ref());
                let sp_conj = sp
                    .and_then(|s| s.rule_group.as_ref())
                    .map(|g| format!("{:?}", g.conjunction));
                let sp_json = sp.and_then(|s| serde_json::to_string(s).ok());
                let vp = p.view_preferences.as_ref();
                let vp_json = vp.and_then(|v| serde_json::to_string(v).ok());

                pl_stmt.execute(params![
                    p.persistent_id,
                    p.parent_persistent_id,
                    p.name,
                    p.description,
                    format!("{:?}", p.kind),
                    i32::from(p.is_master),
                    i32::from(p.is_folder),
                    i32::from(p.is_smart),
                    i32::from(p.show_in_profile_and_search),
                    p.distinguished_kind,
                    format!("{:?}", p.system_kind),
                    format!("{:?}", p.suggestion_flag),
                    p.date_created.map(|d| d.to_rfc3339()),
                    p.date_modified.map(|d| d.to_rfc3339()),
                    p.date_last_played.map(|d| d.to_rfc3339()),
                    p.date_cloud_published.map(|d| d.to_rfc3339()),
                    p.date_cloud_synced.map(|d| d.to_rfc3339()),
                    p.play_count,
                    p.true_play_count,
                    p.refresh_interval_seconds,
                    p.cloud_share_mode,
                    format!("{:?}", p.share_mode),
                    p.artwork_template_kind,
                    p.cloud_saga_id.map(|v| v as i64),
                    p.universal_library_id,
                    p.cloud_uuid,
                    p.subscribed_container_url,
                    p.owner_name,
                    p.owner_id,
                    p.author_social_profile_persistent_id,
                    p.cloud_artwork_url,
                    p.cloud_artwork_token,
                    p.artwork_uuid,
                    p.declared_track_count,
                    sp.map(|s| i32::from(s.live_updating)),
                    sp.map(|s| i32::from(s.match_rules)),
                    sp.map(|s| i32::from(s.match_only_checked_items)),
                    sp_lim.map(|l| l.count),
                    sp_lim.map(|l| format!("{:?}", l.unit)),
                    sp_lim.map(|l| format!("{:?}", l.selection)),
                    sp_conj,
                    sp_json,
                    vp.and_then(|v| v.view_as.as_deref()),
                    vp.and_then(|v| v.sort_field.as_deref()),
                    vp.and_then(|v| v.sort_ascending.map(i32::from)),
                    vp_json,
                ])?;

                for (pos, tpid) in p.track_persistent_ids.iter().enumerate() {
                    item_stmt.execute(params![p.persistent_id, pos as i64, tpid])?;
                }

                if let Some(smart) = sp
                    && let Some(group) = &smart.rule_group
                {
                    let mut rule_index = 0usize;
                    insert_smart_rules_recursive(
                        &mut rule_stmt,
                        &p.persistent_id,
                        group,
                        0,
                        &mut rule_index,
                    )?;
                }
            }
        }

        {
            let mut stmt =
                tx.prepare("INSERT INTO social_profiles VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
            for s in &self.social_profiles {
                stmt.execute(params![
                    s.persistent_id,
                    s.profile_id,
                    s.name,
                    s.handle,
                    s.avatar_id,
                    s.avatar_url,
                ])?;
            }
        }

        tx.commit()?;

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
                pi.position AS position,
                t.persistent_id AS track_persistent_id,
                t.title AS track_title,
                t.artist AS track_artist,
                t.album AS track_album,
                t.duration_ms AS duration_ms
            FROM playlist_items pi
            JOIN playlists p ON p.persistent_id = pi.playlist_persistent_id
            LEFT JOIN tracks t ON t.persistent_id = pi.track_persistent_id
            ORDER BY pi.playlist_persistent_id, pi.position;

            CREATE VIEW v_tracks_enriched AS
            SELECT
                t.*,
                al.title AS linked_album_title,
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
}

fn insert_smart_rules_recursive(
    stmt: &mut rusqlite::Statement<'_>,
    playlist_pid: &str,
    group: &SmartRuleGroup,
    depth: usize,
    rule_index: &mut usize,
) -> Result<()> {
    let conj_str = format!("{:?}", group.conjunction);
    for node in &group.rules {
        match node {
            SmartRuleNode::Group(sub) => {
                insert_smart_rules_recursive(stmt, playlist_pid, sub, depth + 1, rule_index)?;
            }
            SmartRuleNode::Condition(cond) => {
                let val_json = serde_json::to_string(&cond.value)?;
                stmt.execute(params![
                    playlist_pid,
                    *rule_index as i64,
                    depth as i64,
                    conj_str,
                    cond.field_id,
                    format!("{:?}", cond.field),
                    format!("{:?}", cond.operator),
                    cond.comparison_raw,
                    cond.description,
                    val_json,
                ])?;
                *rule_index += 1;
            }
        }
    }
    Ok(())
}
