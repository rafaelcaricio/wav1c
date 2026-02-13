# wav1c — AV1 Encoder Development Protocol

## Goal

Encode a 2-second raw video with 1-second GoP to AV1, decodable by stock dav1d.

## Autonomous Closed-Loop Protocol

Execute phases sequentially. Each phase follows this cycle:

1. **Plan** — Define scope, identify what changes in headers/encoding/container
2. **Research** — Use a subagent to study AV1 spec and dav1d source to understand decoder expectations
3. **Document** — Use a subagent to write implementation plan to `docs/plans/`
4. **Implement** — Subagent-driven development with code review per task
5. **Validate** — Generate test artifacts with ffmpeg, decode with dav1d, write regression tests
6. **Loop** — Commit, update roadmap, update documentation, proceed to next phase

## Development Rules

- Never stop to ask questions — proceed with best approach
- No code comments — self-documenting code through clear naming
- Tasks only done when code compiles with all features working
- Use subagent development for parallel work
- All tests must pass before committing (cargo test + cargo clippy --tests)
- Every phase adds automated regression tests that protect previous work
- dav1d at `../dav1d/build/tools/dav1d` is the authoritative decoder
- Can add debug logging to dav1d for investigation, but encoder must work with stock dav1d
- Must execute a end-to-end transcoding with wav1c and decode with dav1d; When necessary create test frames with FFmpeg

## Test Strategy

- Unit tests for each module (bitwriter, obu, msac, cdf, tile, frame, sequence, ivf)
- Integration tests that encode → decode with dav1d → verify pixel values
- Generate reference inputs with ffmpeg (solid colors, gradients, real images, multi-frame Y4M)
- Every new phase must not break tests from previous phases

## Phase Roadmap

See `docs/ROADMAP.md` for the full phase breakdown.

Current phases: 1 [DONE] → 2+3 [DONE] → 4 [DONE] → 5 [DONE] → 6 [DONE] → 7 [DONE] → 8 [DONE] → 9

## Key Technical Reference

- AV1 spec: `av1-spec.md` (local)
- dav1d source: `../dav1d/src/` (decoder reference implementation)
- base_q_idx = 128, qctx = 3, DC dequant = 140
- MSAC: precarry buffer approach, bytes NOT XOR'd (carry resolved in finalize)
- CDF format: cdf[i] = 32768 - cumulative_prob, last element = adaptation counter
