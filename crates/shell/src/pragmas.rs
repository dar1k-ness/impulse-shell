//! The environment pragmas of a root QML file, read as Quickshell's `launch()` reads them
//! (src/launch/launch.cpp).

/// What the header of a root file says about the shell's own environment.
#[derive(Debug, Default, PartialEq)]
pub struct Env {
    /// `//@ pragma Env NAME=VALUE`, the last line for a name.
    pub overrides: Vec<(String, String)>,
    /// `//@ pragma DefaultEnv NAME=VALUE`, the last line for a name.
    pub defaults: Vec<(String, String)>,
    /// `//@ pragma RespectSystemStyle`.
    pub system_style: bool,
    /// `//@ pragma NativeTextRendering`: a text that sets no render type is native.
    pub native_text: bool,
}

fn put(vars: &mut Vec<(String, String)>, content: &str) {
    let Some((name, value)) = content.split_once('=') else {
        return;
    };
    let (name, value) = (name.trim(), value.trim());
    // A name setenv(3) refuses; Quickshell's qputenv fails on it the same.
    if name.is_empty() || name.contains('\0') || value.contains('\0') {
        return;
    }
    vars.retain(|(n, _)| n != name);
    vars.push((name.to_string(), value.to_string()));
}

impl Env {
    /// The pragmas of `src`: its `//@ pragma ` lines up to the first line that starts with
    /// `import`. As Quickshell, which looks at nothing else in a line: a block comment, a
    /// QML `pragma` or any other text before that line does not end the reading, and a
    /// pragma line inside a block comment is read.
    pub fn parse(src: &str) -> Env {
        let mut env = Env::default();
        for line in src.lines().map(str::trim) {
            if let Some(pragma) = line.strip_prefix("//@ pragma ") {
                let pragma = pragma.trim();
                if let Some(content) = pragma.strip_prefix("Env ") {
                    put(&mut env.overrides, content);
                } else if let Some(content) = pragma.strip_prefix("DefaultEnv ") {
                    put(&mut env.defaults, content);
                } else if pragma == "RespectSystemStyle" {
                    env.system_style = true;
                } else if pragma == "NativeTextRendering" {
                    env.native_text = true;
                }
            } else if line.starts_with("import") {
                break;
            }
        }
        env
    }

    /// The changes to make to the environment, in order: a name with its value, or `None`
    /// to remove it. `is_set` tells whether the environment has a variable before any of
    /// them. Quickshell's order: the style variables, the defaults for what is still unset
    /// after those, the overrides.
    pub fn changes(&self, is_set: impl Fn(&str) -> bool) -> Vec<(&str, Option<&str>)> {
        let mut out: Vec<(&str, Option<&str>)> = Vec::new();
        if !self.system_style {
            out.push(("QT_STYLE_OVERRIDE", None));
            out.push(("QT_QUICK_CONTROLS_STYLE", Some("Fusion")));
        }
        for (name, value) in &self.defaults {
            let set = match out.iter().find(|(n, _)| n == name) {
                Some((_, v)) => v.is_some(),
                None => is_set(name),
            };
            if !set {
                out.push((name, Some(value)));
            }
        }
        for (name, value) in &self.overrides {
            out.push((name, Some(value)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn env_and_default_env_lines_are_read_from_the_header() {
        let env = Env::parse(
            "//@ pragma Env A=1\n  //@ pragma DefaultEnv  B = two words \n\n// note\n\
             //@ pragma Env A=2\n//@ pragma Env NOVALUE\n//@ pragma Env =x\n\
             //@pragma Env C=1\n////@ pragma Env D=1\nimport QtQuick\n//@ pragma Env E=1\n",
        );
        assert_eq!(env.overrides, pairs(&[("A", "2")]));
        assert_eq!(env.defaults, pairs(&[("B", "two words")]));
        assert!(!env.system_style);
        assert!(Env::parse("//@ pragma RespectSystemStyle\n").system_style);
    }

    /// Only a line that starts with `import` ends the reading: Quickshell does not parse
    /// the comments, so a pragma line after or inside a block comment counts.
    #[test]
    fn the_pragmas_are_read_up_to_the_first_import() {
        let env = Env::parse(
            "/*\n * header\n//@ pragma Env INSIDE=1\n */\n//@ pragma Env AFTER_BLOCK=1\n\
             /* one line */ //@ pragma Env SAME_LINE=1\npragma Singleton\n\
             //@ pragma Env AFTER_PRAGMA=1\nanything {\n//@ pragma Env AFTER_TEXT=1\n\
             \timport QtQuick\n//@ pragma Env AFTER_IMPORT=1\n",
        );
        assert_eq!(
            env.overrides,
            pairs(&[
                ("INSIDE", "1"),
                ("AFTER_BLOCK", "1"),
                ("AFTER_PRAGMA", "1"),
                ("AFTER_TEXT", "1")
            ])
        );
        // `importer` starts with `import` too.
        let env = Env::parse("importer\n//@ pragma Env A=1\n");
        assert_eq!(env.overrides, []);
    }

    #[test]
    fn the_style_goes_first_then_defaults_for_what_is_unset_then_overrides() {
        let env = Env::parse(
            "//@ pragma DefaultEnv SET=d\n//@ pragma DefaultEnv UNSET=d\n\
             //@ pragma DefaultEnv QT_STYLE_OVERRIDE=d\n\
             //@ pragma DefaultEnv QT_QUICK_CONTROLS_STYLE=d\n//@ pragma Env SET=o\n",
        );
        // QT_STYLE_OVERRIDE was just removed, so its default applies; the controls style was
        // just set, so its default does not.
        assert_eq!(
            env.changes(|n| n == "SET" || n == "QT_STYLE_OVERRIDE"),
            [
                ("QT_STYLE_OVERRIDE", None),
                ("QT_QUICK_CONTROLS_STYLE", Some("Fusion")),
                ("UNSET", Some("d")),
                ("QT_STYLE_OVERRIDE", Some("d")),
                ("SET", Some("o")),
            ]
        );
        let env = Env::parse("//@ pragma RespectSystemStyle\n//@ pragma DefaultEnv A=d\n");
        assert_eq!(env.changes(|_| false), [("A", Some("d"))]);
        assert_eq!(env.changes(|_| true), []);
    }
}
