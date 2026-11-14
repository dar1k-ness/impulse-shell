//! `trans` (translate-shell 0.9.7.1) against the Rust job: `-list-languages` byte for byte,
//! translations against the live Google endpoint, and canned responses through a local
//! server that `trans` reaches as its `-proxy`.
use super::super::testutil::{self, Resp};
use super::*;

/// The installed translate-shell, if it is the version the port follows.
fn trans() -> bool {
    if !testutil::have("trans") || !testutil::have("gawk") {
        return false;
    }
    let v = std::process::Command::new("trans")
        .arg("-V")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let ok = v.contains("Translate Shell       0.9.7.1");
    if !ok {
        crate::testenv::unavailable("translate-shell is not 0.9.7.1");
    }
    ok
}

fn original(args: &[&str]) -> (String, String, i32) {
    testutil::run(
        std::process::Command::new("trans")
            .args(args)
            .env("LANG", "ru_RU.UTF-8")
            .env_remove("LC_ALL")
            .env_remove("HOST_LANG")
            .env_remove("HOME_LANG")
            .env_remove("USER_AGENT")
            .env_remove("HTTP_PROXY")
            .env_remove("http_proxy"),
    )
}

fn env(base: String) -> Env {
    Env {
        hl: parse_lang("ru_RU.UTF-8"),
        ua: DEFAULT_UA.into(),
        fribidi: crate::osutil::on_path("fribidi"),
        base,
        vars: Box::new(|_| None),
    }
}

#[test]
fn trans_list_languages_identical() {
    if !trans() {
        return;
    }
    let (o, _, c) = original(&["-list-languages", "-no-bidi"]);
    assert_eq!(c, 0);
    assert_eq!(o, languages());
}

#[test]
fn trans_codes_names_endonyms() {
    for (q, c) in [
        ("auto", "auto"),
        ("English", "en"),
        ("Русский", "ru"),
        ("русский", "ru"),
        ("Deutsch", "de"),
        ("zh", "zh-CN"),
        ("Chinese (Simplified)", "zh-CN"),
        ("pt", "pt-BR"),
        ("rus", "ru"),
        ("en-GB", "en"),
        ("xx-YY", "xx"),
    ] {
        assert_eq!(get_code(q).as_deref(), Some(c), "{q}");
    }
    assert_eq!(get_code("por"), None);
    assert_eq!(get_code("Klingonese"), None);
    assert_eq!(parse_lang("ru_RU.UTF-8"), "ru");
    assert_eq!(parse_lang("zh_TW.UTF-8"), "zh-TW");
    assert_eq!(parse_lang("C"), "en");
}

/// The text is translated whatever it looks like: `trans` reads these as an option, a
/// language shortcut, a page to open or a file, and ran instead of the job for them.
#[test]
fn any_text_is_sent_as_text() {
    let srv = testutil::serve(|_| Resp::new(200, r#"[[["перевод","x"]]]"#));
    for t in [
        "-h",
        "en:fr",
        "=ru",
        "12:30",
        "a=b",
        "https://x.org",
        "file:///etc/passwd",
    ] {
        let p = plan("auto", "ru", t).expect(t);
        let (out, err, code) = testutil::capture(|o, e| run(&p, &env(srv.base()), o, e));
        assert_eq!((out.as_str(), code), ("перевод\n", 0), "{t:?}: {err}");
        let reqs = srv.take();
        assert_eq!(reqs.len(), 1, "{t:?}");
        assert_eq!(reqs[0].target, request_path(t, "auto", "ru", "ru"), "{t:?}");
    }
}

/// No text asks for nothing and prints nothing.
#[test]
fn no_text_is_no_request() {
    let srv = testutil::serve(|_| Resp::new(200, r#"[[["?",""]]]"#));
    let p = plan("auto", "ru", "").expect("plan");
    let r = testutil::capture(|o, e| run(&p, &env(srv.base()), o, e));
    assert_eq!(r, (String::new(), String::new(), 0));
    assert!(srv.take().is_empty());
}

/// A language is a code, a name or an endonym, whichever engine translate-shell lists for
/// it (the service decides what it translates); no source is `auto`.
#[test]
fn languages_of_a_translation() {
    for (sl, tl) in [
        ("auto", "ru"),
        ("", "ru"),
        ("English", "Русский"),
        ("en+de", "ru+fr"),
        ("auto", "yue"),
    ] {
        assert!(plan(sl, tl, "hi").is_ok(), "{sl} {tl}");
    }
    for (sl, tl, bad) in [
        ("auto", "@ru", "@ru"),
        ("Klingonese", "ru", "Klingonese"),
        ("auto", "ru&x=1", "ru&x=1"),
    ] {
        assert_eq!(
            plan(sl, tl, "hi").err(),
            Some(format!("[ERROR] Unknown language code: {bad}"))
        );
    }
    assert!(plan("auto", "", "hi").is_err());
    assert!(plan("auto", "+", "hi").is_err());
}

/// The request goes through the proxy the environment names, as the shell's other requests
/// do: with one set, `trans` ran instead of the job.
#[test]
fn request_goes_through_the_environments_proxy() {
    let srv = testutil::serve(|_| Resp::new(200, r#"[[["привет","hi"]]]"#));
    let proxy = srv.base();
    let env = Env {
        vars: Box::new(move |k| (k == "http_proxy").then(|| proxy.clone())),
        ..env(format!("http://{HOST}"))
    };
    let p = plan("en", "ru", "hi").expect("plan");
    let (out, err, code) = testutil::capture(|o, e| run(&p, &env, o, e));
    assert_eq!((out.as_str(), code), ("привет\n", 0), "{err}");
    let reqs = srv.take();
    assert_eq!(reqs.len(), 1);
    assert_eq!(
        reqs[0].target,
        format!("http://{HOST}{}", request_path("hi", "en", "ru", "ru"))
    );
}

fn compare(sl: &str, tl: &str, text: &str, base: &str, proxy: Option<u16>) -> String {
    let mut args = vec!["-brief", "-no-bidi"];
    let px = proxy.map(|p| format!("127.0.0.1:{p}"));
    if let Some(p) = &px {
        args.extend(["-proxy", p]);
    }
    args.extend(["-source", sl, "-target", tl, text]);
    let a = original(&args);
    let p = plan(sl, tl, text).expect("plan");
    let b = testutil::capture(|o, e| run(&p, &env(base.to_string()), o, e));
    assert_eq!(
        (&a.0, a.2),
        (&b.0, b.2),
        "{sl} {tl} {text:?}: stderr {:?} / {:?}",
        a.1,
        b.1
    );
    a.0
}

/// Against Google's live endpoint, so its verdict depends on that service answering this host;
/// the gate checks the same paths offline through `trans_canned_responses`.
/// `cargo test --features live -p qml --lib trans_live_google`
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn trans_live_google() {
    if !trans() {
        return;
    }
    let base = format!("http://{HOST}");
    let probe = original(&[
        "-brief", "-no-bidi", "-source", "en", "-target", "ru", "hello",
    ]);
    if probe.2 != 0 || probe.0.trim().is_empty() {
        crate::testenv::unavailable(format!("no access to {HOST}"));
        return;
    }
    let cases = [
        ("auto", "ru", "Hello world. How are you?"),
        (
            "English",
            "Русский",
            "The quick brown fox jumps over the lazy dog",
        ),
        ("ru", "en", "Привет, как дела?\nВторая строка\n\nчетвёртая"),
        ("auto", "Deutsch", "Good morning, \"friend\" & <colleague>"),
        ("Japanese", "English", "おはようございます"),
        ("en", "العربية", "peace and love"),
        ("auto", "zh-CN", "computer"),
        ("auto", "en+fr", "кошка"),
        ("de+fr", "ru", "Haus"),
        ("auto", "ru", "teh speling misteak"),
        ("en", "ru", "a 'quoted' $HOME `x` tab\there"),
    ];
    for (sl, tl, text) in cases {
        // Google may answer two identical requests differently; one retry.
        let r = std::panic::catch_unwind(|| compare(sl, tl, text, &base, None));
        if r.is_err() {
            compare(sl, tl, text, &base, None);
        }
    }
    let long = "This sentence is repeated to make a long request. ".repeat(60);
    compare("en", "ru", long.trim(), &base, None);
}

#[test]
fn trans_canned_responses() {
    if !trans() {
        return;
    }
    let srv = testutil::serve(|r| {
        let q = r.target.split("&q=").nth(1).unwrap_or_default().to_string();
        let mk = |status: u16, body: &str| {
            let mut x = Resp::new(status, body.as_bytes().to_vec());
            x.chunked = true;
            x
        };
        // The query names the case: the first byte of the text.
        match q.get(..3).unwrap_or_default() {
            "%41" => mk(
                200,
                r#"[[["\u003cb\u003eA\u00e9\n\"q\"\\ \/ x","A",null,null,10],[" second",null],[null,null,"phon"]],null,"en"]"#,
            ),
            "%42" => mk(
                200,
                r#"[[["one",1],["two",2]],null,"en",null,null,null,null,[null,"<b>Bee</b>",null,null,null,false]]"#,
            ),
            "%43" => mk(429, "<html>Too Many Requests</html>"),
            "%44" => mk(500, "[[[\"x\"]]]"),
            "%45" => mk(200, ""),
            "%46" => mk(200, "garbage text ; comment\n# more"),
            "%47" => mk(200, r#"[[["a(*b*)c",null]],[1,[2,[3]]],"x"]"#),
            "%48" => {
                let mut x = mk(302, "");
                let host = r.header("host").unwrap_or_default().to_string();
                x.headers
                    .push(("Location".into(), format!("http://{host}/redir")));
                x
            }
            _ if r.target.ends_with("/redir") => Resp::new(200, r#"[[["redirected",""]]]"#),
            _ => mk(200, r#"[[["?",""]]]"#),
        }
    });
    let base = srv.base();
    let mut targets = vec![];
    for t in ["A x", "B y", "C", "D", "E", "F", "G", "H"] {
        compare("en", "ru", t, &base, Some(srv.port));
        targets.push(srv.take());
    }
    // Same request line (path and query) from both.
    for reqs in targets {
        let paths: Vec<String> = reqs
            .iter()
            .filter(|r| !r.target.ends_with("/redir"))
            .map(|r| r.target.replace(&format!("http://{HOST}"), ""))
            .collect();
        assert_eq!(paths.len(), 2, "{paths:?}");
        assert_eq!(paths[0], paths[1]);
        let ua: Vec<_> = reqs.iter().map(|r| r.header("user-agent")).collect();
        assert_eq!(ua[0], Some(DEFAULT_UA));
        assert_eq!(ua[0], ua[ua.len() - 1]);
    }
}

#[test]
fn trans_unreachable() {
    if !trans() {
        return;
    }
    let a = original(&[
        "-brief",
        "-no-bidi",
        "-proxy",
        "127.0.0.1:1",
        "-source",
        "en",
        "-target",
        "ru",
        "hi",
    ]);
    let p = plan("en", "ru", "hi").unwrap();
    let b = testutil::capture(|o, e| run(&p, &env("http://127.0.0.1:1".into()), o, e));
    assert_eq!((a.0, a.2), (b.0, b.2));
}

/// A server that takes the request and answers nothing sends no event to wait on: after two
/// seconds of silence the request is given up and made again over IPv4 with no limit, as
/// translate-shell does (its warning, word for word).
#[test]
fn a_silent_server_is_given_up_and_asked_again() {
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = std::sync::Mutex::new(Some(held));
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = asked.clone();
    let srv = testutil::serve(move |_| {
        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // The first request is held until the test has its answer; the second is answered.
        let first = held.lock().unwrap().take();
        if let Some(h) = first {
            let _ = h.recv();
        }
        Resp::new(200, r#"[[["привет","hi"]]]"#)
    });
    let p = plan("en", "ru", "hi").unwrap();
    let (out, err, code) = testutil::capture(|o, e| run(&p, &env(srv.base()), o, e));
    assert_eq!(out, "привет\n", "{err}");
    assert_eq!(
        err,
        "\u{1b}[33m[WARNING] Connection timed out. Retrying IPv4 connection.\u{1b}[0m\n"
    );
    assert_eq!(code, 0);
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 2);
    release.send(()).unwrap();
}

/// `cargo test --features live -p qml --release trans_bench -- --nocapture --test-threads=1`
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn trans_bench() {
    if !trans() {
        return;
    }
    let srv = testutil::serve(|_| {
        let mut r = Resp::new(
            200,
            r#"[[["Привет, мир.","Hello world.",null,null,10]],null,"en"]"#,
        );
        r.chunked = true;
        r
    });
    let px = format!("127.0.0.1:{}", srv.port);
    let p = plan("auto", "ru", "Hello world.").unwrap();
    let e = env(srv.base());
    let live = env(format!("http://{HOST}"));
    let n = 20;
    let mut t = [0.0f64; 12];
    for _ in 0..n {
        let r = testutil::measure_cmd(std::process::Command::new("trans").args([
            "-brief",
            "-no-bidi",
            "-proxy",
            &px,
            "-source",
            "auto",
            "-target",
            "ru",
            "Hello world.",
        ]));
        t[0] += r.0;
        t[1] += r.1;
        let r = testutil::measure_thread(|| {
            testutil::capture(|o, er| run(&p, &e, o, er));
        });
        t[2] += r.0;
        t[3] += r.1;
        let r = testutil::measure_cmd(
            std::process::Command::new("trans").args(["-list-languages", "-no-bidi"]),
        );
        t[4] += r.0;
        t[5] += r.1;
        let r = testutil::measure_thread(|| {
            let _ = languages();
        });
        t[6] += r.0;
        t[7] += r.1;
        let r = testutil::measure_cmd(std::process::Command::new("trans").args([
            "-brief",
            "-no-bidi",
            "-source",
            "auto",
            "-target",
            "ru",
            "Hello world.",
        ]));
        t[8] += r.0;
        t[9] += r.1;
        let r = testutil::measure_thread(|| {
            testutil::capture(|o, er| run(&p, &live, o, er));
        });
        t[10] += r.0;
        t[11] += r.1;
    }
    let m = |i: usize| t[i] / n as f64 * 1e3;
    println!(
        "trans local: original wall {:.2} ms cpu {:.2} ms | rust wall {:.2} ms cpu(thread) {:.2} ms",
        m(0),
        m(1),
        m(2),
        m(3)
    );
    println!(
        "trans -list-languages: original wall {:.2} ms cpu {:.2} ms | rust wall {:.3} ms cpu(thread) {:.3} ms",
        m(4),
        m(5),
        m(6),
        m(7)
    );
    println!(
        "trans live google: original wall {:.2} ms cpu {:.2} ms | rust wall {:.2} ms cpu(thread) {:.2} ms",
        m(8),
        m(9),
        m(10),
        m(11)
    );
}
