//! Diagnostics switches, read once per process.
//!
//! `RI_LOG` is a [`tracing_subscriber::EnvFilter`] over the `ri::*` targets (one per former
//! `II_<NAME>` log/profile variable, `ri::<name>`; e.g. `RI_LOG=ri::stats,ri::keys`); events go
//! to stderr as the bare message, like the `eprintln!` lines they replace. A target names a
//! prefix and, without `=<level>`, every level: `ri::ptr=debug` leaves out `ri::ptr`'s motion
//! events (trace).
//! `RI_DEBUG` is a comma list of A/B switches and self-checks ([`Switch`] names in lower case)
//! and of `name=value` parameters for some logs (`prof=<ms>`, `pprof_us=<us>`,
//! `deptrace=<prop>`, `val=<prop>+<prop>`, `val_items=<id>+<id>`, `geom=<type>`) and for one
//! fault (`gpuloss=<n>`).

use std::sync::OnceLock;

bits::bit_flags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Switch: u64 {
        const NOLAYMEMO = 1 << 0;
        const NOREMOVELOCAL = 1 << 1;
        const NOVIEWLOCAL = 1 << 2;
        const NOCTRLLOCAL = 1 << 3;
        const NORASTERCACHE = 1 << 4;
        const NOCANVASCACHE = 1 << 5;
        const NODL = 1 << 6;
        const NOXLATE = 1 << 7;
        const XFLAYER = 1 << 8;
        const NODAMAGE = 1 << 9;
        const NOLCACHE = 1 << 10;
        const WALL_ANIM = 1 << 11;
        const UNROOTED_DROP = 1 << 12;
        const NOLOCAL = 1 << 13;
        const NOFRESH = 1 << 14;
        const NOPOLISH = 1 << 15;
        const NOMOVINGLOCAL = 1 << 16;
        const NOFREEZE = 1 << 17;
        const NOVIRT = 1 << 18;
        const NOMOVED = 1 << 19;
        const NOPLAN = 1 << 20;
        const NOPOST = 1 << 21;
        const NODEFER = 1 << 22;
        const NOLAZY = 1 << 23;
        const NODELEGMEMO = 1 << 24;
        const NODELEGFAST = 1 << 25;
        const LREVALL = 1 << 26;
        const NODELEGLOCAL = 1 << 27;
        const NORUNHOLD = 1 << 28;
        const NODMABUF = 1 << 29;
        const MEMOCHECK = 1 << 30;
        const LAYCHECK = 1 << 31;
        const DMGCHECK = 1 << 32;
        const SKIPCHECK = 1 << 33;
        const INCCHECK = 1 << 34;
        const DLCHECK = 1 << 35;
        const PLANCHECK = 1 << 36;
        /// `ipc call ri-debug dump` answers (it writes the item tree to the instance's run
        /// directory); off, the call is an unknown target like any other.
        const DUMP = 1 << 37;
        /// The GL renderer sends every program/uniform/filter/attrib call instead of skipping
        /// those its state cache knows are unchanged.
        const NOGLCACHE = 1 << 38;
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Opts {
    pub switches: Switch,
    /// `prof=<ms>`: `ri::prof` threshold (inclusive time).
    pub prof_ms: Option<f64>,
    /// `pprof_us=<us>`: `ri::pprof` threshold, 3000 by default.
    pub pprof_us: Option<u128>,
    /// `deptrace=<prop>`: `ri::deptrace` logs dirtying of bindings of this property.
    pub deptrace: Option<String>,
    /// `val=<prop>+...`: `ri::val` logs only writes of these properties (all without it).
    pub val: Option<Vec<String>>,
    /// `val_items=<id>+...`: `ri::val` logs every write to these items.
    pub val_items: Vec<usize>,
    /// `geom=<type>`: `ri::geom` logs geometry changes of items of this type.
    pub geom: Option<String>,
    /// `gpuloss=<n>`: the first GPU context of the process is lost (EGL_CONTEXT_LOST) at its
    /// n-th frame, as after a driver reset.
    pub gpuloss: Option<u64>,
    /// `slow=<n>`: a window's layout and its painting take n times as long, as on a machine
    /// n times slower.
    pub slow: Option<f64>,
    /// Names that are neither a switch nor a parameter.
    pub unknown: Vec<String>,
}

impl Opts {
    pub fn parse(s: &str) -> Opts {
        let mut o = Opts::default();
        let list = |v: &str| v.split('+').map(str::to_owned).collect::<Vec<_>>();
        for e in s.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            match e.split_once('=') {
                Some(("prof", v)) => o.prof_ms = v.parse().ok(),
                Some(("pprof_us", v)) => o.pprof_us = v.parse().ok(),
                Some(("slow", v)) => o.slow = v.parse().ok(),
                Some(("deptrace", v)) => o.deptrace = Some(v.to_owned()),
                Some(("val", v)) => o.val = Some(list(v)),
                Some(("val_items", v)) => {
                    o.val_items = v.split('+').filter_map(|x| x.parse().ok()).collect()
                }
                Some(("geom", v)) => o.geom = Some(v.to_owned()),
                Some(("gpuloss", v)) => o.gpuloss = v.parse().ok(),
                Some(_) => o.unknown.push(e.to_owned()),
                None => match Switch::from_name(&e.to_ascii_uppercase()) {
                    Some(f) => o.switches |= f,
                    None => o.unknown.push(e.to_owned()),
                },
            }
        }
        o
    }
}

/// `RI_DEBUG`, parsed on first use.
pub fn opts() -> &'static Opts {
    static O: OnceLock<Opts> = OnceLock::new();
    O.get_or_init(|| {
        let o = Opts::parse(&std::env::var("RI_DEBUG").unwrap_or_default());
        if !o.unknown.is_empty() {
            tracing::warn!(target: "ri::debug", "RI_DEBUG: unknown {:?}", o.unknown);
        }
        o
    })
}

pub fn on(s: Switch) -> bool {
    on_in(opts(), s)
}

/// [`on`] under the options `o` instead of `RI_DEBUG`.
pub fn on_in(o: &Opts, s: Switch) -> bool {
    o.switches.contains(s)
}

/// The `RI_LOG` filter over `ri=warn` (other crates' events stay off unless named).
pub fn filter(spec: Option<&str>) -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::builder().parse_lossy(format!("ri=warn,{}", spec.unwrap_or("")))
}

/// Former `II_*` diagnostics variables and the settings that replace them.
pub const LEGACY: &[(&str, &str)] = &[
    ("RI_AOTMISS", "RI_LOG=ri::aotmiss"),
    ("RI_AOTSTAT", "RI_LOG=ri::aotstat"),
    ("RI_APPDEBUG", "RI_LOG=ri::appdebug"),
    ("RI_CANVAS_DEBUG", "RI_LOG=ri::canvas_debug"),
    ("RI_CMDSTAT", "RI_LOG=ri::cmdstat"),
    (
        "RI_DEPTRACE",
        "RI_LOG=ri::deptrace RI_DEBUG=deptrace=<prop>",
    ),
    ("RI_DIRTY", "RI_LOG=ri::dirty"),
    ("RI_DLCHECK", "RI_DEBUG=dlcheck"),
    ("RI_DLDBG", "RI_LOG=ri::dldbg"),
    ("RI_DMGCHECK", "RI_DEBUG=dmgcheck"),
    ("RI_DMGDBG", "RI_LOG=ri::dmgdbg"),
    ("RI_DMGDUMP", "RI_LOG=ri::dmgdump"),
    ("RI_DPROF", "RI_LOG=ri::dprof"),
    ("RI_FRAMES", "RI_LOG=ri::frames"),
    ("RI_GEOM", "RI_LOG=ri::geom RI_DEBUG=geom=<type>"),
    ("RI_GLCOUNT", "RI_LOG=ri::glcount"),
    ("RI_GPUPROF", "RI_LOG=ri::gpuprof"),
    ("RI_GPUTIME", "RI_LOG=ri::gputime"),
    ("RI_GRAB", "RI_LOG=ri::grab"),
    ("RI_HPROF", "RI_LOG=ri::hprof"),
    ("RI_IMGDBG", "RI_LOG=ri::imgdbg"),
    ("RI_INCCHECK", "RI_DEBUG=inccheck"),
    ("RI_JSPROF", "RI_LOG=ri::jsprof"),
    ("RI_KEYS", "RI_LOG=ri::keys"),
    ("RI_LAYCHECK", "RI_DEBUG=laycheck"),
    ("RI_LREV", "RI_LOG=ri::lrev"),
    ("RI_LREVALL", "RI_DEBUG=lrevall"),
    ("RI_MEMOCHECK", "RI_DEBUG=memocheck"),
    ("RI_MPROF", "RI_LOG=ri::mprof"),
    ("RI_NOCANVASCACHE", "RI_DEBUG=nocanvascache"),
    ("RI_NOCTRLLOCAL", "RI_DEBUG=noctrllocal"),
    ("RI_NODAMAGE", "RI_DEBUG=nodamage"),
    ("RI_NODEFER", "RI_DEBUG=nodefer"),
    ("RI_NODELEGFAST", "RI_DEBUG=nodelegfast"),
    ("RI_NODELEGMEMO", "RI_DEBUG=nodelegmemo"),
    ("RI_NODL", "RI_DEBUG=nodl"),
    ("RI_NODMABUF", "RI_DEBUG=nodmabuf"),
    ("RI_NOFREEZE", "RI_DEBUG=nofreeze"),
    ("RI_NOFRESH", "RI_DEBUG=nofresh"),
    ("RI_NOLAYMEMO", "RI_DEBUG=nolaymemo"),
    ("RI_NOLAZY", "RI_DEBUG=nolazy"),
    ("RI_NOLCACHE", "RI_DEBUG=nolcache"),
    ("RI_NOLOCAL", "RI_DEBUG=nolocal"),
    ("RI_NOMOVED", "RI_DEBUG=nomoved"),
    ("RI_NOPLAN", "RI_DEBUG=noplan"),
    ("RI_NOPOLISH", "RI_DEBUG=nopolish"),
    ("RI_NOPOST", "RI_DEBUG=nopost"),
    ("RI_NORASTERCACHE", "RI_DEBUG=norastercache"),
    ("RI_NOREMOVELOCAL", "RI_DEBUG=noremovelocal"),
    ("RI_NOVIEWLOCAL", "RI_DEBUG=noviewlocal"),
    ("RI_NOVIRT", "RI_DEBUG=novirt"),
    ("RI_NOXLATE", "RI_DEBUG=noxlate"),
    ("RI_PAINT", "RI_LOG=ri::paint"),
    ("RI_PASSDBG", "RI_LOG=ri::passdbg"),
    ("RI_PLANCHECK", "RI_DEBUG=plancheck"),
    ("RI_PLANDBG", "RI_LOG=ri::plandbg"),
    ("RI_PPROF", "RI_LOG=ri::pprof"),
    ("RI_PPROF_US", "RI_LOG=ri::pprof RI_DEBUG=pprof_us=<us>"),
    ("RI_PROF", "RI_LOG=ri::prof RI_DEBUG=prof=<ms>"),
    (
        "RI_PTR",
        "RI_LOG=ri::ptr=debug (ri::ptr: RI_PTR=2, also motion)",
    ),
    ("RI_PW", "RI_LOG=ri::pw"),
    ("RI_RASTDBG", "RI_LOG=ri::rastdbg"),
    ("RI_SCDUMP", "RI_LOG=ri::scdump"),
    ("RI_SKIPCHECK", "RI_DEBUG=skipcheck"),
    ("RI_SNI", "RI_LOG=ri::sni"),
    ("RI_STATS", "RI_LOG=ri::stats"),
    ("RI_TMPLDBG", "RI_LOG=ri::tmpldbg"),
    ("RI_TOUCH", "RI_LOG=ri::touched"),
    ("RI_TOUCHDBG", "RI_LOG=ri::touchdbg"),
    ("RI_TRACEW", "RI_LOG=ri::tracew"),
    ("RI_UNROOTED_DROP", "RI_DEBUG=unrooted_drop"),
    ("RI_VAL", "RI_LOG=ri::val [RI_DEBUG=val=<prop>+<prop>]"),
    (
        "RI_VAL_ITEMS",
        "RI_LOG=ri::val RI_DEBUG=val_items=<id>+<id>",
    ),
    ("RI_VIEW", "RI_LOG=ri::view"),
    ("RI_WALL_ANIM", "RI_DEBUG=wall_anim"),
    ("RI_WSTAT", "RI_LOG=ri::wstat"),
    ("RI_XFLAYER", "RI_DEBUG=xflayer"),
];

/// Install the stderr subscriber for `RI_LOG`; later calls do nothing. Warns about set
/// [`LEGACY`] variables, which no longer do anything.
pub fn init_log() {
    let spec = std::env::var("RI_LOG").ok();
    // Each event is one `write` of its line: whole on stderr, as an `elog` line.
    let fresh =
        tracing::subscriber::set_global_default(subscriber(filter(spec.as_deref()), || {
            elog::Stderr
        }))
        .is_ok();
    for (old, new) in LEGACY.iter().filter(|_| fresh) {
        if std::env::var_os(old).is_some() {
            tracing::warn!(target: "ri::debug", "{old} is no longer read: use {new}");
        }
    }
}

static LOG_CODE_ERRORS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// From here on [`code_error`] writes to the log instead of panicking. The shell calls it at
/// its start: a mistake found while it runs is said once and the session stays up.
pub fn log_code_errors() {
    LOG_CODE_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Reports a state that this crate's own code rules out: a mistake in the code, not in the
/// configuration it runs. It panics, so a test that reaches it fails; after
/// [`log_code_errors`] it writes one line for `said` (one per place that reports) and
/// returns, and the caller goes on with a harmless answer.
#[track_caller]
pub fn code_error(said: &std::sync::Once, what: std::fmt::Arguments<'_>) {
    if !LOG_CODE_ERRORS.load(std::sync::atomic::Ordering::Relaxed) {
        panic!("{what}");
    }
    let at = std::panic::Location::caller();
    said.call_once(|| elog::eprintln!("impulse-shell: internal error at {at}: {what}"));
}

/// [`code_error`] for a mistake that many places can make through one function: the place is
/// the caller of the `#[track_caller]` function that reports, and the shell says one line for
/// each such place. A build with debug assertions panics after [`log_code_errors`] too.
#[track_caller]
pub fn code_error_at_caller(what: std::fmt::Arguments<'_>) {
    if cfg!(debug_assertions) || !LOG_CODE_ERRORS.load(std::sync::atomic::Ordering::Relaxed) {
        panic!("{what}");
    }
    static SAID: std::sync::Mutex<Vec<(&str, u32, u32)>> = std::sync::Mutex::new(Vec::new());
    let at = std::panic::Location::caller();
    let place = (at.file(), at.line(), at.column());
    let mut said = SAID.lock().unwrap_or_else(|e| e.into_inner());
    if !said.contains(&place) {
        said.push(place);
        elog::eprintln!("impulse-shell: internal error at {at}: {what}");
    }
}

/// Bare-message formatting: no time, level, target or colour.
pub fn subscriber<W>(
    f: tracing_subscriber::EnvFilter,
    w: W,
) -> impl tracing::Subscriber + Send + Sync + 'static
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_env_filter(f)
        .with_writer(w)
        .without_time()
        .with_level(false)
        .with_target(false)
        .with_ansi(false)
        // A failed write is reported with std's `eprintln!`, which panics when it fails too
        // (stderr's reader gone): the event is dropped as `elog` drops a line.
        .log_internal_errors(false)
        .finish()
}

/// Whether `RI_LOG` enables debug events of target `$t` (one cached check per call site).
#[macro_export]
macro_rules! log_on {
    ($t:literal) => {
        $crate::debug::tracing::enabled!(target: $t, $crate::debug::tracing::Level::DEBUG)
    };
}

pub use tracing;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn capture(spec: Option<&str>, f: impl FnOnce()) -> String {
        let b = Buf::default();
        let w = b.clone();
        tracing::subscriber::with_default(subscriber(filter(spec), move || w.clone()), f);
        String::from_utf8(b.0.lock().unwrap().clone()).unwrap()
    }

    /// The bytes `eprintln!` wrote for the same arguments.
    fn eprintln_bytes(args: std::fmt::Arguments) -> String {
        format!("{args}\n")
    }

    #[test]
    fn events_print_like_eprintln() {
        let (n, k, ms) = (3, "Text", 1.25);
        let got = capture(Some("ri::stats"), || {
            tracing::debug!(target: "ri::stats", "frames {n} t {ms:.1}s {k:?} {}", [1, 2].len());
            tracing::debug!(target: "ri::stats", "a\n  b {{}}");
        });
        let want = eprintln_bytes(format_args!("frames {n} t {ms:.1}s {k:?} {}", [1, 2].len()))
            + &eprintln_bytes(format_args!("a\n  b {{}}"));
        assert_eq!(got, want);
    }

    #[test]
    fn filter_selects_targets() {
        let emit = || {
            tracing::debug!(target: "ri::stats", "stats");
            tracing::debug!(target: "ri::keys", "keys");
            tracing::trace!(target: "ri::ptr", "motion");
            tracing::debug!(target: "ri::ptr", "press");
            tracing::warn!(target: "ri::check", "mismatch");
            tracing::warn!(target: "zbus", "other crate");
        };
        // Without RI_LOG only the self-check mismatches print (their switches gate them).
        assert_eq!(capture(None, emit), "mismatch\n");
        assert_eq!(capture(Some("ri::keys"), emit), "keys\nmismatch\n");
        assert_eq!(
            capture(Some("ri::stats,ri::keys"), emit),
            "stats\nkeys\nmismatch\n"
        );
        assert_eq!(capture(Some("ri::ptr=debug"), emit), "press\nmismatch\n");
        assert_eq!(capture(Some("ri::ptr"), emit), "motion\npress\nmismatch\n");
        assert_eq!(
            capture(Some("ri=debug"), emit),
            "stats\nkeys\npress\nmismatch\n"
        );
        assert_eq!(capture(Some("ri=off"), emit), "");
        assert_eq!(capture(Some("zbus=warn"), emit), "mismatch\nother crate\n");
    }

    #[test]
    fn log_on_follows_filter() {
        let on = std::cell::Cell::new((false, false));
        let probe = || on.set((crate::log_on!("ri::wstat"), crate::log_on!("ri::grab")));
        capture(Some("ri::wstat"), probe);
        assert_eq!(on.get(), (true, false));
        capture(None, probe);
        assert_eq!(on.get(), (false, false));
    }

    #[test]
    fn parses_switches_and_params() {
        let o =
            Opts::parse("noplan, memocheck,prof=2.5,val=width+height,val_items=4+7,geom=Item,x");
        assert_eq!(o.switches, Switch::NOPLAN | Switch::MEMOCHECK);
        assert_eq!(o.prof_ms, Some(2.5));
        assert_eq!(
            o.val.as_deref(),
            Some(&["width".to_owned(), "height".to_owned()][..])
        );
        assert_eq!(o.val_items, [4, 7]);
        assert_eq!(o.geom.as_deref(), Some("Item"));
        assert_eq!(o.unknown, ["x"]);
        assert_eq!(Opts::parse(""), Opts::default());
        assert_eq!(Opts::parse("slow=4").slow, Some(4.0));
        let o = Opts::parse("pprof_us=500,deptrace=implicitWidth,gpuloss=3");
        assert_eq!(
            (o.pprof_us, o.deptrace.as_deref(), o.gpuloss),
            (Some(500), Some("implicitWidth"), Some(3))
        );
    }

    /// Every former boolean switch has a `RI_DEBUG` name and every log variable an `ri::` target.
    #[test]
    fn legacy_names_resolve() {
        for (old, new) in LEGACY {
            let name = old.strip_prefix("RI_").unwrap().to_ascii_lowercase();
            if let Some(sw) = new.strip_prefix("RI_DEBUG=") {
                assert_eq!(sw, name);
                assert!(!Opts::parse(sw).switches.is_empty(), "{old}");
            } else {
                assert!(new.starts_with("RI_LOG=ri::"), "{old}");
            }
        }
        // These came after the II_* variables and have no legacy name.
        let later = Switch::DUMP
            | Switch::NOGLCACHE
            | Switch::NOMOVINGLOCAL
            | Switch::NODELEGLOCAL
            | Switch::NORUNHOLD;
        assert_eq!(
            Switch::all().difference(later).iter().count(),
            LEGACY
                .iter()
                .filter(|l| l.1.starts_with("RI_DEBUG="))
                .count()
        );
    }
}
