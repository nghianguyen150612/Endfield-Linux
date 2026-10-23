# endfield-unix

`endfield-unix` is planned as a native launcher and goal-driven optimization application for Arknights: Endfield on Unix-like systems. Its first implementation target is Linux. The intended desktop stack is Rust 2024, GTK4, libadwaita, and Relm4; a portable C17 library will propose optimization plans, and Rust will validate and execute only user-approved actions.

**Status:** P001 establishes the product and architecture documentation. This repository does not yet contain an application, daemon, runtime installer, or optimization engine. No game compatibility claim is made by this documentation.

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

P001 is documentation-only. There is no Rust workspace, C source, GUI, CLI, daemon, or build/test command yet. Implementation work is listed in the roadmap; future checks must be reported only after the corresponding code exists and the commands have actually run.
