//! Hyprland's socket1 `[[BATCH]]` request format for the tests' fake compositors, as
//! `CSocket1::dispatchBatch` (src/ipc/s1/S1.cpp) and `hyprctl --batch` speak it.

const BATCH_TOKEN: &str = "[[BATCH]]";
/// What `joinBatchReplies` puts between the replies of a batch.
const BATCH_DELIMITER: &str = "\n\n\n";

/// The commands of a `[[BATCH]]` request: split at each unescaped `;` (`\;` and `\\` escape
/// one character, any other `\` stays verbatim) and trimmed, each keeping its own `j/` flags.
pub fn batch_commands(request: &str) -> Option<Vec<String>> {
    let body = request.strip_prefix(BATCH_TOKEN)?;
    let mut commands = Vec::new();
    let mut current = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ';' => commands.push(std::mem::take(&mut current).trim().to_owned()),
            '\\' if matches!(chars.peek(), Some('\\' | ';')) => current.extend(chars.next()),
            _ => current.push(c),
        }
    }
    commands.push(current.trim().to_owned());
    Some(commands)
}

/// The reply to `request`: a batch answers each of its commands with `single`, in order,
/// joined as Hyprland joins them; any other request is one command.
pub fn answer(request: &str, single: impl Fn(&str) -> String) -> String {
    match batch_commands(request) {
        Some(commands) => commands
            .iter()
            .map(|c| single(c))
            .collect::<Vec<_>>()
            .join(BATCH_DELIMITER),
        None => single(request),
    }
}

#[test]
fn batch_commands_split_like_dispatch_batch() {
    assert_eq!(batch_commands("j/monitors"), None);
    assert_eq!(
        batch_commands("[[BATCH]]j/monitors; j/clients ;j/activewindow").unwrap(),
        ["j/monitors", "j/clients", "j/activewindow"]
    );
    assert_eq!(
        batch_commands(r"[[BATCH]]dispatch exec a\;b;eval x\\y;dispatch exec c\d").unwrap(),
        ["dispatch exec a;b", r"eval x\y", r"dispatch exec c\d"]
    );
    assert_eq!(
        batch_commands("[[BATCH]]j/clients;").unwrap(),
        ["j/clients", ""]
    );
}

#[test]
fn answer_joins_batch_replies() {
    let single = |c: &str| format!("<{c}>");
    assert_eq!(answer("j/clients", single), "<j/clients>");
    assert_eq!(
        answer("[[BATCH]]j/monitors;j/clients", single),
        "<j/monitors>\n\n\n<j/clients>"
    );
}
