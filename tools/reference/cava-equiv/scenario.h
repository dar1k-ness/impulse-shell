// Shared by harness.c and crates/qml/src/cava.rs tests (integer-only, so both sides agree).
#define FRAMES 300
static void scenario_buffers(int f, int *nb, int *sizes) {
    static const int pat[7][3] = {{512, 0, 0}, {512, 512, 0}, {512, 0, 0}, {512, 512, 0},
                                  {256, 0, 0}, {0, 0, 0},     {512, 512, 512}};
    *nb = 0;
    if (f == 150) { // more than cava_buffer_size: overflow reset
        for (int i = 0; i < 20; i++)
            sizes[(*nb)++] = 512;
        return;
    }
    for (int i = 0; i < 3; i++)
        if (pat[f % 7][i])
            sizes[(*nb)++] = pat[f % 7][i];
}
static int scenario_paused(int f) { return f == 230; }
static int16_t sample(int f, long t, int ch) {
    int loud = (f >= 30 && f < 180) || f >= 240;
    if (!loud)
        return 0;
    int amp = 3000 + (f * 37) % 20000;
    long v = ch == 0 ? ((t * 97) % 2000 - 1000) * amp / 1000 : (long)(lcg() - 16384) * amp / 16384;
    if (f >= 240)
        v = v * 3 / 2 + ((t % 64) < 32 ? 4000 : -4000);
    if (v > 32767)
        v = 32767;
    if (v < -32768)
        v = -32768;
    return (int16_t)v;
}
