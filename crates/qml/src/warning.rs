//! The engine's warnings: one way out for all of them, taken inside the call that warns, as
//! Qt's message handler is, so that a warning stands among the `console.*` lines where Qt
//! prints it.

use std::{path::Path, sync::OnceLock};

/// Where a warning comes from. Every part is optional: the caller passes what it knows, and
/// nothing here looks a place up.
#[derive(Clone, Copy, Debug, Default)]
pub struct Place<'a> {
    pub file: Option<&'a Path>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl Place<'_> {
    /// `msg` at this place as QQmlError::toString writes it: the file (`<Unknown File>` for a
    /// line of no file), `:line`, `:column` only after a line. `msg` alone at no place.
    pub fn text(&self, msg: &str) -> String {
        let file = match self.file {
            Some(f) => format!("file://{}", f.display()),
            None if self.line.is_some() => "<Unknown File>".to_owned(),
            None => return msg.to_owned(),
        };
        match (self.line, self.column) {
            (Some(l), Some(c)) => format!("{file}:{l}:{c}: {msg}"),
            (Some(l), None) => format!("{file}:{l}: {msg}"),
            (None, _) => format!("{file}: {msg}"),
        }
    }
}

/// Where the host takes the warnings: called with each one inside the call that raised it.
pub static SINK: OnceLock<fn(&str)> = OnceLock::new();

/// Says `msg` at `at` now and returns the line said.
pub fn say(at: Place, msg: &str) -> String {
    let line = at.text(msg);
    if let Some(sink) = SINK.get() {
        sink(&line);
    }
    line
}

/// The warnings a world raised, in order (`World::errors`). A line reaches it through
/// [`say`] only, so it has been said by the time it is kept.
#[derive(Debug, Default)]
pub struct Warnings(Vec<String>);

impl Warnings {
    /// Says `line`, a warning that carries its place already, and keeps it.
    pub fn push(&mut self, line: String) {
        self.say(Place::default(), &line);
    }

    /// Says `msg` at `at` and keeps the line.
    pub fn say(&mut self, at: Place, msg: &str) {
        self.0.push(say(at, msg));
    }

    /// The lines kept so far; none are kept after.
    pub fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.0)
    }

    /// Removes the kept lines of `range`, as [`Vec::drain`].
    pub fn drain(
        &mut self,
        range: impl std::ops::RangeBounds<usize>,
    ) -> std::vec::Drain<'_, String> {
        self.0.drain(range)
    }
}

impl Extend<String> for Warnings {
    fn extend<T: IntoIterator<Item = String>>(&mut self, lines: T) {
        for line in lines {
            self.push(line);
        }
    }
}

impl std::ops::Deref for Warnings {
    type Target = Vec<String>;
    fn deref(&self) -> &Vec<String> {
        &self.0
    }
}

impl PartialEq<Vec<String>> for Warnings {
    fn eq(&self, other: &Vec<String>) -> bool {
        self.0 == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_as_qqmlerror() {
        let f = Path::new("/a/b.qml");
        let at = |file, line, column| Place { file, line, column };
        assert_eq!(
            at(Some(f), Some(3), Some(5)).text("m"),
            "file:///a/b.qml:3:5: m"
        );
        assert_eq!(at(Some(f), Some(3), None).text("m"), "file:///a/b.qml:3: m");
        assert_eq!(at(Some(f), None, Some(5)).text("m"), "file:///a/b.qml: m");
        assert_eq!(
            at(None, Some(3), Some(5)).text("m"),
            "<Unknown File>:3:5: m"
        );
        assert_eq!(at(None, None, None).text("m"), "m");
    }
}
