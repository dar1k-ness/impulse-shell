fn main() {
    let b =
        colorgen::live::dconf_change("/org/gnome/desktop/interface/color-scheme", "prefer-dark")
            .unwrap();
    println!(
        "{}",
        b.iter().map(|x| format!("{x:02x}")).collect::<String>()
    );
}
