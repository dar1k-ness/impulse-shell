//! The one client for Hyprland's request socket (`.socket.sock`), shared by the shell's
//! `Rustshell.Hyprland`/hyprctl code and by colorgen's theme switch.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// Where a `.socket.sock` exchange failed, as hyprctl reports it.
#[derive(Debug)]
pub enum IpcError {
    Connect(std::io::Error),
    Write(std::io::Error),
    Read(std::io::Error),
}

/// One request/reply on Hyprland's `.socket.sock`, which closes the connection after the
/// reply: read until EOF. The shell's own requests and colorgen pass no `read_timeout` and
/// wait however long Hyprland takes to answer (a large `j/clients`, a compositor busy in a
/// modeset, a config `eval`): a request Hyprland has read is carried out whether or not anyone
/// waits for its reply, so a deadline would only turn a slow success into a reported failure.
/// Callers run on threads that may block, never the main thread. The `hyprctl` emulation
/// passes hyprctl's own `SO_RCVTIMEO`; a read that waits longer fails with
/// [`std::io::ErrorKind::WouldBlock`].
pub fn exchange(
    sock: &Path,
    cmd: &str,
    read_timeout: Option<Duration>,
) -> Result<String, IpcError> {
    let mut s = UnixStream::connect(sock).map_err(IpcError::Connect)?;
    s.set_read_timeout(read_timeout).map_err(IpcError::Read)?;
    s.write_all(cmd.as_bytes()).map_err(IpcError::Write)?;
    // Window titles are not always UTF-8 (X11 Latin-1 `WM_NAME`): decode lossily like
    // Quickshell, or one such window fails every `j/clients` and stalls all Hyprland state.
    let mut out = Vec::new();
    s.read_to_end(&mut out).map_err(IpcError::Read)?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::{IpcError, exchange};
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;

    fn sock(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("hypripc-{tag}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn reads_the_whole_reply_lossily() {
        let p = sock("reply");
        let l = UnixListener::bind(&p).unwrap();
        let srv = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut buf = [0u8; 64];
            let n = c.read(&mut buf).unwrap();
            assert_eq!(&buf[..n], b"j/clients");
            c.write_all(b"[{\"title\": \"caf\xe9\"}]").unwrap();
        });
        let r = exchange(&p, "j/clients", None).unwrap();
        srv.join().unwrap();
        let _ = std::fs::remove_file(&p);
        assert_eq!(r, "[{\"title\": \"caf\u{fffd}\"}]");
    }

    #[test]
    fn no_compositor_is_a_connect_error() {
        let p = sock("absent");
        assert!(matches!(
            exchange(&p, "j/monitors", None),
            Err(IpcError::Connect(_))
        ));
    }
}
