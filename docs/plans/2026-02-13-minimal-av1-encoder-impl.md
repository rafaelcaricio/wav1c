# Minimal AV1 Encoder Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Rust CLI (`wav1c`) that outputs a valid 64x64 AV1 key frame in an IVF container, decodable by dav1d.

**Architecture:** Six Rust source files: a bit-level writer, OBU framing, IVF container writer, sequence header encoder, frame encoder (header + hardcoded tile data), and a CLI main. No external crate dependencies. The bitstream structure is: IVF header → Temporal Delimiter OBU → Sequence Header OBU → Frame OBU (frame header + hardcoded tile bytes).

**Tech Stack:** Rust (edition 2021), cargo, dav1d (at `../dav1d/build/tools/dav1d`) for validation.

**Reference bitstream (28 bytes of OBU data):**
```
12 00 0a 06 18 15 7f fc 00 08 32 10 18 00 00 00 40 0a 05 79 52 6e 43 d7 e6 42 63 20
```

---

### Task 1: Scaffold the Rust Project

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/lib.rs`

**Step 1: Initialize cargo project**

Run: `cargo init --name wav1c /Users/rafaelcaricio/development/wav1c`

If the directory already has files, cargo may complain. In that case, create `Cargo.toml` and `src/main.rs` manually.

**Step 2: Verify `Cargo.toml`**

```toml
[package]
name = "wav1c"
version = "0.1.0"
edition = "2021"
```

**Step 3: Create `src/lib.rs`** with module declarations:

```rust
pub mod bitwriter;
pub mod obu;
pub mod ivf;
pub mod sequence;
pub mod frame;
```

**Step 4: Create empty module files**

Create these empty files so the project compiles:
- `src/bitwriter.rs`
- `src/obu.rs`
- `src/ivf.rs`
- `src/sequence.rs`
- `src/frame.rs`

**Step 5: Verify it compiles**

Run: `cargo build`
Expected: Compiles with no errors (may have unused warnings).

**Step 6: Commit**

```
git add Cargo.toml src/
git commit -m "Scaffold wav1c Rust project with module structure"
```

---

### Task 2: Implement BitWriter

The bit-level writer packs values MSB-first into a byte buffer. This is the foundation for encoding AV1 headers.

**Files:**
- Create: `src/bitwriter.rs`

**Step 1: Write tests**

Add to `src/bitwriter.rs`:

```rust
pub struct BitWriter {
    buf: Vec<u8>,
    current_byte: u8,
    bits_in_current: u8,
}

impl BitWriter {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            current_byte: 0,
            bits_in_current: 0,
        }
    }

    pub fn write_bit(&mut self, bit: bool) {
        todo!()
    }

    pub fn write_bits(&mut self, value: u64, n: u8) {
        todo!()
    }

    pub fn byte_align(&mut self) {
        todo!()
    }

    pub fn finalize(self) -> Vec<u8> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_bit_true() {
        let mut w = BitWriter::new();
        w.write_bit(true);
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0x80]);
    }

    #[test]
    fn single_bit_false() {
        let mut w = BitWriter::new();
        w.write_bit(false);
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0x00]);
    }

    #[test]
    fn write_byte_value() {
        let mut w = BitWriter::new();
        w.write_bits(0xAB, 8);
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0xAB]);
    }

    #[test]
    fn write_3_bits() {
        let mut w = BitWriter::new();
        w.write_bits(0b101, 3);
        let bytes = w.finalize();
        // 101_00000 = 0xA0
        assert_eq!(bytes, vec![0xA0]);
    }

    #[test]
    fn write_across_byte_boundary() {
        let mut w = BitWriter::new();
        w.write_bits(0b11111, 5);
        w.write_bits(0b11111, 5);
        let bytes = w.finalize();
        // 11111_111 11_000000 = 0xFF 0xC0
        assert_eq!(bytes, vec![0xFF, 0xC0]);
    }

    #[test]
    fn byte_align_no_op_when_aligned() {
        let mut w = BitWriter::new();
        w.write_bits(0xFF, 8);
        w.byte_align();
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0xFF]);
    }

    #[test]
    fn byte_align_pads_with_zeros() {
        let mut w = BitWriter::new();
        w.write_bits(0b111, 3);
        w.byte_align();
        let bytes = w.finalize();
        // 111_00000 = 0xE0
        assert_eq!(bytes, vec![0xE0]);
    }

    #[test]
    fn empty_writer() {
        let w = BitWriter::new();
        let bytes = w.finalize();
        assert_eq!(bytes, vec![]);
    }

    #[test]
    fn write_16_bits() {
        let mut w = BitWriter::new();
        w.write_bits(0xCAFE, 16);
        let bytes = w.finalize();
        assert_eq!(bytes, vec![0xCA, 0xFE]);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib bitwriter`
Expected: All tests fail with `not yet implemented`.

**Step 3: Implement BitWriter**

Replace the `todo!()` bodies:

```rust
pub fn write_bit(&mut self, bit: bool) {
    self.current_byte = (self.current_byte << 1) | (bit as u8);
    self.bits_in_current += 1;
    if self.bits_in_current == 8 {
        self.buf.push(self.current_byte);
        self.current_byte = 0;
        self.bits_in_current = 0;
    }
}

pub fn write_bits(&mut self, value: u64, n: u8) {
    for i in (0..n).rev() {
        self.write_bit((value >> i) & 1 == 1);
    }
}

pub fn byte_align(&mut self) {
    if self.bits_in_current > 0 {
        self.current_byte <<= 8 - self.bits_in_current;
        self.buf.push(self.current_byte);
        self.current_byte = 0;
        self.bits_in_current = 0;
    }
}

pub fn finalize(mut self) -> Vec<u8> {
    self.byte_align();
    self.buf
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib bitwriter`
Expected: All 9 tests pass.

**Step 5: Commit**

```
git add src/bitwriter.rs
git commit -m "Implement MSB-first BitWriter with tests"
```

---

### Task 3: Implement OBU Framing

OBU framing wraps payloads with a header byte and leb128-encoded size.

**Files:**
- Create: `src/obu.rs`

**Step 1: Write tests**

```rust
#[repr(u8)]
#[derive(Clone, Copy)]
pub enum ObuType {
    SequenceHeader = 1,
    TemporalDelimiter = 2,
    Frame = 6,
}

pub fn leb128_encode(mut value: u64) -> Vec<u8> {
    todo!()
}

pub fn obu_wrap(obu_type: ObuType, payload: &[u8]) -> Vec<u8> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leb128_zero() {
        assert_eq!(leb128_encode(0), vec![0x00]);
    }

    #[test]
    fn leb128_small() {
        assert_eq!(leb128_encode(6), vec![0x06]);
    }

    #[test]
    fn leb128_127() {
        assert_eq!(leb128_encode(127), vec![0x7F]);
    }

    #[test]
    fn leb128_128() {
        assert_eq!(leb128_encode(128), vec![0x80, 0x01]);
    }

    #[test]
    fn leb128_300() {
        // 300 = 0x12C -> low 7 bits: 0x2C | 0x80 = 0xAC, high: 0x02
        assert_eq!(leb128_encode(300), vec![0xAC, 0x02]);
    }

    #[test]
    fn obu_temporal_delimiter() {
        let result = obu_wrap(ObuType::TemporalDelimiter, &[]);
        // header: (2 << 3) | (1 << 1) = 0x12, size: 0x00
        assert_eq!(result, vec![0x12, 0x00]);
    }

    #[test]
    fn obu_sequence_header_6bytes() {
        let payload = vec![0x18, 0x15, 0x7f, 0xfc, 0x00, 0x08];
        let result = obu_wrap(ObuType::SequenceHeader, &payload);
        // header: (1 << 3) | (1 << 1) = 0x0A, size: 0x06
        assert_eq!(result[0], 0x0A);
        assert_eq!(result[1], 0x06);
        assert_eq!(&result[2..], &payload[..]);
    }

    #[test]
    fn obu_frame_16bytes() {
        let payload = vec![0u8; 16];
        let result = obu_wrap(ObuType::Frame, &payload);
        // header: (6 << 3) | (1 << 1) = 0x32, size: 0x10
        assert_eq!(result[0], 0x32);
        assert_eq!(result[1], 0x10);
        assert_eq!(result.len(), 2 + 16);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib obu`
Expected: Failures with `not yet implemented`.

**Step 3: Implement leb128_encode and obu_wrap**

```rust
pub fn leb128_encode(mut value: u64) -> Vec<u8> {
    let mut result = Vec::new();
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        result.push(byte);
        if value == 0 {
            break;
        }
    }
    result
}

pub fn obu_wrap(obu_type: ObuType, payload: &[u8]) -> Vec<u8> {
    let header_byte = (obu_type as u8) << 3 | (1 << 1);
    let size_bytes = leb128_encode(payload.len() as u64);
    let mut result = Vec::with_capacity(1 + size_bytes.len() + payload.len());
    result.push(header_byte);
    result.extend_from_slice(&size_bytes);
    result.extend_from_slice(payload);
    result
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib obu`
Expected: All 7 tests pass.

**Step 5: Commit**

```
git add src/obu.rs
git commit -m "Implement OBU framing with leb128 encoding"
```

---

### Task 4: Implement IVF Container Writer

**Files:**
- Create: `src/ivf.rs`

**Step 1: Write tests**

```rust
use std::io::{self, Write};

pub fn write_ivf_header<W: Write>(
    writer: &mut W,
    width: u16,
    height: u16,
    num_frames: u32,
) -> io::Result<()> {
    todo!()
}

pub fn write_ivf_frame<W: Write>(
    writer: &mut W,
    timestamp: u64,
    frame_data: &[u8],
) -> io::Result<()> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ivf_header_64x64() {
        let mut buf = Vec::new();
        write_ivf_header(&mut buf, 64, 64, 1).unwrap();
        assert_eq!(buf.len(), 32);
        assert_eq!(&buf[0..4], b"DKIF");
        assert_eq!(&buf[4..6], &0u16.to_le_bytes());
        assert_eq!(&buf[6..8], &32u16.to_le_bytes());
        assert_eq!(&buf[8..12], b"AV01");
        assert_eq!(&buf[12..14], &64u16.to_le_bytes());
        assert_eq!(&buf[14..16], &64u16.to_le_bytes());
        assert_eq!(&buf[16..20], &25u32.to_le_bytes());
        assert_eq!(&buf[20..24], &1u32.to_le_bytes());
        assert_eq!(&buf[24..28], &1u32.to_le_bytes());
        assert_eq!(&buf[28..32], &0u32.to_le_bytes());
    }

    #[test]
    fn ivf_frame_wrapper() {
        let mut buf = Vec::new();
        let data = vec![0xAA, 0xBB, 0xCC];
        write_ivf_frame(&mut buf, 0, &data).unwrap();
        assert_eq!(buf.len(), 12 + 3);
        assert_eq!(&buf[0..4], &3u32.to_le_bytes());
        assert_eq!(&buf[4..12], &0u64.to_le_bytes());
        assert_eq!(&buf[12..], &[0xAA, 0xBB, 0xCC]);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib ivf`
Expected: Failures.

**Step 3: Implement IVF writer**

```rust
pub fn write_ivf_header<W: Write>(
    writer: &mut W,
    width: u16,
    height: u16,
    num_frames: u32,
) -> io::Result<()> {
    writer.write_all(b"DKIF")?;
    writer.write_all(&0u16.to_le_bytes())?;       // version
    writer.write_all(&32u16.to_le_bytes())?;       // header length
    writer.write_all(b"AV01")?;                    // codec FourCC
    writer.write_all(&width.to_le_bytes())?;
    writer.write_all(&height.to_le_bytes())?;
    writer.write_all(&25u32.to_le_bytes())?;       // framerate numerator
    writer.write_all(&1u32.to_le_bytes())?;        // framerate denominator
    writer.write_all(&num_frames.to_le_bytes())?;
    writer.write_all(&0u32.to_le_bytes())?;        // unused
    Ok(())
}

pub fn write_ivf_frame<W: Write>(
    writer: &mut W,
    timestamp: u64,
    frame_data: &[u8],
) -> io::Result<()> {
    writer.write_all(&(frame_data.len() as u32).to_le_bytes())?;
    writer.write_all(&timestamp.to_le_bytes())?;
    writer.write_all(frame_data)?;
    Ok(())
}
```

**Step 4: Run tests**

Run: `cargo test --lib ivf`
Expected: Both tests pass.

**Step 5: Commit**

```
git add src/ivf.rs
git commit -m "Implement IVF container writer"
```

---

### Task 5: Implement Sequence Header Encoder

**Files:**
- Create: `src/sequence.rs`

**Step 1: Write test**

The test compares our output against the known-good reference bytes.

```rust
use crate::bitwriter::BitWriter;

pub fn encode_sequence_header() -> Vec<u8> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_header_matches_reference() {
        let bytes = encode_sequence_header();
        assert_eq!(bytes, vec![0x18, 0x15, 0x7f, 0xfc, 0x00, 0x08]);
    }

    #[test]
    fn sequence_header_is_6_bytes() {
        let bytes = encode_sequence_header();
        assert_eq!(bytes.len(), 6);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib sequence`
Expected: Failures.

**Step 3: Implement sequence header encoder**

```rust
pub fn encode_sequence_header() -> Vec<u8> {
    let mut w = BitWriter::new();

    w.write_bits(0, 3);  // seq_profile = 0
    w.write_bit(true);   // still_picture = 1
    w.write_bit(true);   // reduced_still_picture_header = 1
    w.write_bits(0, 5);  // seq_level_idx[0] = 0

    w.write_bits(5, 4);  // frame_width_bits_minus_1 = 5
    w.write_bits(5, 4);  // frame_height_bits_minus_1 = 5
    w.write_bits(63, 6); // max_frame_width_minus_1 = 63
    w.write_bits(63, 6); // max_frame_height_minus_1 = 63

    w.write_bit(false);  // use_128x128_superblock = 0
    w.write_bit(false);  // enable_filter_intra = 0
    w.write_bit(false);  // enable_intra_edge_filter = 0
    w.write_bit(false);  // enable_superres = 0
    w.write_bit(false);  // enable_cdef = 0
    w.write_bit(false);  // enable_restoration = 0

    // color_config
    w.write_bit(false);  // high_bitdepth = 0
    w.write_bit(false);  // mono_chrome = 0
    w.write_bit(false);  // color_description_present_flag = 0
    w.write_bit(false);  // color_range = 0 (limited/studio)
    w.write_bits(0, 2);  // chroma_sample_position = 0 (CSP_UNKNOWN)
    w.write_bit(false);  // separate_uv_delta_q = 0
    w.write_bit(false);  // film_grain_params_present = 0

    // trailing_bits: 1 followed by zeros to fill byte
    w.write_bit(true);   // trailing_one_bit
    w.write_bits(0, 3);  // trailing_zero_bits (pad to 48 bits = 6 bytes)

    w.finalize()
}
```

**Step 4: Run tests**

Run: `cargo test --lib sequence`
Expected: Both tests pass.

**Step 5: Commit**

```
git add src/sequence.rs
git commit -m "Implement AV1 sequence header encoder (reduced still picture)"
```

---

### Task 6: Implement Frame Encoder

The frame encoder constructs the uncompressed frame header and appends the hardcoded tile data.

**Files:**
- Create: `src/frame.rs`

**Step 1: Write test**

```rust
use crate::bitwriter::BitWriter;

const TILE_DATA: [u8; 12] = [
    0x40, 0x0a, 0x05, 0x79, 0x52, 0x6e,
    0x43, 0xd7, 0xe6, 0x42, 0x63, 0x20,
];

pub fn encode_frame() -> Vec<u8> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_payload_matches_reference() {
        let bytes = encode_frame();
        let expected = vec![
            0x18, 0x00, 0x00, 0x00,
            0x40, 0x0a, 0x05, 0x79, 0x52, 0x6e,
            0x43, 0xd7, 0xe6, 0x42, 0x63, 0x20,
        ];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn frame_payload_is_16_bytes() {
        let bytes = encode_frame();
        assert_eq!(bytes.len(), 16);
    }

    #[test]
    fn frame_header_is_first_4_bytes() {
        let bytes = encode_frame();
        assert_eq!(&bytes[..4], &[0x18, 0x00, 0x00, 0x00]);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib frame`
Expected: Failures.

**Step 3: Implement frame encoder**

```rust
pub fn encode_frame() -> Vec<u8> {
    let mut w = BitWriter::new();

    w.write_bit(false);      // disable_cdf_update = 0
    w.write_bit(false);      // allow_screen_content_tools = 0
    w.write_bit(false);      // render_and_frame_size_different = 0
    w.write_bits(192, 8);    // base_q_idx = 192
    w.write_bit(false);      // DeltaQYDc delta_coded = 0
    w.write_bit(false);      // diff_uv_delta = 0
    w.write_bit(false);      // DeltaQUDc delta_coded = 0
    w.write_bit(false);      // DeltaQUAc delta_coded = 0
    w.write_bit(false);      // using_qmatrix = 0
    w.write_bit(false);      // segmentation_enabled = 0
    w.write_bit(false);      // delta_q_present = 0
    w.write_bits(0, 6);      // loop_filter_level[0] = 0
    w.write_bits(0, 6);      // loop_filter_level[1] = 0
    w.write_bit(false);      // tx_mode_select = 0
    w.write_bit(true);       // uniform_tile_spacing_flag = 1

    let mut header_bytes = w.finalize();
    header_bytes.extend_from_slice(&TILE_DATA);
    header_bytes
}
```

**Step 4: Run tests**

Run: `cargo test --lib frame`
Expected: All 3 tests pass.

**Important note:** The reference has `uniform_tile_spacing_flag = 0` (bit 31 = 0), producing
frame header `0x18 0x00 0x00 0x00`. Our encoder uses `uniform_tile_spacing_flag = 1` (bit 31 = 1),
which produces `0x18 0x00 0x00 0x01`. For a single 64x64 superblock both are functionally
equivalent, but the bytes differ. The test expects our encoder's output (with flag = 1), not the
exact reference bytes. Validation with dav1d confirms correctness.

**Actually — correction:** Let me recalculate. The frame header is 32 bits:
- Bits 0-2: `000` (disable_cdf, allow_sct, render_diff)
- Bits 3-10: `11000000` (base_q_idx = 192)
- Bits 11-17: `0000000` (all delta/qmatrix/seg/delta_q)
- Bits 18-29: `000000 000000` (loop filter levels)
- Bit 30: `0` (tx_mode_select)
- Bit 31: `1` (uniform_tile_spacing_flag)

Binary: `00011000 00000000 00000000 00000001` = `0x18 0x00 0x00 0x01`

But the reference has bit 31 = 0: `0x18 0x00 0x00 0x00`.

To match the reference exactly for byte-level validation, use `uniform_tile_spacing_flag = 0`:

```rust
w.write_bit(false);      // uniform_tile_spacing_flag = 0 (matches reference)
```

This produces `0x18 0x00 0x00 0x00`, matching the reference. Both values are valid for
a single-superblock frame. Using 0 lets us do exact byte comparison.

Update the test expected bytes to `0x18, 0x00, 0x00, 0x00` and the implementation to
`write_bit(false)` for the tile spacing flag.

**Step 5: Commit**

```
git add src/frame.rs
git commit -m "Implement frame header encoder with hardcoded tile data"
```

---

### Task 7: Wire Up Main and Full Bitstream Assembly

**Files:**
- Create: `src/main.rs`
- Modify: `src/lib.rs`

**Step 1: Write integration test**

Add to `src/lib.rs`:

```rust
pub mod bitwriter;
pub mod obu;
pub mod ivf;
pub mod sequence;
pub mod frame;

pub fn encode_av1_ivf() -> Vec<u8> {
    let td = obu::obu_wrap(obu::ObuType::TemporalDelimiter, &[]);
    let seq = obu::obu_wrap(obu::ObuType::SequenceHeader, &sequence::encode_sequence_header());
    let frm = obu::obu_wrap(obu::ObuType::Frame, &frame::encode_frame());

    let mut frame_data = Vec::new();
    frame_data.extend_from_slice(&td);
    frame_data.extend_from_slice(&seq);
    frame_data.extend_from_slice(&frm);

    let mut output = Vec::new();
    ivf::write_ivf_header(&mut output, 64, 64, 1).unwrap();
    ivf::write_ivf_frame(&mut output, 0, &frame_data).unwrap();
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_bitstream_frame_data_matches_reference() {
        let output = encode_av1_ivf();
        // IVF header is 32 bytes, frame wrapper is 12 bytes, frame data starts at byte 44
        let frame_data = &output[44..];
        let expected = hex_to_bytes("12000a0618157ffc0008321018000000400a0579526e43d7e6426320");
        assert_eq!(frame_data, &expected[..]);
    }

    #[test]
    fn output_total_size() {
        let output = encode_av1_ivf();
        // 32 (IVF header) + 4 (frame size) + 8 (timestamp) + 28 (frame data) = 72
        assert_eq!(output.len(), 72);
    }

    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib tests`
Expected: Failures (functions not connected yet, or `todo!` if still present).

Actually — by this point all functions are implemented. The test should pass immediately after wiring. Run it:

Run: `cargo test --lib`
Expected: All tests pass (bitwriter: 9, obu: 7, ivf: 2, sequence: 2, frame: 3, lib: 2 = 25 total).

**Step 3: Implement main.rs**

```rust
use std::env;
use std::fs::File;
use std::io::Write;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 6 || args[4] != "-o" {
        eprintln!("Usage: wav1c <Y> <U> <V> -o <output.ivf>");
        eprintln!("Example: wav1c 81 91 81 -o green.ivf");
        process::exit(1);
    }

    let _y: u8 = args[1].parse().unwrap_or_else(|_| {
        eprintln!("Error: Y must be 0-255");
        process::exit(1);
    });
    let _u: u8 = args[2].parse().unwrap_or_else(|_| {
        eprintln!("Error: U must be 0-255");
        process::exit(1);
    });
    let _v: u8 = args[3].parse().unwrap_or_else(|_| {
        eprintln!("Error: V must be 0-255");
        process::exit(1);
    });
    let output_path = &args[5];

    eprintln!("Warning: Color input is ignored in this iteration. Output is always solid green (Y=81, U=91, V=81).");

    let output = wav1c::encode_av1_ivf();

    let mut file = File::create(output_path).unwrap_or_else(|e| {
        eprintln!("Error creating {}: {}", output_path, e);
        process::exit(1);
    });
    file.write_all(&output).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {}", output_path, e);
        process::exit(1);
    });

    eprintln!("Wrote {} bytes to {}", output.len(), output_path);
}
```

**Step 4: Build and test the binary**

Run: `cargo build`
Expected: Compiles successfully.

Run: `cargo run -- 81 91 81 -o /tmp/claude/wav1c_test.ivf`
Expected: Prints warning and "Wrote 72 bytes to /tmp/claude/wav1c_test.ivf".

**Step 5: Commit**

```
git add src/main.rs src/lib.rs
git commit -m "Wire up CLI and full bitstream assembly"
```

---

### Task 8: Validate with dav1d

**Files:** None (validation only)

**Step 1: Decode our output with dav1d**

Run: `../dav1d/build/tools/dav1d -i /tmp/claude/wav1c_test.ivf -o /tmp/claude/wav1c_decoded.y4m`
Expected: `Decoded 1/1 frames` with exit code 0.

**Step 2: Verify decoded frame content**

Run a quick check that the decoded Y4M has uniform Y=81, U=91, V=81 values:

```bash
/usr/bin/python3 -c "
with open('/tmp/claude/wav1c_decoded.y4m','rb') as f:
    data = f.read()
    frame_start = data.index(b'FRAME\n') + 6
    y = data[frame_start:frame_start+64*64]
    u = data[frame_start+64*64:frame_start+64*64+32*32]
    v = data[frame_start+64*64+32*32:frame_start+64*64+32*32+32*32]
    assert all(b == 81 for b in y), f'Y mismatch: {set(y)}'
    assert all(b == 91 for b in u), f'U mismatch: {set(u)}'
    assert all(b == 81 for b in v), f'V mismatch: {set(v)}'
    print('PASS: All pixels match expected values')
"
```

Expected: `PASS: All pixels match expected values`

**Step 3: If validation fails**

Compare our output hex against the reference:
```
Reference: 12000a0618157ffc0008321018000000400a0579526e43d7e6426320
```

Use `xxd` on our output (bytes 44 onward) to find the divergence point.

**Step 4: Commit (tag as validated)**

```
git tag v0.1.0-alpha
```

---

### Task 9: Add dav1d Integration Test (Optional)

**Files:**
- Create: `tests/integration.rs`

**Step 1: Write integration test**

```rust
use std::process::Command;
use std::io::Write;

#[test]
fn dav1d_decodes_output() {
    let dav1d_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dav1d/build/tools/dav1d");

    if !dav1d_path.exists() {
        eprintln!("Skipping: dav1d not found at {:?}", dav1d_path);
        return;
    }

    let output = wav1c::encode_av1_ivf();
    let ivf_path = std::env::temp_dir().join("wav1c_test.ivf");
    let mut file = std::fs::File::create(&ivf_path).unwrap();
    file.write_all(&output).unwrap();

    let result = Command::new(dav1d_path)
        .args(["-i", ivf_path.to_str().unwrap(), "-o", "/dev/null"])
        .output()
        .expect("Failed to run dav1d");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "dav1d failed: {}", stderr);
    assert!(
        stderr.contains("Decoded 1/1 frames"),
        "Unexpected dav1d output: {}",
        stderr
    );
}
```

**Step 2: Run integration test**

Run: `cargo test --test integration`
Expected: PASS (or skip message if dav1d not found).

**Step 3: Commit**

```
git add tests/integration.rs
git commit -m "Add dav1d integration test"
```
