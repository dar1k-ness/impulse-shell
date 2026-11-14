//! `regions::cli` against the original `find_regions.py` (goldens: tests/regions_golden.py),
//! for both of OpenCV's filter builds.

use std::path::Path;

/// The cases regions_golden.py writes.
const MIN_CASES: usize = 13;

/// The cases of one golden file, run as OpenCV computes them with (`fma`) or without its AVX2
/// filters.
fn check(file: &str, fma: bool) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regions");
    let golden = std::fs::read_to_string(dir.join(file)).unwrap();
    let mut n = 0;
    for line in golden.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let (image, args, code, stdout) = (f[0], f[1], f[2].parse::<i32>().unwrap(), f[3]);
        let mut argv = vec![
            "--image".to_string(),
            dir.join(image).to_string_lossy().into_owned(),
        ];
        argv.extend(args.split(' ').filter(|a| !a.is_empty()).map(String::from));
        let want = if stdout.is_empty() {
            String::new()
        } else {
            format!("{stdout}\n")
        };
        let got = imgtools::regions::cli_as(&argv, fma);
        // The whole list is too long to read in a failure: name the first difference.
        assert!(
            got == (want.clone(), code),
            "{file}: {image} {args}: exit {} (want {code}), stdout differs from byte {:?}",
            got.1,
            got.0.bytes().zip(want.bytes()).position(|(a, b)| a != b)
        );
        n += 1;
    }
    assert!(n >= MIN_CASES);
}

#[test]
fn matches_original_script() {
    check("golden.tsv", true);
}

#[test]
fn matches_original_script_without_avx2() {
    check("golden_noavx2.tsv", false);
}

/// `cli` follows this machine's CPU like OpenCV does.
#[test]
fn cli_picks_the_build_for_this_cpu() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regions");
    let argv: Vec<String> = [
        "--hyprctl",
        "--image",
        dir.join("ui_1920x1080.png").to_str().unwrap(),
        "--max-width",
        "960",
        "--max-height",
        "540",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let fma = imgtools::regions::ss_cv::filter_fma();
    assert_eq!(
        imgtools::regions::cli(&argv),
        imgtools::regions::cli_as(&argv, fma)
    );
}

#[test]
fn usage_errors_exit_2() {
    for argv in [
        &["--hyprctl", "--max-width"][..],
        &["--hyprctl", "--max-width", "x"],
        &["--hyprctl", "--bogus"],
        &["--hyprctl", "--m", "5"],
        &["--hyprctl=1"],
    ] {
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            imgtools::regions::cli(&argv),
            (String::new(), 2),
            "{argv:?}"
        );
    }
}

/// The script's options the shell never passes are not ported: the port refuses them instead
/// of computing something the script was never compared with.
#[test]
fn options_the_shell_does_not_pass_exit_2() {
    let image = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regions/ui_1920x1080.png");
    let image = image.to_str().unwrap();
    for extra in [
        &["--sigma", "0.5"][..],
        &["--sigma", "0.8"],
        &["--k", "500"],
        &["--min-size", "20"],
        &["--resize-factor", "0.25"],
        &["--min-width", "50"],
        &["--min-height", "50"],
        &["--single"],
        &["--quality"],
        &["--debug-output", "/dev/null"],
        &["-do", "/dev/null"],
    ] {
        let mut argv = vec!["--hyprctl".to_string(), "--image".into(), image.into()];
        argv.extend(extra.iter().map(|s| s.to_string()));
        assert_eq!(
            imgtools::regions::cli(&argv),
            (String::new(), 2),
            "{extra:?}"
        );
    }
    // Without --hyprctl the script prints another format, which nothing reads.
    let argv = vec!["--image".to_string(), image.into()];
    assert_eq!(imgtools::regions::cli(&argv), (String::new(), 2));
}
