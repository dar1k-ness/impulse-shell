//! Timed sources the animation tick steps: QML `Timer`s, `FrameAnimation`s, `AnimatedImage`s
//! and `SystemClock`s. Like Qt's QTimerInfoList and QUnifiedTimer, only running sources are
//! visited: a write to a property that starts, stops or retimes a source queues it for
//! `World::sync_steppers`, which updates its entry, and running Timers sit in a queue ordered by
//! due time whose head is the main loop's next timer deadline. A Timer whose component is
//! complete is updated inside the write, as QQmlTimer's setters do, and triggers through posted
//! events (`Posted`), as QQmlTimer does.

use crate::runtime::{Fire, FrameClock, Posted, Started, Sym, World};
use crate::value::Value;
use rustc_hash::FxHashMap as HashMap;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Timer,
    FrameAnimation,
    AnimatedImage,
    SystemClock,
}

fn kinds() -> &'static [(Sym, Kind); 4] {
    static K: std::sync::LazyLock<[(Sym, Kind); 4]> = std::sync::LazyLock::new(|| {
        [
            (Sym::new("Timer"), Kind::Timer),
            (Sym::new("FrameAnimation"), Kind::FrameAnimation),
            (Sym::new("AnimatedImage"), Kind::AnimatedImage),
            (Sym::new("SystemClock"), Kind::SystemClock),
        ]
    });
    &K
}

/// Properties whose writes can start, stop or retime a stepped source.
fn steps_on(name: &str) -> bool {
    matches!(
        name,
        "running" | "interval" | "repeat" | "triggeredOnStart" | "paused" | "playing" | "precision"
    )
}

/// A due time in loop milliseconds, ordered totally (deadlines are never NaN).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Due(f64);

impl Eq for Due {}

impl PartialOrd for Due {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Due {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&o.0)
    }
}

/// A running Timer: the clock (ms) its current loop began at, its interval, `repeat` and
/// `triggeredOnStart`, whether it has not ticked since it started (QQmlTimer's `firstTick`),
/// and its start order: QQmlTimer's QPauseAnimation joins the unified animation timer's list on
/// every (re)start and stays there across loops. `done`: the countdown of a Timer that does not
/// repeat has ended (its QPauseAnimation is Stopped) and its QEvent_Triggered is posted.
/// `looped`: a loop has ended since the countdown began (the QPauseAnimation's `currentTime()`
/// is above 0 when a QEvent_MaybeTick arrives).
#[derive(Clone, Copy, Debug, PartialEq)]
struct TimerRec {
    seq: u64,
    start: f64,
    iv: f64,
    repeat: bool,
    on_start: bool,
    first: bool,
    done: bool,
    looped: bool,
}

/// What a write to a stepped source's property asks of it at the next sync.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Change(u8);

impl Change {
    /// A new source, or a property that only gates it (paused, playing, ...).
    const STATE: Change = Change(0);
    /// `running` changed (QQmlTimer::setRunning): a new countdown with a first tick.
    const RUN: Change = Change(1);
    /// `interval`, `repeat` or `triggeredOnStart` changed: QQmlTimer::update restarts the
    /// countdown of a running Timer.
    const RETIME: Change = Change(2);

    fn of(name: &str) -> Change {
        match name {
            "running" => Change::RUN,
            "interval" | "repeat" | "triggeredOnStart" => Change::RETIME,
            _ => Change::STATE,
        }
    }

    fn has(self, c: Change) -> bool {
        self.0 & c.0 != 0
    }

    fn restarts(self) -> bool {
        self.has(Change(Change::RUN.0 | Change::RETIME.0))
    }
}

impl TimerRec {
    /// When the current loop ends; `None` once the countdown has ended, and for a repeating
    /// Timer with interval 0: its QPauseAnimation (duration 0, infinite loops) never completes
    /// a loop, so it never triggers.
    fn due(&self) -> Option<Due> {
        (!self.done && (!self.repeat || self.iv > 0.0)).then_some(Due(self.start + self.iv))
    }
}

/// What an AnimatedImage plays: the file it was loaded for (`\x01frameSrc`), its movie and
/// the signals of the movie that have not set their property yet.
struct MovieRec {
    tag: String,
    player: crate::animated::Player,
    acts: std::collections::VecDeque<crate::animated::Act>,
}

/// Properties of an AnimatedImage its movie follows.
fn movie_prop(name: &str) -> bool {
    matches!(
        name,
        "currentFrame"
            | "playing"
            | "paused"
            | "loops"
            | "cache"
            | "finishBehavior"
            | "\x01frameSrc"
    )
}

#[derive(Default)]
pub struct Steppers {
    /// Items below this id were checked for a stepped type.
    seen: usize,
    /// Sources whose state may have changed, with the animation clock at the change and what
    /// changed.
    dirty: Vec<(usize, f64, Change)>,
    timers: HashMap<usize, TimerRec>,
    /// Running Timers by due time.
    queue: BTreeSet<(Due, usize)>,
    /// Start order of the next Timer (re)start.
    next_seq: u64,
    /// Timers and FrameAnimations at or above `seen` that are complete
    /// (`World::timers_complete`).
    early: BTreeSet<usize>,
    /// How many jobs of FrameAnimations were started (`FrameClock::seq`).
    frame_jobs: u64,
    /// The jobs still to step at this tick of the clock, the next one last: which start it
    /// was and the FrameAnimation.
    pub(crate) frame_due: Vec<(u64, usize)>,
    /// Timers with a QEvent_MaybeTick posted and not delivered (QQmlTimer's `awaitingTick`).
    awaiting: BTreeSet<usize>,
    /// Objects a `destroy(delay)` call deletes later, by due time.
    destroy: BTreeSet<(Due, usize)>,
    /// AnimatedImages whose movie waits for its next frame.
    anim_imgs: BTreeSet<usize>,
    /// The movie of each AnimatedImage whose file the shell has read.
    movies: HashMap<usize, MovieRec>,
    /// AnimatedImages whose properties the movie has not looked at since they changed.
    movie_dirty: Vec<usize>,
    clocks: BTreeSet<usize>,
    /// Loop ms of the next SystemClock boundary (`None`: compute it again), like Quickshell's
    /// SystemClock timer armed for the next boundary at its precision.
    clock_due: Option<f64>,
    /// What each SystemClock read last: the seconds between its ticks and the second since
    /// the epoch at which the step it shows began.
    clock_read: HashMap<usize, (i64, i64)>,
}

impl Steppers {
    /// Running Timers.
    pub fn timers(&self) -> usize {
        self.timers.len()
    }
}

impl World {
    fn stepped_kind(&self, i: usize) -> Option<Kind> {
        let ks = kinds();
        self.items[i]
            .types
            .iter()
            .find_map(|t| ks.iter().find(|(s, _)| s == t).map(|(_, k)| *k))
    }

    /// A property of `item` changed. A Timer that is complete runs QQmlTimer::update inside
    /// the write, as each QQmlTimer setter does: what it posts takes its place in the queue of
    /// posted events there. Any other stepped source re-reads its state at the next sync.
    pub(crate) fn stepper_changed(&mut self, item: usize, name: &str) {
        let kind = self.stepped_kind(item);
        if kind == Some(Kind::AnimatedImage) && movie_prop(name) {
            self.steppers.movie_dirty.push(item);
            return;
        }
        if !steps_on(name) {
            return;
        }
        let c = Change::of(name);
        match kind {
            Some(Kind::Timer) if c.restarts() && self.timer_complete(item) => {
                self.sync_timer(item, self.now, c);
            }
            // QQuickFrameAnimation::setRunning and setPaused run updateState() inside the call.
            Some(Kind::FrameAnimation) if item < self.steppers.seen => {
                if matches!(name, "running" | "paused") {
                    self.frame_anim_update(item);
                }
            }
            Some(_) => self.steppers.dirty.push((item, self.now, c)),
            None => {}
        }
    }

    fn timer_complete(&self, t: usize) -> bool {
        t < self.steppers.seen || self.steppers.early.contains(&t)
    }

    /// QQmlTimer::componentComplete of the Timers created from `n0` on, last-created first as
    /// Qt completes a component's objects: each runs QQmlTimer::update for what its properties
    /// were given, so one that is running with interval 0 posts its trigger before any
    /// `Component.onCompleted` runs. The FrameAnimations complete in the same pass, which is
    /// the order their jobs start in.
    pub(crate) fn timers_complete(&mut self, n0: usize) {
        for t in (n0.max(self.steppers.seen)..self.items.len()).rev() {
            let kind = self.stepped_kind(t);
            if !matches!(kind, Some(Kind::Timer | Kind::FrameAnimation))
                || !self.steppers.early.insert(t)
            {
                continue;
            }
            self.steppers.dirty.retain(|e| e.0 != t);
            if self.dead.contains(&t) {
                continue;
            }
            match kind {
                Some(Kind::Timer) => self.sync_timer(t, self.now, Change::STATE),
                _ => self.sync_frame_anim(t),
            }
        }
    }

    /// `obj.destroy(delay)`: QObjectWrapper::method_destroy deletes the object `delay` ms
    /// later through a single-shot timer.
    pub(crate) fn destroy_later(&mut self, obj: usize, delay: f64) {
        self.steppers.destroy.insert((Due(self.now + delay), obj));
    }

    /// `i` was destroyed.
    pub(crate) fn drop_stepper(&mut self, i: usize) {
        self.unqueue_timer(i);
        // QCoreApplication::removePostedEvents of a deleted object.
        if self.steppers.awaiting.remove(&i) || !self.posted.is_empty() {
            self.posted
                .retain(|e| !matches!(e, Posted::Triggered(t) | Posted::MaybeTick(t) if *t == i));
        }
        self.steppers.early.remove(&i);
        self.steppers.destroy.retain(|&(_, o)| o != i);
        self.frame_anims.remove(&i);
        self.steppers.anim_imgs.remove(&i);
        self.steppers.movies.remove(&i);
        self.anim_img_next.remove(&i);
        if self.steppers.clocks.remove(&i) {
            self.steppers.clock_due = None;
        }
        self.steppers.clock_read.remove(&i);
    }

    /// Whether SystemClock `c`, ticking every `step` seconds, is to keep what it shows at
    /// `now`, in seconds since the epoch: it is still within the step it read last.
    /// Quickshell's SystemClock reads the time only when its timer fires, which it arms for
    /// the end of that step, so a change of the system's zone shows there and not before.
    pub(crate) fn clock_holds(&self, c: usize, step: i64, now: i64) -> bool {
        let read = self.steppers.clock_read.get(&c);
        read.is_some_and(|&(every, from)| every == step && (from..from + step).contains(&now))
    }

    /// SystemClock `c`, ticking every `step` seconds, shows the step that began at `from`.
    pub(crate) fn clock_shows(&mut self, c: usize, step: i64, from: i64) {
        self.steppers.clock_read.insert(c, (step, from));
    }

    /// Registers sources created since the last sync (at `now`) and applies queued changes.
    pub(crate) fn sync_steppers(&mut self, now: f64) {
        let n = self.items.len();
        for i in self.steppers.seen..n {
            if self.stepped_kind(i).is_some() && !self.steppers.early.contains(&i) {
                self.steppers.dirty.push((i, now, Change::STATE));
            }
        }
        self.steppers.seen = n;
        self.steppers.early.clear();
        if self.steppers.dirty.is_empty() {
            return;
        }
        // Every write counts, as each QQmlTimer setter runs `update()` on its own: `repeat =
        // false; repeat = true` restarts the countdown although the value ends up unchanged.
        // Each restart also moves the Timer to the end of the start order, so a source syncs
        // at the place of its last restarting write.
        let mut changes: Vec<(usize, f64, Change)> = Vec::new();
        for (i, at, c) in std::mem::take(&mut self.steppers.dirty) {
            match changes.iter().position(|e| e.0 == i) {
                Some(k) if c.restarts() => {
                    let (_, _, old) = changes.remove(k);
                    changes.push((i, at, Change(old.0 | c.0)));
                }
                Some(k) => changes[k].2 = Change(changes[k].2.0 | c.0),
                None => changes.push((i, at, c)),
            }
        }
        for (i, at, c) in changes {
            if self.dead.contains(&i) {
                self.drop_stepper(i);
                continue;
            }
            match self.stepped_kind(i) {
                Some(Kind::Timer) => self.sync_timer(i, at, c),
                Some(Kind::FrameAnimation) => self.sync_frame_anim(i),
                Some(Kind::AnimatedImage) => self.steppers.movie_dirty.push(i),
                Some(Kind::SystemClock) => {
                    self.steppers.clocks.insert(i);
                    self.steppers.clock_due = None;
                }
                None => {}
            }
        }
    }

    fn prop_on(&mut self, i: usize, name: &str) -> bool {
        self.read(i, name).is_some_and(|v| v.truthy())
    }

    /// QQmlTimer::update for the writes `c` made at `at`: a running Timer whose `running`,
    /// `interval`, `repeat` or `triggeredOnStart` was set counts down again from `at`, and
    /// triggers at once when `triggeredOnStart` is set and it has not ticked since it started.
    fn sync_timer(&mut self, t: usize, at: f64, c: Change) {
        // A Timer reads its own state without a notifier.
        let reading = self.reading.take();
        let running = self.prop_on(t, "running");
        let iv = self
            .read(t, "interval")
            .map_or(1000.0, |v| v.to_number())
            .max(0.0);
        let repeat = self.prop_on(t, "repeat");
        let on_start = self.prop_on(t, "triggeredOnStart");
        self.reading = reading;
        if !running {
            self.unqueue_timer(t);
            return;
        }
        let old = self.steppers.timers.get(&t).copied();
        if old.is_none_or(|r| c.has(Change::RUN) || c.has(Change::RETIME) || r.iv != iv) {
            self.note_started(Started::Timer(t));
        }
        let (start, first) = match old {
            None => (at, true),
            Some(_) if c.has(Change::RUN) => (at, true),
            Some(r) if c.has(Change::RETIME) || r.iv != iv => (at, r.first),
            Some(r) => {
                self.queue_timer(
                    t,
                    TimerRec {
                        repeat,
                        on_start,
                        ..r
                    },
                );
                return;
            }
        };
        let seq = self.steppers.next_seq;
        self.steppers.next_seq += 1;
        let mut r = TimerRec {
            seq,
            start,
            iv,
            repeat,
            on_start,
            first,
            done: false,
            looped: false,
        };
        // A QPauseAnimation of duration 0 that does not loop finishes inside `start()`:
        // QQmlTimerPrivate::animationFinished clears `firstTick` and posts QEvent_Triggered.
        if iv == 0.0 && !repeat {
            r.done = true;
            r.first = false;
            self.posted.push_back(Posted::Triggered(t));
        }
        self.queue_timer(t, r);
        if r.on_start && r.first {
            self.timer_maybe_tick(t);
        }
    }

    /// QQmlTimerPrivate::maybeTick: one QEvent_MaybeTick is posted at a time.
    fn timer_maybe_tick(&mut self, t: usize) {
        if self.steppers.awaiting.insert(t) {
            self.posted.push_back(Posted::MaybeTick(t));
        }
    }

    /// QQmlTimer::event for QEvent_Triggered: a running Timer whose countdown has ended stops
    /// and triggers. One that was stopped, or started counting again, since the event was
    /// posted does nothing.
    pub(crate) fn timer_triggered(&mut self, t: usize) {
        if self.dead.contains(&t) {
            self.unqueue_timer(t);
            return;
        }
        if !self.steppers.timers.get(&t).is_some_and(|r| r.done) {
            return;
        }
        self.steppers.timers.remove(&t);
        self.fire.push(Fire::Handler(t, Sym::new("onTriggered")));
        // runningChanged follows triggered.
        self.write_withheld(t, "running", Value::Bool(false));
        self.fire.push(Fire::Notify(t, Sym::new("running")));
    }

    /// QQmlTimer::event for QEvent_MaybeTick, QQmlTimer::ticked: a running Timer triggers when
    /// a loop has ended, or when it has `triggeredOnStart` and has not ticked since it started.
    pub(crate) fn timer_ticked(&mut self, t: usize) {
        self.steppers.awaiting.remove(&t);
        if self.dead.contains(&t) {
            self.unqueue_timer(t);
            return;
        }
        let Some(r) = self.steppers.timers.get_mut(&t) else {
            return;
        };
        let tick = r.looped || (r.on_start && r.first);
        r.first = false;
        if tick {
            self.fire.push(Fire::Handler(t, Sym::new("onTriggered")));
        }
    }

    fn queue_timer(&mut self, t: usize, r: TimerRec) {
        if let Some(due) = self.steppers.timers.insert(t, r).and_then(|old| old.due()) {
            self.steppers.queue.remove(&(due, t));
        }
        if let Some(due) = r.due() {
            self.steppers.queue.insert((due, t));
        }
    }

    /// The countdown of Timer `t` started `dt` ms later than was written down.
    pub(crate) fn timer_started_later(&mut self, t: usize, dt: f64) {
        if let Some(mut r) = self.steppers.timers.get(&t).copied() {
            r.start += dt;
            self.queue_timer(t, r);
        }
    }

    fn unqueue_timer(&mut self, t: usize) {
        if let Some(due) = self.steppers.timers.remove(&t).and_then(|old| old.due()) {
            self.steppers.queue.remove(&(due, t));
        }
    }

    /// QQuickFrameAnimation::componentComplete.
    fn sync_frame_anim(&mut self, f: usize) {
        self.frame_anim_update(f);
    }

    /// What a FrameAnimation's QElapsedTimer reads, ms.
    pub(crate) fn frame_time(&self) -> f64 {
        self.frame_wall.unwrap_or(self.now)
    }

    /// QQuickFrameAnimationPrivate::updateState after `running` or `paused` of `f` was set: a
    /// FrameAnimation that runs unpaused starts its job, and before its first tick since
    /// `running` was set also its elapsed timer, with `elapsedTime` back at 0 and no
    /// `elapsedTimeChanged`; one that is paused stops its job and keeps its counters.
    fn frame_anim_update(&mut self, f: usize) {
        // It reads its own state without a notifier.
        let reading = self.reading.take();
        let running = self.prop_on(f, "running");
        let paused = self.prop_on(f, "paused");
        self.reading = reading;
        if !running {
            self.frame_anims.remove(&f);
            return;
        }
        let (t, now) = (self.frame_time(), self.now);
        self.steppers.frame_jobs += 1;
        let seq = self.steppers.frame_jobs;
        let c = self.frame_anims.entry(f).or_insert(FrameClock {
            first: true,
            t0: t,
            prev: 0.0,
            job: None,
            seq,
        });
        if paused {
            c.job = None;
            return;
        }
        if c.job.is_some() {
            return;
        }
        let first = c.first;
        if first {
            c.t0 = t;
        }
        c.prev = t - c.t0;
        c.job = Some((now, 0));
        c.seq = seq;
        self.note_started(Started::Frame(f));
        if first && let Some(p) = self.items[f].props.get_mut("elapsedTime") {
            p.value = Value::Num(0.0);
        }
    }

    /// The end of QQuickFrameAnimation::reset: the elapsed timer starts again.
    pub(crate) fn frame_anim_reset(&mut self, f: usize) {
        let t = self.frame_time();
        if let Some(c) = self.frame_anims.get_mut(&f) {
            c.prev = 0.0;
            c.t0 = t;
        }
    }

    /// The FrameAnimations whose job runs: each wants every step of the animation clock.
    pub fn frame_anims_ticking(&self) -> impl Iterator<Item = (usize, f64)> + '_ {
        (self.frame_anims.iter()).filter_map(|(&f, c)| c.job.map(|(start, _)| (f, start)))
    }

    /// The movie of AnimatedImage `a` follows what was written since the last sync: the shell
    /// published a file (`\x01frameSrc`), or QML set `currentFrame`, `playing`, `paused`,
    /// `loops`, `finishBehavior` or `cache`. Those properties show what the movie plays, so a
    /// value QML wrote is a call of its setter: the property goes back to what it was and the
    /// movie's signals set it, one by one (`Fire::Movie`).
    fn sync_anim_img(&mut self, a: usize) {
        let tag = self.read(a, "\x01frameSrc").map(|v| v.to_js_string());
        let Some(tag) = tag.filter(|t| !t.is_empty()) else {
            self.steppers.movies.remove(&a);
            self.steppers.anim_imgs.remove(&a);
            self.anim_img_next.remove(&a);
            return;
        };
        let num =
            |w: &mut World, name: &str, or: f64| w.read(a, name).map_or(or, |v| v.to_number());
        let on = |w: &mut World, name: &str, or: bool| w.read(a, name).map_or(or, |v| v.truthy());
        let playing = on(self, "playing", true);
        let paused = on(self, "paused", false);
        let frame = num(self, "currentFrame", 0.0) as i64;
        let mut loops = num(self, "loops", -1.0) as i64;
        if loops < crate::animated::INFINITE {
            elog::eprintln!(
                "QML AnimatedImage: Loops must be AnimatedImage.Infinite, 0, or a positive integer, got {loops}"
            );
            loops = crate::animated::INFINITE;
            self.write(a, "loops", Value::Num(-1.0));
        }
        let finish_final = num(self, "finishBehavior", 0.0) == 1.0;
        let cache = on(self, "cache", true);
        let old = self.steppers.movies.get(&a);
        let fresh = old.is_none_or(|m| m.tag != tag);
        // A new file starts at the frame set while there was none (presetCurrentFrame).
        let preset = match old {
            Some(m) if m.player.frame() == frame => 0,
            _ => frame,
        };
        if fresh {
            let frames = num(self, "frameCount", 0.0) as i64;
            let file_loops = num(self, "\x01frameLoops", -1.0) as i64;
            let late = on(self, "\x01frameLate", false);
            let player = crate::animated::Player::load(
                frames,
                file_loops,
                late,
                playing,
                paused,
                preset,
                loops,
                finish_final,
                cache,
            );
            let rec = MovieRec {
                tag,
                player,
                acts: Default::default(),
            };
            self.steppers.movies.insert(a, rec);
        } else {
            let m = self.steppers.movies.get_mut(&a).unwrap();
            if !m.acts.is_empty() {
                return;
            }
            m.player.cache = cache;
            m.player.finish_final = finish_final;
            m.player.set_loops(loops);
            let (was_playing, was_paused, was_at) = (
                m.player.is_playing(),
                m.player.is_paused(),
                m.player.frame(),
            );
            if playing != was_playing {
                m.player.set_playing(playing);
                self.unsay(a, "playing", Value::Bool(was_playing));
            }
            let m = self.steppers.movies.get_mut(&a).unwrap();
            if paused != was_paused {
                m.player.set_paused(paused);
                self.unsay(a, "paused", Value::Bool(was_paused));
            }
            let m = self.steppers.movies.get_mut(&a).unwrap();
            if frame != was_at {
                m.player.set_current_frame(frame);
                self.unsay(a, "currentFrame", Value::Num(was_at as f64));
            }
        }
        let m = self.steppers.movies.get_mut(&a).unwrap();
        let acts = std::mem::take(&mut m.player.acts);
        let (timer, rearm) = (m.player.timer, std::mem::take(&mut m.player.rearm));
        if !acts.is_empty() {
            m.acts.extend(acts);
            self.fire.push(Fire::Movie(a));
        }
        if rearm || !timer {
            self.anim_img_next.remove(&a);
        }
        if timer {
            self.steppers.anim_imgs.insert(a);
        } else {
            self.steppers.anim_imgs.remove(&a);
        }
    }

    /// Takes back what QML wrote to a property the movie sets: the value and the change
    /// signal of the write.
    fn unsay(&mut self, a: usize, name: &str, v: Value) {
        self.write(a, name, v);
        let key = (a, Sym::new(name));
        self.changed.retain(|c| c.0 != key);
    }

    /// AnimatedImages whose movie has to look at their properties again.
    pub(crate) fn sync_movies(&mut self) {
        if self.steppers.movie_dirty.is_empty() {
            return;
        }
        let mut dirty = std::mem::take(&mut self.steppers.movie_dirty);
        dirty.sort_unstable();
        dirty.dedup();
        for a in dirty {
            if self.dead.contains(&a) {
                self.drop_stepper(a);
            } else {
                self.sync_anim_img(a);
            }
        }
    }

    /// `Fire::Movie`: the next signal of `a`'s movie sets its property; `None` when there is
    /// none left. Handlers run between two signals, as with Qt's direct connections.
    pub(crate) fn movie_act(&mut self, a: usize) -> Option<crate::animated::Act> {
        use crate::animated::Act;
        if self.dead.contains(&a) {
            return None;
        }
        let m = self.steppers.movies.get_mut(&a)?;
        let Some(act) = m.acts.pop_front() else {
            // What a handler wrote meanwhile is looked at now.
            self.steppers.movie_dirty.push(a);
            return None;
        };
        let (name, v) = match act {
            Act::Frame(k) => ("currentFrame", Value::Num(k as f64)),
            Act::Playing(b) => ("playing", Value::Bool(b)),
            Act::Paused(b) => ("paused", Value::Bool(b)),
            Act::Count(n) => {
                self.unsay(a, "frameCount", Value::Num(n as f64));
                return Some(act);
            }
            Act::Finished => {
                self.signals
                    .entry(a)
                    .or_default()
                    .entry("finished".to_string())
                    .or_default();
                return Some(act);
            }
        };
        // The signal comes also when the value stays (a movie loaded at frame 0 emits
        // frameChanged(0)), and when the write stored a default without one.
        let key = (a, Sym::new(name));
        let before = self.changed.len();
        self.write(a, name, v);
        if !self.changed[before.min(self.changed.len())..]
            .iter()
            .any(|c| c.0 == key)
        {
            self.notify(a, name);
        }
        Some(act)
    }

    /// The movie of `a` shows its next frame: its timer fired. `false` without a movie.
    pub(crate) fn movie_timeout(&mut self, a: usize) -> bool {
        let Some(m) = self.steppers.movies.get_mut(&a) else {
            return false;
        };
        if !m.player.timer {
            return false;
        }
        m.player.timeout();
        m.player.rearm = false;
        let acts = std::mem::take(&mut m.player.acts);
        if !acts.is_empty() {
            m.acts.extend(acts);
            self.fire.push(Fire::Movie(a));
        }
        if !m.player.timer {
            self.steppers.anim_imgs.remove(&a);
        }
        true
    }

    /// The frame the movie of `a` is at and whether its timer runs.
    pub(crate) fn movie_at(&self, a: usize) -> Option<(i64, bool)> {
        let m = self.steppers.movies.get(&a)?;
        Some((m.player.frame(), m.player.timer))
    }

    /// Fires the Timers due at `now` in start order: QQmlTimer is a QPauseAnimation, and
    /// QUnifiedTimer advances the running animations of one tick in the order they started.
    pub(crate) fn step_timers(&mut self, now: f64) {
        while let Some(&(d, o)) = self.steppers.destroy.first()
            && d.0 <= now
        {
            self.steppers.destroy.pop_first();
            if !self.dead.contains(&o) && !self.doomed.contains(&o) {
                self.doomed.push(o);
            }
        }
        let mut due = Vec::new();
        while let Some(&(d, t)) = self.steppers.queue.first()
            && d.0 <= now
        {
            self.steppers.queue.pop_first();
            if let Some(r) = self.steppers.timers.get(&t).copied() {
                due.push((t, r));
            }
        }
        due.sort_unstable_by_key(|(_, r)| r.seq);
        for (t, r) in due {
            if self.dead.contains(&t) {
                self.steppers.timers.remove(&t);
                continue;
            }
            if r.repeat {
                // QQmlTimerPrivate::animationCurrentLoopChanged.
                let mut start = r.start + ((now - r.start) / r.iv).floor() * r.iv;
                // The loop that just ended is over even when the division rounds below it
                // (start 50.1, now 100.1: `0.9999999999999999` loops): the next one ends
                // after `now`, or the Timer would trigger again in every turn at this time.
                if start + r.iv <= now {
                    start += r.iv;
                }
                self.queue_timer(
                    t,
                    TimerRec {
                        start,
                        looped: true,
                        ..r
                    },
                );
                self.timer_maybe_tick(t);
            } else {
                // QQmlTimerPrivate::animationFinished.
                self.queue_timer(
                    t,
                    TimerRec {
                        first: false,
                        done: true,
                        ..r
                    },
                );
                self.posted.push_back(Posted::Triggered(t));
            }
        }
    }

    /// When the Timer due first is due, on the animation clock (`INFINITY`: none is running).
    pub fn timer_due(&self) -> f64 {
        (self.steppers.queue.first()).map_or(f64::INFINITY, |(due, _)| due.0)
    }

    /// AnimatedImages to step this tick.
    pub(crate) fn playing_images(&self) -> Vec<usize> {
        self.steppers.anim_imgs.iter().copied().collect()
    }

    /// Milliseconds from `now` until a Timer, AnimatedImage frame or SystemClock boundary is
    /// due (`INFINITY`: none), and whether syncing queued a handler that must run first.
    pub(crate) fn steppers_deadline(&mut self, now: f64) -> (f64, bool) {
        self.sync_steppers(now);
        if !self.posted.is_empty() {
            return (0.0, true);
        }
        let mut d = self
            .steppers
            .queue
            .first()
            .map_or(f64::INFINITY, |(due, _)| due.0 - now);
        if let Some((due, _)) = self.steppers.destroy.first() {
            d = d.min(due.0 - now);
        }
        for &at in self.anim_img_next.values() {
            d = d.min(at - now);
        }
        (d.min(self.clock_deadline(now)), false)
    }

    fn clock_deadline(&mut self, now: f64) -> f64 {
        if self.steppers.clocks.is_empty() {
            return f64::INFINITY;
        }
        if let Some(due) = self.steppers.clock_due
            && now < due
        {
            return due - now;
        }
        let clocks: Vec<usize> = self.steppers.clocks.iter().copied().collect();
        let finest = clocks
            .into_iter()
            .map(|c| self.read(c, "precision").map_or(3.0, |v| v.to_number()) as i64)
            .max()
            .unwrap_or(3);
        let step: i64 = match finest {
            1 => 3_600_000,
            2 => 60_000,
            _ => 1000,
        };
        // The clock `clock_tick` reads: pinned (virtual time) or the wall clock.
        let t = crate::builtins::now_ms().floor() as i64;
        let local = t + i64::from(crate::tz::local_ahead(t.div_euclid(1000))) * 1000;
        let ms = (step - local.rem_euclid(step)) as f64 + 1.0;
        self.steppers.clock_due = Some(now + ms);
        ms
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::Runtime;
    use crate::value::Value;

    fn timers_qml(n: usize) -> String {
        let mut s =
            String::from("import QtQuick\nItem { property int other: 0; property int fired: 0\n");
        for k in 0..n {
            s.push_str(&format!(
                " Timer {{ interval: {}; running: true; repeat: true; onTriggered: fired++ }}\n",
                1000 + k
            ));
        }
        s.push('}');
        s
    }

    #[test]
    fn unrelated_writes_leave_the_timer_queue_alone() {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(&timers_qml(50)).unwrap();
        rt.tick(0.0);
        assert_eq!(rt.world.borrow().steppers.timers(), 50);
        assert!(rt.world.borrow().steppers.dirty.is_empty());
        for k in 0..100 {
            rt.set(root, "other", Value::Num(k as f64));
            rt.tick(k as f64);
        }
        assert!(
            rt.world.borrow().steppers.dirty.is_empty(),
            "writes to other properties queue no timer"
        );
        assert_eq!(
            rt.next_deadline(500.0),
            500.0,
            "the queue head bounds the wait"
        );
        rt.tick(1000.5);
        assert_eq!(
            rt.get(root, "fired").to_number(),
            1.0,
            "only the due timer fired"
        );
        assert_eq!(rt.next_deadline(1000.5), 0.5);
    }

    #[test]
    fn a_running_timer_has_its_deadline_before_the_first_tick() {
        let mut rt = Runtime::new(None);
        rt.load_str("import QtQuick\nTimer { interval: 250; running: true }")
            .unwrap();
        assert_eq!(
            rt.next_deadline(0.0),
            250.0,
            "registered at once, not woken immediately to be noticed"
        );
    }

    #[test]
    fn a_timer_started_by_a_handler_counts_from_the_write() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str("import QtQuick\nItem { property int n: 0\n function go() { t.running = true }\n Timer { id: t; interval: 100; onTriggered: n++ } }")
            .unwrap();
        rt.tick(0.0);
        assert_eq!(rt.next_deadline(0.0), f64::INFINITY);
        // The shell stamps the clock before it dispatches an event.
        rt.world.borrow_mut().now = 5000.0;
        rt.call_method(root, "go", vec![]).unwrap().unwrap();
        assert_eq!(rt.next_deadline(5010.0), 90.0);
        rt.tick(5099.0);
        assert_eq!(rt.get(root, "n").to_number(), 0.0);
        rt.tick(5100.0);
        assert_eq!(rt.get(root, "n").to_number(), 1.0);
        assert!(!rt.get(rt.id("t").unwrap(), "running").truthy());
        assert_eq!(rt.world.borrow().steppers.timers(), 0);
    }

    #[test]
    fn a_finished_timer_triggers_before_running_changed() {
        // Qt 6.12: QQmlTimer::finished clears `running`, emits triggered, then runningChanged;
        // bindings on `running` re-run with runningChanged.
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str("import QtQuick\nItem { property string out: \"\"\n property bool dep: t.running\n onDepChanged: out += \"dep:\" + dep + \";\"\n Timer { id: t; interval: 10; running: true\n onTriggered: out += \"trig:\" + running + \"/dep:\" + dep + \";\"\n onRunningChanged: out += \"rc:\" + running + \";\" } }")
            .unwrap();
        rt.tick(0.0);
        rt.set(root, "out", Value::str(""));
        rt.settle();
        rt.tick(10.0);
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        assert_eq!(
            rt.get(root, "out").to_js_string(),
            "trig:false/dep:true;rc:false;dep:false;"
        );
    }

    #[test]
    fn triggered_on_start_runs_on_the_next_turn() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str("import QtQuick\nItem { property int n: 0\n Timer { interval: 100; running: true; triggeredOnStart: true; onTriggered: n++ } }")
            .unwrap();
        assert_eq!(rt.next_deadline(0.0), 0.0, "the start trigger is queued");
        rt.tick(0.0);
        assert_eq!(rt.get(root, "n").to_number(), 1.0);
        assert_eq!(rt.next_deadline(0.0), 100.0);
    }

    #[test]
    fn destroyed_and_stopped_timers_leave_the_queue() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str("import QtQuick\nItem { property bool on: true\n Loader { active: on; sourceComponent: Timer { interval: 100; running: true } }\n Timer { id: t; interval: 300; running: on } }")
            .unwrap();
        rt.tick(0.0);
        assert_eq!(rt.world.borrow().steppers.timers(), 2);
        rt.set(root, "on", Value::Bool(false));
        rt.tick(10.0);
        assert_eq!(rt.world.borrow().steppers.timers(), 0);
        assert_eq!(rt.next_deadline(10.0), f64::INFINITY);
    }

    #[test]
    fn only_running_frame_animations_are_stepped() {
        let mut rt = Runtime::new(None);
        let mut q = String::from("import QtQuick\nItem { property bool on: false\n");
        for _ in 0..20 {
            q.push_str(" FrameAnimation {}\n");
        }
        q.push_str(" FrameAnimation { id: fa; running: on }\n}");
        let root = rt.load_str(&q).unwrap();
        assert!(!rt.tick(0.0), "stopped FrameAnimations ask for no frames");
        assert!(rt.world.borrow().frame_anims.is_empty());
        rt.set(root, "on", Value::Bool(true));
        assert!(rt.tick(16.0));
        let fa = rt.id("fa").unwrap();
        assert_eq!(
            rt.world
                .borrow()
                .frame_anims
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [fa]
        );
        rt.set(root, "on", Value::Bool(false));
        assert!(!rt.tick(32.0));
        assert!(rt.world.borrow().frame_anims.is_empty());
    }

    fn timer_root(timer: &str) -> (Runtime, usize) {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(&format!(
                "import QtQuick\nItem {{ property int n: 0\n function flip() {{ t.repeat = false; t.repeat = true }}\n function onStart() {{ t.triggeredOnStart = true }}\n function rerun() {{ t.running = false; t.running = true }}\n function every50() {{ t.interval = 50 }}\n function zero() {{ t.interval = 0 }}\n Timer {{ id: t; {timer}; onTriggered: n++ }} }}"
            ))
            .unwrap();
        rt.tick(0.0);
        (rt, root)
    }

    fn call_at(rt: &mut Runtime, root: usize, now: f64, f: &str) {
        rt.world.borrow_mut().now = now;
        rt.call_method(root, f, vec![]).unwrap().unwrap();
        rt.tick(now);
    }

    /// QQmlTimer::setRepeating runs `update()`: a running timer counts down again from the
    /// write, for each write (PySide6 6.12: `repeat = false; repeat = true` at 200 ms on a
    /// 300 ms repeating timer triggers at 500, 800, ...).
    #[test]
    fn repeat_write_restarts_the_countdown() {
        let (mut rt, root) = timer_root("interval: 300; running: true; repeat: true");
        call_at(&mut rt, root, 200.0, "flip");
        rt.tick(300.0);
        assert_eq!(rt.get(root, "n").to_number(), 0.0);
        assert_eq!(rt.next_deadline(300.0), 200.0);
        rt.tick(500.0);
        assert_eq!(rt.get(root, "n").to_number(), 1.0);
    }

    /// QQmlTimer::setTriggeredOnStart runs `update()`, which triggers at once while the timer
    /// has not ticked since it started (`firstTick`) and restarts the countdown either way
    /// (PySide6 6.12: set at 200 ms -> 200, 500, 800; set at 400 ms -> 300, 700, 1000).
    #[test]
    fn triggered_on_start_write_triggers_before_the_first_tick_and_restarts() {
        let (mut rt, root) = timer_root("interval: 300; running: true; repeat: true");
        call_at(&mut rt, root, 200.0, "onStart");
        assert_eq!(rt.get(root, "n").to_number(), 1.0, "no tick yet: triggers");
        rt.tick(300.0);
        assert_eq!(rt.get(root, "n").to_number(), 1.0);
        rt.tick(500.0);
        assert_eq!(rt.get(root, "n").to_number(), 2.0);

        let (mut rt, root) = timer_root("interval: 300; running: true; repeat: true");
        rt.tick(300.0);
        assert_eq!(rt.get(root, "n").to_number(), 1.0);
        call_at(&mut rt, root, 400.0, "onStart");
        assert_eq!(
            rt.get(root, "n").to_number(),
            1.0,
            "ticked already: no trigger"
        );
        rt.tick(600.0);
        assert_eq!(rt.get(root, "n").to_number(), 1.0);
        rt.tick(700.0);
        assert_eq!(rt.get(root, "n").to_number(), 2.0);
    }

    /// `running = false; running = true` in one handler is setRunning twice: a new countdown
    /// with `firstTick` set again, so triggeredOnStart triggers (as `stop(); start()`).
    #[test]
    fn stop_and_start_by_writes_restarts_with_a_first_tick() {
        let (mut rt, root) = timer_root("interval: 300; running: true; triggeredOnStart: true");
        assert_eq!(rt.get(root, "n").to_number(), 1.0);
        call_at(&mut rt, root, 200.0, "rerun");
        assert_eq!(rt.get(root, "n").to_number(), 2.0);
        rt.tick(300.0);
        assert_eq!(rt.get(root, "n").to_number(), 2.0);
        rt.tick(500.0);
        assert_eq!(rt.get(root, "n").to_number(), 3.0);
    }

    /// A repeating QQmlTimer is a looping QPauseAnimation: triggers stay on the grid of its
    /// start however late a tick runs, and loops skipped by a stall trigger once (PySide6 6.12,
    /// 100 ms timer whose 4th handler busy-waits 40 ms and 6th 350 ms: 100, ..., 400, 500, 600,
    /// 950, 1000, 1100).
    #[test]
    fn repeating_timer_keeps_its_grid_after_late_ticks() {
        let (mut rt, root) = timer_root("interval: 100; running: true; repeat: true");
        rt.tick(140.0);
        assert_eq!(rt.get(root, "n").to_number(), 1.0);
        assert_eq!(
            rt.next_deadline(140.0),
            60.0,
            "next loop ends at 200, not 240"
        );
        rt.tick(200.0);
        assert_eq!(rt.get(root, "n").to_number(), 2.0);
        rt.tick(560.0);
        assert_eq!(rt.get(root, "n").to_number(), 3.0, "a stall triggers once");
        assert_eq!(rt.next_deadline(560.0), 40.0);
    }

    /// A repeating QQmlTimer with interval 0 is a QPauseAnimation of duration 0 with infinite
    /// loops, which never completes a loop: it stays running but never triggers and must not
    /// wake the loop (PySide6 6.12: 0 triggers in 300 ms, `running` stays true).
    #[test]
    fn repeating_zero_interval_timer_never_triggers() {
        let (mut rt, root) = timer_root("interval: 0; running: true; repeat: true");
        for k in 1..=5 {
            let now = k as f64 * 16.0;
            rt.tick(now);
            assert_eq!(rt.next_deadline(now), f64::INFINITY);
        }
        assert_eq!(rt.get(root, "n").to_number(), 0.0);
        assert_eq!(rt.world.borrow().steppers.timers(), 1, "it keeps running");
    }

    /// Only `triggeredOnStart` triggers a repeating interval-0 Timer, once (PySide6 6.12).
    #[test]
    fn repeating_zero_interval_timer_triggers_only_on_start() {
        let (mut rt, root) =
            timer_root("interval: 0; running: true; repeat: true; triggeredOnStart: true");
        assert_eq!(rt.get(root, "n").to_number(), 1.0);
        for k in 1..=5 {
            let now = k as f64 * 16.0;
            rt.tick(now);
            assert_eq!(rt.next_deadline(now), f64::INFINITY);
        }
        assert_eq!(rt.get(root, "n").to_number(), 1.0);
    }

    /// QQmlTimer::setInterval restarts a running Timer into or out of interval 0 (PySide6 6.12:
    /// `interval = 50` at 100 ms triggers at 150, 200, 250; a 50 ms timer set to 0 at 100 ms
    /// triggers no more).
    #[test]
    fn interval_writes_restart_a_repeating_timer_into_and_out_of_zero() {
        let (mut rt, root) = timer_root("interval: 0; running: true; repeat: true");
        call_at(&mut rt, root, 100.0, "every50");
        assert_eq!(rt.next_deadline(100.0), 50.0);
        rt.tick(150.0);
        rt.tick(200.0);
        assert_eq!(rt.get(root, "n").to_number(), 2.0);
        call_at(&mut rt, root, 220.0, "zero");
        assert_eq!(rt.next_deadline(220.0), f64::INFINITY);
        rt.tick(400.0);
        assert_eq!(rt.get(root, "n").to_number(), 2.0);
        assert_eq!(rt.world.borrow().steppers.timers(), 1);
    }

    /// A one-shot interval-0 Timer triggers once and stops (PySide6 6.12).
    #[test]
    fn one_shot_zero_interval_timer_triggers_once() {
        let (mut rt, root) = timer_root("interval: 0; running: true");
        rt.tick(16.0);
        rt.tick(32.0);
        assert_eq!(rt.get(root, "n").to_number(), 1.0);
        assert_eq!(rt.next_deadline(32.0), f64::INFINITY);
        assert_eq!(rt.world.borrow().steppers.timers(), 0);
    }

    fn ordered_timers(a: &str, b: &str, started: &str) -> (Runtime, usize) {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(&format!(
                "import QtQuick\nItem {{ property string log: \"\"\n function again() {{ a.restart() }}\n Timer {{ id: a; {a}; onTriggered: log += \"A;\" }}\n Timer {{ id: b; {b}; onTriggered: log += \"B;\" }}\n Component.onCompleted: {{ {started} }} }}"
            ))
            .unwrap();
        rt.tick(0.0);
        (rt, root)
    }

    /// A repeating Timer that triggered has ended a loop, whatever the division by its interval
    /// rounds to: at 100.1 a 50 ms loop started at 50.1 is `0.9999999999999999` loops old. The
    /// animation clock stands still between two steps, and the Timer is stepped at that time
    /// again in every loop turn until the next one: it must not trigger again.
    #[test]
    fn a_repeating_timer_due_exactly_now_triggers_once() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nItem { id: root; property int n: 0\n \
                 function go() { t.start() }\n \
                 Timer { id: t; interval: 50; repeat: true; onTriggered: root.n++ } }",
            )
            .unwrap();
        rt.tick(0.0);
        rt.world.borrow_mut().now = 0.1;
        rt.call_method(root, "go", vec![]).unwrap().unwrap();
        rt.tick(0.1);
        rt.tick(50.1);
        assert_eq!(rt.get(root, "n").to_number(), 1.0, "the first loop");
        for _ in 0..3 {
            rt.tick(100.1);
        }
        assert_eq!(rt.get(root, "n").to_number(), 2.0, "the second loop");
        rt.tick(150.1);
        assert_eq!(rt.get(root, "n").to_number(), 3.0, "the third loop");
    }

    /// QQmlTimer is a QPauseAnimation, and QUnifiedTimer advances the animations running in one
    /// tick in the order they started, not by due time (PySide6 6.12, 100 ms `a` and 90 ms `b`
    /// overdue after a 150 ms stall: `a.start(); b.start()` -> A;B;, `b.start(); a.start()` ->
    /// B;A;). A repeating Timer keeps its place across loops; a restart stops and starts its
    /// animation, which moves it to the end.
    #[test]
    fn timers_due_in_one_turn_trigger_in_start_order() {
        let (mut rt, root) =
            ordered_timers("interval: 100", "interval: 90", "a.start(); b.start()");
        rt.tick(160.0);
        assert_eq!(rt.get(root, "log").to_js_string(), "A;B;");

        let (mut rt, root) =
            ordered_timers("interval: 100", "interval: 90", "b.start(); a.start()");
        rt.tick(160.0);
        assert_eq!(rt.get(root, "log").to_js_string(), "B;A;");

        let (mut rt, root) = ordered_timers(
            "interval: 100; repeat: true",
            "interval: 90; repeat: true",
            "a.start(); b.start()",
        );
        rt.tick(160.0);
        rt.tick(400.0);
        assert_eq!(
            rt.get(root, "log").to_js_string(),
            "A;B;A;B;",
            "loops keep their place"
        );

        let (mut rt, root) = ordered_timers("interval: 50", "interval: 90", "a.start(); b.start()");
        rt.world.borrow_mut().now = 10.0;
        rt.call_method(root, "again", vec![]).unwrap().unwrap();
        rt.tick(10.0);
        rt.tick(160.0);
        assert_eq!(
            rt.get(root, "log").to_js_string(),
            "B;A;",
            "a restart starts last"
        );
    }
}
