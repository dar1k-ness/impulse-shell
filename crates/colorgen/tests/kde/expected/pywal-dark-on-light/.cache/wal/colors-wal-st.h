const char *colorname[] = {

  /* 8 normal colors */
  [0] = "#101417", /* black   */
  [1] = "#b2a8c3", /* red     */
  [2] = "#8eb1cf", /* green   */
  [3] = "#8cbade", /* yellow  */
  [4] = "#c0b8d3", /* blue    */
  [5] = "#9cc0de", /* magenta */
  [6] = "#c6bade", /* cyan    */
  [7] = "#aac6de", /* white   */

  /* 8 bright colors */
  [8]  = "#a6b5c3",  /* black   */
  [9]  = "#bcb3cb",  /* red     */
  [10] = "#9dbbd5", /* green   */
  [11] = "#9bc3e3", /* yellow  */
  [12] = "#c8c1d8", /* blue    */
  [13] = "#a9c8e3", /* magenta */
  [14] = "#cdc2e2", /* cyan    */
  [15] = "#b5cde2", /* white   */

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
