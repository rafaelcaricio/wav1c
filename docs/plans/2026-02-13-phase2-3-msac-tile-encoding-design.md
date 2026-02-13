# Phase 2+3: MSAC Encoder + Solid-Color Tile Encoding — Design Document

## Goal

Replace the hardcoded tile data blob with a real arithmetic encoder (MSAC) and tile
encoder that produces valid AV1 tile data for any solid Y/U/V color at 64x64.

## Constraints

- Same 64x64 single-frame, single-tile configuration as Phase 1
- Must produce output decodable by dav1d for all valid Y/U/V (0-255) inputs
- Default CDF tables extracted from dav1d source
- CDF adaptation enabled (disable_cdf_update = 0)

## Architecture

```
src/
  main.rs         CLI: parse Y,U,V args, encode, write IVF
  lib.rs          Public API: encode_av1_ivf(y, u, v)
  bitwriter.rs    (existing) Bit-level writer for headers
  obu.rs          (existing) OBU framing
  ivf.rs          (existing) IVF container
  sequence.rs     (existing) Sequence header
  frame.rs        (modified) Frame header + calls tile encoder
  msac.rs         NEW: Multi-symbol arithmetic encoder
  cdf.rs          NEW: CDF table definitions, defaults, CdfContext
  tile.rs         NEW: Tile encoder (symbol sequence for 64x64 intra)
```

## MSAC Encoder

The AV1 multi-symbol arithmetic coder maintains a range-low pair. For each symbol,
the range is subdivided according to the CDF. The interval for the chosen symbol
becomes the new range. Bytes are output when the range drops below threshold.

### State

```rust
struct MsacEncoder {
    low: u32,       // lower bound of current interval
    rng: u16,       // current range (maintained in [32768, 65536))
    cnt: i16,       // bit counter (tracks when to flush bytes)
    buf: Vec<u8>,   // output buffer
}
```

### Operations

- `new()` — initialize with rng=32768, low=0, cnt=-15
- `encode_symbol(&mut self, symbol: u32, cdf: &mut [u16])` — encode symbol, update CDF
- `encode_bool(&mut self, val: bool)` — encode boolean (flat CDF, no update)
- `finalize(self) -> Vec<u8>` — flush, write trailing 1-bit + zero padding

### Encoding Algorithm (per symbol)

```
Given: symbol value s, CDF array cdf[0..N-1], count in cdf[N]

1. Compute cumulative ranges same as decoder:
   For each i from 0 to N-1:
     f = 32768 - cdf[i]
     cur[i] = ((rng >> 8) * (f >> EC_PROB_SHIFT)) >> (7 - EC_PROB_SHIFT)
     cur[i] += EC_MIN_PROB * (N - i - 1)

2. Narrow interval for symbol s:
   low += cur[s]
   rng = cur[s-1] - cur[s]   (or initial_range - cur[s] if s==0)

3. Renormalize: while rng < 32768, shift left, output bits from low

4. Update CDF (same algorithm as decoder)
```

### Finalization

After all symbols:
1. Flush remaining bits from low
2. Write a single 1-bit (trailing termination bit)
3. Write zero bits to byte-align
4. Return buf

## CDF Tables

### Source

Extracted from dav1d's `src/cdf.c`. The default CDF tables are ~40KB of u16 arrays.

### Organization

```rust
struct CdfContext {
    kfym: [[[u16; 14]; 5]; 5],          // key frame Y mode
    uv_mode: [[[u16; 14]; 13]; 2],      // UV mode (cfl_allowed × y_mode)
    partition: [[[u16; 10]; 4]; 4],      // partition (bl × ctx)
    skip: [[u16; 3]; 3],                // skip flag
    txb_skip: [[[u16; 3]; 5]; 13],      // all-zero coeff flag
    eob_pt_1024: [[u16; 17]; 2],        // EOB position (1024-coeff TXs)
    eob_pt_512: [[u16; 17]; 2],         // EOB position (512-coeff TXs)
    coeff_base_eob: [[[u16; 4]; 2]; 13],// base coeff at EOB
    coeff_br: [[[[u16; 5]; 4]; 2]; 13], // coefficient bracket
    dc_sign: [[[u16; 3]; 3]; 2],        // DC sign
    txtp_intra2: [[[u16; 8]; 4]; 7],    // transform type (intra, set2)
    txtp_intra1: [[[u16; 8]; 4]; 4],    // transform type (intra, set1)
}
```

### Initialization

```rust
impl CdfContext {
    fn new(base_q_idx: u8) -> Self {
        let qctx = match base_q_idx {
            0..=20 => 0,
            21..=60 => 1,
            61..=120 => 2,
            _ => 3,
        };
        // Copy non-coeff CDFs from single default set
        // Copy coeff CDFs from default set indexed by qctx
    }
}
```

## Tile Encoder — Symbol Sequence

For a 64x64 solid-color key frame (Y, U, V inputs):

### Predictor Value

DC_PRED for the first block (no neighbors) predicts 128 (midpoint for 8-bit).
Residual = actual_value - 128.

### Complete Symbol Sequence

```
1. partition = PARTITION_NONE           [CDF: partition[bl=0][ctx=0]]
2. skip = 0                             [CDF: skip[ctx=0]]
3. y_mode = DC_PRED (0)                 [CDF: kfym[0][0]]
4. uv_mode = DC_PRED (0)               [CDF: uv_mode[cfl=0][ymode=0]]

LUMA (TX_64X64):
5. tx_type = DCT_DCT                    [CDF: txtp_intra2 or txtp_intra1]
6. IF Y == 128:
     txb_skip = 1 (all zero)            [CDF: txb_skip[tx_ctx][ctx]]
   ELSE:
     txb_skip = 0 (has coefficients)    [CDF: txb_skip[tx_ctx][ctx]]
     eob_pt = 1 (DC only, symbol=0)     [CDF: eob_pt_1024[0]]
     coeff_base_eob = min(level, 3)     [CDF: coeff_base_eob[tx_ctx][0]]
     coeff_br × N (if level > 3)        [CDF: coeff_br[tx_ctx][0][ctx]]
     dc_sign                            [CDF: dc_sign[0][ctx]]
     golomb bits (if level > 14)        [literal bits via encode_bool]

CHROMA U (TX_32X32):
7. Same as luma but for U value, plane_type=1

CHROMA V (TX_32X32):
8. Same as luma but for V value, plane_type=1
```

### Coefficient Value Encoding

For residual `val = pixel_value - 128`:

1. `level = abs(val)`
2. If `level == 0`: `txb_skip = 1`, done
3. Else: `txb_skip = 0`
4. `eob_pt = 1` (DC-only, symbol index 0)
5. `coeff_base_eob` symbol = `min(level, 3) - 1` (range 0-2)
   - Actually for EOB position: symbol 0 means level=1, 1 means level=2, 2 means level≥3
6. If `level > 3`: encode `coeff_br` symbols
   - Each symbol 0-3; symbol < 3 terminates
   - Remaining level is accumulated
7. If `level > 14`: encode remaining via Golomb coding (literal bits)
8. `dc_sign` = 1 if val < 0, else 0

## Changes to Existing Code

### lib.rs

```rust
pub fn encode_av1_ivf(y: u8, u: u8, v: u8) -> Vec<u8> {
    // ... same assembly but frame::encode_frame(y, u, v)
}
```

### frame.rs

```rust
pub fn encode_frame(y: u8, u: u8, v: u8) -> Vec<u8> {
    let mut w = BitWriter::new();
    // ... same frame header bits ...
    let mut header_bytes = w.finalize();
    let tile_data = tile::encode_tile(y, u, v);
    header_bytes.extend_from_slice(&tile_data);
    header_bytes
}
```

### main.rs

Pass Y/U/V values through to `encode_av1_ivf()`. Remove the "color input ignored" warning.

## Validation

1. Primary: `wav1c Y U V -o out.ivf && dav1d -i out.ivf -o decoded.y4m`
   Then verify decoded pixels match input Y/U/V values
2. Test multiple colors: (0,128,128), (128,128,128), (255,128,128), (81,91,81)
3. Test edge cases: Y=128 (zero DC residual), all channels zero, all 255
4. dav1d integration test checks all the above automatically
