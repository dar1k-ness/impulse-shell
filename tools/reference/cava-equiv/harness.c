// Reference output of cava 1.0.0 for the cava::tests equivalence test: cavacore.c from the
// cava 1.0.0 tag driven like cava.c's raw output with the original's raw_output_config.txt
// (50 bars, 48 kHz stereo S16 from PipeWire, autosens, noise_reduction 20, mono average,
// ascii 0..1000). The audio buffer follows input/common.c (overflow reset, paused reset).
#include "cavacore.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint32_t lcg_state = 12345;
static int lcg(void) {
    lcg_state = lcg_state * 1103515245u + 12345u;
    return (lcg_state >> 16) & 0x7fff;
}

static double *cava_in;
static int samples_counter, cava_buffer_size;

static void write_samples(int16_t samples, const int16_t *buf) { // input/common.c
    if (samples == 0)
        return;
    if (samples_counter + samples > cava_buffer_size) {
        for (uint16_t n = 0; n < cava_buffer_size; n++)
            cava_in[n] = 0;
        samples_counter = 0;
    }
    for (uint16_t i = 0; i < samples; i++)
        cava_in[i + samples_counter] = buf[i];
    samples_counter += samples;
}

static void reset_output_buffers(void) {
    for (uint16_t n = 0; n < cava_buffer_size; n++)
        cava_in[n] = 0;
    samples_counter = cava_buffer_size;
}

#include "scenario.h"

int main(void) {
    const int bars = 50, channels = 2;
    struct cava_plan *plan = cava_init(bars, 48000, channels, 1, 20.0 / 100, 50, 8000);
    if (plan->status) {
        fprintf(stderr, "%s\n", plan->error_message);
        return 1;
    }
    cava_buffer_size = plan->input_buffer_size;
    cava_in = calloc(cava_buffer_size, sizeof(double));
    double *cava_out = calloc(bars * channels, sizeof(double));
    float bars_left[50], bars_right[50], bars_raw[50];
    int16_t buf[2 * 512 * 32];
    long t = 0;
    for (int f = 0; f < FRAMES; f++) {
        int nb, sizes[32];
        scenario_buffers(f, &nb, sizes);
        for (int b = 0; b < nb; b++) {
            for (int i = 0; i < sizes[b]; i++, t++) {
                buf[2 * i] = sample(f, t, 0);
                buf[2 * i + 1] = sample(f, t, 1);
            }
            write_samples(sizes[b] * 2, buf);
        }
        if (scenario_paused(f))
            reset_output_buffers();
        cava_execute(cava_in, samples_counter, cava_out, plan); // cava.c: !high_framerate
        samples_counter = 0;
        for (int n = 0; n < bars * channels; n++) {
            cava_out[n] *= 1.0; // p.sens = 100 / 100
            if (cava_out[n] > 1.0)
                cava_out[n] = 1.0;
            else if (cava_out[n] < 0.0)
                cava_out[n] = 0.0;
            cava_out[n] *= 1000; // *dimension_value = ascii_max_range
        }
        for (int n = 0; n < bars; n++) {
            bars_left[n] = cava_out[n];
            bars_right[n] = cava_out[n + bars];
        }
        for (int n = 0; n < bars; n++)
            bars_raw[n] = (bars_left[n] + bars_right[n]) / 2;
        for (int n = 0; n < bars; n++) {
            int v = bars_raw[n];
            if (v > 1000)
                v = 1000;
            printf("%d;", v);
        }
        printf("\n");
    }
    return 0;
}
