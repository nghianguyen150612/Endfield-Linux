# endfield-unix

`endfield-unix` is planned as a native launcher and goal-driven optimization application for Arknights: Endfield on Unix-like systems. Its first implementation target is Linux. The intended desktop stack is Rust 2024, GTK4, libadwaita, and Relm4; a portable C17 library will propose optimization plans, and Rust will validate and execute only user-approved actions.

**Status:** P001 established the product and architecture baseline. P002 adds the Rust 2024 workspace and the single `endfield-unix` executable. P003 adds a portable C17 ABI library and safe Rust wrapper. The C implementation is an ABI skeleton: valid requests return an empty plan set, and no candidate generation or performance prediction exists. This is not yet a functioning optimization system.

The public GitHub repository is currently named [Endfield-Linux](https://github.com/nghianguyen150612/Endfield-Linux). That temporary repository identity is intentionally unchanged; local product and executable naming is `endfield-unix`.

## Project documentation

- [Product scope and requirements](docs/PRODUCT.md)
- [Architecture and C ABI contract](docs/ARCHITECTURE.md)
- [Optimizer ABI v1, ownership, and status codes](docs/ABI.md)
- [Naming rules](docs/NAMING.md)
- [P001–P090 roadmap](docs/ROADMAP.md)
- [Security and recovery principles](docs/SECURITY.md)
- [Testing strategy and current validation](docs/TESTING.md)
- [Rust and C17 architecture decision](docs/adr/0001-rust-c17-architecture.md)
- [Cross-platform boundary decision](docs/adr/0002-cross-platform-boundaries.md)
- [Runtime and rollback decision](docs/adr/0003-runtime-and-rollback-policy.md)

## Current contribution state

P001 was documentation-only. P002 provides one Rust application package and one `endfield-unix` executable. P003 adds the `optimizer-ffi` library package, C17 implementation, public ABI 1.0 header, and independent C/Rust contract tests. The application command dispatch and executable behavior remain unchanged. Starting without arguments reaches the reserved GUI boundary and reports that the GTK4 shell is unavailable.

The following are not implemented: the full C17 optimizer and candidate generation, runtime optimization, GPU/kernel tuning, GTK4 GUI, Wine/Proton integration, game launcher session, background daemon, telemetry collection, and benchmark execution. P003 does not claim game compatibility or optimization availability.

Run the independent native ABI test with `native/optimizer/tests/run-c-tests.sh`. It compiles the C source as a static library using strict C17 warnings and links a standalone test executable against it.

## Build and run

Rust 1.85 or newer is required. The repository pins Rust 1.85.0 in `rust-toolchain.toml`; this is the minimum version that supports the required Rust 2024 edition. Cargo will select that toolchain when it is installed.

A C17-capable compiler is also required for the optimizer-ffi build script; the cc crate follows Cargo's target compiler selection.

```sh
cargo build --locked --workspace
cargo run --locked -p endfield-unix -- --help
cargo run --locked -p endfield-unix -- --version
```

Running `cargo run --locked -p endfield-unix` with no options follows the future GUI entry point and currently prints that GUI mode is unavailable until P004, then exits with status 69. Invalid options print a diagnostic and exit with status 2. Full CLI subcommands belong to P005. The skeleton does not require a GPU, Wine, Proton, game installation, network access, or root privileges.
