# Phase 2+3: MSAC Encoder + Tile Encoding — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the hardcoded tile data blob with a real MSAC encoder and tile encoder that produces valid AV1 tile data for any solid Y/U/V color at 64x64.

**Architecture:** Three new modules: `msac.rs` (arithmetic encoder), `cdf.rs` (default CDF tables extracted from dav1d), and `tile.rs` (tile-level symbol sequencer). The tile encoder walks the 64x64 block structure, encoding partition/mode/coefficient symbols via the MSAC encoder using the CDF tables. Existing modules (`frame.rs`, `lib.rs`, `main.rs`) are updated to pass Y/U/V values through.

**Tech Stack:** Rust (edition 2024), dav1d (at `../dav1d/build/tools/dav1d`) for round-trip validation.

**Key reference files in dav1d:**
- `../dav1d/src/msac.c` — MSAC decoder algorithm (we invert to build encoder)
- `../dav1d/src/cdf.c` — Default CDF table values
- `../dav1d/src/decode.c` — Symbol sequence for tile decoding
- `../dav1d/src/recon_tmpl.c` — Coefficient decoding logic

---

### Task 1: Extract Default CDF Tables from dav1d

Extract the CDF table values needed for encoding a 64x64 intra key frame. Write a Python script that parses dav1d's `cdf.c` and generates a Rust module with const arrays.

**Files:**
- Create: `scripts/extract_cdfs.py`
- Create: `src/cdf.rs`

**Step 1: Write the CDF extraction script**

Create `scripts/extract_cdfs.py` that reads `/Users/rafaelcaricio/development/dav1d/src/cdf.c` and `/Users/rafaelcaricio/development/dav1d/src/cdf.h` to extract default CDF values. The script should output Rust source code.

The tables we need (with their dav1d names and dimensions):

| dav1d Name | Rust Name | Dimensions | Used For |
|-----------|-----------|------------|----------|
| `default_kf_y_mode_cdf` | `DEFAULT_KF_Y_MODE_CDF` | `[5][5][16]` | Key frame Y mode (13 symbols + 3 padding) |
| `default_uv_mode_*_cdf` | `DEFAULT_UV_MODE_CDF` | `[2][13][16]` | UV mode (cfl_allowed × y_mode) |
| `default_partition_cdf` | `DEFAULT_PARTITION_CDF` | `[5][4][16]` | Partition (bl × ctx) |
| `default_skip_cdf` | `DEFAULT_SKIP_CDF` | `[3][4]` | Skip flag (3 contexts, 2 symbols + padding) |
| `default_coef_cdf[4].skip` | `DEFAULT_TXB_SKIP_CDF` | `[4][5][13][4]` | All-zero coeff flag (qctx × tx_sz × ctx) |
| `default_coef_cdf[4].eob_bin_1024` | `DEFAULT_EOB_BIN_1024_CDF` | `[4][2][16]` | EOB position for 1024 coeffs (qctx × chroma) |
| `default_coef_cdf[4].eob_bin_512` | `DEFAULT_EOB_BIN_512_CDF` | `[4][2][16]` | EOB position for 512 coeffs |
| `default_coef_cdf[4].eob_base_tok` | `DEFAULT_EOB_BASE_TOK_CDF` | `[4][5][2][4][4]` | Base coeff at EOB |
| `default_coef_cdf[4].base_tok` | `DEFAULT_BASE_TOK_CDF` | `[4][5][2][41][4]` | Base coefficient tokens |
| `default_coef_cdf[4].br_tok` | `DEFAULT_BR_TOK_CDF` | `[4][4][2][21][4]` | Coefficient bracket |
| `default_coef_cdf[4].dc_sign` | `DEFAULT_DC_SIGN_CDF` | `[4][2][3][4]` | DC coefficient sign (qctx × chroma × ctx) |
| `default_coef_cdf[4].eob_hi_bit` | `DEFAULT_EOB_HI_BIT_CDF` | `[4][5][2][11][4]` | EOB high bit |
| `default_txtp_intra1_cdf` | `DEFAULT_TXTP_INTRA1_CDF` | `[2][13][8]` | Transform type set 1 |
| `default_txtp_intra2_cdf` | `DEFAULT_TXTP_INTRA2_CDF` | `[3][13][8]` | Transform type set 2 |

The CDF format in dav1d: `CDF1(x)` expands to `32768 - x`. So `CDF1(16000)` = 16768. The CDFs are "inverse cumulative" — cdf[i] represents the probability of symbols > i.

Each CDF array has an extra element at the end (the count field) initialized to 0, used for adaptation rate.

**Step 2: Run the extraction script**

Run: `python3 scripts/extract_cdfs.py > src/cdf.rs`

If automated extraction proves too complex, manually transcribe the critical tables. The script should at minimum extract: `DEFAULT_KF_Y_MODE_CDF`, `DEFAULT_PARTITION_CDF`, `DEFAULT_SKIP_CDF`, `DEFAULT_DC_SIGN_CDF`, and the coefficient CDFs for qctx=3 (our base_q_idx=192 maps to qctx=3).

**Step 3: Create `src/cdf.rs` with CdfContext struct**

The generated file should include a `CdfContext` struct with mutable CDF arrays:

```rust
#[derive(Clone)]
pub struct CdfContext {
    pub kfym: [[[u16; 16]; 5]; 5],
    pub uv_mode: [[[u16; 16]; 13]; 2],
    pub partition: [[[u16; 16]; 4]; 5],
    pub skip: [[u16; 4]; 3],
    pub txb_skip: [[[u16; 4]; 13]; 5],
    pub eob_bin_1024: [[u16; 16]; 2],
    pub eob_bin_512: [[u16; 16]; 2],
    pub eob_base_tok: [[[u16; 4]; 4]; 2],  // [tx_ctx mapped][chroma][ctx]
    pub base_tok: [[[u16; 4]; 41]; 2],     // [tx_ctx mapped][chroma][ctx]
    pub br_tok: [[[u16; 4]; 21]; 2],       // [min(tx_ctx,3)][chroma][ctx]
    pub dc_sign: [[[u16; 4]; 3]; 2],
    pub eob_hi_bit: [[[u16; 4]; 11]; 2],   // [tx_ctx mapped][chroma][eob_bin]
    pub txtp_intra1: [[u16; 8]; 13],       // [tx_sz_ctx][y_mode]
    pub txtp_intra2: [[u16; 8]; 13],       // [tx_sz_ctx][y_mode]
}

impl CdfContext {
    pub fn new(base_q_idx: u8) -> Self {
        let qctx = match base_q_idx {
            0..=20 => 0,
            21..=60 => 1,
            61..=120 => 2,
            _ => 3,
        };
        // Initialize from default tables indexed by qctx for coeff CDFs
        // Initialize from single default set for non-coeff CDFs
        todo!()
    }
}
```

**Step 4: Verify it compiles**

Run: `cargo build`
Expected: Compiles (CdfContext::new will have `todo!()` initially, that's fine since nothing calls it yet).

**Step 5: Commit**

```
git add scripts/extract_cdfs.py src/cdf.rs
git commit -m "Extract default CDF tables from dav1d for MSAC encoding"
```

---

### Task 2: Implement MSAC Encoder

The MSAC encoder is the mathematical inverse of dav1d's decoder. It maintains a `low`/`rng` pair and outputs bytes as the range narrows.

**Files:**
- Create: `src/msac.rs`

**Step 1: Write the MSAC encoder with tests**

```rust
const EC_PROB_SHIFT: u32 = 6;
const EC_MIN_PROB: u32 = 4;

pub struct MsacEncoder {
    low: u64,
    rng: u32,
    cnt: i32,
    buf: Vec<u8>,
    allow_update_cdf: bool,
}

impl MsacEncoder {
    pub fn new() -> Self {
        Self {
            low: 0,
            rng: 0x8000,
            cnt: -24,
            buf: Vec::new(),
            allow_update_cdf: true,
        }
    }

    pub fn encode_symbol(&mut self, symbol: u32, cdf: &mut [u16], n_symbols: u32) {
        let r = self.rng >> 8;

        let mut u = self.rng;
        let mut v = self.rng;

        for i in 0..=symbol {
            u = v;
            let f = cdf[i as usize] >> EC_PROB_SHIFT;
            v = ((r * f) >> (7 - EC_PROB_SHIFT)) + EC_MIN_PROB * (n_symbols - i - 1);
        }

        self.low += v as u64;
        self.rng = u - v;
        self.normalize();

        if self.allow_update_cdf {
            Self::update_cdf(cdf, symbol, n_symbols);
        }
    }

    pub fn encode_bool(&mut self, val: bool, cdf: &mut [u16]) {
        self.encode_symbol(val as u32, cdf, 2);
    }

    fn normalize(&mut self) {
        let d = self.rng.leading_zeros() as i32 - 16;
        self.rng <<= d;
        self.low <<= d;
        self.cnt += d;

        if self.cnt >= 0 {
            let mut low = self.low;
            let mut cnt = self.cnt;

            loop {
                self.buf.push((!low >> 24) as u8);
                low = (low & 0xFF_FFFF) << 8;
                cnt -= 8;
                if cnt < 0 {
                    break;
                }
            }

            self.low = low;
            self.cnt = cnt;
        }
    }

    fn update_cdf(cdf: &mut [u16], symbol: u32, n_symbols: u32) {
        let count = cdf[n_symbols as usize];
        let rate = 4 + (count >> 4) + (n_symbols > 2) as u16;
        for i in 0..symbol {
            cdf[i as usize] += (32768 - cdf[i as usize]) >> rate;
        }
        for i in symbol..n_symbols {
            cdf[i as usize] -= cdf[i as usize] >> rate;
        }
        cdf[n_symbols as usize] = count + (count < 32) as u16;
    }

    pub fn finalize(mut self) -> Vec<u8> {
        let mut low = self.low;
        let mut cnt = self.cnt + 16;

        while cnt >= 0 {
            self.buf.push((!low >> 24) as u8);
            low = (low & 0xFF_FFFF) << 8;
            cnt -= 8;
        }

        self.buf
    }
}
```

**Note:** This is the initial implementation. The exact encoder algorithm may need adjustment during testing — the renormalization and byte output logic must match what dav1d's decoder expects. The test in Task 3 will validate this.

**Step 2: Write basic unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_single_symbol_produces_bytes() {
        let mut enc = MsacEncoder::new();
        let mut cdf = [16384u16, 0, 0]; // 50/50 binary CDF, count=0
        enc.encode_bool(false, &mut cdf);
        let bytes = enc.finalize();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn cdf_update_moves_probability() {
        let mut cdf = [16384u16, 0, 0];
        MsacEncoder::update_cdf(&mut cdf, 0, 2);
        assert!(cdf[0] < 16384); // Probability of 0 increased (CDF decreased)
        assert_eq!(cdf[2], 1); // Count incremented
    }

    #[test]
    fn encode_empty_produces_empty() {
        let enc = MsacEncoder::new();
        let bytes = enc.finalize();
        assert!(bytes.is_empty() || bytes.iter().all(|&b| b == 0xFF));
    }
}
```

**Step 3: Verify tests pass**

Run: `cargo test --lib msac`
Expected: All tests pass.

**Step 4: Commit**

```
git add src/msac.rs
git commit -m "Implement MSAC arithmetic encoder (initial)"
```

---

### Task 3: Round-Trip Validation of MSAC Encoder

This is the critical task: produce tile data bytes from the MSAC encoder and verify dav1d can decode them. Start with the absolute minimal tile: a single 64x64 block with partition NONE, DC_PRED, and all-zero coefficients (Y=128, U=128, V=128).

**Files:**
- Create: `src/tile.rs`
- Modify: `src/frame.rs`
- Modify: `src/lib.rs`

**Step 1: Implement minimal tile encoder for all-zero case**

This encodes the simplest possible tile: Y=128 (zero residual for all planes).

```rust
use crate::cdf::CdfContext;
use crate::msac::MsacEncoder;

pub fn encode_tile(y: u8, u: u8, v: u8, base_q_idx: u8) -> Vec<u8> {
    let mut cdf = CdfContext::new(base_q_idx);
    let mut enc = MsacEncoder::new();

    encode_partition_none(&mut enc, &mut cdf);
    encode_intra_mode_info(&mut enc, &mut cdf);
    encode_residual(&mut enc, &mut cdf, y, u, v);

    enc.finalize()
}

fn encode_partition_none(enc: &mut MsacEncoder, cdf: &mut CdfContext) {
    // Partition symbol for 64x64 block level (BL_64X64 = 1)
    // Context 0 (no neighbors for first block)
    // PARTITION_NONE = 0
    let n_partitions = 10; // for bl=1 (64x64)
    enc.encode_symbol(0, &mut cdf.partition[1][0], n_partitions);
}

fn encode_intra_mode_info(enc: &mut MsacEncoder, cdf: &mut CdfContext) {
    // Skip flag: skip=0 (not skipped)
    enc.encode_bool(false, &mut cdf.skip[0]);

    // Y mode: DC_PRED (0) for key frame
    // Context: above=0 (DC_PRED), left=0 (DC_PRED) since no neighbors
    enc.encode_symbol(0, &mut cdf.kfym[0][0], 13);

    // UV mode: DC_PRED (0)
    // cfl_allowed depends on block size — for 64x64 in 4:2:0, check mask
    // For now assume cfl_not_allowed (index 0)
    enc.encode_symbol(0, &mut cdf.uv_mode[0][0], 13);
}

fn encode_residual(
    enc: &mut MsacEncoder,
    cdf: &mut CdfContext,
    y: u8, u: u8, v: u8,
) {
    // DC prediction for first block predicts 128 (midpoint)
    let y_residual = y as i16 - 128;
    let u_residual = u as i16 - 128;
    let v_residual = v as i16 - 128;

    // Luma TX_64X64 (tx_ctx=5 in dav1d, mapped to our CDF index)
    // For TX >= TX_64X64: transform type is forced to DCT_DCT, no symbol
    encode_coefficients(enc, cdf, y_residual, false, 5);

    // Chroma U TX_32X32 (tx_ctx=4)
    encode_coefficients(enc, cdf, u_residual, true, 4);

    // Chroma V TX_32X32 (tx_ctx=4)
    encode_coefficients(enc, cdf, v_residual, true, 4);
}

fn encode_coefficients(
    enc: &mut MsacEncoder,
    cdf: &mut CdfContext,
    dc_residual: i16,
    is_chroma: bool,
    tx_ctx: usize,
) {
    let level = dc_residual.unsigned_abs() as u32;
    let chroma_idx = is_chroma as usize;

    if level == 0 {
        // txb_skip = 1 (all zero)
        enc.encode_bool(true, &mut cdf.txb_skip[tx_ctx][0]);
        return;
    }

    // txb_skip = 0 (has coefficients)
    enc.encode_bool(false, &mut cdf.txb_skip[tx_ctx][0]);

    // Transform type: for TX_64X64, forced to DCT_DCT (no symbol)
    // For TX_32X32 intra DC_PRED: check if reduced_txtp_set or t_dim->min >= TX_16X16
    // TX_32X32 min = 4 which is >= TX_16X16(3), so use txtp_intra2
    if tx_ctx < 5 {
        // DCT_DCT = index 0 in the transform type set
        enc.encode_symbol(0, &mut cdf.txtp_intra2[0], 5);
    }

    // EOB position: eob_pt = 1 (DC only) = symbol 0
    let eob_cdf = if tx_ctx >= 5 {
        &mut cdf.eob_bin_1024[chroma_idx]
    } else {
        &mut cdf.eob_bin_512[chroma_idx]
    };
    let n_eob_symbols = if tx_ctx >= 5 { 11 } else { 10 };
    enc.encode_symbol(0, eob_cdf, n_eob_symbols);

    // EOB base token (at EOB position, DC coefficient)
    // Symbol: 0 means level=1, 1 means level=2, 2 means level>=3
    let base_sym = if level <= 2 { level - 1 } else { 2 };
    enc.encode_symbol(base_sym, &mut cdf.eob_base_tok[chroma_idx][0], 3);

    // Bracket tokens (if level > 3)
    if level > 3 {
        let mut remaining = level - 3;
        for ctx in 0..21 {
            let br_sym = remaining.min(3);
            enc.encode_symbol(br_sym, &mut cdf.br_tok[chroma_idx][ctx], 4);
            remaining = remaining.saturating_sub(3);
            if br_sym < 3 {
                break;
            }
        }
    }

    // Golomb coding for very large values (level > 14)
    if level > 14 {
        // encode_golomb(enc, level - 15)
        // Uses encode_bool with flat probability
        todo!("Golomb coding for large coefficients — needed when |Y-128| > 14");
    }

    // DC sign
    let sign = if dc_residual < 0 { 1u32 } else { 0 };
    enc.encode_bool(sign != 0, &mut cdf.dc_sign[chroma_idx][0]);
}
```

**Important notes:**
- The exact CDF indices, number of symbols, and context computations may need adjustment during testing
- The partition symbol count for BL_64X64 needs verification (it may be 4 or 10 depending on allowed partition types)
- The coefficient encoding sequence (eob_base_tok, br_tok contexts) needs careful alignment with dav1d

**Step 2: Wire up to frame.rs**

Modify `frame.rs` to call `tile::encode_tile()` instead of using the hardcoded `TILE_DATA`:

```rust
use crate::bitwriter::BitWriter;
use crate::tile;

pub fn encode_frame(y: u8, u: u8, v: u8) -> Vec<u8> {
    let mut w = BitWriter::new();
    // ... existing frame header bits (unchanged) ...
    let mut header_bytes = w.finalize();
    let tile_data = tile::encode_tile(y, u, v, 192); // base_q_idx=192
    header_bytes.extend_from_slice(&tile_data);
    header_bytes
}
```

**Step 3: Wire up lib.rs and main.rs**

Update `lib.rs`:
```rust
pub fn encode_av1_ivf(y: u8, u: u8, v: u8) -> Vec<u8> {
    // ... same OBU assembly but frame::encode_frame(y, u, v)
}
```

Update `main.rs` to pass Y/U/V through and remove the "ignored" warning.

**Step 4: Test with Y=128, U=128, V=128 (all-zero residuals)**

This is the simplest case — all txb_skip=1, minimal symbols.

Run: `cargo run -- 128 128 128 -o /tmp/claude/test_128.ivf && ../dav1d/build/tools/dav1d -i /tmp/claude/test_128.ivf -o /dev/null`

Expected: `Decoded 1/1 frames`

**If this fails:** Compare our tile data bytes against a reference. Generate a reference:
```
ffmpeg -y -f lavfi -i "color=c=0x808080:s=64x64:d=0.04" -pix_fmt yuv420p -frames:v 1 /tmp/claude/gray128.y4m
aomenc --passes=1 --end-usage=q --cq-level=32 --cpu-used=9 --width=64 --height=64 --bit-depth=8 --ivf --limit=1 --enable-cdef=0 --enable-restoration=0 --enable-filter-intra=0 --enable-intra-edge-filter=0 -o /tmp/claude/gray128_ref.ivf /tmp/claude/gray128.y4m
```

Debug by comparing tile data byte by byte against the reference.

**Step 5: Commit**

```
git add src/tile.rs src/frame.rs src/lib.rs src/main.rs
git commit -m "Implement tile encoder with MSAC — validates with dav1d for Y=128"
```

---

### Task 4: Support Non-Zero DC Residuals

Extend the tile encoder to handle Y/U/V values other than 128 (non-zero DC residuals).

**Files:**
- Modify: `src/tile.rs` — fix coefficient encoding
- Modify: `src/msac.rs` — add Golomb coding if needed

**Step 1: Test with Y=81, U=91, V=81 (our original green)**

Run: `cargo run -- 81 91 81 -o /tmp/claude/test_green.ivf && ../dav1d/build/tools/dav1d -i /tmp/claude/test_green.ivf -o /dev/null`

If this fails, debug the coefficient encoding. The residuals are Y=81-128=-47, U=91-128=-37, V=81-128=-47. These require:
- `coeff_base_eob` symbol = 2 (level >= 3)
- Multiple `coeff_br` symbols
- `dc_sign` = 1 (negative)

**Step 2: Test edge cases**

```
cargo run -- 0 128 128 -o /tmp/claude/test_y0.ivf && ../dav1d/build/tools/dav1d -i /tmp/claude/test_y0.ivf -o /dev/null
cargo run -- 255 128 128 -o /tmp/claude/test_y255.ivf && ../dav1d/build/tools/dav1d -i /tmp/claude/test_y255.ivf -o /dev/null
cargo run -- 0 0 0 -o /tmp/claude/test_000.ivf && ../dav1d/build/tools/dav1d -i /tmp/claude/test_000.ivf -o /dev/null
cargo run -- 255 255 255 -o /tmp/claude/test_fff.ivf && ../dav1d/build/tools/dav1d -i /tmp/claude/test_fff.ivf -o /dev/null
```

Expected: All decode successfully.

**Step 3: If Golomb coding needed (|residual| > 14)**

Implement `encode_golomb()` in `msac.rs`:

```rust
pub fn encode_golomb(&mut self, mut val: u32) {
    // Exponential Golomb coding using equiprobable bools
    let mut i = 0;
    while val >= (1 << i) {
        val -= 1 << i;
        self.encode_bool_equi(false);  // length bit = 0
        i += 1;
    }
    self.encode_bool_equi(true);  // termination bit = 1
    // Write i data bits
    for j in (0..i).rev() {
        self.encode_bool_equi((val >> j) & 1 == 1);
    }
}

pub fn encode_bool_equi(&mut self, val: bool) {
    // Equiprobable (50/50) boolean — no CDF update
    let v = ((self.rng >> 8) << 7) + EC_MIN_PROB;
    if val {
        self.low += v as u64;
        self.rng -= v;
    } else {
        self.rng = v;
    }
    self.normalize();
}
```

**Step 4: Commit**

```
git add src/tile.rs src/msac.rs
git commit -m "Support non-zero DC residuals in tile encoder"
```

---

### Task 5: Verify Decoded Pixel Values Match Input

**Files:**
- Modify: `tests/integration.rs`

**Step 1: Update integration test to verify pixel values**

```rust
#[test]
fn dav1d_decodes_correct_color() {
    let test_cases: &[(u8, u8, u8)] = &[
        (128, 128, 128),
        (81, 91, 81),
        (0, 128, 128),
        (255, 128, 128),
        (16, 128, 128),
        (235, 128, 128),
    ];

    let dav1d_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dav1d/build/tools/dav1d");
    if !dav1d_path.exists() {
        eprintln!("Skipping: dav1d not found");
        return;
    }

    for &(y, u, v) in test_cases {
        let output = wav1c::encode_av1_ivf(y, u, v);
        let ivf_path = std::env::temp_dir().join(format!("wav1c_test_{}_{}.ivf", y, u));
        let y4m_path = std::env::temp_dir().join(format!("wav1c_test_{}_{}.y4m", y, u));
        std::fs::write(&ivf_path, &output).unwrap();

        let result = std::process::Command::new(&dav1d_path)
            .args(["-i", ivf_path.to_str().unwrap(), "-o", y4m_path.to_str().unwrap()])
            .output()
            .expect("Failed to run dav1d");

        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(result.status.success(), "dav1d failed for ({},{},{}): {}", y, u, v, stderr);

        // Verify decoded pixel values
        let y4m_data = std::fs::read(&y4m_path).unwrap();
        let frame_marker = b"FRAME\n";
        let frame_start = y4m_data.windows(6)
            .position(|w| w == frame_marker)
            .expect("No FRAME marker") + 6;

        let y_plane = &y4m_data[frame_start..frame_start + 64 * 64];
        let u_plane = &y4m_data[frame_start + 64 * 64..frame_start + 64 * 64 + 32 * 32];
        let v_plane = &y4m_data[frame_start + 64 * 64 + 32 * 32..];

        // Allow ±1 for quantization rounding
        for &py in y_plane.iter() {
            assert!((py as i16 - y as i16).abs() <= 1,
                "Y mismatch for input ({},{},{}): got {}", y, u, v, py);
        }
        // Chroma tolerance may be higher due to quantization
        // Just verify decode succeeded for now
    }
}
```

**Step 2: Run integration tests**

Run: `cargo test --test integration`
Expected: All test cases pass.

**Step 3: Commit**

```
git add tests/integration.rs
git commit -m "Add pixel-value verification to integration tests"
```

---

### Task 6: Clean Up and Remove Hardcoded Tile Data

**Files:**
- Modify: `src/frame.rs` — remove `TILE_DATA` constant
- Modify: `src/lib.rs` — update tests

**Step 1: Remove TILE_DATA from frame.rs**

Delete the `const TILE_DATA: [u8; 12] = [...]` constant. The frame.rs tests should now test the new encode_frame(y, u, v) signature.

**Step 2: Update frame.rs tests**

Replace the exact-byte-match tests with structural tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_header_starts_correctly() {
        let bytes = encode_frame(128, 128, 128);
        // First 4 bytes are the frame header (same as before)
        assert_eq!(&bytes[..3], &[0x18, 0x00, 0x00]);
    }

    #[test]
    fn frame_payload_longer_than_header() {
        let bytes = encode_frame(128, 128, 128);
        assert!(bytes.len() > 4); // Header (4) + at least some tile data
    }
}
```

**Step 3: Update lib.rs tests**

The `full_bitstream_frame_data_matches_reference` test must be updated since the tile data is now dynamically generated (no longer matches the aomenc reference exactly).

Replace with:

```rust
#[test]
fn output_starts_with_valid_obu_structure() {
    let output = encode_av1_ivf(128, 128, 128);
    let frame_data = &output[44..];
    // TD OBU header
    assert_eq!(frame_data[0], 0x12);
    assert_eq!(frame_data[1], 0x00);
    // SEQ OBU header
    assert_eq!(frame_data[2], 0x0A);
    assert_eq!(frame_data[3], 0x06);
    // SEQ payload should match (unchanged)
    assert_eq!(&frame_data[4..10], &[0x18, 0x15, 0x7f, 0xfc, 0x00, 0x08]);
    // Frame OBU header
    assert_eq!(frame_data[10], 0x32);
}
```

**Step 4: Run all tests**

Run: `cargo test`
Expected: All tests pass.

**Step 5: Commit**

```
git add src/frame.rs src/lib.rs
git commit -m "Remove hardcoded tile data, use real MSAC tile encoding"
```

---

### Task 7: Final Validation and Clippy

**Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

**Step 2: Run clippy**

Run: `cargo clippy --tests`
Expected: No warnings.

**Step 3: Manual end-to-end test**

```
cargo run -- 81 91 81 -o /tmp/claude/final_green.ivf
../dav1d/build/tools/dav1d -i /tmp/claude/final_green.ivf -o /tmp/claude/final_green.y4m
```

Verify the decoded frame is solid green (Y=81, U=91, V=81).

**Step 4: Tag release**

```
git tag v0.2.0-alpha
```

---

## Debugging Notes

**If dav1d fails to decode:**

1. Check dav1d's error output — it usually says which OBU or symbol failed
2. Generate a reference with aomenc for the same color and compare tile data bytes
3. Add `eprintln!` to the MSAC encoder to log each symbol and its CDF state
4. Common issues:
   - Wrong CDF array dimensions or indices
   - Missing or extra symbols in the sequence
   - MSAC normalization producing wrong bytes
   - Wrong partition count for the block level
   - CFL_ALLOWED flag incorrect for the block size
   - Transform type symbol present when it should be skipped (TX >= TX_64X64)

**MSAC debugging strategy:**
- Encode a known symbol sequence, output the bytes
- Use dav1d with debug logging to see what symbols it decodes
- Compare symbol-by-symbol
