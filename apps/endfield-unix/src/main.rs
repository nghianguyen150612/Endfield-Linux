use std::process;

fn main() {
    let exit_status = endfield_unix::run(std::env::args_os().skip(1));
    process::exit(i32::from(exit_status));
}
