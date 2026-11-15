//! Readers of an impulse-shell log (`WAYLAND_DEBUG=1` plus `RI_LOG` targets on one stderr, in the order
//! the loop wrote them) for tests that wait on what the shell and the compositor did, via
//! [`crate::Log::wait`], never on time.

/// A commit impulse-shell made after the last line `mark` matches is on screen: the
/// wp_presentation_feedback it requested for it (under `RI_LOG=ri::present`) was `presented`.
/// A feedback ends with its `presented` or `discarded`, and only then may its id be reused, so
/// the first of those after the request is its own. False while no line matches `mark`.
pub fn presented_after(log: &str, mark: impl Fn(&str) -> bool) -> bool {
    let lines: Vec<&str> = log.lines().collect();
    let Some(from) = lines.iter().rposition(|l| mark(l)) else {
        return false;
    };
    (from + 1..lines.len()).any(|i| {
        let Some(id) = lines[i]
            .split_once("-> wp_presentation#")
            .and_then(|(_, r)| r.split_once("new id wp_presentation_feedback#"))
            .and_then(|(_, r)| r.split(')').next())
        else {
            return false;
        };
        let (presented, discarded) = (
            format!("wp_presentation_feedback#{id}.presented("),
            format!("wp_presentation_feedback#{id}.discarded("),
        );
        lines[i + 1..]
            .iter()
            .find(|l| l.contains(&presented) || l.contains(&discarded))
            .is_some_and(|l| l.contains(&presented))
    })
}

/// The loop's last turn (`RI_LOG=ri::sched` logs each turn's next deadline as `t=… next …`)
/// came after the last line `mark` matches and has nothing due: whatever that line started has
/// run its course and the loop blocks until an event.
pub fn blocked_after(log: &str, mark: impl Fn(&str) -> bool) -> bool {
    let lines: Vec<&str> = log.lines().collect();
    let turn = lines.iter().rposition(|l| turn_time(l).is_some());
    let m = lines.iter().rposition(|l| mark(l));
    turn.is_some_and(|t| lines[t].contains(" next None") && m.is_none_or(|m| m < t))
}

/// The loop time (ms since its start) of an `RI_LOG=ri::sched` turn line `t=… next …`.
pub fn turn_time(line: &str) -> Option<f64> {
    let (t, _) = line.trim_start().strip_prefix("t=")?.split_once(" next ")?;
    t.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presented_after_needs_the_feedback_of_a_later_commit() {
        let log = "\
-> wp_presentation#5.feedback(wl_surface#3, new id wp_presentation_feedback#9)
wp_presentation_feedback#9.presented(0, 1, 2, 3, 4, 5, 6)
MARK
-> wp_presentation#5.feedback(wl_surface#3, new id wp_presentation_feedback#9)
";
        assert!(!presented_after(log, |l| l == "MARK"));
        assert!(!presented_after(log, |l| l == "missing"));
        let discarded = format!("{log}wp_presentation_feedback#9.discarded()\n");
        assert!(!presented_after(&discarded, |l| l == "MARK"));
        let shown = format!("{log}wp_presentation_feedback#9.presented(0, 1, 2, 3, 4, 5, 6)\n");
        assert!(presented_after(&shown, |l| l == "MARK"));
    }

    #[test]
    fn blocked_after_needs_an_idle_turn_after_the_mark() {
        let idle = "t=10.0 next None\nMARK\n";
        assert!(!blocked_after(idle, |l| l == "MARK"));
        assert!(blocked_after(&format!("{idle}t=12.5 next None\n"), |l| l == "MARK"));
        assert!(!blocked_after(
            &format!("{idle}t=12.5 next Some((40.0, KeyRepeat))\n"),
            |l| l == "MARK"
        ));
        assert_eq!(turn_time("  t=12.5 next None"), Some(12.5));
        assert_eq!(turn_time("qml: t=1"), None);
    }
}
