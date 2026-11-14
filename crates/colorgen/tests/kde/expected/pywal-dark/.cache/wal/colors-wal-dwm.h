static const char norm_fg[] = "#f8cab4";
static const char norm_bg[] = "#101417";
static const char norm_border[] = "#a6b5c3";

static const char sel_fg[] = "#f8cab4";
static const char sel_bg[] = "#c1e4b7";
static const char sel_border[] = "#f8cab4";

static const char urg_fg[] = "#f8cab4";
static const char urg_bg[] = "#f3acb6";
static const char urg_border[] = "#f3acb6";

static const char *colors[][3]      = {
    /*               fg           bg         border                         */
    [SchemeNorm] = { norm_fg,     norm_bg,   norm_border }, // unfocused wins
    [SchemeSel]  = { sel_fg,      sel_bg,    sel_border },  // the focused win
    [SchemeUrg] =  { urg_fg,      urg_bg,    urg_border },
};
