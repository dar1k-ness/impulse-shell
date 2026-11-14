//! What an `AnimatedImage` plays, without its pixels: QMoviePrivate (qmovie.cpp) and
//! QQuickAnimatedImage (qquickanimatedimage.cpp) of Qt 6.12 as one state machine over frame
//! numbers. The shell reads the file and shows the frame `currentFrame` names.

/// QMovie::MovieState.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    NotRunning,
    Paused,
    Running,
}

/// A signal of QQuickAnimatedImage, with the value its property has when it is emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    /// currentFrameChanged.
    Frame(i64),
    /// playingChanged.
    Playing(bool),
    /// pausedChanged.
    Paused(bool),
    /// finished.
    Finished,
    /// `frameCount` reads another number, with no signal.
    Count(i64),
}

/// `AnimatedImage.Infinite`.
pub(crate) const INFINITE: i64 = -1;

/// What QMoviePrivate::infoForFrame gives for a frame number.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Info {
    Frame,
    End,
    Invalid,
}

#[derive(Clone, Debug)]
pub(crate) struct Player {
    /// Frames in the file and the rounds it asks for after the first (QImageReader::loopCount:
    /// -1 for ever).
    frames: i64,
    file_loops: i64,
    /// The file is a picture of a format that is no animation: QImageReader::imageCount()
    /// says 1 while it can read and 0 after, so QMovie::frameCount() is 0 from the first
    /// frame read until QMovie has looked for a second one.
    late: bool,
    count: i64,
    // QMoviePrivate.
    state: State,
    cur: i64,
    next: i64,
    read_all: bool,
    first: bool,
    counter: i64,
    pub(crate) cache: bool,
    /// nextImageTimer runs; `rearm`: it was started since the flag was last cleared.
    pub(crate) timer: bool,
    pub(crate) rearm: bool,
    // QQuickAnimatedImagePrivate.
    playing: bool,
    paused: bool,
    loops: i64,
    pub(crate) finish_final: bool,
    current_loop: i64,
    last: i64,
    /// handleLoopCompletion is running.
    completing: bool,
    pub(crate) acts: Vec<Act>,
}

impl Player {
    /// QQuickAnimatedImage::movieRequestFinished for a file of `frames` frames, on an item
    /// whose `playing`, `paused`, `currentFrame`, `loops`, `finishBehavior` and `cache` were
    /// set before.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn load(
        frames: i64,
        file_loops: i64,
        late: bool,
        playing: bool,
        paused: bool,
        preset: i64,
        loops: i64,
        finish_final: bool,
        cache: bool,
    ) -> Player {
        let mut p = Player {
            frames,
            file_loops,
            late,
            count: frames,
            state: State::NotRunning,
            cur: -1,
            next: 0,
            read_all: false,
            first: true,
            counter: -1,
            cache,
            timer: false,
            rearm: false,
            playing,
            paused,
            loops,
            finish_final,
            current_loop: 0,
            last: 0,
            completing: false,
            acts: Vec::new(),
        };
        let paused_at_start = p.paused;
        if p.playing && p.loops != 0 {
            p.start();
        }
        if paused_at_start {
            p.movie_set_paused(true);
        }
        if p.paused || !p.playing || p.loops == 0 {
            p.jump(preset);
        }
        if p.is_playing() != playing {
            p.acts.push(Act::Playing(p.is_playing()));
        }
        p
    }

    /// QQuickAnimatedImage::currentFrame.
    pub(crate) fn frame(&self) -> i64 {
        self.cur
    }

    /// QMoviePrivate::frameCount, said when it changed.
    fn recount(&mut self) {
        let count = match self.late && self.cur >= 0 && !self.read_all {
            true => 0,
            false => self.frames,
        };
        if count != self.count {
            self.count = count;
            self.acts.push(Act::Count(count));
        }
    }

    /// QQuickAnimatedImage::isPlaying.
    pub(crate) fn is_playing(&self) -> bool {
        self.state != State::NotRunning
    }

    /// QQuickAnimatedImage::isPaused.
    pub(crate) fn is_paused(&self) -> bool {
        self.paused
    }

    /// QMoviePrivate::infoForFrame. With `cache` (CacheAll) QMovie has every frame up to the
    /// greatest one read and reads on from there, so any frame of the file is to be had.
    /// Without (CacheNone) only the frame after the current one and the first one are:
    /// neither QGifHandler nor QWebpHandler can jump.
    fn info(&mut self, k: i64) -> Info {
        if k < 0 {
            return Info::Invalid;
        }
        let n = self.frames;
        if k >= n && (self.cache || self.read_all) {
            self.read_all = true;
            return if k == n { Info::End } else { Info::Invalid };
        }
        if !self.cache && k != self.cur + 1 && k != 0 {
            return Info::Invalid;
        }
        if k >= n {
            self.read_all = true;
            return Info::End;
        }
        Info::Frame
    }

    /// QMoviePrivate::next: past the last frame the movie starts another round at its first
    /// while the file has rounds left.
    fn next_frame(&mut self) -> bool {
        match self.info(self.next) {
            Info::Invalid => return false,
            Info::Frame => {}
            Info::End => {
                if self.first {
                    if self.next == 0 {
                        return false;
                    }
                    self.counter = self.file_loops;
                    self.first = false;
                }
                if self.counter == 0 {
                    return false;
                }
                if self.counter != -1 {
                    self.counter -= 1;
                }
                self.next = 0;
                if self.info(0) != Info::Frame {
                    return false;
                }
            }
        }
        self.cur = self.next;
        self.next += 1;
        true
    }

    /// QMoviePrivate::_q_loadNextFrame.
    fn load_next(&mut self, starting: bool) {
        let read = self.next_frame();
        self.recount();
        if read {
            if starting && self.state == State::NotRunning {
                self.enter(State::Running);
            }
            self.movie_update();
            if self.state == State::Running {
                self.timer = true;
                self.rearm = true;
            }
        } else if self.state != State::Paused {
            self.next = 0;
            self.first = true;
            self.counter = -1;
            self.enter(State::NotRunning);
            self.on_finished();
        }
    }

    /// QMoviePrivate::enterState and QQuickAnimatedImage::playingStatusChanged, which
    /// answers it.
    fn enter(&mut self, state: State) {
        self.state = state;
        let playing = state != State::NotRunning;
        if playing != self.playing {
            self.playing = playing;
            self.acts.push(Act::Playing(playing));
        }
        let paused = state == State::Paused;
        if paused != self.paused {
            self.paused = paused;
            self.acts.push(Act::Paused(paused));
        }
    }

    /// QQuickAnimatedImage::movieUpdate: a frame before the last one shown ends a round.
    fn movie_update(&mut self) {
        let mut cur = self.cur;
        if cur < self.last && self.last > 0 {
            self.current_loop += 1;
            if self.loops != INFINITE && self.current_loop >= self.loops {
                self.handle_loop_completion();
                cur = self.cur;
            }
        }
        self.last = cur;
        self.acts.push(Act::Frame(cur));
    }

    /// QQuickAnimatedImage::onMovieFinished: the file's rounds are over.
    fn on_finished(&mut self) {
        if self.frames <= 1 {
            return;
        }
        self.current_loop += 1;
        if self.loops == INFINITE || self.current_loop < self.loops {
            self.last = 0;
            self.start();
        } else {
            self.handle_loop_completion();
        }
    }

    /// QQuickAnimatedImagePrivate::handleLoopCompletion. Qt 6.12 jumps to the frame to stop
    /// at through QMovie, which without `cache` cannot reach it from most frames, reports
    /// that it finished and lands here again, without end; the frame is shown here.
    fn handle_loop_completion(&mut self) {
        if self.completing {
            return;
        }
        self.completing = true;
        let target = if self.finish_final {
            (self.frames - 1).max(0)
        } else {
            0
        };
        self.last = target;
        self.stop();
        let cache = std::mem::replace(&mut self.cache, true);
        self.jump(target);
        self.cache = cache;
        self.acts.push(Act::Finished);
        self.completing = false;
    }

    /// QMoviePrivate::jumpToFrame.
    fn jump(&mut self, k: i64) {
        if k < 0 || self.cur == k {
            return;
        }
        self.next = k;
        if self.state == State::Running {
            self.timer = false;
        }
        self.load_next(false);
    }

    /// QMovie::start.
    fn start(&mut self) {
        match self.state {
            State::NotRunning => self.load_next(true),
            State::Paused => self.movie_set_paused(false),
            State::Running => {}
        }
    }

    /// QMovie::stop.
    fn stop(&mut self) {
        if self.state == State::NotRunning {
            return;
        }
        self.enter(State::NotRunning);
        self.timer = false;
        self.next = 0;
    }

    /// QMovie::setPaused.
    fn movie_set_paused(&mut self, paused: bool) {
        if paused {
            if self.state == State::NotRunning {
                return;
            }
            self.enter(State::Paused);
            self.timer = false;
        } else {
            if self.state == State::Running {
                return;
            }
            self.enter(State::Running);
            self.timer = true;
            self.rearm = true;
        }
    }

    /// nextImageTimer fired.
    pub(crate) fn timeout(&mut self) {
        self.timer = false;
        self.load_next(false);
    }

    /// QQuickAnimatedImage::setCurrentFrame.
    pub(crate) fn set_current_frame(&mut self, k: i64) {
        self.last = k;
        self.jump(k);
    }

    /// QQuickAnimatedImage::setPlaying.
    pub(crate) fn set_playing(&mut self, play: bool) {
        if play == self.playing {
            return;
        }
        if play {
            if self.loops == 0 {
                return;
            }
            self.current_loop = 0;
            self.last = 0;
            self.jump(0);
            self.start();
        } else {
            self.stop();
        }
    }

    /// QQuickAnimatedImage::setPaused.
    pub(crate) fn set_paused(&mut self, paused: bool) {
        if paused != self.paused {
            self.movie_set_paused(paused);
        }
    }

    /// QQuickAnimatedImage::setLoops, after its check of the value.
    pub(crate) fn set_loops(&mut self, loops: i64) {
        if self.loops == loops {
            return;
        }
        self.loops = loops;
        if loops == 0 && self.state != State::NotRunning {
            self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(p: &mut Player) -> String {
        let acts: Vec<String> = std::mem::take(&mut p.acts)
            .into_iter()
            .map(|a| match a {
                Act::Frame(k) => format!("f{k}"),
                Act::Playing(b) => format!("p{}", u8::from(b)),
                Act::Paused(b) => format!("P{}", u8::from(b)),
                Act::Finished => "F".to_string(),
                Act::Count(n) => format!("n{n}"),
            })
            .collect();
        acts.join(",")
    }

    /// Plays three frames until the movie stops or `events` signals came.
    fn play(file_loops: i64, loops: i64, finish_final: bool, cache: bool, events: usize) -> String {
        let mut p = Player::load(
            3,
            file_loops,
            false,
            true,
            false,
            0,
            loops,
            finish_final,
            cache,
        );
        let mut out = vec![log(&mut p)];
        while p.timer && out.join(",").split(',').count() < events {
            p.timeout();
            out.push(log(&mut p));
        }
        out.retain(|s| !s.is_empty());
        out.join(",")
    }

    // The signals Qt 6.12 emits (crates/shell/tests/animated_image_qt.rs compares them with
    // Qt itself): `loopCount` 0 is a GIF with no NETSCAPE block or a WebP with loop count 1.
    #[test]
    fn rounds_of_the_file_and_of_loops() {
        assert_eq!(play(-1, -1, false, true, 7), "f0,f1,f2,f0,f1,f2,f0");
        assert_eq!(
            play(0, -1, false, true, 10),
            "f0,f1,f2,p0,p1,f0,f1,f2,p0,p1,f0"
        );
        assert_eq!(play(1, -1, false, true, 9), "f0,f1,f2,f0,f1,f2,p0,p1,f0");
        assert_eq!(play(-1, 1, false, true, 99), "f0,f1,f2,p0,F,f0");
        assert_eq!(play(0, 1, false, true, 99), "f0,f1,f2,p0,f0,F");
        assert_eq!(play(3, 2, false, false, 99), "f0,f1,f2,f0,f1,f2,p0,F,f0");
        assert_eq!(play(1, 2, false, true, 99), "f0,f1,f2,f0,f1,f2,p0,f0,F");
        assert_eq!(
            play(0, 2, false, true, 99),
            "f0,f1,f2,p0,p1,f0,f1,f2,p0,f0,F"
        );
        assert_eq!(play(-1, 1, true, true, 99), "f0,f1,f2,p0,f2,F,f2");
        assert_eq!(play(0, 1, true, true, 99), "f0,f1,f2,p0,F");
        assert_eq!(play(1, 2, true, true, 99), "f0,f1,f2,f0,f1,f2,p0,F");
        assert_eq!(play(0, 0, false, true, 99), "f0,p0");
    }

    #[test]
    fn frames_set_by_hand_after_the_movie_stopped() {
        let step = |p: &mut Player, ks: &[i64]| {
            let at: Vec<String> = ks
                .iter()
                .map(|&k| {
                    p.set_current_frame(k);
                    format!("{}:{}", p.frame(), u8::from(p.is_playing()))
                })
                .collect();
            format!("{} {}", at.join(" "), log(p))
        };
        let ks = [1, 2, 0, 5, 2, 1];
        let mut p = Player::load(3, -1, false, true, false, 0, 2, false, true);
        while p.timer {
            p.timeout();
        }
        p.acts.clear();
        assert_eq!(
            step(&mut p, &ks),
            "1:0 2:0 0:0 0:0 2:0 1:0 f1,f2,f0,F,f2,f1"
        );
        p.set_playing(true);
        assert_eq!(log(&mut p), "f0,p1,f1");

        let mut p = Player::load(3, -1, false, true, false, 0, 2, false, false);
        while p.timer {
            p.timeout();
        }
        p.acts.clear();
        assert_eq!(step(&mut p, &ks), "1:0 2:0 0:0 0:0 0:0 1:0 f1,f2,f0,F,F,f1");

        let mut p = Player::load(3, -1, false, true, false, 0, 2, true, true);
        while p.timer {
            p.timeout();
        }
        p.acts.clear();
        assert_eq!(
            step(&mut p, &ks),
            "1:0 2:0 0:0 2:0 2:0 1:0 f1,f2,f0,f2,F,f1"
        );

        let mut p = Player::load(3, -1, false, true, false, 0, 0, false, true);
        p.acts.clear();
        assert_eq!(
            step(&mut p, &ks),
            "1:0 2:0 0:0 0:0 2:0 1:0 f1,f2,f0,p0,F,f2,f1"
        );
        p.set_playing(true);
        assert_eq!(log(&mut p), "");

        let mut p = Player::load(3, 0, false, true, false, 0, -1, false, true);
        p.acts.clear();
        assert_eq!(
            step(&mut p, &ks),
            "1:1 2:1 0:1 0:1 2:1 1:1 f1,f2,f0,p0,p1,f0,f2,f1"
        );
    }
}
