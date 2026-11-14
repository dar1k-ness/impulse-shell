const char *colorname[] = {

  /* 8 normal colors */
  [0] = "#f4f6fc", /* black   */
  [1] = "#2c0e7d", /* red     */
  [2] = "#35108e", /* green   */
  [3] = "#3912a0", /* yellow  */
  [4] = "#0a5189", /* blue    */
  [5] = "#0c599b", /* magenta */
  [6] = "#0d62ad", /* cyan    */
  [7] = "#4180c7", /* white   */

  /* 8 bright colors */
  [8]  = "#505e6a",  /* black   */
  [9]  = "#250f6a",  /* red     */
  [10] = "#2c1179", /* green   */
  [11] = "#301388", /* yellow  */
  [12] = "#0a4475", /* blue    */
  [13] = "#0c4c83", /* magenta */
  [14] = "#0d5392", /* cyan    */
  [15] = "#376da8", /* white   */

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
