#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "wav1c.h"

static void write_le16(uint8_t *buf, uint16_t val) {
    buf[0] = val & 0xFF;
    buf[1] = (val >> 8) & 0xFF;
}

static void write_le32(uint8_t *buf, uint32_t val) {
    buf[0] = val & 0xFF;
    buf[1] = (val >> 8) & 0xFF;
    buf[2] = (val >> 16) & 0xFF;
    buf[3] = (val >> 24) & 0xFF;
}

static void write_le64(uint8_t *buf, uint64_t val) {
    for (int i = 0; i < 8; i++)
        buf[i] = (val >> (i * 8)) & 0xFF;
}

static int write_ivf_header(FILE *f, uint16_t width, uint16_t height,
                            uint32_t num_frames, uint32_t fps_num,
                            uint32_t fps_den) {
    uint8_t hdr[32];
    memcpy(hdr, "DKIF", 4);
    write_le16(hdr + 4, 0);
    write_le16(hdr + 6, 32);
    memcpy(hdr + 8, "AV01", 4);
    write_le16(hdr + 12, width);
    write_le16(hdr + 14, height);
    write_le32(hdr + 16, fps_num);
    write_le32(hdr + 20, fps_den);
    write_le32(hdr + 24, num_frames);
    write_le32(hdr + 28, 0);
    return fwrite(hdr, 1, 32, f) == 32 ? 0 : -1;
}

static int write_ivf_frame(FILE *f, uint64_t pts, const uint8_t *data,
                           uint32_t size) {
    uint8_t hdr[12];
    write_le32(hdr, size);
    write_le64(hdr + 4, pts);
    if (fwrite(hdr, 1, 12, f) != 12) return -1;
    if (fwrite(data, 1, size, f) != size) return -1;
    return 0;
}

static void print_usage(const char *prog) {
    fprintf(stderr,
        "Usage: %s <width> <height> <Y> <U> <V> <num_frames> -o <output.ivf> [options]\n"
        "\n"
        "Encodes solid-color frames to AV1 in an IVF container.\n"
        "\n"
        "Options:\n"
        "  -q <0-255>      Quantizer index (default=128)\n"
        "  --keyint <N>    Keyframe interval (default=25)\n"
        "  --bitrate <N>   Target bitrate in bps (0=CQP, default=0)\n"
        "  --fps <N>       Frames per second (default=25)\n",
        prog);
}

int main(int argc, char **argv) {
    if (argc < 9) {
        print_usage(argv[0]);
        return 1;
    }

    uint32_t width = (uint32_t)atoi(argv[1]);
    uint32_t height = (uint32_t)atoi(argv[2]);
    uint8_t y_val = (uint8_t)atoi(argv[3]);
    uint8_t u_val = (uint8_t)atoi(argv[4]);
    uint8_t v_val = (uint8_t)atoi(argv[5]);
    uint32_t num_frames = (uint32_t)atoi(argv[6]);
    const char *output_path = NULL;

    Wav1cConfig cfg = wav1c_default_config();

    for (int i = 7; i < argc; i++) {
        if (strcmp(argv[i], "-o") == 0 && i + 1 < argc) {
            output_path = argv[++i];
        } else if (strcmp(argv[i], "-q") == 0 && i + 1 < argc) {
            cfg.base_q_idx = (uint8_t)atoi(argv[++i]);
        } else if (strcmp(argv[i], "--keyint") == 0 && i + 1 < argc) {
            cfg.keyint = (size_t)atoi(argv[++i]);
        } else if (strcmp(argv[i], "--bitrate") == 0 && i + 1 < argc) {
            cfg.target_bitrate = (uint64_t)atoll(argv[++i]);
        } else if (strcmp(argv[i], "--fps") == 0 && i + 1 < argc) {
            cfg.fps = atof(argv[++i]);
        }
    }

    if (!output_path) {
        fprintf(stderr, "Error: missing -o <output.ivf>\n");
        print_usage(argv[0]);
        return 1;
    }

    if (width == 0 || height == 0 || num_frames == 0) {
        fprintf(stderr, "Error: width, height, and num_frames must be > 0\n");
        return 1;
    }

    Wav1cEncoder *enc = wav1c_encoder_new(width, height, &cfg);
    if (!enc) {
        fprintf(stderr, "Error: failed to create encoder for %ux%u\n",
                width, height);
        return 1;
    }

    FILE *f = fopen(output_path, "wb");
    if (!f) {
        fprintf(stderr, "Error: cannot open %s for writing\n", output_path);
        wav1c_encoder_free(enc);
        return 1;
    }

    if (write_ivf_header(f, (uint16_t)width, (uint16_t)height,
                         num_frames, (uint32_t)cfg.fps, 1) != 0) {
        fprintf(stderr, "Error: failed to write IVF header\n");
        fclose(f);
        wav1c_encoder_free(enc);
        return 1;
    }

    size_t y_size = (size_t)width * height;
    size_t uv_w = (width + 1) / 2;
    size_t uv_h = (height + 1) / 2;
    size_t uv_size = uv_w * uv_h;

    uint8_t *y_plane = malloc(y_size);
    uint8_t *u_plane = malloc(uv_size);
    uint8_t *v_plane = malloc(uv_size);
    if (!y_plane || !u_plane || !v_plane) {
        fprintf(stderr, "Error: out of memory\n");
        free(y_plane); free(u_plane); free(v_plane);
        fclose(f);
        wav1c_encoder_free(enc);
        return 1;
    }

    memset(y_plane, y_val, y_size);
    memset(u_plane, u_val, uv_size);
    memset(v_plane, v_val, uv_size);

    size_t total_bytes = 0;

    for (uint32_t i = 0; i < num_frames; i++) {
        int ret = wav1c_encoder_send_frame(enc,
            y_plane, y_size, u_plane, uv_size, v_plane, uv_size, 0, 0);
        if (ret != 0) {
            fprintf(stderr, "Error: send_frame failed at frame %u\n", i);
            break;
        }

        Wav1cPacket *pkt = wav1c_encoder_receive_packet(enc);
        if (pkt) {
            fprintf(stderr, "frame %4lu  %5s  %zu bytes\n",
                    (unsigned long)pkt->frame_number,
                    pkt->is_keyframe ? "KEY" : "INTER",
                    pkt->size);

            write_ivf_frame(f, pkt->frame_number, pkt->data, (uint32_t)pkt->size);
            total_bytes += pkt->size;
            wav1c_packet_free(pkt);
        }
    }

    wav1c_encoder_flush(enc);

    Wav1cPacket *pkt;
    while ((pkt = wav1c_encoder_receive_packet(enc)) != NULL) {
        write_ivf_frame(f, pkt->frame_number, pkt->data, (uint32_t)pkt->size);
        total_bytes += pkt->size;
        wav1c_packet_free(pkt);
    }

    free(y_plane);
    free(u_plane);
    free(v_plane);
    fclose(f);
    wav1c_encoder_free(enc);

    fprintf(stderr, "\nWrote %zu bytes to %s (%u frames, %ux%u, q=%u, keyint=%zu)\n",
            total_bytes + 32 + (size_t)num_frames * 12,
            output_path, num_frames, width, height,
            cfg.base_q_idx, cfg.keyint);

    return 0;
}
