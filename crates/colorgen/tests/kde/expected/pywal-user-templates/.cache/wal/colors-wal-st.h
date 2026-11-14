const char *colorname[] = {

  /* 8 normal colors */
  [0] = "#180f0e", /* black   */
  [1] = "#f4acb6", /* red     */
  [2] = "#c1e3b7", /* green   */
  [3] = "#f2dfbe", /* yellow  */
  [4] = "#adc5f7", /* blue    */
  [5] = "#f7cfec", /* magenta */
  [6] = "#b1e0d9", /* cyan    */
  [7] = "#f8c2a8", /* white   */

  /* 8 bright colors */
  [8]  = "#caaba7",  /* black   */
  [9]  = "#f6b7c0",  /* red     */
  [10] = "#cae7c1", /* green   */
  [11] = "#f4e3c7", /* yellow  */
  [12] = "#b8ccf8", /* blue    */
  [13] = "#f8d5ef", /* magenta */
  [14] = "#bce3de", /* cyan    */
  [15] = "#f9cab4", /* white   */

  /* special colors */
  [256] = "#180f0e", /* background */
  [257] = "#f9d5d0", /* foreground */
  [258] = "#f1dedc",     /* cursor */
};

/* Default colors (colorname index)
 * foreground, background, cursor */
 unsigned int defaultbg = 0;
 unsigned int defaultfg = 257;
 unsigned int defaultcs = 258;
 unsigned int defaultrcs= 258;
