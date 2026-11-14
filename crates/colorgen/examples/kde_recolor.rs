//! `kde_recolor (-d|-l) --color '#rrggbb' [-sv N]`: one native Qt/KDE recolour of $HOME, as
//! switchwall does it, for trying it outside the shell.
use colorgen::kde;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let at = |k: &str| {
        a.iter()
            .position(|x| x == k)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    let dark = !a.iter().any(|x| x == "-l");
    let seed = at("--color").expect("--color '#rrggbb'");
    let sv = at("-sv").map_or(5, |v| v.parse().expect("-sv N"));
    let home = std::path::PathBuf::from(std::env::var("HOME").expect("HOME"));
    let t = std::time::Instant::now();
    let o = kde::recolor(
        &kde::Dirs::from_env(&home),
        &seed,
        dark,
        sv,
        &kde::LiveBus::session(),
    );
    eprintln!("{:?} in {:?}", o, t.elapsed());
}
