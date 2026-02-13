# wav1c: Minimal AV1 Encoder - Design Document

## Goal

A Rust CLI that produces a valid AV1 bitstream (IVF container) encoding a single
64x64 solid-color key frame, decodable by dav1d.

## Constraints

- Single frame, single tile, 64x64 pixels (one 64x64 superblock)
- YUV 4:2:0, 8-bit, profile 0
- Hardcoded entropy-coded tile data from aomenc reference (no arithmetic coder yet)
- CLI accepts Y, U, V values but ignores them in this iteration (color is fixed)
- `reduced_still_picture_header` for minimal sequence header
- All optional coding tools disabled (no CDEF, restoration, superres, filter intra, intra edge filter)

## Architecture

```
src/
  main.rs        CLI entry point: parse args, orchestrate encoding, write output
  ivf.rs         IVF container writer (32-byte header + per-frame wrapper)
  obu.rs         OBU framing (header byte + leb128 size + payload)
  bitwriter.rs   Bit-level MSB-first writer
  sequence.rs    Sequence Header OBU payload construction
  frame.rs       Frame OBU payload construction (header bits + tile data blob)
```

## Bitstream Layout

The encoder produces this byte sequence inside each IVF frame:

```
OBU: Temporal Delimiter  (type=2, 0 bytes payload)
OBU: Sequence Header     (type=1, 6 bytes payload)
OBU: Frame               (type=6, 16 bytes payload)
     |- Frame header      (4 bytes, bit-packed)
     \- Tile data         (12 bytes, hardcoded blob)
```

Total frame data: 28 bytes. Total file: 60 bytes (32 IVF header + 12 frame wrapper + 28 OBU data... actually 32 + 12 + 28 = 72, but the 12 includes 4-byte size + 8-byte timestamp).

## Reference Bitstream

Generated with:
```
aomenc --passes=1 --end-usage=q --cq-level=32 --cpu-used=9 \
  --width=64 --height=64 --bit-depth=8 --ivf --limit=1 \
  --enable-cdef=0 --enable-restoration=0 \
  --enable-filter-intra=0 --enable-intra-edge-filter=0 \
  -o ref.ivf solid_green_64x64.y4m
```

Frame data hex: `12000a0618157ffc0008321018000000400a0579526e43d7e6426320`

Verified decodable by dav1d 1.5.3. Decodes to solid green (Y=81, U=91, V=81).

## OBU Framing

Each OBU:
```
[header_byte] [leb128_size] [payload...]

header_byte = (obu_type << 3) | (obu_has_size_field << 1)
  - obu_forbidden_bit = 0 (bit 7)
  - obu_type (bits 6-3)
  - obu_extension_flag = 0 (bit 2)
  - obu_has_size_field = 1 (bit 1)
  - reserved = 0 (bit 0)
```

OBU types used: `OBU_TEMPORAL_DELIMITER=2`, `OBU_SEQUENCE_HEADER=1`, `OBU_FRAME=6`

### leb128 Encoding

Variable-length unsigned integer. Each byte: 7 data bits (LSB first) + 1 continuation bit.
For sizes < 128: single byte, continuation bit = 0.

## Sequence Header (6 bytes, 44 meaningful bits + 4 trailing)

| Bits | Field | Value | Notes |
|------|-------|-------|-------|
| 3 | seq_profile | 0 | 8-bit YUV 4:2:0 |
| 1 | still_picture | 1 | Single frame |
| 1 | reduced_still_picture_header | 1 | Minimal header |
| 5 | seq_level_idx[0] | 0 | Level 2.0 |
| 4 | frame_width_bits_minus_1 | 5 | 6 bits for width field |
| 4 | frame_height_bits_minus_1 | 5 | 6 bits for height field |
| 6 | max_frame_width_minus_1 | 63 | Width = 64 |
| 6 | max_frame_height_minus_1 | 63 | Height = 64 |
| 1 | use_128x128_superblock | 0 | 64x64 superblocks |
| 1 | enable_filter_intra | 0 | Disabled |
| 1 | enable_intra_edge_filter | 0 | Disabled |
| 1 | enable_superres | 0 | Disabled |
| 1 | enable_cdef | 0 | Disabled |
| 1 | enable_restoration | 0 | Disabled |
| 1 | high_bitdepth | 0 | 8-bit |
| 1 | mono_chrome | 0 | 3 planes (YUV) |
| 1 | color_description_present_flag | 0 | Skip color metadata |
| 1 | color_range | 0 | Limited/studio range |
| 2 | chroma_sample_position | 0 | CSP_UNKNOWN |
| 1 | separate_uv_delta_q | 0 | Same U/V quantization |
| 1 | film_grain_params_present | 0 | No film grain |
| 1 | trailing_one_bit | 1 | Required by spec |
| 3 | trailing_zero_bits | 0 | Pad to byte boundary |

Total: 48 bits = 6 bytes.
Expected hex: `18 15 7f fc 00 08`

### Implied Values (from reduced_still_picture_header=1)

- timing_info_present_flag = 0
- decoder_model_info_present_flag = 0
- initial_display_delay_present_flag = 0
- operating_points_cnt_minus_1 = 0
- operating_point_idc[0] = 0
- seq_tier[0] = 0
- frame_id_numbers_present_flag = 0
- enable_interintra_compound = 0
- enable_masked_compound = 0
- enable_warped_motion = 0
- enable_dual_filter = 0
- enable_order_hint = 0 (OrderHintBits = 0)
- enable_jnt_comp = 0
- enable_ref_frame_mvs = 0
- seq_force_screen_content_tools = SELECT_SCREEN_CONTENT_TOOLS (2)
- seq_force_integer_mv = SELECT_INTEGER_MV (2)

## Frame Header (32 bits = 4 bytes)

### Implied Values (from reduced_still_picture_header=1 and KEY_FRAME)

- show_existing_frame = 0
- frame_type = KEY_FRAME (0)
- FrameIsIntra = 1
- show_frame = 1
- showable_frame = 0
- error_resilient_mode = 1
- primary_ref_frame = PRIMARY_REF_NONE (7)
- refresh_frame_flags = 0xFF
- frame_size_override_flag = 0 (frame uses max dimensions)
- allow_intrabc = 0 (not signaled when allow_screen_content_tools=0)

### Signaled Fields

| Bits | Field | Value | Notes |
|------|-------|-------|-------|
| 1 | disable_cdf_update | 0 | Allow CDF updates |
| 1 | allow_screen_content_tools | 0 | Disabled |
| 1 | render_and_frame_size_different | 0 | Same render size |
| 8 | base_q_idx | 192 | Quantizer (matches reference) |
| 1 | DeltaQYDc delta_coded | 0 | No Y DC delta |
| 1 | diff_uv_delta | 0 | Same U/V deltas |
| 1 | DeltaQUDc delta_coded | 0 | No U DC delta |
| 1 | DeltaQUAc delta_coded | 0 | No U AC delta |
| 1 | using_qmatrix | 0 | No quant matrices |
| 1 | segmentation_enabled | 0 | No segmentation |
| 1 | delta_q_present | 0 | No per-SB Q deltas |
| 6 | loop_filter_level[0] | 0 | No Y deblock |
| 6 | loop_filter_level[1] | 0 | No UV deblock |
| 1 | tx_mode_select | 0 | TX_MODE_LARGEST |
| 1 | uniform_tile_spacing_flag | 1 | Uniform tiles |

Total: 32 bits = 4 bytes, naturally byte-aligned (no padding needed).

Note: `uniform_tile_spacing_flag` differs from reference (which uses 0), but both
produce identical results for a single superblock (0 additional bits in either case).

### Tile Info Details

- sbCols = (MiCols + 15) >> 4 = (16 + 15) >> 4 = 1
- sbRows = 1
- With uniform=1: minLog2TileCols = maxLog2TileCols = 0, no increment bits
- TileCols = 1, TileRows = 1, NumTiles = 1
- TileColsLog2 + TileRowsLog2 = 0: no context_update_tile_id or tile_size_bytes

## Tile Data (12 bytes, hardcoded)

```rust
const TILE_DATA: [u8; 12] = [
    0x40, 0x0a, 0x05, 0x79, 0x52, 0x6e,
    0x43, 0xd7, 0xe6, 0x42, 0x63, 0x20,
];
```

This blob contains the arithmetic-coded tile data for a 64x64 solid green frame
(Y=81, U=91, V=81) at base_q_idx=192 with all optional tools disabled. It encodes:
- Partition decisions for the single 64x64 superblock
- Intra prediction modes (DC_PRED for all blocks)
- Transform types and quantized coefficients

The arithmetic decoder initializes with the first ~8 bytes to fill its state,
then decodes symbols from the remaining ~4 bytes.

## IVF Container

32-byte file header:
```
bytes 0-3:   "DKIF" (signature)
bytes 4-5:   0x0000 (version, little-endian)
bytes 6-7:   0x0020 (header length = 32, little-endian)
bytes 8-11:  "AV01" (codec FourCC)
bytes 12-13: 0x0040 (width = 64, little-endian)
bytes 14-15: 0x0040 (height = 64, little-endian)
bytes 16-19: 0x00000019 (framerate numerator = 25, little-endian)
bytes 20-23: 0x00000001 (framerate denominator = 1, little-endian)
bytes 24-27: 0x00000001 (number of frames = 1, little-endian)
bytes 28-31: 0x00000000 (unused)
```

Per frame:
```
4 bytes: frame_size (little-endian)
8 bytes: timestamp (little-endian, 0 for first frame)
N bytes: frame data (concatenated OBUs)
```

## CLI Interface

```
wav1c <Y> <U> <V> -o <output.ivf>

# Example:
wav1c 81 91 81 -o green.ivf
```

In this iteration, the Y/U/V values are accepted but ignored (a warning is printed).
The output always contains the hardcoded green frame.

## Validation

1. Primary: `dav1d -i output.ivf -o decoded.y4m` exits with code 0
2. Byte-level: compare our output against reference hex
3. Unit tests for bitwriter, leb128, OBU framing, IVF header
4. Integration test: encode + dav1d decode (skip if dav1d not found)

## Future Work (Not This Iteration)

- Implement arithmetic encoder to replace hardcoded tile data
- Support actual Y/U/V color input
- Support arbitrary frame dimensions
- Support multiple frames
- Implement CDEF, loop filter, restoration as needed
