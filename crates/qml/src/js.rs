//! JS validation of extracted script spans via oxc.
use crate::ast::{Script, ScriptKind};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

fn parses(src: &str) -> Result<(), String> {
    let mut first = None;
    for st in [SourceType::mjs(), SourceType::ts()] {
        let alloc = Allocator::default();
        let r = Parser::new(&alloc, src, st).parse();
        if r.diagnostics.is_empty() {
            return Ok(());
        }
        first.get_or_insert_with(|| {
            r.diagnostics
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        });
    }
    Err(first.unwrap())
}

/// QML binding values may be expressions, statements (`switch`, `if`) or blocks; QML
/// functions may carry TypeScript-style annotations and `as` casts.
pub fn check(s: &Script) -> Result<(), String> {
    let src = &*crate::qqmljs::for_async_of(&s.src);
    let expr = format!("({src}\n);");
    let body = format!("function __qml() {{\n{src}\n}}");
    let cands: Vec<&str> = match s.kind {
        ScriptKind::Expression => vec![&expr, &body],
        ScriptKind::Block => vec![&body, &expr],
        ScriptKind::Function => vec![src],
    };
    let mut err = None;
    for c in cands {
        match parses(c) {
            Ok(()) => return Ok(()),
            Err(e) => {
                err.get_or_insert(e);
            }
        }
    }
    Err(err.unwrap())
}
