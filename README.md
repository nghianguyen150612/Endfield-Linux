# endfield-unix

`endfield-unix` is planned as a native launcher and goal-driven optimization application for Arknights: Endfield on Unix-like systems. Its first implementation target is Linux. The intended desktop stack is Rust 2024, GTK4, libadwaita, and Relm4; a portable C17 library will propose optimization plans, and Rust will validate and execute only user-approved actions.

**Status:** P001 established the product and architecture baseline. P002 adds a Rust 2024 workspace and the single `endfield-unix` executable with help, version, and a GUI entry-point boundary. The GTK window, full CLI, daemon, runtime installer, and optimization engine are not implemented. No game compatibility claim is made.

The public GitHub repository is currently named [Endfield-Linux](https://github.com/nghianguyen150612/Endfield-Linux). That temporary repository identity is intentionally unchanged; local product and executable naming is `endfield-unix`.

## Project documentation

- [Product scope and requirements](docs/PRODUCT.md)
- [Architecture and C ABI contract](docs/ARCHITECTURE.md)
- [Naming rules](docs/NAMING.md)
- [P001–P090 roadmap](docs/ROADMAP.md)
- [Security and recovery principles](docs/SECURITY.md)
- [Testing strategy and current validation](docs/TESTING.md)
- [Rust and C17 architecture decision](docs/adr/0001-rust-c17-architecture.md)
- [Cross-platform boundary decision](docs/adr/0002-cross-platform-boundaries.md)
- [Runtime and rollback decision](docs/adr/0003-runtime-and-rollback-policy.md)

## Current contribution state

P001 was documentation-only. P002 now provides one Rust application package and one `endfield-unix` executable. Its command dispatch is shared through a testable Rust library. The executable currently supports `--help` and `--version`; starting without arguments reaches the reserved GUI boundary and reports that the GTK4 shell is unavailable. There is no GTK window, full CLI, daemon, C source, game/runtime management, optimizer, or platform adapter yet.

## Build and run

Rust 1.85 or newer is required. The repository pins Rust 1.85.0 in `rust-toolchain.toml`; this is the minimum version that supports the required Rust 2024 edition. Cargo will select that toolchain when it is installed.

```sh
cargo build --locked --workspace
cargo run --locked -p endfield-unix -- --help
cargo run --locked -p endfield-unix -- --version
```

Running `cargo run --locked -p endfield-unix` with no options follows the future GUI entry point and currently prints that GUI mode is unavailable until P004, then exits with status 69. Invalid options print a diagnostic and exit with status 2. Full CLI subcommands belong to P005. The skeleton does not require a GPU, Wine, Proton, game installation, network access, or root privileges.
