static const char norm_fg[] = "#376da8";
static const char norm_bg[] = "#f4f6fc";
static const char norm_border[] = "#505e6a";

static const char sel_fg[] = "#376da8";
static const char sel_bg[] = "#35108e";
static const char sel_border[] = "#376da8";

static const char urg_fg[] = "#376da8";
static const char urg_bg[] = "#2c0e7d";
static const char urg_border[] = "#2c0e7d";

static const char *colors[][3]      = {
    /*               fg           bg         border                         */
    [SchemeNorm] = { norm_fg,     norm_bg,   norm_border }, // unfocused wins
    [SchemeSel]  = { sel_fg,      sel_bg,    sel_border },  // the focused win
    [SchemeUrg] =  { urg_fg,      urg_bg,    urg_border },
};
