//! `thumbgen DIR SIZE`: the in-process thumbnail job (as `DirectoryThumbnails` runs it) on a
//! real directory.
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let home = std::env::var_os("HOME").unwrap_or_default();
    let (out, code) = imgtools::thumbs::generate_dir(
        std::path::Path::new(&a[0]),
        &a[1],
        std::path::Path::new(&home),
        &|src, n, out| {
            let _ = imgtools::magick::resize_to_png(src, n, out);
        },
    );
    print!("{out}");
    std::process::exit(code);
}
