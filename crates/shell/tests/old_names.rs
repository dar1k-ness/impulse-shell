//! The names the project had before it was renamed are in no tracked file, outside the places
//! `tools/rename/rename.py` lists with the reason for each (`ALLOWED`, and the files it leaves
//! as they are): `tools/rename/rename.py --stale` finds none.
use std::path::Path;
use std::process::Command;

#[test]
fn no_old_name_is_left() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let o = Command::new("python3")
        .arg("tools/rename/rename.py")
        .arg("--stale")
        .current_dir(&root)
        // The checkout may be another user's (a container that mounts it).
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "safe.directory")
        .env("GIT_CONFIG_VALUE_0", "*")
        .output()
        .expect("python3");
    assert!(
        o.status.success(),
        "old names are left:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}
