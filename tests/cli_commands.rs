//! End-to-end CLI integration tests for core `musicdb` subcommands using synthetic Rush fixtures.

#![cfg(feature = "cli")]

use std::process::Command;

use musicdb::{DEFAULT_MUSICDB_KEY, compress_and_encrypt_bytes, load_raw_musicdb};
use serde_json::Value;

const LIB_PID: u64 = 0x2112_0000_0000_0001;
const ALBUM_PID: u64 = 0xAA00_0000_0000_0001;
const ARTIST_PID: u64 = 0xBB00_0000_0000_0001;
const PLAYLIST_PID: u64 = 0xCC00_0000_0000_0001;
const TRACK_TOM_SAWYER: u64 = 0x2112_0000_0000_0010;
const TRACK_LIMELIGHT: u64 = 0x2112_0000_0000_0011;
const TRACK_YYZ: u64 = 0x2112_0000_0000_0012;
const TS_2026_OCT: u32 = 3_876_136_992;

fn put_u16(buf: &mut [u8], off: usize, val: u16) {
    buf[off..off + 2].copy_from_slice(&val.to_le_bytes());
}

fn put_u32(buf: &mut [u8], off: usize, val: u32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

fn put_i32(buf: &mut [u8], off: usize, val: i32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

fn put_u64(buf: &mut [u8], off: usize, val: u64) {
    buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
}

fn make_boma(subtype: u32, payload: &[u8]) -> Vec<u8> {
    let total = 20u32 + u32::try_from(payload.len()).unwrap();
    let mut buf = vec![0u8; 20];
    buf[0..4].copy_from_slice(b"boma");
    put_u32(&mut buf, 4, 20);
    put_u32(&mut buf, 8, total);
    put_u32(&mut buf, 12, subtype);
    buf.extend_from_slice(payload);
    buf
}

fn make_str_boma(subtype: u32, text: &str) -> Vec<u8> {
    let mut utf16 = Vec::new();
    for unit in text.encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    let mut payload = vec![0u8; 16];
    put_u32(&mut payload, 0, 1); // UTF-16LE
    put_u32(&mut payload, 4, u32::try_from(utf16.len()).unwrap());
    payload.extend_from_slice(&utf16);
    make_boma(subtype, &payload)
}

fn wrap_hsma(subtype: u32, child: &[u8]) -> Vec<u8> {
    let mut hdr = vec![0u8; 56];
    hdr[0..4].copy_from_slice(b"hsma");
    put_u32(&mut hdr, 4, 56);
    put_u32(&mut hdr, 8, 56 + u32::try_from(child.len()).unwrap());
    put_u32(&mut hdr, 12, subtype);
    hdr.extend_from_slice(child);
    hdr
}

struct TrackSpec<'a> {
    pid: u64,
    counter: u32,
    title: &'a str,
    track_no: u16,
    duration_ms: u32,
    plays: u32,
}

fn make_track_item(spec: &TrackSpec<'_>) -> Vec<u8> {
    let mut num = vec![0u8; 364];
    put_u32(&mut num, 0, spec.counter);
    put_u32(&mut num, 4, 1); // local subrecord
    put_u64(&mut num, 8, spec.pid);
    put_u32(&mut num, 28, 1); // is_local_subrecord
    num[60..64].copy_from_slice(&44_100.0f32.to_le_bytes());
    put_u32(&mut num, 64, 0x4D34_4120); // "M4A "
    put_u16(&mut num, 78, 51); // MPEG-4 AAC
    put_u32(&mut num, 88, 256); // bit_rate
    put_u32(&mut num, 92, TS_2026_OCT); // date_added
    put_u32(&mut num, 128, TS_2026_OCT); // date_modified
    put_u32(&mut num, 148, 0x6D70_3461); // "mp4a"
    put_u32(&mut num, 156, spec.duration_ms);
    put_u32(&mut num, 296, 8_900_000); // file_size
    put_u32(&mut num, 356, TS_2026_OCT); // date_cloud_synced

    let mut ps = vec![0u8; 52];
    put_u64(&mut ps, 0, spec.pid);
    put_u32(&mut ps, 12, spec.plays);
    put_u32(&mut ps, 16, spec.plays);

    let bomas = [
        make_str_boma(0x0002, spec.title),
        make_str_boma(0x0003, "Moving Pictures"),
        make_str_boma(0x0004, "Rush"),
        make_boma(0x0001, &num),
        make_boma(0x0017, &ps),
    ];
    let boma_bytes: usize = bomas.iter().map(Vec::len).sum();
    let mut item = vec![0u8; 376];
    item[0..4].copy_from_slice(b"itma");
    put_u32(&mut item, 4, 376);
    put_u32(&mut item, 8, 376 + u32::try_from(boma_bytes).unwrap());
    put_u32(&mut item, 12, u32::try_from(bomas.len()).unwrap());
    put_u64(&mut item, 16, spec.pid);
    put_u32(&mut item, 24, spec.counter);
    item[31] = u8::from(spec.plays > 0);
    item[55] = u8::from(spec.plays > 0);
    item[57] = 1; // downloaded
    item[66..72].copy_from_slice(&[0x80; 6]); // auto_sort_flags (no sort bomas)
    put_u16(&mut item, 84, 1); // disc_number
    put_u16(&mut item, 90, 1); // disc_total
    put_u32(&mut item, 108, 1); // media_kind = music
    put_u16(&mut item, 116, 7); // track_total
    put_u16(&mut item, 160, spec.track_no);
    put_u32(&mut item, 168, 1981); // year
    put_u64(&mut item, 172, ALBUM_PID);
    put_u64(&mut item, 180, ARTIST_PID);
    put_u32(&mut item, 352, TS_2026_OCT);
    for b in &bomas {
        item.extend_from_slice(b);
    }
    item
}

fn make_album_list() -> Vec<u8> {
    let bomas = [
        make_str_boma(0x012C, "Moving Pictures"),
        make_str_boma(0x012D, "Rush"),
    ];
    let boma_bytes: usize = bomas.iter().map(Vec::len).sum();
    let mut item = vec![0u8; 140];
    item[0..4].copy_from_slice(b"iama");
    put_u32(&mut item, 4, 140);
    put_u32(&mut item, 8, 140 + u32::try_from(boma_bytes).unwrap());
    put_u32(&mut item, 12, 2);
    put_u64(&mut item, 16, ALBUM_PID);
    put_u64(&mut item, 32, TRACK_TOM_SAWYER);
    for b in &bomas {
        item.extend_from_slice(b);
    }

    let mut list = vec![0u8; 48];
    list[0..4].copy_from_slice(b"lama");
    put_u32(&mut list, 4, 48);
    put_u32(&mut list, 8, 1);
    list.extend_from_slice(&item);
    list
}

fn make_artist_list() -> Vec<u8> {
    let name = make_str_boma(0x0190, "Rush");
    let mut item = vec![0u8; 132];
    item[0..4].copy_from_slice(b"iAma");
    put_u32(&mut item, 4, 132);
    put_u32(&mut item, 8, 132 + u32::try_from(name.len()).unwrap());
    put_u32(&mut item, 12, 1);
    put_u64(&mut item, 16, ARTIST_PID);
    item.extend_from_slice(&name);

    let mut list = vec![0u8; 48];
    list[0..4].copy_from_slice(b"lAma");
    put_u32(&mut list, 4, 48);
    put_u32(&mut list, 8, 1);
    list.extend_from_slice(&item);
    list
}

fn make_playlist_list(track_pids: &[u64]) -> Vec<u8> {
    let mut bomas = vec![make_str_boma(0x00C8, "Rush: Studio")];
    for (idx, &pid) in track_pids.iter().enumerate() {
        let mut ipfa = vec![0u8; 32];
        ipfa[0..4].copy_from_slice(b"IPfa");
        put_u32(&mut ipfa, 4, 32);
        put_u64(&mut ipfa, 8, 0xDD00_0000_0000_0001 + idx as u64);
        put_u64(&mut ipfa, 16, pid);
        bomas.push(make_boma(0x00DA, &ipfa));
    }
    let boma_bytes: usize = bomas.iter().map(Vec::len).sum();
    let mut item = vec![0u8; 368];
    item[0..4].copy_from_slice(b"lpma");
    put_u32(&mut item, 4, 368);
    put_u32(&mut item, 8, 368 + u32::try_from(boma_bytes).unwrap());
    put_u32(&mut item, 12, u32::try_from(bomas.len()).unwrap());
    put_u32(&mut item, 16, u32::try_from(track_pids.len()).unwrap());
    put_u32(&mut item, 22, TS_2026_OCT);
    put_u64(&mut item, 30, PLAYLIST_PID);
    put_u32(&mut item, 138, TS_2026_OCT);
    item[190..192].copy_from_slice(&[1, 1]);
    for b in &bomas {
        item.extend_from_slice(b);
    }

    let mut list = vec![0u8; 92];
    list[0..4].copy_from_slice(b"lPma");
    put_u32(&mut list, 4, 92);
    put_u32(&mut list, 8, 1);
    list.extend_from_slice(&item);
    list
}

fn build_rush_musicdb(tracks: &[TrackSpec<'_>]) -> Vec<u8> {
    let mut hfma = vec![0u8; 160];
    hfma[0..4].copy_from_slice(b"hfma");
    put_u32(&mut hfma, 4, 160);
    put_u16(&mut hfma, 12, 23);
    put_u16(&mut hfma, 14, 1);
    hfma[16..25].copy_from_slice(b"1.7.0.156");
    put_u64(&mut hfma, 48, LIB_PID);
    put_u32(&mut hfma, 56, 5); // hsma_count
    put_u32(&mut hfma, 60, 20);
    put_u32(&mut hfma, 68, u32::try_from(tracks.len()).unwrap());
    put_u32(&mut hfma, 72, 1); // playlist_count
    put_u32(&mut hfma, 76, 1); // album_count
    put_u32(&mut hfma, 80, 1); // artist_count
    put_u32(&mut hfma, 84, 102_400);
    put_i32(&mut hfma, 88, -10_800);
    put_u32(&mut hfma, 100, TS_2026_OCT);

    let mut plma = vec![0u8; 160];
    plma[0..4].copy_from_slice(b"plma");
    put_u32(&mut plma, 4, 160);
    put_u64(&mut plma, 48, LIB_PID);

    let mut track_list = vec![0u8; 92];
    track_list[0..4].copy_from_slice(b"ltma");
    put_u32(&mut track_list, 4, 92);
    put_u32(&mut track_list, 8, u32::try_from(tracks.len()).unwrap());
    let mut track_pids = Vec::with_capacity(tracks.len());
    for spec in tracks {
        track_list.extend_from_slice(&make_track_item(spec));
        track_pids.push(spec.pid);
    }

    let mut raw = hfma;
    raw.extend_from_slice(&wrap_hsma(6, &plma));
    raw.extend_from_slice(&wrap_hsma(4, &make_album_list()));
    raw.extend_from_slice(&wrap_hsma(5, &make_artist_list()));
    raw.extend_from_slice(&wrap_hsma(1, &track_list));
    raw.extend_from_slice(&wrap_hsma(2, &make_playlist_list(&track_pids)));
    compress_and_encrypt_bytes(&raw, DEFAULT_MUSICDB_KEY).expect("encrypt synthetic Rush musicdb")
}

fn baseline_tracks() -> Vec<TrackSpec<'static>> {
    vec![
        TrackSpec {
            pid: TRACK_TOM_SAWYER,
            counter: 10,
            title: "Tom Sawyer",
            track_no: 1,
            duration_ms: 274_000,
            plays: 5,
        },
        TrackSpec {
            pid: TRACK_LIMELIGHT,
            counter: 11,
            title: "Limelight",
            track_no: 4,
            duration_ms: 260_000,
            plays: 2,
        },
    ]
}

fn run_musicdb(args: &[&str]) -> std::process::Output {
    let out = Command::new(env!("CARGO_BIN_EXE_musicdb"))
        .args(args)
        .output()
        .expect("spawn musicdb binary");
    assert!(
        out.status.success(),
        "musicdb {args:?} failed ({}): {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

#[test]
fn cli_info_decrypt_and_sanity_check_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("Library.musicdb");
    std::fs::write(&db_path, build_rush_musicdb(&baseline_tracks())).unwrap();
    let db_str = db_path.to_str().unwrap();

    // `musicdb info`
    let info_out = run_musicdb(&["info", db_str]);
    let info_text = String::from_utf8_lossy(&info_out.stdout);
    assert!(info_text.contains("23.1"));
    assert!(info_text.contains("1.7.0.156"));
    assert!(info_text.contains("2112000000000001"));

    // `musicdb decrypt`
    let raw_out_path = dir.path().join("decrypted.bin");
    run_musicdb(&["decrypt", db_str, raw_out_path.to_str().unwrap()]);
    let decrypted = std::fs::read(&raw_out_path).unwrap();
    assert!(decrypted.starts_with(b"hfma"));
    assert_eq!(decrypted, load_raw_musicdb(&db_path).unwrap());

    // `musicdb sanity-check` (text + JSON + category filter)
    let sanity_text_out = run_musicdb(&["sanity-check", db_str, "--now", "2026-11-22T00:00:00Z"]);
    assert!(
        String::from_utf8_lossy(&sanity_text_out.stdout).contains("0 warnings"),
        "expected summary header in sanity-check text output"
    );

    let sanity_json_out = run_musicdb(&[
        "sanity-check",
        db_str,
        "--include-info",
        "--now",
        "2026-11-22T00:00:00Z",
        "--json",
    ]);
    let sanity_json: Value = serde_json::from_slice(&sanity_json_out.stdout).unwrap();
    assert_eq!(sanity_json["total_warnings"], 0);
    assert_eq!(sanity_json["total_info"], 0);
}

#[test]
fn cli_export_json_and_sqlite_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("Library.musicdb");
    std::fs::write(&db_path, build_rush_musicdb(&baseline_tracks())).unwrap();
    let db_str = db_path.to_str().unwrap();

    // `musicdb export-json` to stdout and to file
    let json_stdout = run_musicdb(&["export-json", db_str]);
    let exported: Value = serde_json::from_slice(&json_stdout.stdout).unwrap();
    assert_eq!(exported["tracks"].as_array().unwrap().len(), 2);
    assert_eq!(exported["tracks"][0]["title"], "Tom Sawyer");
    assert_eq!(exported["albums"][0]["title"], "Moving Pictures");
    assert_eq!(exported["artists"][0]["name"], "Rush");
    assert_eq!(exported["playlists"][0]["name"], "Rush: Studio");

    let json_file = dir.path().join("rush.json");
    run_musicdb(&["export-json", db_str, "-o", json_file.to_str().unwrap()]);
    let file_json: Value = serde_json::from_slice(&std::fs::read(&json_file).unwrap()).unwrap();
    assert_eq!(file_json["tracks"][1]["title"], "Limelight");

    // `musicdb export-sqlite`
    let sqlite_path = dir.path().join("rush.sqlite");
    run_musicdb(&["export-sqlite", db_str, sqlite_path.to_str().unwrap()]);
    let conn = rusqlite::Connection::open(&sqlite_path).unwrap();
    let track_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .unwrap();
    let playlist_item_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM playlist_items", [], |r| r.get(0))
        .unwrap();
    let first_title: String = conn
        .query_row(
            "SELECT title FROM tracks WHERE persistent_id = '2112000000000010'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(track_count, 2);
    assert_eq!(playlist_item_count, 2);
    assert_eq!(first_title, "Tom Sawyer");
}

#[test]
fn cli_query_track_and_query_playlist_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("Library.musicdb");
    std::fs::write(&db_path, build_rush_musicdb(&baseline_tracks())).unwrap();
    let db_str = db_path.to_str().unwrap();

    // `musicdb query-track` by substring and by Persistent ID
    let by_name = run_musicdb(&["query-track", db_str, "sawyer", "--json"]);
    let by_name_json: Value = serde_json::from_slice(&by_name.stdout).unwrap();
    assert_eq!(by_name_json.as_array().unwrap().len(), 1);
    assert_eq!(by_name_json[0]["title"], "Tom Sawyer");

    let by_pid = run_musicdb(&["query-track", db_str, "2112000000000011", "--json"]);
    let by_pid_json: Value = serde_json::from_slice(&by_pid.stdout).unwrap();
    assert_eq!(by_pid_json.as_array().unwrap().len(), 1);
    assert_eq!(by_pid_json[0]["title"], "Limelight");

    // `musicdb query-playlist` list all + case-insensitive substring fallback
    let list_out = run_musicdb(&["query-playlist", db_str, "--json"]);
    let list_json: Value = serde_json::from_slice(&list_out.stdout).unwrap();
    assert_eq!(list_json.as_array().unwrap().len(), 1);
    assert_eq!(list_json[0]["name"], "Rush: Studio");

    let by_sub = run_musicdb(&["query-playlist", db_str, "studio", "--json"]);
    let by_sub_json: Value = serde_json::from_slice(&by_sub.stdout).unwrap();
    assert_eq!(by_sub_json["name"], "Rush: Studio");
    assert_eq!(
        by_sub_json["track_persistent_ids"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn cli_diff_and_unknowns_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let db_a = dir.path().join("Library_A.musicdb");
    let db_b = dir.path().join("Library_B.musicdb");

    let tracks_a = baseline_tracks();
    let mut tracks_b = baseline_tracks();
    tracks_b[0].plays = 6;
    tracks_b.push(TrackSpec {
        pid: TRACK_YYZ,
        counter: 12,
        title: "YYZ",
        track_no: 3,
        duration_ms: 266_000,
        plays: 1,
    });

    std::fs::write(&db_a, build_rush_musicdb(&tracks_a)).unwrap();
    std::fs::write(&db_b, build_rush_musicdb(&tracks_b)).unwrap();

    // `musicdb diff --json`
    let diff_out = run_musicdb(&[
        "diff",
        db_a.to_str().unwrap(),
        db_b.to_str().unwrap(),
        "--json",
    ]);
    let diff_json: Value = serde_json::from_slice(&diff_out.stdout).unwrap();
    assert_eq!(diff_json["tracks_in_a"], 2);
    assert_eq!(diff_json["tracks_in_b"], 3);
    assert_eq!(diff_json["common_tracks"], 2);
    assert_eq!(diff_json["added_tracks"].as_array().unwrap().len(), 1);
    assert!(
        diff_json["added_tracks"][0]
            .as_str()
            .unwrap()
            .contains("YYZ")
    );
    assert_eq!(diff_json["sample_track_diffs"].as_array().unwrap().len(), 1);

    // `musicdb unknowns --json`
    let unk_out = run_musicdb(&["unknowns", db_b.to_str().unwrap(), "--json"]);
    let unk_json: Value = serde_json::from_slice(&unk_out.stdout).unwrap();
    assert_eq!(unk_json["global_counter_analysis"]["album_count"], 1);
    assert_eq!(unk_json["global_counter_analysis"]["artist_count"], 1);
}
