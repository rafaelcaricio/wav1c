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
} Wav1cConfig;

Wav1cEncoder *wav1c_encoder_new(uint32_t width, uint32_t height,
                                const Wav1cConfig *cfg);

void wav1c_encoder_free(Wav1cEncoder *enc);

size_t wav1c_encoder_headers(Wav1cEncoder *enc, const uint8_t **out_data);

int wav1c_encoder_send_frame(Wav1cEncoder *enc,
                             const uint8_t *y, size_t y_len,
                             const uint8_t *u, size_t u_len,
                             const uint8_t *v, size_t v_len);

Wav1cPacket *wav1c_encoder_receive_packet(Wav1cEncoder *enc);

void wav1c_packet_free(Wav1cPacket *pkt);

void wav1c_encoder_flush(Wav1cEncoder *enc);

#ifdef __cplusplus
}
#endif

#endif
