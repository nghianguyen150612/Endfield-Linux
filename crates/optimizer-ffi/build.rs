fn main() {
    println!("cargo:rerun-if-changed=../../native/optimizer/include/endfield_optimizer.h");
    println!("cargo:rerun-if-changed=../../native/optimizer/src/optimizer.c");

    cc::Build::new()
        .file("../../native/optimizer/src/optimizer.c")
        .include("../../native/optimizer/include")
        .std("c17")
        .warnings(true)
        .extra_warnings(true)
        .warnings_into_errors(true)
        .flag_if_supported("-Wpedantic")
        .compile("endfield_optimizer");
}
