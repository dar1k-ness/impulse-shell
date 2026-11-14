use crate::sine_table::QT_SINE_TABLE;
const N: i32 = 256;
pub fn q_fast_sin(x: f64) -> f64 {
    let mut si = (x * (0.5 * N as f64 / std::f64::consts::PI)) as i32;
    let d = x - si as f64 * (2.0 * std::f64::consts::PI / N as f64);
    let mut ci = si.wrapping_add(N / 4);
    si &= N - 1;
    ci &= N - 1;
    let (s, c) = (QT_SINE_TABLE[si as usize], QT_SINE_TABLE[ci as usize]);
    s + (c - 0.5 * s * d) * d
}
pub fn q_fast_cos(x: f64) -> f64 {
    let mut ci = (x * (0.5 * N as f64 / std::f64::consts::PI)) as i32;
    let d = x - ci as f64 * (2.0 * std::f64::consts::PI / N as f64);
    let mut si = ci.wrapping_add(N / 4);
    si &= N - 1;
    ci &= N - 1;
    let (s, c) = (QT_SINE_TABLE[si as usize], QT_SINE_TABLE[ci as usize]);
    s - (c + 0.5 * s * d) * d
}
