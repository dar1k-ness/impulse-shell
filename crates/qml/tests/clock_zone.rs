//! Every local clock reads the system's zone where `Date` reads it: in the time zone
//! database, looked for under `TZDIR` first, as Qt 6.12 and the C library do.

use qml::runtime::Runtime;

/// Runs `test` in a process of its own whose zone, `Test/Clock`, is 5 h 17 min ahead of UTC
/// and is only in a database that `TZDIR` names. `Test/Other` there is 2 h ahead.
fn run_in_zone_under_tzdir(test: &str) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Test")).unwrap();
    let zone = testenv::tzif(19020, "+0517", "<+0517>-5:17");
    std::fs::write(dir.path().join("Test/Clock"), zone).unwrap();
    let other = testenv::tzif(7200, "+02", "<+02>-2");
    std::fs::write(dir.path().join("Test/Other"), other).unwrap();
    let table = "XX\t+0000+00000\tTest/Clock\nXX\t+0000+00000\tTest/Other\n";
    std::fs::write(dir.path().join("zone.tab"), table).unwrap();
    let vars = [
        ("TZDIR", dir.path().as_os_str()),
        ("TZ", "Test/Clock".as_ref()),
    ];
    testenv::run_in_own_process(test, vars);
}

/// 2024-01-10 12:00:30 UTC, in milliseconds since the epoch.
const NOON: f64 = 1_704_888_030_000.0;

#[test]
fn clocks_read_the_zone_of_the_database_under_tzdir() {
    const TEST: &str = concat!(
        module_path!(),
        "::clocks_read_the_zone_of_the_database_under_tzdir"
    );
    if !testenv::in_own_process(TEST) {
        return run_in_zone_under_tzdir(TEST);
    }
    qml::builtins::set_clock(Some(NOON));
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            "import QtQuick\nimport Quickshell\nItem {\n\
             SystemClock { id: clock; precision: SystemClock.Seconds }\n\
             SystemClock { id: hourly; precision: SystemClock.Hours }\n\
             property date now: new Date()\n\
             property string date: [now.getHours(), now.getMinutes(), now.getSeconds()].join(':')\n\
             property string read: [clock.hours, clock.minutes, clock.seconds].join(':')\n\
             property string hour: [hourly.hours, hourly.minutes, hourly.seconds].join(':')\n\
             property string began: hourly.date.toISOString() }",
        )
        .unwrap();
    rt.settle();
    let text = |rt: &mut Runtime, name: &str| rt.get(root, name).to_js_string();
    assert_eq!(text(&mut rt, "date"), "17:17:30");
    assert_eq!(text(&mut rt, "read"), "17:17:30");
    // The local hour began 17 min 30 s before.
    assert_eq!(text(&mut rt, "hour"), "17:0:0");
    assert_eq!(text(&mut rt, "began"), "2024-01-10T11:43:00.000Z");
    // The next local second: the clock of seconds is the first one due.
    assert_eq!(rt.next_deadline(0.0), 1001.0);
    // A file's stamp is the local date and time.
    let stamps =
        [0, 1].map(|later| qml::ports::date_stamp_at(qml::civil::now_utc_secs() + 19020 + later));
    let stamp = qml::ports::date_stamp();
    assert!(stamps.contains(&stamp), "{stamp} is none of {stamps:?}");
}

/// The wall clock a test sets (`testenv::local_time_ms`) is in the zone the engine reads.
#[test]
fn a_tests_wall_clock_is_in_the_zone_under_tzdir() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_tests_wall_clock_is_in_the_zone_under_tzdir"
    );
    if !testenv::in_own_process(TEST) {
        return run_in_zone_under_tzdir(TEST);
    }
    // 2026-01-15 21:40 there is 16:23 UTC.
    let at = testenv::local_time_ms(21, 40);
    assert_eq!(at, 1_768_494_180_000.0);
    qml::builtins::set_clock(Some(at));
    let mut rt = Runtime::new(None);
    let source = "import QtQml\nQtObject { property date now: new Date()\n\
                  property string read: now.getDate() + ' ' + now.getHours() + ':' + now.getMinutes() }";
    let root = rt.load_str(source).unwrap();
    rt.settle();
    assert_eq!(rt.get(root, "read").to_js_string(), "15 21:40");
}

#[test]
fn an_hourly_clock_is_next_due_at_the_local_hour() {
    const TEST: &str = concat!(
        module_path!(),
        "::an_hourly_clock_is_next_due_at_the_local_hour"
    );
    if !testenv::in_own_process(TEST) {
        return run_in_zone_under_tzdir(TEST);
    }
    qml::builtins::set_clock(Some(NOON));
    let mut rt = Runtime::new(None);
    let source = "import Rustshell\nSystemClock { precision: SystemClock.Hours }";
    rt.load_str(source).unwrap();
    rt.settle();
    // 18:00 there is 42 min 30 s away.
    assert_eq!(rt.next_deadline(0.0), 2_550_001.0);
}

#[test]
fn a_change_of_the_zone_shows_at_once_in_date_and_at_a_clocks_next_tick() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_change_of_the_zone_shows_at_once_in_date_and_at_a_clocks_next_tick"
    );
    if !testenv::in_own_process(TEST) {
        return run_in_zone_under_tzdir(TEST);
    }
    qml::builtins::set_clock(Some(NOON));
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            "import QtQuick\nimport Quickshell\nItem {\n\
             SystemClock { id: hourly; precision: SystemClock.Hours }\n\
             property string hour: [hourly.hours, hourly.minutes, hourly.seconds].join(':')\n\
             function date() { const now = new Date()\n\
             return [now.getHours(), now.getMinutes(), now.getSeconds()].join(':') } }",
        )
        .unwrap();
    rt.settle();
    let date = |rt: &mut Runtime| {
        let date = rt.call_method(root, "date", vec![]);
        date.unwrap().unwrap().to_js_string()
    };
    let hour = |rt: &mut Runtime| rt.get(root, "hour").to_js_string();
    assert_eq!(
        (date(&mut rt), hour(&mut rt)),
        ("17:17:30".into(), "17:0:0".into())
    );
    assert_eq!(rt.next_deadline(0.0), 2_550_001.0);

    #[expect(
        clippy::disallowed_methods,
        reason = "the system's zone changes under a running process"
    )]
    // SAFETY: this process runs this test only, and nothing else reads its environment
    // meanwhile.
    unsafe {
        std::env::set_var("TZ", "Test/Other")
    };
    // Qt 6.12 reads the zone anew for every Date.
    assert_eq!(date(&mut rt), "14:0:30");
    let stamps =
        [0, 1].map(|later| qml::ports::date_stamp_at(qml::civil::now_utc_secs() + 7200 + later));
    let stamp = qml::ports::date_stamp();
    assert!(stamps.contains(&stamp), "{stamp} is none of {stamps:?}");
    // Quickshell 0.3.2's SystemClock reads the time when its timer fires, and the timer is
    // armed for the end of the hour of the zone that was: until then it shows that hour.
    rt.poll_native();
    rt.settle();
    assert_eq!(hour(&mut rt), "17:0:0");
    assert_eq!(rt.next_deadline(0.0), 2_550_001.0);
    qml::builtins::set_clock(Some(NOON + 2_549_000.0));
    rt.poll_native();
    rt.settle();
    assert_eq!(hour(&mut rt), "17:0:0");
    // 12:43:00 UTC, where that hour ends: the clock reads the new zone's 14:43 at its
    // precision, and is next due at 15:00 there.
    qml::builtins::set_clock(Some(NOON + 2_550_000.0));
    rt.poll_native();
    rt.settle();
    assert_eq!(
        (date(&mut rt), hour(&mut rt)),
        ("14:43:0".into(), "14:0:0".into())
    );
    assert_eq!(rt.next_deadline(2_550_001.0), 1_020_001.0);
}
