//! End-to-end CLI tests for `musicdb query-unknowns`.

#![cfg(feature = "cli")]

use std::process::Command;

use musicdb::{DEFAULT_MUSICDB_KEY, compress_and_encrypt_bytes};
use serde_json::Value;

fn put_u16(buf: &mut [u8], off: usize, val: u16) {
    buf[off..off + 2].copy_from_slice(&val.to_le_bytes());
}

fn put_u32(buf: &mut [u8], off: usize, val: u32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

fn put_u64(buf: &mut [u8], off: usize, val: u64) {
    buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
}

fn make_str_boma(subtype: u32, text: &str) -> Vec<u8> {
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
    put_u32(&mut buf, 20, 1);
    put_u32(&mut buf, 24, u32::try_from(utf16.len()).unwrap());
    buf.extend_from_slice(&utf16);
    buf
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

fn build_synthetic_musicdb_bytes() -> Vec<u8> {
    let mut hfma = vec![0u8; 160];
    hfma[0..4].copy_from_slice(b"hfma");
    put_u32(&mut hfma, 4, 160);
    put_u16(&mut hfma, 12, 23);
    put_u16(&mut hfma, 14, 1);
    hfma[16..25].copy_from_slice(b"1.7.0.156");
    put_u64(&mut hfma, 48, 0x1111_2222_3333_4444);
    put_u32(&mut hfma, 56, 9);
    put_u32(&mut hfma, 60, 20);
    put_u32(&mut hfma, 76, 1);
    put_u32(&mut hfma, 84, 102_400);

    let mut plma = vec![0u8; 160];
    plma[0..4].copy_from_slice(b"plma");
    put_u32(&mut plma, 4, 160);
    put_u32(&mut plma, 8, 0);

    let title_boma = make_str_boma(0x012C, "Synthetic Album");
    let artist_boma = make_str_boma(0x012D, "Synthetic Artist");
    let mut album_item = vec![0u8; 140];
    album_item[0..4].copy_from_slice(b"iama");
    put_u32(&mut album_item, 4, 140);
    put_u32(
        &mut album_item,
        8,
        140 + u32::try_from(title_boma.len() + artist_boma.len()).unwrap(),
    );
    put_u32(&mut album_item, 12, 2);
    put_u64(&mut album_item, 16, 0xAA00_0000_0000_0001);
    album_item[24] = 2;
    put_u32(&mut album_item, 28, 7);
    album_item.extend_from_slice(&title_boma);
    album_item.extend_from_slice(&artist_boma);

    let mut album_list = vec![0u8; 48];
    album_list[0..4].copy_from_slice(b"lama");
    put_u32(&mut album_list, 4, 48);
    put_u32(&mut album_list, 8, 1);
    album_list.extend_from_slice(&album_item);

    let mut raw = hfma;
    raw.extend_from_slice(&wrap_hsma(6, &plma));
    raw.extend_from_slice(&wrap_hsma(4, &album_list));

    compress_and_encrypt_bytes(&raw, DEFAULT_MUSICDB_KEY).expect("encrypt synthetic musicdb")
}

#[test]
fn cli_query_unknowns_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("Library.musicdb");
    std::fs::write(&db_path, build_synthetic_musicdb_bytes()).unwrap();

    let bin = env!("CARGO_BIN_EXE_musicdb");

    let out_field = Command::new(bin)
        .args(["query-unknowns"])
        .arg(&db_path)
        .args(["-f", "iama.unk_28", "--json"])
        .output()
        .unwrap();
    assert!(
        out_field.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_field.stderr)
    );
    let json_field: Value = serde_json::from_slice(&out_field.stdout).unwrap();
    assert_eq!(json_field["results"]["mode"], "GroupedByValue");
    assert_eq!(json_field["results"]["fields"][0]["section"], "iama");
    assert_eq!(json_field["results"]["fields"][0]["field_name"], "unk_28");
    assert_eq!(
        json_field["results"]["fields"][0]["buckets"][0]["value"],
        "7 (0x00000007)"
    );
    assert_eq!(
        json_field["results"]["fields"][0]["buckets"][0]["examples"][0]["entity"]["title"],
        "Synthetic Album"
    );

    let out_entity = Command::new(bin)
        .args(["query-unknowns"])
        .arg(&db_path)
        .args(["-e", "album:Synthetic", "--json"])
        .output()
        .unwrap();
    assert!(
        out_entity.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_entity.stderr)
    );
    let json_entity: Value = serde_json::from_slice(&out_entity.stdout).unwrap();
    assert_eq!(json_entity["results"]["mode"], "GroupedByEntity");
    assert_eq!(
        json_entity["results"]["entities"][0]["entity"]["title"],
        "Synthetic Album"
    );

    let out_list = Command::new(bin)
        .args(["query-unknowns"])
        .arg(&db_path)
        .args(["--list-fields"])
        .output()
        .unwrap();
    assert!(
        out_list.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out_list.stderr)
    );
    let text_list = String::from_utf8_lossy(&out_list.stdout);
    assert!(text_list.contains("iama.unk_28"));
    assert!(text_list.contains("hfma(outer).unk_60"));
}
