//! A child's output read as it arrives, for tests that wait on what it prints.

use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, Condvar, Mutex};

/// A child's output, appended by a reader thread as it arrives; waiters wake on each line.
#[derive(Default)]
pub struct Log {
    /// The output so far, and whether the child closed it (exited).
    text: Mutex<(String, bool)>,
    grew: Condvar,
}

impl Log {
    /// Reads `r` (the read end of the child's stdout/stderr pipe) on a thread of its own.
    pub fn capture(r: impl Read + Send + 'static) -> Arc<Self> {
        Self::read(r, None)
    }

    /// [`Log::capture`] that also copies each line to this process's stderr as it arrives,
    /// after `name: `. A test waits without a deadline of its own, so when it hangs nothing
    /// of it runs again: what nextest shows of the test it ended (`slow-timeout`) is what the
    /// test had printed by then.
    pub fn capture_shown(name: &'static str, r: impl Read + Send + 'static) -> Arc<Self> {
        Self::read(r, Some(name))
    }

    fn read(r: impl Read + Send + 'static, show: Option<&'static str>) -> Arc<Self> {
        let log = Arc::new(Log::default());
        let l = log.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(r);
            let mut line = Vec::new();
            loop {
                line.clear();
                let n = r.read_until(b'\n', &mut line).unwrap_or(0);
                if let Some(name) = show.filter(|_| n > 0) {
                    let line = String::from_utf8_lossy(&line);
                    elog::eprintln!("{name}: {}", line.trim_end_matches('\n'));
                }
                let mut g = l.text.lock().unwrap();
                g.0.push_str(&String::from_utf8_lossy(&line));
                g.1 = n == 0;
                drop(g);
                l.grew.notify_all();
                if n == 0 {
                    return;
                }
            }
        });
        log
    }

    pub fn text(&self) -> String {
        self.text.lock().unwrap().0.clone()
    }

    /// The child closed its output: it exited.
    pub fn closed(&self) -> bool {
        self.text.lock().unwrap().1
    }

    /// Blocks until the child closed its output (exited) and returns all of it. No deadline of
    /// its own: a hang is nextest's `slow-timeout` to end.
    pub fn until_closed(&self) -> String {
        let mut g = self.text.lock().unwrap();
        while !g.1 {
            g = self.grew.wait(g).unwrap();
        }
        g.0.clone()
    }

    /// Blocks until `f(output)` holds, or the child closes its output (exits) without it holding:
    /// then `Err(output)`. No deadline of its own: a child that hangs leaves the test to nextest's
    /// slow-timeout (`.config/nextest.toml`), which terminates it.
    pub fn wait(&self, f: impl Fn(&str) -> bool) -> Result<(), String> {
        let g = self.text.lock().unwrap();
        let g = self
            .grew
            .wait_while(g, |(text, closed)| !f(text) && !*closed)
            .unwrap();
        if f(&g.0) { Ok(()) } else { Err(g.0.clone()) }
    }
}
