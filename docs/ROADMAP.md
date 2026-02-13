# wav1c AV1 Encoder Roadmap

## Phase 1: Minimal Valid Bitstream (DONE)

- IVF container, OBU framing, sequence header, frame header
- Hardcoded tile data from aomenc reference
- 64x64 single-frame key frame, decodable by dav1d
- 27 tests passing

## Phase 2+3: Arithmetic Coder + Solid-Color Tile Encoding (NEXT)

- Implement AV1 multi-symbol arithmetic coder (MSAC encoder)
- Encode real tile data: partition, prediction mode, transform, coefficients
- Wire up CLI Y/U/V input to produce actual encoded colors
- Remove hardcoded tile data blob

## Phase 4: Arbitrary Frame Dimensions

- Parameterize sequence/frame headers for any width/height
- Handle multiple superblocks per frame
- Multiple tiles for larger frames

## Phase 5: Real Image Input

- Accept Y4M or raw YUV file input
- Encode real images with DC prediction
- Single intra frame

## Phase 6: Transform Coding (DCT)

- Encode residuals with DCT transforms
- Support multiple transform sizes
- Proper coefficient encoding

## Phase 7: Rate Control / Configurable QP

- Make base_q_idx configurable via CLI
- Basic rate-distortion decisions

## Phase 8: Loop Filter, CDEF

- Post-processing filters for decoded quality
- Enable corresponding sequence/frame header flags

## Phase 9: Multi-Frame / Inter Prediction

- Reference frame management
- Motion estimation and compensation
- Actual video encoding (multiple frames)
