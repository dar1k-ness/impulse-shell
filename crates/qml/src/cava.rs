//! In-process audio visualizer: a port of cava 1.0.0 (`cavacore.c` plus the raw-output path of
//! `cava.c` and the PipeWire input) for the original's `scripts/cava/raw_output_config.txt`, so the media
//! controls no longer run a `cava` process. `CavaVisualizer` items get one frame of
//! `points` (50 bars, 0..1000) per 1/60 s while `running`, like the `cava -p ...` output lines.

use crate::runtime::World;
use crate::value::Value;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex};

/// `cavacore.c` `struct cava_plan` (linear scaling; 1.0.0 has no dB mode).
pub struct Plan {
    bars: usize,
    channels: usize,
    rate: i32,
    autosens: bool,
    sens_init: bool,
    sens: f64,
    framerate: f64,
    frame_skip: i32,
    noise_reduction: f64,
    bass_size: usize,
    size: usize,
    pub input_buffer_size: usize,
    input_buffer: Vec<f64>,
    lower: Vec<i32>,
    upper: Vec<i32>,
    eq: Vec<f64>,
    bass_cut_off_bar: usize,
    fall: Vec<f64>,
    mem: Vec<f64>,
    peak: Vec<f64>,
    prev: Vec<f64>,
    bass_multiplier: Vec<f64>,
    multiplier: Vec<f64>,
    fft_bass: Arc<dyn realfft::RealToComplex<f64>>,
    fft: Arc<dyn realfft::RealToComplex<f64>>,
    scratch_in: Vec<f64>,
    out_bass: [Vec<Complex<f64>>; 2],
    out: [Vec<Complex<f64>>; 2],
}

impl Plan {
    /// `cava_init(number_of_bars, rate, channels, autosens, noise_reduction, low_cut_off,
    /// high_cut_off)`; `None` where cava reports an error.
    pub fn new(
        bars: usize,
        rate: u32,
        channels: usize,
        autosens: bool,
        noise_reduction: f64,
        low_cut_off: i32,
        high_cut_off: i32,
    ) -> Option<Plan> {
        if !(1..=2).contains(&channels) || !(1..=384000).contains(&rate) {
            return None;
        }
        let mut fft_buffer_size = 512usize;
        fft_buffer_size *= match rate {
            8126..=16250 => 2,
            16251..=32500 => 4,
            32501..=75000 => 8,
            75001..=150000 => 16,
            150001..=300000 => 32,
            300001.. => 64,
            _ => 1,
        };
        if bars < 1
            || bars > fft_buffer_size / 2 + 1
            || low_cut_off < 1
            || high_cut_off < 1
            || low_cut_off >= high_cut_off
            || high_cut_off as u32 > rate / 2
        {
            return None;
        }
        let rate = rate as i32;
        let bass_size = fft_buffer_size * 2;
        let size = fft_buffer_size;
        let hann = |n: usize| -> Vec<f64> {
            (0..n)
                .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / (n - 1) as f64).cos()))
                .collect()
        };
        let mut planner = RealFftPlanner::<f64>::new();
        let fft_bass = planner.plan_fft_forward(bass_size);
        let fft = planner.plan_fft_forward(size);

        let mut lower = vec![0i32; bars + 1];
        let mut upper = vec![0i32; bars + 1];
        let mut cut_off_frequency = vec![0f32; bars + 1];
        let mut relative_cut_off = vec![0f32; bars + 1];
        let bass_cut_off = 100.0f32;
        let frequency_constant = ((low_cut_off as f32 / high_cut_off as f32) as f64).log10()
            / (1.0 / (bars as f32 + 1.0) - 1.0) as f64;
        let mut bass_cut_off_bar = 0usize;
        let mut first_bar;
        let min_bandwidth = (rate / bass_size as i32) as f32;
        let half_bass = (bass_size / 2) as i32;
        let half = (size / 2) as i32;
        for n in 0..bars + 1 {
            let mut coefficient = -frequency_constant;
            coefficient += ((n as f32 + 1.0) / (bars as f32 + 1.0)) as f64 * frequency_constant;
            cut_off_frequency[n] = (high_cut_off as f64 * 10f64.powf(coefficient)) as f32;
            if n > 0 && cut_off_frequency[n - 1] >= cut_off_frequency[n] {
                cut_off_frequency[n] = cut_off_frequency[n - 1] + min_bandwidth;
            }
            relative_cut_off[n] = cut_off_frequency[n] / (rate / 2) as f32;
            if cut_off_frequency[n] < bass_cut_off {
                lower[n] = (relative_cut_off[n] * half_bass as f32) as i32;
                bass_cut_off_bar += 1;
                first_bar = bass_cut_off_bar <= 1;
                if lower[n] > half_bass {
                    lower[n] = half_bass;
                }
            } else {
                lower[n] = (relative_cut_off[n] * half as f32).ceil() as i32;
                if n == bass_cut_off_bar {
                    first_bar = true;
                    if n > 0 {
                        upper[n - 1] = (relative_cut_off[n] * half_bass as f32 - 1.0) as i32;
                    }
                } else {
                    first_bar = false;
                }
                if lower[n] > half {
                    lower[n] = half;
                }
            }
            if n > 0 {
                if !first_bar {
                    upper[n - 1] = lower[n] - 1;
                    if lower[n] <= lower[n - 1] {
                        let room_for_more = if n < bass_cut_off_bar {
                            lower[n - 1] + 1 < half_bass + 1
                        } else {
                            lower[n - 1] + 1 < half + 1
                        };
                        if room_for_more {
                            lower[n] = lower[n - 1] + 1;
                            upper[n - 1] = lower[n] - 1;
                        }
                    }
                } else if upper[n - 1] < lower[n - 1] {
                    upper[n - 1] = lower[n - 1] + 1;
                }
            }
            relative_cut_off[n] = if n < bass_cut_off_bar {
                lower[n] as f32 / (bass_size as f32 / 2.0)
            } else {
                lower[n] as f32 / (size as f32 / 2.0)
            };
            cut_off_frequency[n] = relative_cut_off[n] * (rate as f32 / 2.0);
        }
        let eq = (0..bars)
            .map(|n| {
                let mut e = 1.0 / 2f64.powf(28.0);
                e *= (cut_off_frequency[n + 1] as f64).powf(0.85);
                e /= if n < bass_cut_off_bar {
                    (bass_size as f64).log2()
                } else {
                    (size as f64).log2()
                };
                e / (upper[n] - lower[n] + 1) as f64
            })
            .collect();
        let input_buffer_size = bass_size * channels;
        let nc = bars * channels;
        Some(Plan {
            bars,
            channels,
            rate,
            autosens,
            sens_init: true,
            sens: 1.0,
            framerate: 75.0,
            frame_skip: 1,
            noise_reduction,
            bass_size,
            size,
            input_buffer_size,
            input_buffer: vec![0.0; input_buffer_size],
            lower,
            upper,
            eq,
            bass_cut_off_bar,
            fall: vec![0.0; nc],
            mem: vec![0.0; nc],
            peak: vec![0.0; nc],
            prev: vec![0.0; nc],
            bass_multiplier: hann(bass_size),
            multiplier: hann(size),
            scratch_in: vec![0.0; bass_size],
            out_bass: [fft_bass.make_output_vec(), fft_bass.make_output_vec()],
            out: [fft.make_output_vec(), fft.make_output_vec()],
            fft_bass,
            fft,
        })
    }

    /// Window and FFT one channel (0 = left, 1 = right: cava reads interleaved input reversed,
    /// so the left sample of a frame is the odd index).
    fn transform(&mut self, bass: bool, ch: usize) {
        let (n, mul, fft) = if bass {
            (self.bass_size, &self.bass_multiplier, &self.fft_bass)
        } else {
            (self.size, &self.multiplier, &self.fft)
        };
        let input = &mut self.scratch_in[..n];
        for (i, x) in input.iter_mut().enumerate() {
            let raw = if self.channels == 2 {
                self.input_buffer[i * 2 + 1 - ch]
            } else {
                self.input_buffer[i]
            };
            *x = mul[i] * raw;
        }
        let out = if bass {
            &mut self.out_bass[ch]
        } else {
            &mut self.out[ch]
        };
        let _ = fft.process(input, out);
    }

    /// `cava_execute(cava_in, new_samples, cava_out, plan)`: `out` gets `bars * channels`
    /// values (left bars, then right bars).
    #[allow(clippy::needless_range_loop)]
    pub fn execute(&mut self, cava_in: &[f64], new_samples: usize, out: &mut [f64]) {
        let new_samples = new_samples.min(self.input_buffer_size);
        let mut silence = true;
        if new_samples > 0 {
            self.framerate -= self.framerate / 64.0;
            self.framerate +=
                (self.rate * self.frame_skip) as f64 / (new_samples / self.channels) as f64 / 64.0;
            self.frame_skip = 1;
            self.input_buffer
                .copy_within(..self.input_buffer_size - new_samples, new_samples);
            for (n, &v) in cava_in[..new_samples].iter().enumerate() {
                self.input_buffer[new_samples - n - 1] = v;
                if v != 0.0 {
                    silence = false;
                }
            }
        } else {
            self.frame_skip += 1;
        }
        for ch in 0..self.channels {
            self.transform(true, ch);
            self.transform(false, ch);
        }
        let bars = self.bars;
        for n in 0..bars {
            let mut temp = [0.0f64; 2];
            for i in self.lower[n]..=self.upper[n] {
                let i = i as usize;
                for (ch, t) in temp.iter_mut().enumerate().take(self.channels) {
                    let c = if n < self.bass_cut_off_bar {
                        self.out_bass[ch][i]
                    } else {
                        self.out[ch][i]
                    };
                    *t += c.re.hypot(c.im);
                }
            }
            out[n] = temp[0] * self.eq[n];
            if self.channels == 2 {
                out[n + bars] = temp[1] * self.eq[n];
            }
        }
        let nc = bars * self.channels;
        if self.autosens {
            for v in &mut out[..nc] {
                *v *= self.sens;
            }
        }
        let mut overshoot = false;
        let framerate_mod = 66.0 / self.framerate;
        let gravity_mod = framerate_mod.powf(2.5) * 2.0 / self.noise_reduction;
        let integral_mod = framerate_mod.powf(0.1);
        for n in 0..nc {
            if out[n] < self.prev[n] && self.noise_reduction > 0.1 {
                out[n] = self.peak[n] * (1.0 - (self.fall[n] * self.fall[n] * gravity_mod));
                if out[n] < 0.0 {
                    out[n] = 0.0;
                }
                self.fall[n] += 0.028;
            } else {
                self.peak[n] = out[n];
                self.fall[n] = 0.0;
            }
            self.prev[n] = out[n];
            out[n] += self.mem[n] * self.noise_reduction / integral_mod;
            self.mem[n] = out[n];
            if self.autosens && out[n] > 1.0 {
                overshoot = true;
                out[n] = 1.0;
            }
        }
        if self.autosens {
            if overshoot {
                self.sens *= 1.0 - 0.02 * framerate_mod;
                self.sens_init = false;
            } else if !silence {
                self.sens *= 1.0 + 0.001 * framerate_mod;
                if self.sens_init {
                    self.sens *= 1.0 + 0.1 * framerate_mod;
                }
            }
        }
    }
}

/// `input/common.c`: the samples PipeWire delivered since the last frame (`audio.cava_in`).
pub struct Input {
    pub cava_in: Vec<f64>,
    pub samples_counter: usize,
}

impl Input {
    pub fn new(cava_buffer_size: usize) -> Input {
        Input {
            cava_in: vec![0.0; cava_buffer_size],
            samples_counter: 0,
        }
    }

    /// `write_to_cava_input_buffers` for S16 samples.
    pub fn write(&mut self, buf: &[i16]) {
        if buf.is_empty() {
            return;
        }
        let size = self.cava_in.len();
        if self.samples_counter + buf.len() > size {
            self.cava_in.fill(0.0);
            self.samples_counter = 0;
        }
        let n = buf.len().min(size - self.samples_counter);
        for (d, s) in self.cava_in[self.samples_counter..][..n]
            .iter_mut()
            .zip(buf)
        {
            *d = *s as f64;
        }
        self.samples_counter += n;
    }

    /// [`Input::write`] for the bytes of a PipeWire S16LE buffer.
    pub fn write_s16le(&mut self, bytes: &[u8]) {
        let samples: Vec<i16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect();
        self.write(&samples);
    }

    /// `reset_output_buffers` (stream paused).
    pub fn reset(&mut self) {
        self.cava_in.fill(0.0);
        self.samples_counter = self.cava_in.len();
    }
}

/// Bar count, rate, channels and smoothing of `raw_output_config.txt` with cava 1.0.0's
/// defaults for everything it leaves out (PipeWire input: 48 kHz stereo S16, cutoffs 50..8000,
/// sensitivity 100, ascii range 1000).
pub const BARS: usize = 50;
pub const RATE: u32 = 48000;
pub const FRAMERATE: u32 = 60;
const ASCII_RANGE: i32 = 1000;

/// The time between two frames of the capture's timer: [`FRAMERATE`] of them a second.
#[cfg_attr(not(feature = "pipewire"), allow(dead_code))]
fn frame_period() -> std::time::Duration {
    std::time::Duration::from_nanos(1_000_000_000 / FRAMERATE as u64)
}

pub fn plan() -> Plan {
    Plan::new(BARS, RATE, 2, true, 20.0 / 100.0, 50, 8000).expect("valid cava plan")
}

/// One frame of `cava.c`'s main loop (all available samples, raw output, mono average): the
/// values of one output line as the original parses them (`data.split(";").map(parseFloat)`).
pub fn frame(plan: &mut Plan, input: &mut Input, out: &mut [f64]) -> Vec<f64> {
    plan.execute(&input.cava_in, input.samples_counter, out);
    input.samples_counter = 0;
    let bars = plan.bars;
    for v in out.iter_mut().take(bars * 2) {
        *v = v.clamp(0.0, 1.0) * ASCII_RANGE as f64;
    }
    (0..bars)
        .map(|n| {
            let left = out[n] as f32;
            let right = out[n + bars] as f32;
            ((((left + right) / 2.0) as i32).min(ASCII_RANGE)) as f64
        })
        .collect()
}

/// A new `CavaVisualizer` (`Impulse/cavavisualizer.qmltypes`): `running` (in) and `points`
/// (out); writes to `running` are what `poll` follows.
fn created(w: &mut World, id: usize) {
    w.declare(id, "running", "bool");
    w.declare(id, "points", "var");
    w.items[id].props.get_mut("points").unwrap().value = Value::array(vec![]);
    w.write_watch.insert(id, crate::svc::Svc::Cava);
    w.mirror(id, crate::svc::Svc::Cava);
    w.watched_writes.push((crate::svc::Svc::Cava, id));
}

pub static ELEMENT: crate::impulse::Element = crate::impulse::Element {
    created: Some(created),
    ..crate::impulse::Element::new("CavaVisualizer")
};

/// A `CavaVisualizer` instance, as QML creates one.
pub fn new_item(w: &mut World, parent: Option<usize>) -> usize {
    let id = w.new_inst(ELEMENT.name, parent);
    created(w, id);
    id
}

/// Every `CavaVisualizer` shares one capture: they all run the same plan on the same audio.
#[derive(Default)]
pub struct Cava {
    items: Vec<usize>,
    running: Vec<usize>,
    capture: Option<Capture>,
    last: Option<Vec<f64>>,
    /// Nothing the shell draws can be seen ([`set_unseen`]): no capture runs.
    unseen: bool,
}

impl Cava {
    /// Forgets a destroyed visualizer.
    pub(crate) fn remove(&mut self, i: usize) {
        self.items.retain(|&x| x != i);
        self.running.retain(|&x| x != i);
    }
}

/// The embedding shell says whether nothing it draws can be seen: no output is on, or the
/// session is locked. The capture stops for that time, so no frame wakes the loop and no
/// thread reads the audio; the visualizers keep their last points and get new ones as soon
/// as the capture is back.
pub fn set_unseen(w: &mut World, unseen: bool) {
    if w.cava.unseen != unseen {
        w.cava.unseen = unseen;
        w.waker.wake(crate::svc::Svc::Cava);
    }
}

/// Latest frame of the capture thread (`None` once read).
#[derive(Default)]
struct Shared {
    frame: Mutex<Option<Vec<f64>>>,
    #[cfg_attr(not(feature = "pipewire"), allow(dead_code))]
    wake: Option<crate::svc::Handle>,
}

impl Shared {
    #[cfg_attr(not(feature = "pipewire"), allow(dead_code))]
    fn publish(&self, points: Vec<f64>) {
        *self.frame.lock().unwrap() = Some(points);
        if let Some(h) = &self.wake {
            h.wake();
        }
    }
}

/// The capture thread while any `CavaVisualizer` runs. It follows the PipeWire daemon across
/// restarts on its own; dropping it requests its stop, which ends the thread.
pub struct Capture {
    shared: Arc<Shared>,
    stop: Arc<crate::svc::Stop>,
    /// Closes the capture loop's stop pipe at the stop.
    _close: crate::svc::OnStop<'static>,
    #[cfg_attr(not(test), allow(dead_code))]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Capture {
    /// No capture when the system gives no pipe or no thread (`EMFILE`, `EAGAIN`): said once
    /// in the log, and [`poll`] starts it again at its next pass. Both ends of the stop pipe
    /// are closed then.
    fn start(h: crate::svc::Handle) -> Option<Capture> {
        static SAID: std::sync::Once = std::sync::Once::new();
        let shared = Arc::new(Shared {
            wake: Some(h),
            ..Default::default()
        });
        let stop = Arc::new(crate::svc::Stop::default());
        let (stop_rx, stop_tx) = std::io::pipe()
            .inspect_err(|e| {
                SAID.call_once(|| elog::eprintln!("cava: cannot create the stop pipe: {e}"));
            })
            .ok()?;
        // The capture's main loop watches the read end; the stop closes the write end.
        let close = stop.on_stop_owned(move || drop(stop_tx));
        let (sh, st) = (shared.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("cava".into())
            .spawn(move || {
                // Said by the thread itself, which has its name by now: a reader of the line
                // finds the capture among the threads.
                if crate::log_on!("ri::cava") {
                    tracing::debug!(target: "ri::cava", "cava: capture started");
                }
                run(&sh, &st, stop_rx);
            })
            .inspect_err(|e| {
                SAID.call_once(|| elog::eprintln!("cava: cannot start its thread: {e}"));
            })
            .ok()?;
        Some(Capture {
            shared,
            stop,
            _close: close,
            thread: Some(thread),
        })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// One capture session per PipeWire connection, supervised like [`crate::pw`]'s: a session the
/// daemon ended is retried at once (a socket-activated daemon is started by the connect), a
/// failed connect waits for the daemon's socket to appear. Each ended session publishes `[]`,
/// as the QML version's points clear when its `cava` process dies with the daemon.
#[cfg(feature = "pipewire")]
fn run(shared: &Arc<Shared>, st: &Arc<crate::svc::Stop>, stop: std::io::PipeReader) {
    use std::ops::ControlFlow;
    pipewire::init();
    // Set up before the first attempt so a socket created in between is not missed.
    let mut watch = crate::pw::socket_path()
        .as_deref()
        .and_then(crate::pw::watch);
    if let Some(w) = watch.as_mut() {
        w.stop_on(st);
    }
    crate::pw::supervise(
        || {
            let mut connected = false;
            match capture(shared, &stop, &mut connected) {
                Ok(Ended::Stopped) => return ControlFlow::Break(()),
                Ok(Ended::Lost) => {}
                Err(e) => elog::eprintln!("cava: {e}"),
            }
            if connected {
                shared.publish(Vec::new());
            }
            ControlFlow::Continue(connected)
        },
        || watch.as_mut().is_some_and(crate::sysbus::SocketWatch::wait),
    );
}

#[cfg(not(feature = "pipewire"))]
fn run(_: &Arc<Shared>, _: &crate::svc::Stop, _: std::io::PipeReader) {}

#[cfg(feature = "pipewire")]
enum Ended {
    /// The [`Capture`] was dropped.
    Stopped,
    /// The daemon went away (core error on `PW_ID_CORE`, e.g. `EPIPE`).
    Lost,
}

/// What ends a turn of the capture's main loop.
#[cfg(feature = "pipewire")]
#[derive(Default)]
struct Events {
    ready: std::cell::Cell<bool>,
    lost: std::cell::Cell<bool>,
    stopped: std::cell::Cell<bool>,
}

/// `input/pipewire.c` (source `auto`, 48 kHz S16 stereo, remix, active, virtual) plus the
/// 60 fps frame loop of `cava.c` on a timer of the same loop, for one connection to the daemon.
/// `connected` is set once the stream is set up on a live connection.
#[cfg(feature = "pipewire")]
fn capture(
    shared: &Arc<Shared>,
    stop: &std::io::PipeReader,
    connected: &mut bool,
) -> Result<Ended, pipewire::Error> {
    use pipewire as pw;
    use pw::spa;
    use pw::stream::{StreamBox, StreamFlags, StreamState};
    use std::os::fd::AsRawFd;
    use std::rc::Rc;
    let ml = pw::main_loop::MainLoopRc::new(None)?;
    let events = Rc::new(Events::default());
    let quit = |set: fn(&Events) -> &std::cell::Cell<bool>| {
        let (events, ml) = (events.clone(), ml.downgrade());
        move || {
            set(&events).set(true);
            if let Some(ml) = ml.upgrade() {
                ml.quit();
            }
        }
    };
    let _stop = ml.loop_().add_io(
        stop.as_raw_fd(),
        spa::support::system::IoFlags::IN | spa::support::system::IoFlags::HUP,
        {
            let stopped = quit(|e| &e.stopped);
            move |_| stopped()
        },
    );
    let ctx = pw::context::ContextRc::new(&ml, Some(crate::pw::context_properties()))?;
    let core = ctx.connect_rc(None)?;
    // The daemon going away is a fatal error on the core proxy (pw_core_events.error).
    let _core = core
        .add_listener_local()
        .error({
            let lost = quit(|e| &e.lost);
            move |id, _, _, _| {
                if id == pw::core::PW_ID_CORE {
                    lost();
                }
            }
        })
        .register();
    let plan = plan();
    let input = Arc::new(Mutex::new(Input::new(plan.input_buffer_size)));
    // `next_power_of_2((input_buffer_size / 2) * rate / 48000)` with cava's 2 * 512 samples.
    let latency = format!("{}/{RATE}", (512 * RATE / 48000).next_power_of_two());
    let props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::STREAM_CAPTURE_SINK => "true",
        *pw::keys::NODE_LATENCY => latency,
        *pw::keys::NODE_ALWAYS_PROCESS => "true",
        *pw::keys::NODE_VIRTUAL => "true",
        *pw::keys::STREAM_DONT_REMIX => "false",
        "channelmix.upmix" => "true",
    };
    let stream = StreamBox::new(&core, "cava", props)?;
    let _listener = stream
        .add_local_listener_with_user_data(())
        .state_changed({
            let input = input.clone();
            let ready = quit(|e| &e.ready);
            let events = events.clone();
            move |_, _, _, new| match new {
                StreamState::Paused | StreamState::Streaming => {
                    if matches!(new, StreamState::Paused) {
                        input.lock().unwrap().reset();
                    }
                    if !events.ready.get() {
                        ready();
                    }
                }
                // Unlinked by the session manager or its target gone: the bars fall silent
                // until the stream is linked again; only the daemon going away ends the session.
                StreamState::Error(_) | StreamState::Unconnected => input.lock().unwrap().reset(),
                StreamState::Connecting => {}
            }
        })
        .process({
            let input = input.clone();
            move |stream, _| {
                let Some(mut b) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = b.datas_mut();
                let Some(d) = datas.first_mut() else { return };
                let size = d.chunk().size() as usize;
                if let Some(bytes) = d.data() {
                    let n = size.min(bytes.len()) / 2;
                    input.lock().unwrap().write_s16le(&bytes[..n * 2]);
                }
            }
        })
        .register()?;
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::S16LE);
    info.set_rate(RATE);
    info.set_channels(2);
    let mut position = [0; spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    info.set_position(position);
    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|_| pw::Error::CreationFailed)?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&values).ok_or(pw::Error::CreationFailed)?];
    stream.connect(
        spa::utils::Direction::Input,
        None,
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
        &mut params,
    )?;
    *connected = true;
    let ended = || {
        if events.stopped.get() {
            Some(Ended::Stopped)
        } else if events.lost.get() {
            Some(Ended::Lost)
        } else {
            None
        }
    };
    // The frame loop starts once the daemon has the stream (as `cava.c` starts drawing once its
    // input thread is up): a connect into a socket-activated daemon's backlog that never gets an
    // answer publishes nothing.
    ml.run();
    if let Some(e) = ended() {
        return Ok(e);
    }
    let state = std::cell::RefCell::new((plan, vec![0.0; BARS * 2], Vec::<f64>::new()));
    let timer = ml.loop_().add_timer({
        let shared = shared.clone();
        move |_| {
            let (plan, out, last) = &mut *state.borrow_mut();
            let points = frame(plan, &mut input.lock().unwrap(), out);
            // An unchanged line draws the same bars: only new values reach the QML side.
            if points != *last {
                *last = points.clone();
                shared.publish(points);
            }
        }
    });
    let _ = timer.update_timer(Some(frame_period()), Some(frame_period()));
    ml.run();
    Ok(ended().unwrap_or(Ended::Lost))
}

/// Start/stop the shared capture with the visualizers' `running` and publish frames as
/// `points` (`[]` once stopped, like the QML version when cava exits). No capture runs while
/// nothing is on screen ([`set_unseen`]).
pub(crate) fn poll(w: &mut World) -> bool {
    let dirty = w.take_watched(crate::svc::Svc::Cava);
    let mut c = std::mem::take(&mut w.cava);
    w.native_writer = Some(crate::svc::Svc::Cava);
    let mut changed = false;
    for i in dirty {
        if !c.items.contains(&i) {
            c.items.push(i);
        }
    }
    c.items.retain(|i| !w.dead.contains(i));
    c.running.retain(|i| !w.dead.contains(i));
    for &i in &c.items {
        let running = w.read(i, "running").is_some_and(|v| v.truthy());
        match (running, c.running.contains(&i)) {
            (true, false) => {
                c.running.push(i);
                if let Some(f) = &c.last {
                    w.write(i, "points", points(f));
                    changed = true;
                }
            }
            (false, true) => {
                c.running.retain(|&r| r != i);
                w.write(i, "points", Value::array(vec![]));
                changed = true;
            }
            _ => {}
        }
    }
    if c.running.is_empty() {
        c.capture = None;
        c.last = None;
    } else if c.unseen {
        if c.capture.take().is_some() && crate::log_on!("ri::cava") {
            tracing::debug!(target: "ri::cava", "cava: capture stopped, nothing is on screen");
        }
    } else if c.capture.is_none() {
        c.capture = Capture::start(w.waker.handle(crate::svc::Svc::Cava));
    }
    if let Some(f) = c
        .capture
        .as_ref()
        .and_then(|k| k.shared.frame.lock().unwrap().take())
    {
        for &i in &c.running {
            w.write(i, "points", points(&f));
        }
        c.last = Some(f);
        changed = true;
    }
    w.native_writer = None;
    w.cava = c;
    changed
}

fn points(f: &[f64]) -> Value {
    Value::array(f.iter().copied().map(Value::Num).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The system refuses the capture's thread (the limit on threads is reached): there is no
    /// capture, so the next pass starts it again; the refusal is in the log, and both ends of
    /// the stop pipe are closed. In a process of its own: the test reads what that process
    /// wrote to its stderr and counts its open descriptors.
    #[test]
    fn a_refused_capture_thread_is_logged_and_leaves_no_descriptor() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_capture_thread_is_logged_and_leaves_no_descriptor"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let open = || std::fs::read_dir("/proc/self/fd").unwrap().count();
        let rt = crate::runtime::Runtime::new(None);
        let mut w = rt.world.borrow_mut();
        let a = new_item(&mut w, None);
        poll(&mut w);
        w.write(a, "running", Value::Bool(true));
        crate::testenv::refuse_new_threads();
        let before = open();
        let (_, text) = crate::testenv::stderr_with(|| poll(&mut w));
        assert!(w.cava.capture.is_none());
        assert_eq!(open(), before, "open descriptors");
        assert!(
            text.contains("cava: cannot start its thread:"),
            "stderr: {text:?}"
        );
    }

    /// All running visualizers share one capture and each gets every frame; it stops with
    /// the last of them.
    #[test]
    fn visualizers_share_one_capture() {
        let rt = crate::runtime::Runtime::new(None);
        let mut w = rt.world.borrow_mut();
        let (a, b) = (new_item(&mut w, None), new_item(&mut w, None));
        poll(&mut w);
        assert!(w.cava.capture.is_none());
        w.write(a, "running", Value::Bool(true));
        w.write(b, "running", Value::Bool(true));
        poll(&mut w);
        let shared = w.cava.capture.as_ref().expect("capture").shared.clone();
        *shared.frame.lock().unwrap() = Some(vec![1.0, 2.0]);
        poll(&mut w);
        let pts = |w: &mut World, i| w.read(i, "points").unwrap().to_js_string();
        assert_eq!(
            (pts(&mut w, a), pts(&mut w, b)),
            ("1,2".into(), "1,2".into())
        );
        assert!(!w.has_watched(crate::svc::Svc::Cava));
        w.write(a, "running", Value::Bool(false));
        poll(&mut w);
        assert_eq!(pts(&mut w, a), "");
        assert!(Arc::ptr_eq(
            &w.cava.capture.as_ref().unwrap().shared,
            &shared
        ));
        let thread = w.cava.capture.as_mut().unwrap().thread.take();
        w.write(b, "running", Value::Bool(false));
        poll(&mut w);
        assert!(w.cava.capture.is_none());
        // Dropping the capture ends its thread, whether it was capturing or waiting for the
        // daemon's socket.
        thread.expect("capture thread").join().unwrap();
    }

    /// With no output on or the session locked the capture is gone: its thread ends and the
    /// loop gets no frame. The visualizer keeps running for QML, with the points it had, and
    /// the capture is back in the pass after the screen is.
    #[test]
    fn nothing_on_screen_stops_the_capture() {
        let rt = crate::runtime::Runtime::new(None);
        let mut w = rt.world.borrow_mut();
        let a = new_item(&mut w, None);
        w.write(a, "running", Value::Bool(true));
        poll(&mut w);
        let shared = w.cava.capture.as_ref().expect("capture").shared.clone();
        *shared.frame.lock().unwrap() = Some(vec![1.0, 2.0]);
        poll(&mut w);
        let thread = w.cava.capture.as_mut().unwrap().thread.take();
        w.waker.take();
        set_unseen(&mut w, true);
        assert!(
            w.waker.take().has(crate::svc::Svc::Cava),
            "the loop is not woken for the visualizer"
        );
        assert!(
            !poll(&mut w),
            "a pass that only stops the capture changed an item"
        );
        assert!(w.cava.capture.is_none(), "a capture with nothing on screen");
        thread.expect("capture thread").join().unwrap();
        let pts = |w: &mut World, i| w.read(i, "points").unwrap().to_js_string();
        assert_eq!(pts(&mut w, a), "1,2");
        assert!(w.read(a, "running").unwrap().truthy());
        // A visualizer that starts meanwhile waits too.
        let b = new_item(&mut w, None);
        w.write(b, "running", Value::Bool(true));
        poll(&mut w);
        assert!(w.cava.capture.is_none(), "a capture with nothing on screen");
        set_unseen(&mut w, true);
        assert!(
            !w.waker.take().has(crate::svc::Svc::Cava),
            "woken for no change"
        );
        set_unseen(&mut w, false);
        assert!(w.waker.take().has(crate::svc::Svc::Cava));
        poll(&mut w);
        let shared = w.cava.capture.as_ref().expect("capture").shared.clone();
        *shared.frame.lock().unwrap() = Some(vec![3.0]);
        poll(&mut w);
        assert_eq!((pts(&mut w, a), pts(&mut w, b)), ("3".into(), "3".into()));
    }

    /// `tools/reference/cava-equiv/scenario.h`.
    struct Scenario(u32);
    const FRAMES: usize = 300;

    impl Scenario {
        fn lcg(&mut self) -> i64 {
            self.0 = self.0.wrapping_mul(1103515245).wrapping_add(12345);
            ((self.0 >> 16) & 0x7fff) as i64
        }
        fn buffers(f: usize) -> Vec<usize> {
            const PAT: [[usize; 3]; 7] = [
                [512, 0, 0],
                [512, 512, 0],
                [512, 0, 0],
                [512, 512, 0],
                [256, 0, 0],
                [0, 0, 0],
                [512, 512, 512],
            ];
            if f == 150 {
                return vec![512; 20];
            }
            PAT[f % 7].iter().copied().filter(|&s| s != 0).collect()
        }
        fn sample(&mut self, f: usize, t: i64, ch: usize) -> i16 {
            let loud = (30..180).contains(&f) || f >= 240;
            if !loud {
                return 0;
            }
            let amp = 3000 + (f as i64 * 37) % 20000;
            let mut v = if ch == 0 {
                ((t * 97) % 2000 - 1000) * amp / 1000
            } else {
                (self.lcg() - 16384) * amp / 16384
            };
            if f >= 240 {
                v = v * 3 / 2 + if t % 64 < 32 { 4000 } else { -4000 };
            }
            v.clamp(-32768, 32767) as i16
        }
    }

    /// cava draws `framerate` frames a second: the capture's timer ticks that often.
    #[test]
    fn a_second_holds_as_many_frames_as_the_framerate() {
        let second = frame_period() * FRAMERATE;
        let short = std::time::Duration::from_secs(1) - second;
        // What integer nanoseconds lose: under one per frame.
        assert!(
            short < std::time::Duration::from_nanos(FRAMERATE.into()),
            "{short:?}"
        );
        assert_eq!(frame_period().as_micros(), 16_666);
    }

    /// Bit-for-bit the lines `tools/reference/cava-equiv/harness.c` prints with cava 1.0.0's own
    /// `cavacore.c` (fftw) for the same PCM.
    #[test]
    fn matches_cava_1_0_0_raw_output() {
        let reference: Vec<&str> = include_str!("cava_ref.txt").lines().collect();
        assert_eq!(reference.len(), FRAMES);
        let mut plan = plan();
        let mut input = Input::new(plan.input_buffer_size);
        let mut out = vec![0.0; BARS * 2];
        let mut sc = Scenario(12345);
        let mut t = 0i64;
        let mut mismatches = vec![];
        for (f, want) in reference.iter().enumerate() {
            for size in Scenario::buffers(f) {
                let mut buf = Vec::with_capacity(size * 2);
                for _ in 0..size {
                    buf.push(sc.sample(f, t, 0));
                    buf.push(sc.sample(f, t, 1));
                    t += 1;
                }
                input.write(&buf);
            }
            if f == 230 {
                input.reset();
            }
            let got: String = frame(&mut plan, &mut input, &mut out)
                .iter()
                .map(|v| format!("{v};"))
                .collect();
            if got != *want {
                mismatches.push(format!("frame {f}:\n  got  {got}\n  want {want}"));
            }
        }
        assert!(
            mismatches.is_empty(),
            "{} frames differ:\n{}",
            mismatches.len(),
            mismatches[..mismatches.len().min(3)].join("\n")
        );
    }
}
