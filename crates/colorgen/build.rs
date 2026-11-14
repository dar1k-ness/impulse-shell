fn main() {
    let files = ["wu.cc", "wsmeans.cc", "lab.cc", "utils.cc", "celebi_ffi.cc"];
    let mut b = cc::Build::new();
    b.cpp(true).std("c++17").warnings(false);
    for f in files {
        b.file(format!("mcu/{f}"));
        println!("cargo:rerun-if-changed=mcu/{f}");
    }
    println!("cargo:rerun-if-changed=mcu");
    b.compile("mcu_quantize");
}
