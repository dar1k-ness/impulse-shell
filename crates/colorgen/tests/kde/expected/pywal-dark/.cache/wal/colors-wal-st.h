const char *colorname[] = {

  /* 8 normal colors */
  [0] = "#101417", /* black   */
  [1] = "#f3acb6", /* red     */
  [2] = "#c1e4b7", /* green   */
  [3] = "#f1dfbe", /* yellow  */
  [4] = "#acc5f7", /* blue    */
  [5] = "#f6d0ec", /* magenta */
  [6] = "#b0e0d9", /* cyan    */
  [7] = "#f7c2a8", /* white   */

  /* 8 bright colors */
  [8]  = "#a6b5c3",  /* black   */
  [9]  = "#f5b7c0",  /* red     */
  [10] = "#c9e7c1", /* green   */
  [11] = "#f2e3c7", /* yellow  */
  [12] = "#b7cdf8", /* blue    */
  [13] = "#f7d6ef", /* magenta */
  [14] = "#bbe4de", /* cyan    */
  [15] = "#f8cab4", /* white   */

  /* special colors */
  [256] = "#101417", /* background */
  [257] = "#cedff0", /* foreground */
  [258] = "#e0e3e8",     /* cursor */
};

/* Default colors (colorname index)
 * foreground, background, cursor */
 unsigned int defaultbg = 0;
 unsigned int defaultfg = 257;
 unsigned int defaultcs = 258;
 unsigned int defaultrcs= 258;
