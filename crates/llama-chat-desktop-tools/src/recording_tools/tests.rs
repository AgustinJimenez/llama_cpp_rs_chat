//! Tests for recording_tools.

use std::process::{Command, Stdio};
use std::time::Instant;

use super::gif_encoder::{
    build_palette_median_cut, lzw_compress, quantize_to_palette, write_sub_blocks, BitWriter, Rgb,
};
use super::state::RecordingState;

#[test]
fn test_bit_writer_basic() {
    let mut bw = BitWriter::new();
    bw.write_bits(0b101, 3);
    bw.write_bits(0b1100, 4);
    bw.write_bits(0b1, 1);
    // bits: 101 1100 1 = 1_1100_101 = 0xE5
    let result = bw.finish();
    assert_eq!(result, vec![0xE5]);
}

#[test]
fn test_bit_writer_multi_byte() {
    let mut bw = BitWriter::new();
    bw.write_bits(0xFF, 8);
    bw.write_bits(0x01, 8);
    let result = bw.finish();
    assert_eq!(result, vec![0xFF, 0x01]);
}

#[test]
fn test_quantize_exact_match() {
    let palette: Vec<Rgb> = vec![[255, 0, 0], [0, 255, 0], [0, 0, 255]];
    let rgba = vec![0, 255, 0, 255]; // green pixel
    let indices = quantize_to_palette(&rgba, &palette);
    assert_eq!(indices, vec![1]);
}

#[test]
fn test_quantize_closest() {
    let palette: Vec<Rgb> = vec![[0, 0, 0], [255, 255, 255]];
    let rgba = vec![200, 200, 200, 255]; // close to white
    let indices = quantize_to_palette(&rgba, &palette);
    assert_eq!(indices, vec![1]);
}

#[test]
fn test_build_palette_non_empty() {
    let rgba: Vec<u8> = (0..256)
        .flat_map(|i| vec![i as u8, 0, 0, 255])
        .collect();
    let palette = build_palette_median_cut(&rgba);
    assert_eq!(palette.len(), 256);
}

#[test]
fn test_lzw_compress_produces_output() {
    let indices = vec![0u8, 0, 0, 1, 1, 1, 0, 0];
    let compressed = lzw_compress(&indices, 8);
    assert!(!compressed.is_empty());
}

#[test]
fn test_write_sub_blocks_small() {
    let data = vec![1u8, 2, 3];
    let mut out = Vec::new();
    write_sub_blocks(&mut out, &data).unwrap();
    assert_eq!(out, vec![3, 1, 2, 3]); // size=3, then data
}

#[test]
fn test_write_sub_blocks_large() {
    let data = vec![0xAA; 300];
    let mut out = Vec::new();
    write_sub_blocks(&mut out, &data).unwrap();
    // First block: 255 bytes, second block: 45 bytes
    assert_eq!(out[0], 255);
    assert_eq!(out[256], 45);
    assert_eq!(out.len(), 1 + 255 + 1 + 45);
}

// ─── Round 7: RecordingState Drop ───────────────────────────────────

#[test]
fn test_recording_state_drop_without_child() {
    // Drop with None child should not panic
    let state = RecordingState {
        child: None,
        output_path: "/tmp/test.gif".to_string(),
        started_at: Instant::now(),
    };
    drop(state); // should not panic
}

#[test]
fn test_recording_state_drop_with_child() {
    // Spawn a short-lived process, wrap in RecordingState, drop should kill it
    let child = Command::new(if cfg!(windows) { "timeout" } else { "sleep" })
        .args(if cfg!(windows) { &["/t", "60"][..] } else { &["60"][..] })
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(child) = child {
        let state = RecordingState {
            child: Some(child),
            output_path: "/tmp/test.gif".to_string(),
            started_at: Instant::now(),
        };
        drop(state); // Drop should kill + wait, not panic
    }
}
