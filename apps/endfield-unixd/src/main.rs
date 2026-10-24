fn main() {
    let status = endfield_unixd::cli::run(std::env::args_os().skip(1));
    std::process::exit(status);
}
