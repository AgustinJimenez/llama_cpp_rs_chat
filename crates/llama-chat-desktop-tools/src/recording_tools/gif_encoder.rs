//! Minimal animated GIF encoder (pure Rust, no external gif crate).
//!
//! Encodes a sequence of RGBA frames into a GIF89a file using:
//! - Median-cut colour quantization to build a 256-colour global palette
//! - LZW compression (variable-width codes, LSB-first, GIF spec)

use std::io::Write;

// ─── Public surface used by mod.rs ───────────────────────────────────────────

/// A single captured frame as raw RGBA pixel data.
pub(crate) struct CapturedFrame {
    pub(crate) rgba: Vec<u8>,
}

/// A simple RGB color.
pub(crate) type Rgb = [u8; 3];

// ─── GIF encoder ─────────────────────────────────────────────────────────────

/// Encode multiple RGBA frames into an animated GIF file.
pub(crate) fn encode_animated_gif(
    path: &str,
    width: u32,
    height: u32,
    frames: &[CapturedFrame],
    delay_cs: u16,
) -> Result<(), String> {
    use std::fs::File;

    let mut file = File::create(path).map_err(|e| format!("creating file: {e}"))?;

    // Build a global palette from the first frame (256 colors via simple quantization)
    let palette = build_palette_median_cut(&frames[0].rgba);

    // GIF89a header
    file.write_all(b"GIF89a").map_err(|e| format!("writing header: {e}"))?;

    // Logical Screen Descriptor
    let w_bytes = (width as u16).to_le_bytes();
    let h_bytes = (height as u16).to_le_bytes();
    file.write_all(&w_bytes).map_err(|e| format!("writing width: {e}"))?;
    file.write_all(&h_bytes).map_err(|e| format!("writing height: {e}"))?;
    // packed: global color table flag=1, color resolution=7 (8 bits), sort=0, size=7 (256 entries)
    file.write_all(&[0xF7, 0x00, 0x00])
        .map_err(|e| format!("writing LSD: {e}"))?;

    // Global Color Table (256 * 3 = 768 bytes)
    let mut gct = [0u8; 768];
    for (i, color) in palette.iter().enumerate() {
        gct[i * 3] = color[0];
        gct[i * 3 + 1] = color[1];
        gct[i * 3 + 2] = color[2];
    }
    file.write_all(&gct).map_err(|e| format!("writing GCT: {e}"))?;

    // NETSCAPE2.0 Application Extension (for looping)
    file.write_all(&[
        0x21, 0xFF, 0x0B, // extension introducer, app extension label, block size
        b'N', b'E', b'T', b'S', b'C', b'A', b'P', b'E', b'2', b'.', b'0', // "NETSCAPE2.0"
        0x03, // sub-block size
        0x01, // sub-block ID
        0x00, 0x00, // loop count (0 = infinite)
        0x00, // block terminator
    ])
    .map_err(|e| format!("writing NETSCAPE ext: {e}"))?;

    // Write each frame
    for frame in frames {
        // Graphic Control Extension
        file.write_all(&[
            0x21, 0xF9, // extension introducer, GCE label
            0x04, // block size
            0x00, // packed: disposal=none, no user input, no transparent color
        ])
        .map_err(|e| format!("writing GCE: {e}"))?;
        file.write_all(&delay_cs.to_le_bytes())
            .map_err(|e| format!("writing delay: {e}"))?;
        file.write_all(&[0x00, 0x00]) // transparent color index, block terminator
            .map_err(|e| format!("writing GCE end: {e}"))?;

        // Image Descriptor
        file.write_all(&[0x2C]) // image separator
            .map_err(|e| format!("writing image sep: {e}"))?;
        file.write_all(&[0x00, 0x00, 0x00, 0x00]) // left, top (both 0)
            .map_err(|e| format!("writing image pos: {e}"))?;
        file.write_all(&w_bytes)
            .map_err(|e| format!("writing image width: {e}"))?;
        file.write_all(&h_bytes)
            .map_err(|e| format!("writing image height: {e}"))?;
        file.write_all(&[0x00]) // packed: no local color table, not interlaced
            .map_err(|e| format!("writing image desc packed: {e}"))?;

        // Quantize frame pixels to palette indices
        let indices = quantize_to_palette(&frame.rgba, &palette);

        // LZW compress and write image data
        let min_code_size = 8u8; // for 256 colors
        file.write_all(&[min_code_size])
            .map_err(|e| format!("writing min code size: {e}"))?;
        let compressed = lzw_compress(&indices, min_code_size);
        write_sub_blocks(&mut file, &compressed)?;
        file.write_all(&[0x00]) // block terminator
            .map_err(|e| format!("writing block terminator: {e}"))?;
    }

    // GIF Trailer
    file.write_all(&[0x3B]).map_err(|e| format!("writing trailer: {e}"))?;

    Ok(())
}

/// Build a 256-color palette using median-cut quantization.
pub(crate) fn build_palette_median_cut(rgba: &[u8]) -> Vec<Rgb> {
    // Sample pixels (every 4th pixel for speed on large images)
    let pixels: Vec<Rgb> = rgba
        .chunks_exact(4)
        .step_by(4)
        .map(|c| [c[0], c[1], c[2]])
        .collect();

    if pixels.is_empty() {
        // Fallback: grayscale palette
        return (0..=255u8).map(|i| [i, i, i]).collect();
    }

    // Median-cut: recursively split the largest-range box
    let mut boxes: Vec<Vec<Rgb>> = vec![pixels];

    while boxes.len() < 256 {
        // Find the box with the largest color range
        let split_idx = match boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .max_by_key(|(_, b)| {
                let (mut min_r, mut min_g, mut min_b) = (255u8, 255u8, 255u8);
                let (mut max_r, mut max_g, mut max_b) = (0u8, 0u8, 0u8);
                for p in b.iter() {
                    min_r = min_r.min(p[0]);
                    min_g = min_g.min(p[1]);
                    min_b = min_b.min(p[2]);
                    max_r = max_r.max(p[0]);
                    max_g = max_g.max(p[1]);
                    max_b = max_b.max(p[2]);
                }
                let range_r = (max_r - min_r) as u32;
                let range_g = (max_g - min_g) as u32;
                let range_b = (max_b - min_b) as u32;
                range_r.max(range_g).max(range_b)
            }) {
            Some((idx, _)) => idx,
            None => break, // no boxes with > 1 pixel left to split
        };

        let mut box_to_split = boxes.swap_remove(split_idx);

        // Find which channel has the largest range
        let (mut min_r, mut min_g, mut min_b) = (255u8, 255u8, 255u8);
        let (mut max_r, mut max_g, mut max_b) = (0u8, 0u8, 0u8);
        for p in box_to_split.iter() {
            min_r = min_r.min(p[0]);
            min_g = min_g.min(p[1]);
            min_b = min_b.min(p[2]);
            max_r = max_r.max(p[0]);
            max_g = max_g.max(p[1]);
            max_b = max_b.max(p[2]);
        }
        let range_r = max_r - min_r;
        let range_g = max_g - min_g;
        let range_b = max_b - min_b;

        let channel = if range_r >= range_g && range_r >= range_b {
            0
        } else if range_g >= range_b {
            1
        } else {
            2
        };

        box_to_split.sort_unstable_by_key(|p| p[channel]);
        let mid = box_to_split.len() / 2;
        let right = box_to_split.split_off(mid);
        boxes.push(box_to_split);
        boxes.push(right);
    }

    // Average each box to get the palette color
    let mut palette: Vec<Rgb> = boxes
        .iter()
        .map(|b| {
            if b.is_empty() {
                return [0, 0, 0];
            }
            let (mut sr, mut sg, mut sb) = (0u64, 0u64, 0u64);
            for p in b {
                sr += p[0] as u64;
                sg += p[1] as u64;
                sb += p[2] as u64;
            }
            let n = b.len() as u64;
            [(sr / n) as u8, (sg / n) as u8, (sb / n) as u8]
        })
        .collect();

    // Pad to 256 entries if needed
    while palette.len() < 256 {
        palette.push([0, 0, 0]);
    }

    palette
}

/// Map each RGBA pixel to the closest palette index.
pub(crate) fn quantize_to_palette(rgba: &[u8], palette: &[Rgb]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .map(|px| {
            let r = px[0] as i32;
            let g = px[1] as i32;
            let b = px[2] as i32;
            let mut best = 0u8;
            let mut best_dist = i32::MAX;
            for (i, pal) in palette.iter().enumerate() {
                let dr = r - pal[0] as i32;
                let dg = g - pal[1] as i32;
                let db = b - pal[2] as i32;
                let dist = dr * dr + dg * dg + db * db;
                if dist < best_dist {
                    best_dist = dist;
                    best = i as u8;
                    if dist == 0 {
                        break;
                    }
                }
            }
            best
        })
        .collect()
}

/// LZW compress a stream of palette indices for GIF.
/// Uses variable-width codes starting at (min_code_size + 1) bits.
pub(crate) fn lzw_compress(indices: &[u8], min_code_size: u8) -> Vec<u8> {
    let clear_code = 1u16 << min_code_size;
    let eoi_code = clear_code + 1;
    let mut next_code = clear_code + 2;
    let mut code_size = min_code_size as u32 + 1;

    // Dictionary: maps (prefix_code, byte) -> code
    // Using a HashMap for simplicity. For GIF LZW this is fine.
    let mut dict = std::collections::HashMap::<(u16, u8), u16>::new();

    // Single-byte entries (0..clear_code) are implicit — they map to themselves.
    // The dictionary only stores multi-byte sequences discovered during compression.

    let mut output = BitWriter::new();

    // Write clear code
    output.write_bits(clear_code as u32, code_size);

    if indices.is_empty() {
        output.write_bits(eoi_code as u32, code_size);
        return output.finish();
    }

    let mut prefix = indices[0] as u16;

    for &byte in &indices[1..] {
        let key = (prefix, byte);
        if let Some(&code) = dict.get(&key) {
            prefix = code;
        } else {
            // Output the prefix code
            output.write_bits(prefix as u32, code_size);

            // Add new entry to dictionary
            if next_code < 4096 {
                dict.insert(key, next_code);
                next_code += 1;

                // Increase code size if needed
                if next_code > (1 << code_size) && code_size < 12 {
                    code_size += 1;
                }
            } else {
                // Dictionary full — emit clear code and reset
                output.write_bits(clear_code as u32, code_size);
                dict.clear();
                next_code = clear_code + 2;
                code_size = min_code_size as u32 + 1;
            }

            prefix = byte as u16;
        }
    }

    // Output remaining prefix
    output.write_bits(prefix as u32, code_size);

    // End of information
    output.write_bits(eoi_code as u32, code_size);

    output.finish()
}

/// Bit-level writer for LZW output (LSB first, as required by GIF).
pub(crate) struct BitWriter {
    buf: Vec<u8>,
    current: u32,
    bits_in_current: u32,
}

impl BitWriter {
    pub(crate) fn new() -> Self {
        Self {
            buf: Vec::new(),
            current: 0,
            bits_in_current: 0,
        }
    }

    pub(crate) fn write_bits(&mut self, value: u32, num_bits: u32) {
        self.current |= value << self.bits_in_current;
        self.bits_in_current += num_bits;
        while self.bits_in_current >= 8 {
            self.buf.push((self.current & 0xFF) as u8);
            self.current >>= 8;
            self.bits_in_current -= 8;
        }
    }

    pub(crate) fn finish(mut self) -> Vec<u8> {
        if self.bits_in_current > 0 {
            self.buf.push((self.current & 0xFF) as u8);
        }
        self.buf
    }
}

/// Write data as GIF sub-blocks (max 255 bytes each).
pub(crate) fn write_sub_blocks(file: &mut impl Write, data: &[u8]) -> Result<(), String> {
    for chunk in data.chunks(255) {
        file.write_all(&[chunk.len() as u8])
            .map_err(|e| format!("writing sub-block size: {e}"))?;
        file.write_all(chunk)
            .map_err(|e| format!("writing sub-block data: {e}"))?;
    }
    Ok(())
}
