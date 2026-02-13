# Phase 6: Multi-Frame Encoding (All-Intra)

## Goal

Encode multi-frame Y4M files to AV1 IVF, with each frame independently decodable as a keyframe. Validate with dav1d.

## Design Decisions

### All-Keyframe Approach
Every frame is encoded as KEY_FRAME (frame_type=0). This reuses the existing single-frame encoder exactly — no new frame header logic needed. AV1 allows consecutive keyframes; dav1d handles this correctly.

Each IVF frame contains: `OBU_TD + OBU_SEQ_HDR + OBU_FRAME` — identical to the current single-frame output.

### Why Not INTRA_ONLY
INTRA_ONLY (frame_type=2) would be slightly more compact (no repeated seq header) but requires:
- Different frame header bits (explicit error_resilient_mode, refresh_frame_flags)
- CDF adaptation state management between frames
- primary_ref_frame handling

All-keyframe is simpler and sufficient for Phase 6. INTRA_ONLY can be added later as an optimization.

### IVF Timestamps
Each frame gets timestamp = frame_index (0, 1, 2, ...). IVF timescale stays at 25/1 (25fps). The Y4M framerate is extracted from the header but only used for potential future CLI output — the encoder produces valid output regardless.

## File Changes

### 1. src/y4m.rs — Multi-Frame Parsing

Add `all_from_y4m(data: &[u8]) -> Vec<FramePixels>` that iterates over all FRAME markers in a Y4M file, extracting each frame's Y/U/V planes. The existing `from_y4m()` becomes a convenience wrapper returning just the first frame.

Add `all_from_y4m_file(path: &Path) -> io::Result<Vec<FramePixels>>`.

Frame layout in Y4M multi-frame:
```
YUV4MPEG2 W<w> H<h> F<n>:<d> C420...\n
FRAME\n
<Y plane><U plane><V plane>
FRAME\n
<Y plane><U plane><V plane>
...
```

Each FRAME marker is followed by exactly `y_size + 2*uv_size` bytes. Search for `FRAME\n` at the expected byte offsets after the header.

### 2. src/lib.rs — Multi-Frame API

Add `encode_av1_ivf_multi(frames: &[FramePixels]) -> Vec<u8>`:
- Validates all frames have the same dimensions
- Writes IVF header with `num_frames = frames.len()`
- For each frame at index `i`:
  - Calls existing `obu_wrap(TemporalDelimiter, &[])`, `encode_sequence_header(w, h)`, `encode_frame(pixels)`
  - Wraps each in OBU framing
  - Writes as IVF frame with `timestamp = i`
- Returns complete IVF bytestream

Make `encode_av1_ivf_y4m()` delegate to `encode_av1_ivf_multi(&[pixels])` for a single frame.

### 3. src/main.rs — CLI Update

Y4M mode: use `all_from_y4m_file()` + `encode_av1_ivf_multi()`.
Solid-color mode: unchanged (single frame).

### 4. tests/integration.rs — Multi-Frame Tests

- `dav1d_decodes_multi_frame_solid`: 5 frames of solid gray, verify "Decoded 5/5 frames"
- `dav1d_decodes_multi_frame_varying`: 3 frames with different content per frame
- `dav1d_decodes_multi_frame_gradient`: 10 frames with per-frame gradients at 320x240

Y4M test helper: `create_multi_frame_y4m(width, height, frames: &[fn(col,row)->(y,u,v)])` that generates valid multi-frame Y4M data.

## Task Breakdown

### Task 1: Y4M Multi-Frame Parser (y4m.rs)
- Add `all_from_y4m()` that finds all FRAME markers
- Add `all_from_y4m_file()`
- Refactor `from_y4m()` to use `all_from_y4m().remove(0)` internally
- Unit tests: single-frame backward compat, multi-frame parsing, varying frame content

### Task 2: Multi-Frame Encoding API (lib.rs)
- Add `encode_av1_ivf_multi()`
- Dimension validation across all frames
- IVF header with correct frame count
- Per-frame OBU generation with sequential timestamps
- Refactor `encode_av1_ivf_y4m()` to delegate to multi
- Unit tests: multi-frame IVF structure, single-frame backward compat

### Task 3: CLI Update + Integration Tests (main.rs, tests/integration.rs)
- Update main.rs Y4M path to use multi-frame API
- Add multi-frame Y4M test helper
- Add dav1d integration tests for 2, 5, 10 frame sequences
- Verify frame count in dav1d output

## Validation

1. `cargo test` — all existing + new unit tests pass
2. `cargo clippy` — zero warnings
3. Create multi-frame Y4M test files, encode, decode with dav1d
4. dav1d must report "Decoded N/N frames" for N > 1
5. All existing single-frame tests must still pass (backward compat)
