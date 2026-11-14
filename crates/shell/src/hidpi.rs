//! `QT_SCALE_FACTOR` and `QT_SCREEN_SCALE_FACTORS`, read as Qt 6.12 reads them
//! (src/gui/kernel/qhighdpiscaling.cpp). A screen's factor is the global one times the
//! screen's own; everything QML sees stays in logical pixels and the surface is that many
//! times larger.

/// One entry of `QT_SCREEN_SCALE_FACTORS`: a factor for the screen at the entry's place in
/// the list, or for the screen of that name.
#[derive(Debug, PartialEq)]
struct Entry {
    name: Option<String>,
    factor: f64,
}

#[derive(Debug, PartialEq)]
pub struct Factors {
    /// `QT_SCALE_FACTOR`.
    global: f64,
    /// `QT_SCREEN_SCALE_FACTORS`, the entries Qt keeps.
    screens: Vec<Entry>,
}

impl Default for Factors {
    fn default() -> Factors {
        Factors {
            global: 1.0,
            screens: Vec::new(),
        }
    }
}

/// `QString::toDouble` (`qt_asciiToDouble`, qlocale_tools.cpp): a number in the C locale,
/// spaces around it ignored; `inf` with or without a sign and `nan` without one, in any
/// case. Anything else, and a number too large for a double: `None`.
fn number(text: &str) -> Option<f64> {
    let t = text.trim();
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.eq_ignore_ascii_case("inf") {
        return Some(if t.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if t.eq_ignore_ascii_case("nan") {
        return Some(f64::NAN);
    }
    // Rust also reads `infinity` and a signed `nan`, which Qt does not.
    if !digits.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return None;
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

impl Factors {
    /// The two variables' values (`None`: not set). The second result is what to warn about,
    /// a line for each: a `QT_SCALE_FACTOR` that is a number but not a finite one above 0,
    /// and an infinite entry of `QT_SCREEN_SCALE_FACTORS`. Qt takes these as they are and
    /// shows no window, without a word; the shell reads the first as 1, drops the second,
    /// and says so (docs/quickshell-differences.md).
    pub fn parse(global: Option<&str>, screens: Option<&str>) -> (Factors, Vec<String>) {
        let mut warnings = Vec::new();
        // `qEnvironmentVariableOptionalReal`: text that is no number is no factor.
        let global = match global.map(|text| (text.trim(), number(text))) {
            Some((_, Some(v))) if v > 0.0 && v.is_finite() => v,
            Some((text, Some(_))) => {
                warnings.push(format!(
                    "QT_SCALE_FACTOR={text} is not a scale factor a window can have: the factor is 1"
                ));
                1.0
            }
            _ => 1.0,
        };
        // `parseScreenScaleFactorsSpec`: entries split at `;`, each a factor or `name=factor`
        // (the name ends at the last `=`); one whose factor is no number above 0 is dropped.
        let screens = screens
            .unwrap_or_default()
            .split(';')
            .filter_map(|spec| {
                let (name, factor) = match spec.rsplit_once('=') {
                    Some((name, factor)) => (Some(name.to_string()), factor),
                    None => (None, spec),
                };
                let factor = number(factor).filter(|f| *f > 0.0)?;
                if factor.is_infinite() {
                    warnings.push(format!(
                        "QT_SCREEN_SCALE_FACTORS: the entry {} is not a scale factor a window can have: the entry is dropped",
                        spec.trim()
                    ));
                    return None;
                }
                Some(Entry { name, factor })
            })
            .collect();
        (Factors { global, screens }, warnings)
    }

    /// The factors of this process's environment; the warnings go to the log.
    pub fn from_env() -> Factors {
        let var = |name: &str| std::env::var(name).ok();
        let (factors, warnings) = Factors::parse(
            var("QT_SCALE_FACTOR").as_deref(),
            var("QT_SCREEN_SCALE_FACTORS").as_deref(),
        );
        for w in warnings {
            elog::eprintln!("WARN: {w}");
        }
        factors
    }

    /// The own factor of the screen `name`, the `index`th of the screens
    /// (`updateHighDpiScaling`): the last entry for it, by its place in the list (dropped
    /// entries not counted) or by name. A factor below 1 is 1 (`roundScaleFactor`, with
    /// the default `PassThrough` policy).
    fn screen_own(&self, index: usize, name: &str) -> f64 {
        self.screens
            .iter()
            .enumerate()
            .filter(|(i, e)| match &e.name {
                Some(n) => n == name,
                None => *i == index,
            })
            .map(|(_, e)| e.factor.max(1.0))
            .next_back()
            .unwrap_or(1.0)
    }

    /// What Qt multiplies the sizes of the screen `name`, the `index`th of the screens, and
    /// of the windows on it by (`QHighDpiScaling::scaleAndOrigin`).
    pub fn screen(&self, index: usize, name: &str) -> f64 {
        self.global * self.screen_own(index, name)
    }

    /// No factor is set: nothing differs from a shell without the two variables.
    pub fn is_identity(&self) -> bool {
        self.global == 1.0 && self.screens.iter().all(|e| e.factor <= 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The factors of two screens, `HEADLESS-1` and `HEADLESS-2`.
    fn two(global: Option<&str>, screens: Option<&str>) -> (f64, f64) {
        let (f, _) = Factors::parse(global, screens);
        (f.screen(0, "HEADLESS-1"), f.screen(1, "HEADLESS-2"))
    }

    /// `QT_SCALE_FACTOR` as qml6 of Qt 6.12 showed it (`Screen.devicePixelRatio` on an
    /// output of scale 1).
    #[test]
    fn the_global_factor_reads_as_qt_s() {
        for (text, want) in [
            ("2", 2.0),
            ("1.5", 1.5),
            ("1.3", 1.3),
            ("0.5", 0.5),
            (" 2 ", 2.0),
            ("abc", 1.0),
            ("2,5", 1.0),
            ("2x", 1.0),
            ("", 1.0),
            ("1e999", 1.0),
            ("infinity", 1.0),
            ("+nan", 1.0),
            ("in", 1.0),
        ] {
            let (f, warnings) = Factors::parse(Some(text), None);
            assert_eq!(f.screen(0, "HEADLESS-1"), want, "{text:?}");
            assert_eq!(warnings, [""; 0], "{text:?}");
        }
        assert_eq!(two(None, None), (1.0, 1.0));
        assert!(Factors::parse(None, None).0.is_identity());
        assert!(Factors::parse(Some("abc"), Some("0.5;x")).0.is_identity());
        assert!(!Factors::parse(Some("2"), None).0.is_identity());
        assert!(!Factors::parse(None, Some("1;2")).0.is_identity());
    }

    /// A number that is not a finite one above 0: Qt takes it as the factor and shows a
    /// window of 2147483647 pixels or none; the shell stays at 1 and says so once.
    #[test]
    fn a_global_factor_no_window_can_have_is_one_with_a_warning() {
        for text in [
            "0", "-0", "-1", "-0.5", "nan", "NaN", "inf", "+inf", "INF", "-inf", " 0 ",
        ] {
            let (f, warnings) = Factors::parse(Some(text), Some("2"));
            assert_eq!(f.screen(1, "HEADLESS-2"), 1.0, "{text}");
            assert_eq!(f.screen(0, "HEADLESS-1"), 2.0, "{text}");
            let named = format!("QT_SCALE_FACTOR={} ", text.trim());
            assert!(
                warnings.len() == 1 && warnings[0].starts_with(&named),
                "{text}: {warnings:?}"
            );
        }
    }

    /// An infinite entry: Qt keeps it and shows no window on that screen; the shell drops
    /// it like the entries Qt drops, and says so once.
    #[test]
    fn an_infinite_screen_factor_is_dropped_with_a_warning() {
        for (text, want, entry) in [
            ("inf;2", (2.0, 1.0), "inf"),
            ("2;+INF", (2.0, 1.0), "+INF"),
            ("HEADLESS-1=inf;HEADLESS-2=2", (1.0, 2.0), "HEADLESS-1=inf"),
        ] {
            let (f, warnings) = Factors::parse(Some("abc"), Some(text));
            let got = (f.screen(0, "HEADLESS-1"), f.screen(1, "HEADLESS-2"));
            assert_eq!(got, want, "{text}");
            let named = format!("QT_SCREEN_SCALE_FACTORS: the entry {entry} ");
            assert!(
                warnings.len() == 1 && warnings[0].starts_with(&named),
                "{text}: {warnings:?}"
            );
        }
    }

    /// `QT_SCREEN_SCALE_FACTORS` as qml6 of Qt 6.12 showed it on two outputs.
    #[test]
    fn the_screen_factors_read_as_qt_s() {
        for (text, want) in [
            ("1.5;2", (1.5, 2.0)),
            ("2", (2.0, 1.0)),
            ("HEADLESS-2=1.5", (1.0, 1.5)),
            ("HEADLESS-1=2;1.5", (2.0, 1.5)),
            ("abc;2", (2.0, 1.0)),
            ("1;1", (1.0, 1.0)),
            ("0.5;0.5", (1.0, 1.0)),
            ("0;2", (2.0, 1.0)),
            ("-1;2", (2.0, 1.0)),
            ("", (1.0, 1.0)),
            (";;", (1.0, 1.0)),
            ("NO-SUCH=3", (1.0, 1.0)),
            ("1.5;HEADLESS-1=2", (2.0, 1.0)),
            ("a=b=2", (1.0, 1.0)),
            (";2", (2.0, 1.0)),
            ("nan;2", (2.0, 1.0)),
            ("-inf;2", (2.0, 1.0)),
            ("1e999;2", (2.0, 1.0)),
            ("HEADLESS-1=0;HEADLESS-2=2", (1.0, 2.0)),
            ("HEADLESS-1=-1;HEADLESS-2=2", (1.0, 2.0)),
            ("HEADLESS-1=nan;HEADLESS-2=2", (1.0, 2.0)),
            ("HEADLESS-1=abc;HEADLESS-2=2", (1.0, 2.0)),
        ] {
            assert_eq!(two(None, Some(text)), want, "{text:?}");
            // Qt drops these without a word.
            assert_eq!(Factors::parse(None, Some(text)).1, [""; 0], "{text:?}");
        }
    }

    /// The two multiply.
    #[test]
    fn the_global_factor_multiplies_the_screen_s() {
        assert_eq!(two(Some("2"), Some("1.5;1.25")), (3.0, 2.5));
        assert_eq!(two(Some("0.5"), Some("2")), (1.0, 0.5));
    }
}
