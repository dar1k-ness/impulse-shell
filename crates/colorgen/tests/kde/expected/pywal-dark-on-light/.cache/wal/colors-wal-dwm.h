static const char norm_fg[] = "#b5cde2";
static const char norm_bg[] = "#101417";
static const char norm_border[] = "#a6b5c3";

static const char sel_fg[] = "#b5cde2";
static const char sel_bg[] = "#8eb1cf";
static const char sel_border[] = "#b5cde2";

static const char urg_fg[] = "#b5cde2";
static const char urg_bg[] = "#b2a8c3";
static const char urg_border[] = "#b2a8c3";

static const char *colors[][3]      = {
    /*               fg           bg         border                         */
    [SchemeNorm] = { norm_fg,     norm_bg,   norm_border }, // unfocused wins
    [SchemeSel]  = { sel_fg,      sel_bg,    sel_border },  // the focused win
    [SchemeUrg] =  { urg_fg,      urg_bg,    urg_border },
};
