#ifndef WAV1C_H
#define WAV1C_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct Wav1cEncoder Wav1cEncoder;

typedef struct {
    const uint8_t *data;
    size_t         size;
    uint64_t       frame_number;
    int32_t        is_keyframe;
} Wav1cPacket;

typedef struct {
    uint8_t  base_q_idx;
    size_t   keyint;
    uint64_t target_bitrate;
    double   fps;
    int32_t  b_frames;
    size_t   gop_size;
} Wav1cConfig;

typedef struct {
    uint8_t  base_q_idx;
    size_t   keyint;
    uint64_t target_bitrate;
    double   fps;
    int32_t  b_frames;
    size_t   gop_size;
    uint8_t  bit_depth; /* 8 or 10 */
    int32_t  color_range; /* 0 = limited, 1 = full */
    int32_t  color_primaries; /* -1 unset */
    int32_t  transfer_characteristics; /* -1 unset */
    int32_t  matrix_coefficients; /* -1 unset */
    int32_t  has_cll;
    uint16_t max_cll;
    uint16_t max_fall;
    int32_t  has_mdcv;
    uint16_t red_x;
    uint16_t red_y;
    uint16_t green_x;
    uint16_t green_y;
    uint16_t blue_x;
    uint16_t blue_y;
    uint16_t white_x;
    uint16_t white_y;
    uint32_t max_luminance;
    uint32_t min_luminance;
} Wav1cConfigEx;

Wav1cEncoder *wav1c_encoder_new(uint32_t width, uint32_t height,
                                const Wav1cConfig *cfg);
Wav1cEncoder *wav1c_encoder_new_ex(uint32_t width, uint32_t height,
                                   const Wav1cConfigEx *cfg);

void wav1c_encoder_free(Wav1cEncoder *enc);

size_t wav1c_encoder_headers(Wav1cEncoder *enc, const uint8_t **out_data);

int wav1c_encoder_send_frame(Wav1cEncoder *enc,
                             const uint8_t *y, size_t y_len,
                             const uint8_t *u, size_t u_len,
                             const uint8_t *v, size_t v_len,
                             int y_stride, int uv_stride);
int wav1c_encoder_send_frame_u16(Wav1cEncoder *enc,
                                 const uint16_t *y, size_t y_len,
                                 const uint16_t *u, size_t u_len,
                                 const uint16_t *v, size_t v_len,
                                 int y_stride, int uv_stride);

Wav1cPacket *wav1c_encoder_receive_packet(Wav1cEncoder *enc);

void wav1c_packet_free(Wav1cPacket *pkt);

void wav1c_encoder_flush(Wav1cEncoder *enc);

#ifdef __cplusplus
}
#endif

#endif
