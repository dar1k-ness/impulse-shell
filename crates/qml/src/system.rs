//! `Impulse.System`: facts about the machine and the session that do not change while the
//! shell runs. What the original asked `whoami` and `lscpu | grep 'CPU max MHz' | awk '{print $4}'`
//! (SystemInfo.qml, ResourceUsage.qml).

use crate::runtime::World;
use crate::value::Value;
use std::path::Path;

/// The name of the user this process runs as (coreutils `whoami`: the passwd entry of the
/// effective uid); empty when there is none.
pub fn user_name() -> String {
    // SAFETY: geteuid(2) takes no arguments and always succeeds.
    let uid = unsafe { libc::geteuid() };
    crate::osutil::passwd(crate::osutil::PwKey::Uid(uid), |e| {
        e.name().map(|n| n.to_string_lossy().into_owned())
    })
    .ok()
    .flatten()
    .flatten()
    .unwrap_or_default()
}

/// The highest frequency a CPU of this machine reaches, in MHz (util-linux's "CPU max MHz":
/// the largest cpufreq `cpuinfo_max_freq` over CPUs); 0 without cpufreq.
pub fn cpu_max_mhz() -> f64 {
    cpu_max_mhz_in(Path::new("/sys/devices/system/cpu"))
}

pub fn cpu_max_mhz_in(sys: &Path) -> f64 {
    let mut max = 0.0f64;
    for e in std::fs::read_dir(sys).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if !n
            .strip_prefix("cpu")
            .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let khz = std::fs::read_to_string(e.path().join("cpufreq/cpuinfo_max_freq"))
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok());
        if let Some(khz) = khz {
            max = max.max(khz / 1000.0);
        }
    }
    max
}

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "System",
    methods: &[],
    init: Some(init),
};

fn init(w: &mut World, item: usize) {
    for (name, v) in [
        ("userName", Value::str(&user_name())),
        ("cpuMaxMhz", Value::Num(cpu_max_mhz())),
    ] {
        w.write(item, name, v);
    }
}
