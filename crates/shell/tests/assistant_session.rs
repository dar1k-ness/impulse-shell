//! The original's assistant chat (services/Assistant.qml, services/assistant/AcpSession.qml) runs the agent
//! set in `sidebar.assistant.command` and talks to it over stdio with the compiled QML code:
//! JSON-RPC replies find their callbacks, `authenticate` unblocks `session/new`, the mode goes
//! through `session/set_mode` and the model through `session/set_config_option`.
//!
//! An agent that opens no session, takes its time to end or dies in the middle of a turn
//! leaves nothing waiting: each prompt gets its error and its end (`responseFinished`).
//!
//! An agent is ended in ACP's order: `session/close` if it closes sessions, and once that is
//! answered its stdin is closed and it gets SIGTERM. While it has not exited the chat has a
//! line that says so; `kill` (the line's button) ends an agent that does not end by itself,
//! and the prompts sent meanwhile go to the agent started after it. The tree `qml` began as
//! did none of this: see docs/original-differences.md.
//!
//! The fake is this test binary (`harness = false`): run through a symlink named
//! `fake-acp-agent`, it is an Agent Client Protocol agent (newline-delimited JSON-RPC 2.0 on
//! stdio, https://agentclientprotocol.com/) that requires one sign-in; its first argument
//! makes it one of the agents that go wrong (`fake_agent`).
use qml::value::Value;
use serde_json::{Value as Json, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const FAKE_NAME: &str = "fake-acp-agent";

/// What a test does with the chat `in_chat` gives it.
type Test = fn(&mut Chat);

const TESTS: &[(&str, Test)] = &[
    ("assistant_chat_streams_reply", assistant_chat_streams_reply),
    (
        "prompt_to_a_program_that_cannot_start_ends_with_an_error",
        prompt_to_a_program_that_cannot_start_ends_with_an_error,
    ),
    (
        "prompt_to_a_program_that_is_no_agent_ends_with_what_it_wrote",
        prompt_to_a_program_that_is_no_agent_ends_with_what_it_wrote,
    ),
    (
        "prompt_to_an_agent_that_exits_cleanly_without_a_session_ends_with_an_error",
        prompt_to_an_agent_that_exits_cleanly_without_a_session_ends_with_an_error,
    ),
    (
        "agent_that_refuses_initialize_is_ended_and_started_again_by_the_next_prompt",
        agent_that_refuses_initialize_is_ended_and_started_again_by_the_next_prompt,
    ),
    (
        "agent_that_refuses_a_session_is_ended_and_started_again_by_the_next_prompt",
        agent_that_refuses_a_session_is_ended_and_started_again_by_the_next_prompt,
    ),
    (
        "answer_keeps_the_name_of_the_agent_that_wrote_it",
        answer_keeps_the_name_of_the_agent_that_wrote_it,
    ),
    (
        "restart_starts_the_new_agent_when_a_slow_one_has_exited",
        restart_starts_the_new_agent_when_a_slow_one_has_exited,
    ),
    (
        "agent_that_does_not_end_when_asked_is_killed_by_the_button",
        agent_that_does_not_end_when_asked_is_killed_by_the_button,
    ),
    (
        "agent_that_closes_sessions_has_its_session_closed_before_it_is_ended",
        agent_that_closes_sessions_has_its_session_closed_before_it_is_ended,
    ),
    (
        "agent_is_not_ended_before_it_answers_session_close",
        agent_is_not_ended_before_it_answers_session_close,
    ),
    (
        "session_opened_by_an_agent_asked_to_end_takes_no_prompt",
        session_opened_by_an_agent_asked_to_end_takes_no_prompt,
    ),
    (
        "agent_killed_in_a_turn_ends_the_turn",
        agent_killed_in_a_turn_ends_the_turn,
    ),
];

fn main() {
    let mut argv = std::env::args_os();
    let argv0 = argv.next().map(PathBuf::from).unwrap_or_default();
    if argv0.file_name().is_some_and(|n| n == FAKE_NAME) {
        let role = argv.next().unwrap_or_default();
        return fake_agent(&role.to_string_lossy(), argv.next().map(PathBuf::from));
    }
    // libtest's command line and output (filters, `--exact`, `--skip`, `--list`, options with
    // values), as cargo test, cargo-nextest
    // (https://nexte.st/docs/design/custom-test-harnesses/) and `testenv::run_in_own_process`
    // (which reads the summary line) use them.
    let args = libtest_mimic::Arguments::from_args();
    let tests = TESTS
        .iter()
        .map(|&(name, test)| {
            libtest_mimic::Trial::test(name, move || {
                in_chat(name, test);
                Ok(())
            })
        })
        .collect();
    libtest_mimic::run(&args, tests).exit();
}

/// The agent `role` makes of the fake, a `+`-separated list of:
/// - none: one that requires a sign-in and answers each prompt;
/// - `not-acp`: a program that prints its usage and exits with 0, no agent at all;
/// - `quit`: one that reads `initialize` and exits with 0;
/// - `refuse-initialize`, `refuse-session`: one that answers that request with an error and
///   stays;
/// - `second`: one that names itself "Second Agent", not "Fake Agent";
/// - `slow-exit`: one that ignores SIGTERM and the end of its stdin, and exits once it has
///   read that end and the FIFO `held` is released;
/// - `closes`: one that closes sessions (`sessionCapabilities.close`);
/// - `close-unanswered`: one that says so and leaves `session/close` unanswered;
/// - `late-session`: one that answers `session/new` when its stdin has ended;
/// - `killed`: one that is killed in its first turn, with a message begun, a tool running
///   and a permission asked for.
///
/// Each method it reads, and `eof` for the end of its stdin, is a line `<role>: <method>` of
/// `$HOME/seen` ([`seen`]), written before it answers, in one `write`: SIGTERM ends an agent
/// with the whole line written or none of it (`writeln!` on a file writes the line in pieces,
/// and the next agent's first line then continued a cut `closes: `).
fn fake_agent(role: &str, held: Option<PathBuf>) {
    let is = |trait_: &str| role.split('+').any(|t| t == trait_);
    if is("not-acp") {
        println!("usage: {FAKE_NAME} FILE");
        return;
    }
    let saw = |what: &str| {
        let log = PathBuf::from(std::env::var_os("HOME").expect("HOME")).join("seen");
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .unwrap();
        log.write_all(format!("{role}: {what}\n").as_bytes())
            .unwrap();
    };
    let mut out = std::io::stdout().lock();
    let input = std::io::stdin().lock();
    let sid = format!("sess-{}", std::process::id());
    let mut signed_in = !role.is_empty();
    let mut mode = "ask";
    let mut model = "m1";
    let mut send = |o: Json| {
        writeln!(out, "{o}").unwrap();
        out.flush().unwrap();
    };
    let (stdin_ended, after_stdin) = std::sync::mpsc::channel::<()>();
    if is("slow-exit") {
        let held = held.expect("slow-exit FIFO");
        // SAFETY: SIG_IGN is a disposition the kernel applies; no handler of ours runs.
        unsafe { libc::signal(libc::SIGTERM, libc::SIG_IGN) };
        std::thread::spawn(move || {
            // Opening blocks until the test opens the other end (`release_fifo`).
            let _ = std::fs::File::open(held);
            let _ = after_stdin.recv();
            std::process::exit(0);
        });
    }
    let title = if is("second") {
        "Second Agent"
    } else {
        "Fake Agent"
    };
    let mut late_session = None;
    for line in input.lines() {
        let m: Json = serde_json::from_str(&line.unwrap()).expect("JSON-RPC message");
        let id = m.get("id").cloned().unwrap_or(Json::Null);
        saw(m["method"].as_str().unwrap_or("an answer"));
        let reply = |result: Json| json!({"jsonrpc": "2.0", "id": id, "result": result});
        let models = |current: &str| {
            json!([{"id": "model", "name": "Model", "category": "model", "type": "select",
                "currentValue": current, "options": [
                    {"value": "m1", "name": "Model One"}, {"value": "m2", "name": "Model Two"}]}])
        };
        let refuse = |message: &str| {
            json!({"jsonrpc": "2.0", "id": id,
            "error": {"code": -32603, "message": message}})
        };
        let update = |update: Json| {
            json!({"jsonrpc": "2.0", "method": "session/update",
            "params": {"sessionId": sid, "update": update}})
        };
        match m["method"].as_str() {
            Some("initialize") if is("quit") => return,
            Some("initialize") if is("refuse-initialize") => send(refuse("no protocol")),
            Some("session/new") if is("refuse-session") => send(refuse("no sessions")),
            Some("session/new") if is("late-session") => late_session = Some(id),
            Some("session/close") if is("close-unanswered") => {}
            Some("initialize") if is("closes") || is("close-unanswered") => {
                send(reply(json!({"protocolVersion": 1,
                    "agentInfo": {"name": "fake", "title": title},
                    "agentCapabilities": {"sessionCapabilities": {"close": {}}}})))
            }
            Some("session/prompt") if is("killed") => {
                send(update(json!({"sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "Starting"}})));
                send(update(
                    json!({"sessionUpdate": "tool_call", "toolCallId": "t1",
                    "title": "Build", "kind": "execute", "status": "in_progress"}),
                ));
                send(json!({"jsonrpc": "2.0", "id": "ask-1",
                    "method": "session/request_permission", "params": {"sessionId": sid,
                        "toolCall": {"toolCallId": "t2", "title": "Deploy"},
                        "options": [{"optionId": "yes", "name": "Allow", "kind": "allow_once"}]}}));
                // SAFETY: raise(3) takes a plain integer; SIGKILL ends this process here.
                unsafe { libc::raise(libc::SIGKILL) };
            }
            Some("initialize") => send(reply(json!({"protocolVersion": 1,
                "agentInfo": {"name": "fake", "title": title},
                "authMethods": [{"id": "fake-login", "name": "Sign in"}]}))),
            Some("authenticate") => {
                assert_eq!(m["params"]["methodId"], "fake-login");
                signed_in = true;
                send(reply(json!({})));
            }
            Some("session/new") if !signed_in => send(json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32000, "message": "Authentication required"}})),
            Some("session/new") => send(reply(json!({
                "sessionId": sid,
                "modes": {"currentModeId": mode, "availableModes": [
                    {"id": "ask", "name": "Ask"}, {"id": "code", "name": "Code"}]},
                "configOptions": models(model)}))),
            Some("session/set_mode") => {
                mode = if m["params"]["modeId"] == "code" {
                    "code"
                } else {
                    "ask"
                };
                send(reply(json!({})));
            }
            Some("session/set_config_option") => {
                assert_eq!(m["params"]["configId"], "model");
                model = if m["params"]["value"] == "m2" {
                    "m2"
                } else {
                    "m1"
                };
                send(reply(json!({"configOptions": models(model)})));
            }
            Some("session/prompt") => {
                let text = m["params"]["prompt"][0]["text"]
                    .as_str()
                    .unwrap_or_default();
                for chunk in ["Hello ", &format!("from {mode}/{model}: "), text] {
                    send(update(json!({"sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": chunk}})));
                }
                send(reply(json!({"stopReason": "end_turn"})));
            }
            _ if !id.is_null() => send(reply(json!({}))),
            _ => {}
        }
    }
    saw("eof");
    if let Some(id) = late_session {
        send(json!({"jsonrpc": "2.0", "id": id, "result": {"sessionId": sid}}));
    }
    if is("slow-exit") {
        stdin_ended.send(()).unwrap();
        loop {
            std::thread::park();
        }
    }
}

/// What the fake in `role` read, in the order it read it ([`fake_agent`]).
fn seen(chat: &Chat, role: &str) -> Vec<String> {
    let role = format!("{role}: ");
    std::fs::read_to_string(chat.home.join("seen"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix(&role).map(str::to_string))
        .collect()
}

fn item(v: Value) -> usize {
    match v {
        Value::Item(i) => i,
        v => panic!("not an item: {v:?}"),
    }
}

fn message_items(rt: &qml::runtime::Runtime, s: usize) -> Vec<usize> {
    let Value::Obj(o) = rt.get(s, "messages") else {
        return vec![];
    };
    let a = o.borrow().array.clone().unwrap_or_default();
    a.into_iter().map(item).collect()
}

fn messages(rt: &qml::runtime::Runtime, s: usize) -> Vec<(String, String)> {
    message_items(rt, s)
        .into_iter()
        .map(|m| {
            (
                rt.get(m, "role").to_js_string(),
                rt.get(m, "content").to_js_string(),
            )
        })
        .collect()
}

/// The chat's `interface` lines: what it tells the user about the agent.
fn notices(rt: &qml::runtime::Runtime, s: usize) -> Vec<String> {
    messages(rt, s)
        .into_iter()
        .filter(|(role, _)| role == "interface")
        .map(|(_, text)| text)
        .collect()
}

fn queued(rt: &qml::runtime::Runtime, s: usize) -> usize {
    match rt.get(s, "pendingPrompts") {
        Value::Obj(o) => o.borrow().array.as_ref().map_or(0, Vec::len),
        _ => 0,
    }
}

thread_local! {
    /// `responseFinished` emissions of the session: the ends of its prompts.
    static ENDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn ends() -> usize {
    ENDS.get()
}

/// The shell's loop around the Assistant singleton of the compiled QML tree, in a process of
/// its own with a home whose config names the fake as the agent.
struct Chat {
    rt: qml::runtime::Runtime,
    /// Assistant.session, the AcpSession.
    s: usize,
    fake: PathBuf,
    home: PathBuf,
    wake: testenv::Wakeups,
    ep: Option<std::os::fd::RawFd>,
    clock: Instant,
    /// What the test added to the loop's clock (`pass`), in ms.
    passed: f64,
}

impl Chat {
    fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64() * 1000.0 + self.passed
    }

    /// The shell's loop: a turn per wakeup (pool jobs, the agent's output and exit on the
    /// epoll set) and per timer, sleeping in poll(2) until the next of them. Each state
    /// `trace` reports on the way goes to the test's captured output, so a run that hangs
    /// (nextest's slow-timeout ends it) shows where it stopped.
    fn pump(
        &mut self,
        trace: &dyn Fn(&qml::runtime::Runtime) -> String,
        done: &dyn Fn(&qml::runtime::Runtime) -> bool,
    ) {
        let mut last = String::new();
        loop {
            let now = self.now();
            let rt = &mut self.rt;
            rt.poll_native();
            rt.tick(now);
            rt.poll_native();
            rt.settle();
            if done(rt) {
                return;
            }
            let state = trace(rt);
            if state != last {
                eprintln!("waiting: {state}");
                last = state;
            }
            let next = rt.next_deadline(now);
            let woke = self.wake.wait(
                self.ep,
                next.is_finite()
                    .then(|| Duration::from_secs_f64(next / 1000.0)),
            );
            if woke.fd {
                rt.process_fd_ready();
            }
        }
    }

    /// `ms` of the loop's clock pass at once: the timers due in them run.
    fn pass(&mut self, ms: f64) {
        self.passed += ms;
        let now = self.now();
        self.rt.tick(now);
        self.rt.settle();
    }

    /// Makes `command` the agent, as a config reload does.
    fn agent(&mut self, command: &[&str]) {
        let command = command.iter().map(|a| Value::str(a)).collect();
        self.rt.set(self.s, "command", Value::array(command));
    }

    /// The fake in `role` as the agent.
    fn fake(&mut self, role: &str) {
        let fake = self.fake.to_string_lossy().into_owned();
        self.agent(&[&fake, role]);
    }

    fn call(&mut self, method: &str, arg: &str) {
        self.rt
            .call_method(self.s, method, vec![Value::str(arg)])
            .expect(method)
            .unwrap();
    }

    fn get(&self, name: &str) -> Value {
        self.rt.get(self.s, name)
    }

    fn notices(&self) -> Vec<String> {
        notices(&self.rt, self.s)
    }

    /// Runs the loop until the agent's process has exited.
    fn until_exited(&mut self) {
        let s = self.s;
        self.pump(
            &|rt| format!("the agent's exit: {:?}", messages(rt, s)),
            &|rt| !rt.get(s, "running").truthy(),
        );
    }

    /// Runs the loop until the chat shows `count` interface lines.
    fn until_notices(&mut self, count: usize) {
        let s = self.s;
        self.pump(
            &|rt| format!("{count} interface lines: {:?}", messages(rt, s)),
            &|rt| notices(rt, s).len() >= count,
        );
    }

    /// The prompt `text`, queued for a session that did not open, got its error and its end:
    /// the chat's last line is `notice`, nothing is left queued and `ends` prompts ended.
    #[track_caller]
    fn assert_ended_with(&self, notice: &str, ends_wanted: usize) {
        let all = messages(&self.rt, self.s);
        assert_eq!(
            all.last(),
            Some(&("interface".to_string(), notice.to_string())),
            "the chat's last line, of {all:?}"
        );
        assert_eq!(queued(&self.rt, self.s), 0, "prompts left queued: {all:?}");
        assert_eq!(ends(), ends_wanted, "responseFinished emissions: {all:?}");
        assert!(!self.get("busy").truthy(), "still busy: {all:?}");
    }
}

/// Runs `test` on the chat of a process of its own.
fn in_chat(name: &str, test: Test) {
    let test_id = format!("{}::{name}", module_path!());
    if !testenv::in_own_process(&test_id) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let config_dir = home.join(".config/rusty-impulse");
        std::fs::create_dir_all(&config_dir).unwrap();
        let fake = home.join(FAKE_NAME);
        std::os::unix::fs::symlink(std::env::current_exe().unwrap(), &fake).unwrap();
        let config = json!({"sidebar": {"assistant": {"command": [fake.to_string_lossy()]}}});
        std::fs::write(config_dir.join("config.json"), config.to_string()).unwrap();
        testenv::run_in_own_process(&test_id, testenv::home_vars(home));
        return;
    }
    let root = PathBuf::from(ricompiled::ROOT);

    let mut rt = qml::runtime::Runtime::new(Some(root.clone()));
    rt.eng()
        .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&root))));
    let dirs: Vec<PathBuf> = [
        "modules/common",
        "modules/common/functions",
        "modules/common/utils",
        "services",
        "services/assistant",
    ]
    .iter()
    .map(|d| root.join(d))
    .collect();
    rt.load_singletons_where(&dirs, &|n| {
        matches!(
            n,
            "Assistant" | "Persistent" | "Directories" | "Config" | "FileUtils"
        )
    });
    let assistant = *rt
        .world
        .borrow()
        .singletons
        .get("Assistant")
        .expect("Assistant");
    let s = item(rt.get(assistant, "session"));
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let mut chat = Chat {
        rt,
        s,
        fake: home.join(FAKE_NAME),
        home,
        wake,
        ep,
        clock: Instant::now(),
        passed: 0.0,
    };
    let config = *chat
        .rt
        .world
        .borrow()
        .singletons
        .get("Config")
        .expect("Config");
    chat.pump(
        &|rt| {
            format!(
                "{:?} (ready={:?}) gave no sidebar.assistant.command",
                rt.get(config, "filePath"),
                rt.get(config, "ready")
            )
        },
        &|rt| rt.get(assistant, "configured").truthy(),
    );
    let ended = qml::builtins::native("ended", 0, |_, _, _| {
        ENDS.set(ENDS.get() + 1);
        Ok(Value::Undefined)
    });
    assert!(
        chat.rt
            .eng()
            .it
            .host
            .connect(s, "responseFinished", Value::Undefined, ended, true),
        "AcpSession.responseFinished is no signal"
    );
    test(&mut chat);
}

fn assistant_chat_streams_reply(chat: &mut Chat) {
    let s = chat.s;
    chat.call("sendPrompt", "task one");
    chat.pump(
        &|rt| format!("session/new's auth error: {:?}", messages(rt, s)),
        &|rt| rt.get(s, "authRequired").truthy(),
    );
    assert_eq!(chat.get("agentName").to_js_string(), "Fake Agent");

    chat.call("authenticate", "fake-login");
    let reply = |want: &'static str| {
        move |rt: &qml::runtime::Runtime| {
            messages(rt, s)
                .iter()
                .any(|(r, c)| r == "assistant" && c == want)
                && !rt.get(s, "busy").truthy()
        }
    };
    chat.pump(
        &|rt| {
            format!(
                "queued prompt after authenticate: {:?} session={:?}",
                messages(rt, s),
                rt.get(s, "sessionId")
            )
        },
        &reply("Hello from ask/m1: task one"),
    );
    assert!(!chat.get("authRequired").truthy());

    chat.call("setMode", "code");
    chat.call("setModel", "m2");
    chat.pump(
        &|rt| {
            format!(
                "mode={:?} model={:?}: {:?}",
                rt.get(s, "currentMode"),
                rt.get(s, "currentModel"),
                messages(rt, s)
            )
        },
        &|rt| {
            rt.get(s, "currentMode").to_js_string() == "code"
                && rt.get(s, "currentModel").to_js_string() == "m2"
        },
    );
    chat.call("sendPrompt", "task two");
    chat.pump(
        &|rt| format!("second prompt: {:?}", messages(rt, s)),
        &reply("Hello from code/m2: task two"),
    );
    // What the user sees of a session where nothing went wrong: the prompts, their replies
    // and no line about the agent; each prompt ended once.
    assert_eq!(
        messages(&chat.rt, s),
        [
            ("user", "task one"),
            ("assistant", "Hello from ask/m1: task one"),
            ("user", "task two"),
            ("assistant", "Hello from code/m2: task two"),
        ]
        .map(|(role, text)| (role.to_string(), text.to_string()))
    );
    assert_eq!(ends(), 2);
}

/// A command that names no program: Process sends no `exited` for it (it never ran).
fn prompt_to_a_program_that_cannot_start_ends_with_an_error(chat: &mut Chat) {
    let missing = chat.home.join("no-such-agent");
    chat.agent(&[&missing.to_string_lossy()]);
    chat.call("sendPrompt", "task");
    assert!(!chat.get("running").truthy());
    chat.assert_ended_with("no-such-agent failed to start", 1);
}

/// A program that does not speak ACP: what it wrote is no JSON-RPC message.
fn prompt_to_a_program_that_is_no_agent_ends_with_what_it_wrote(chat: &mut Chat) {
    chat.fake("not-acp");
    chat.call("sendPrompt", "task");
    chat.until_exited();
    chat.assert_ended_with(
        &format!("{FAKE_NAME} exited (code 0): usage: {FAKE_NAME} FILE"),
        1,
    );
}

fn prompt_to_an_agent_that_exits_cleanly_without_a_session_ends_with_an_error(chat: &mut Chat) {
    chat.fake("quit");
    chat.call("sendPrompt", "task");
    chat.until_exited();
    chat.assert_ended_with(&format!("{FAKE_NAME} exited (code 0)"), 1);
}

/// `refused` is the chat's line for the request the fake in `role` answers with an error.
/// The agent stays alive after it, so it is ended; a prompt sent while it is still ending
/// starts it again from its exit instead of waiting in the queue.
fn refusing_agent_is_ended_and_started_again(chat: &mut Chat, role: &str, refused: &str) {
    chat.fake(role);
    chat.call("sendPrompt", "task one");
    chat.until_notices(1);
    chat.assert_ended_with(refused, 1);

    chat.call("sendPrompt", "task two");
    chat.until_notices(2);
    chat.assert_ended_with(refused, 2);
    chat.until_exited();
    assert_eq!(chat.notices(), [refused, refused]);
}

fn agent_that_refuses_initialize_is_ended_and_started_again_by_the_next_prompt(chat: &mut Chat) {
    refusing_agent_is_ended_and_started_again(chat, "refuse-initialize", "initialize: no protocol");
}

fn agent_that_refuses_a_session_is_ended_and_started_again_by_the_next_prompt(chat: &mut Chat) {
    refusing_agent_is_ended_and_started_again(chat, "refuse-session", "no sessions");
}

/// Runs the loop until a session is open; its id.
fn until_session(chat: &mut Chat) -> String {
    let s = chat.s;
    chat.pump(
        &|rt| {
            format!(
                "a session: running={:?} {:?}",
                rt.get(s, "running"),
                messages(rt, s)
            )
        },
        &|rt| !rt.get(s, "sessionId").to_js_string().is_empty(),
    );
    chat.get("sessionId").to_js_string()
}

/// The chat's lines, each its role and its text.
fn lines(chat: &Chat) -> Vec<(String, String)> {
    messages(&chat.rt, chat.s)
}

fn pairs<const N: usize>(want: [(&str, &str); N]) -> Vec<(String, String)> {
    want.map(|(role, text)| (role.to_string(), text.to_string()))
        .to_vec()
}

/// The user's prompts the chat marks as waiting for a session.
fn marked_queued(chat: &Chat) -> Vec<String> {
    message_items(&chat.rt, chat.s)
        .into_iter()
        .filter(|&m| chat.rt.get(m, "queued").truthy())
        .map(|m| chat.rt.get(m, "content").to_js_string())
        .collect()
}

/// The fake in `role`, holding the FIFO `held`, as the agent with a session open; the
/// session's id.
fn held_agent(chat: &mut Chat, role: &str) -> (PathBuf, String) {
    let held = chat.home.join("held");
    testenv::fifo(&held);
    let fake = chat.fake.to_string_lossy().into_owned();
    chat.agent(&[&fake, role, &held.to_string_lossy()]);
    chat.call("start", "");
    (held, until_session(chat))
}

/// Another agent is put in the config while one runs, and a prompt is sent: the chat's only
/// line names the agent it waits for (the header reads `waitingForExit` and
/// `startedProgram`), and the prompt waits in the queue, marked as waiting.
fn replaced_and_prompted(chat: &mut Chat) {
    chat.fake("second");
    chat.pass(0.0);
    assert_eq!(lines(chat), pairs([("ending", FAKE_NAME)]));
    assert!(chat.get("waitingForExit").truthy());
    assert_eq!(chat.get("startedProgram").to_js_string(), FAKE_NAME);
    chat.call("sendPrompt", "task");
    assert_eq!(
        lines(chat),
        pairs([("ending", FAKE_NAME), ("user", "task")])
    );
    assert_eq!(marked_queued(chat), ["task"]);
    assert_eq!(queued(&chat.rt, chat.s), 1);
}

/// The agent after the one with the session `old` has a session and answered the prompt
/// sent while that one was waited for: the line about it is gone, nothing is marked.
fn new_agent_answered(chat: &mut Chat, old: &str) {
    let s = chat.s;
    let old = old.to_string();
    chat.pump(
        &|rt| format!("the new agent's reply: {:?}", messages(rt, s)),
        &|rt| {
            let sid = rt.get(s, "sessionId").to_js_string();
            !sid.is_empty() && sid != old && ends() == 1 && !rt.get(s, "busy").truthy()
        },
    );
    assert_eq!(
        lines(chat),
        pairs([("user", "task"), ("assistant", "Hello from ask/m1: task")])
    );
    assert!(marked_queued(chat).is_empty());
    assert!(!chat.get("waitingForExit").truthy());
    assert_eq!(ends(), 1, "responseFinished emissions");
}

/// `running = false` only sends SIGTERM. The agent here takes a second of the loop's clock
/// to end after it: the chat's line about it is there for that second and goes when it has
/// exited by itself, however long that took, and the new agent starts then and answers the
/// prompt sent in that second. It does not close sessions, so it is sent no `session/close`.
fn restart_starts_the_new_agent_when_a_slow_one_has_exited(chat: &mut Chat) {
    let (held, old) = held_agent(chat, "slow-exit");
    replaced_and_prompted(chat);
    chat.pass(1000.0);
    assert!(chat.get("running").truthy(), "the slow agent has ended");
    assert_eq!(
        lines(chat),
        pairs([("ending", FAKE_NAME), ("user", "task")])
    );

    testenv::release_fifo(&held);
    new_agent_answered(chat, &old);
    assert_eq!(
        seen(chat, "slow-exit"),
        ["initialize", "session/new", "eof"]
    );
}

/// An answer is headed by the agent that wrote it (`agent` of its message, which
/// AssistantMessage.qml shows): the agent put in the config after it names its own answers only.
fn answer_keeps_the_name_of_the_agent_that_wrote_it(chat: &mut Chat) {
    let s = chat.s;
    let answered = |want: &'static str| {
        move |rt: &qml::runtime::Runtime| {
            messages(rt, s)
                .iter()
                .any(|(r, c)| r == "assistant" && c == want)
                && !rt.get(s, "busy").truthy()
        }
    };
    chat.fake("first");
    chat.call("sendPrompt", "one");
    chat.pump(
        &|rt| format!("the first agent's reply: {:?}", messages(rt, s)),
        &answered("Hello from ask/m1: one"),
    );
    chat.fake("second");
    chat.call("sendPrompt", "two");
    chat.pump(
        &|rt| format!("the second agent's reply: {:?}", messages(rt, s)),
        &answered("Hello from ask/m1: two"),
    );
    assert_eq!(chat.get("agentName").to_js_string(), "Second Agent");
    let writers: Vec<(String, String)> = message_items(&chat.rt, s)
        .into_iter()
        .filter(|&m| chat.rt.get(m, "role").to_js_string() == "assistant")
        .map(|m| {
            (
                chat.rt.get(m, "content").to_js_string(),
                chat.rt.get(m, "agent").to_js_string(),
            )
        })
        .collect();
    assert_eq!(
        writers,
        pairs([
            ("Hello from ask/m1: one", "Fake Agent"),
            ("Hello from ask/m1: two", "Second Agent")
        ])
    );
}

/// An agent that ignores SIGTERM and never exits: `kill`, the button of the chat's line,
/// ends it, and its exit starts the new agent, which answers the prompt that waited.
fn agent_that_does_not_end_when_asked_is_killed_by_the_button(chat: &mut Chat) {
    let (_held, old) = held_agent(chat, "slow-exit");
    replaced_and_prompted(chat);
    chat.pass(1000.0);
    assert!(chat.get("running").truthy(), "the agent has ended");

    chat.call("kill", "");
    new_agent_answered(chat, &old);
}

/// An agent that closes sessions is sent `session/close` for its session, and exits by
/// itself once its stdin is closed after its answer.
fn agent_that_closes_sessions_has_its_session_closed_before_it_is_ended(chat: &mut Chat) {
    chat.fake("closes");
    chat.call("start", "");
    let old = until_session(chat);
    replaced_and_prompted(chat);
    new_agent_answered(chat, &old);
    let seen = seen(chat, "closes");
    assert_eq!(seen[..3], ["initialize", "session/new", "session/close"]);
    // SIGTERM may end it before it reads the end of its stdin.
    assert!(seen[3..].iter().all(|m| m == "eof"), "{seen:?}");
}

/// The answer to `session/close` is waited for: until it comes the agent's stdin is open
/// (it reads the `authenticate` sent after `session/close`) and it has no SIGTERM (it would
/// exit on one). This agent never answers, so only `kill` ends it.
fn agent_is_not_ended_before_it_answers_session_close(chat: &mut Chat) {
    chat.fake("close-unanswered");
    chat.call("start", "");
    let old = until_session(chat);
    replaced_and_prompted(chat);

    chat.call("authenticate", "fake-login");
    let s = chat.s;
    let log = chat.home.join("seen");
    chat.pump(
        &|rt| format!("the agent's answer to authenticate: {:?}", messages(rt, s)),
        &|rt| {
            let read = std::fs::read_to_string(&log).unwrap_or_default();
            read.contains(": authenticate") || !rt.get(s, "waitingForExit").truthy()
        },
    );
    assert_eq!(
        seen(chat, "close-unanswered"),
        ["initialize", "session/new", "session/close", "authenticate"]
    );
    assert!(chat.get("waitingForExit").truthy());
    assert_eq!(
        lines(chat),
        pairs([("ending", FAKE_NAME), ("user", "task")])
    );

    chat.call("kill", "");
    new_agent_answered(chat, &old);
}

/// The agent is asked to end before it has answered `session/new`, and answers it then:
/// that session is not the chat's, so the prompt in the queue is kept for the new agent.
fn session_opened_by_an_agent_asked_to_end_takes_no_prompt(chat: &mut Chat) {
    let held = chat.home.join("held");
    testenv::fifo(&held);
    let fake = chat.fake.to_string_lossy().into_owned();
    chat.agent(&[&fake, "slow-exit+late-session", &held.to_string_lossy()]);
    chat.call("start", "");
    let s = chat.s;
    chat.pump(
        &|rt| format!("the answer to initialize: {:?}", messages(rt, s)),
        &|rt| rt.get(s, "initialized").truthy(),
    );
    replaced_and_prompted(chat);

    // The agent exits when it has read the end of its stdin and answered `session/new`.
    testenv::release_fifo(&held);
    new_agent_answered(chat, "");
    assert_eq!(
        seen(chat, "slow-exit+late-session"),
        ["initialize", "session/new", "eof"]
    );
}

/// The agent is killed while it answers a prompt: the request it left unanswered gets its
/// answer, so the turn ends as one the agent ended does.
fn agent_killed_in_a_turn_ends_the_turn(chat: &mut Chat) {
    chat.fake("killed");
    chat.call("sendPrompt", "task");
    chat.until_exited();

    let s = chat.s;
    let shown: Vec<(String, String, bool)> = message_items(&chat.rt, s)
        .into_iter()
        .map(|m| {
            let get = |name: &str| chat.rt.get(m, name).to_js_string();
            let text = match get("role").as_str() {
                "tool" => format!("{} {}", get("title"), get("status")),
                "permission" => format!("{} {}", get("title"), get("resolved")),
                _ => get("content"),
            };
            (get("role"), text, chat.rt.get(m, "done").truthy())
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("user", "task", true),
            ("assistant", "Starting", true),
            ("tool", "Build failed", true),
            ("permission", "Deploy Cancelled", true),
            ("interface", "fake-acp-agent exited (code 9)", true),
        ]
        .map(|(role, text, done)| (role.to_string(), text.to_string(), done))
    );
    assert_eq!(ends(), 1, "responseFinished emissions");
    assert!(!chat.get("busy").truthy());
    assert!(!chat.get("authRequired").truthy());
}
