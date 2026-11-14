const char *colorname[] = {

  /* 8 normal colors */
  [0] = "#f4f6fc", /* black   */
  [1] = "#b84844", /* red     */
  [2] = "#5bb850", /* green   */
  [3] = "#cf9a50", /* yellow  */
  [4] = "#3161c1", /* blue    */
  [5] = "#cf4fa2", /* magenta */
  [6] = "#54b4a1", /* cyan    */
  [7] = "#cd6947", /* white   */

  /* 8 bright colors */
  [8]  = "#505e6a",  /* black   */
  [9]  = "#9b3e3b",  /* red     */
  [10] = "#4d9b46", /* green   */
  [11] = "#ae8246", /* yellow  */
  [12] = "#2952a3", /* blue    */
  [13] = "#ae4489", /* magenta */
  [14] = "#479888", /* cyan    */
  [15] = "#ad593e", /* white   */

  /* special colors */
  [256] = "#f4f6fc", /* background */
  [257] = "#303f4c", /* foreground */
  [258] = "#e0e3e8",     /* cursor */
};

/* Default colors (colorname index)
 * foreground, background, cursor */
 unsigned int defaultbg = 0;
 unsigned int defaultfg = 257;
 unsigned int defaultcs = 258;
 unsigned int defaultrcs= 258;
