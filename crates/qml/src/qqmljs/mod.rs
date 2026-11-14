//! Qt 6.12's QML parser: the tables `qlalr` builds from qqmljs.g, the lexer, the driver
//! with the actions of the grammar, the document they build and the checks of QmlIR on it.

mod build;
#[rustfmt::skip]
mod grammar;
mod ir;
mod lexer;
mod parser;

#[cfg(feature = "interp")]
pub use build::for_async_of;
pub use build::{Failure, parse};

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    /// The committed tables are built from the committed grammar (tools/gen/qqmljs/tables).
    #[test]
    fn tables_are_those_of_the_grammar() {
        let grammar = include_bytes!("../../../../tools/gen/qqmljs/qqmljs.g");
        let hash: String = Sha256::digest(grammar)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hash, super::grammar::QQMLJS_G_SHA256);
    }
}
