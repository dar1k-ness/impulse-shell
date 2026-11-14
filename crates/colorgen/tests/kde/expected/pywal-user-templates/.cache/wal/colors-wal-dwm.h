static const char norm_fg[] = "#f9cab4";
static const char norm_bg[] = "#180f0e";
static const char norm_border[] = "#caaba7";

static const char sel_fg[] = "#f9cab4";
static const char sel_bg[] = "#c1e3b7";
static const char sel_border[] = "#f9cab4";

static const char urg_fg[] = "#f9cab4";
static const char urg_bg[] = "#f4acb6";
static const char urg_border[] = "#f4acb6";

static const char *colors[][3]      = {
    /*               fg           bg         border                         */
    [SchemeNorm] = { norm_fg,     norm_bg,   norm_border }, // unfocused wins
    [SchemeSel]  = { sel_fg,      sel_bg,    sel_border },  // the focused win
    [SchemeUrg] =  { urg_fg,      urg_bg,    urg_border },
};
