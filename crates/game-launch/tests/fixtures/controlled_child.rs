use std::{env, io::{self, Write}, process};

fn main() {
    println!("cwd={}", env::current_dir().unwrap().display());
    println!("marker={}", env::var("P034_ALLOWED_MARKER").unwrap_or_default());
    println!("inherited={}", env::var("P034_INHERITED_SECRET").unwrap_or_default());
    println!("path_present={}", env::var_os("PATH").is_some());
    for argument in env::args_os().skip(1) {
        println!("arg={}", argument.to_string_lossy());
    }
    let byte_count = env::var("P034_OUTPUT_BYTES").ok()
        .and_then(|value| value.parse::<usize>().ok()).unwrap_or(0);
    let mut stdout = io::stdout().lock();
    let block = [b'x'; 4096];
    let mut remaining = byte_count;
    while remaining > 0 {
        let count = remaining.min(block.len());
        if stdout.write_all(&block[..count]).is_err() { break; }
        remaining -= count;
    }
    let code = env::var("P034_EXIT_CODE").ok()
        .and_then(|value| value.parse::<i32>().ok()).unwrap_or(0);
    process::exit(code);
}
