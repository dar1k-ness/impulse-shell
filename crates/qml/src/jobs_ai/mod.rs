//! The original's Gemini translation and wallpaper-categorisation scripts in process. Each is its own
//! job on its own thread.
pub mod categorize;
pub mod gemini;
pub mod jq;
pub mod translate;

/// curl's exit code for a transfer that failed before a response (`man curl`, EXIT CODES).
pub(crate) fn curl_code(e: &crate::http::Error) -> i32 {
    use ureq::Error as E;
    match e {
        E::BadUri(_) | E::Http(_) => 3,
        E::HostNotFound => 6,
        // curl 8: a proxy that refuses the CONNECT tunnel is a failed connect.
        E::ConnectProxyFailed(_) => 7,
        E::Timeout(_) => 28,
        E::Io(e) => match e.kind() {
            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound => 7,
            std::io::ErrorKind::TimedOut => 28,
            _ => 56,
        },
        E::Tls(_) => 35,
        _ => 56,
    }
}
