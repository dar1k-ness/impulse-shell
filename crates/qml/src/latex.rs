//! The `MicroTexRender` type (crate::procs) that LatexRenderer.qml creates per expression: it
//! runs MicroTeX from its directory without bash, with the expression verbatim (the original's
//! `bash -c` quoting dropped every `'` and an odd count broke the script), and exits 0 only
//! when the image was made.
use crate::jobs::Job;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::Stdio;

/// MicroTeX's arguments, after `./LaTeX`.
pub fn microtex_args(
    expr: &str,
    image: &str,
    textsize: &str,
    pad: &str,
    color: &str,
) -> Vec<String> {
    vec![
        "-headless".into(),
        format!("-input={expr}"),
        format!("-output={image}"),
        format!("-textsize={textsize}"),
        format!("-padding={pad}"),
        format!("-foreground={color}"),
        "-maxwidth=0.85".into(),
    ]
}

/// `DIR/NAME ARGS…` writing `image`: exit 0 only when MicroTeX succeeded and made it.
pub fn job(dir: String, name: String, image: String, args: Vec<String>) -> Job {
    Box::new(move |io, stop| {
        let _ = std::fs::remove_file(&image);
        let (mut code, out, err) = run(&dir, &name, &args, stop);
        if code == 0 && !Path::new(&image).is_file() {
            code = 1;
        }
        if code != 0 {
            let _ = std::fs::remove_file(&image);
        }
        if !out.is_empty() {
            io.out(out);
        }
        if !err.is_empty() {
            io.err(err);
        }
        code
    })
}

/// `DIR/NAME ARGS…` run from DIR (MicroTeX finds its `res` there): exit code, stdout, stderr.
/// `stop` (the renderer or shell going away) terminates MicroTeX's whole process group.
pub fn run(
    dir: &str,
    name: &str,
    args: &[String],
    stop: &std::sync::Arc<crate::svc::Stop>,
) -> (i32, String, String) {
    let mut cmd = workpool::userenv::command(format!("./{name}"));
    cmd.args(args)
        .current_dir(dir)
        .env("PWD", dir)
        .stdin(Stdio::null());
    let o = match crate::child::run(&mut cmd, stop) {
        Ok(o) => o,
        Err(e) => {
            use std::io::ErrorKind::{NotADirectory, NotFound};
            let code = if matches!(e.kind(), NotFound | NotADirectory) {
                127
            } else {
                126
            };
            return (code, String::new(), format!("{dir}/{name}: {e}\n"));
        }
    };
    let Some(status) = o.status else {
        return (128 + libc::SIGTERM, String::new(), String::new());
    };
    let code = status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
    (
        code,
        String::from_utf8_lossy(&o.out).into_owned(),
        String::from_utf8_lossy(&o.err).into_owned(),
    )
}
