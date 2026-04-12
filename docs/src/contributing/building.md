# Building from Source

## Prerequisites

- Rust toolchain (stable): [rustup.rs](https://rustup.rs/)
- System C compiler (`cc` on macOS/Linux, MSVC on Windows)

## Build

```bash
git clone https://github.com/skelpo/perry.git
cd perry

# Build all crates (release mode recommended)
cargo build --release
```

The binary is at `target/release/perry`.

## Build Specific Crates

```bash
# Runtime only (must rebuild stdlib too!)
cargo build --release -p perry-runtime -p perry-stdlib

# Codegen only
cargo build --release -p perry-codegen-llvm
```

> **Important**: When rebuilding `perry-runtime`, you must also rebuild `perry-stdlib` because `libperry_stdlib.a` embeds perry-runtime as a static dependency.

## Run Tests

```bash
# All tests (exclude iOS crate on non-iOS host)
cargo test --workspace --exclude perry-ui-ios

# Specific crate
cargo test -p perry-hir
cargo test -p perry-codegen-llvm
```

## Compile and Run TypeScript

```bash
# Compile a TypeScript file
cargo run --release -- hello.ts -o hello
./hello

# Debug: print HIR
cargo run --release -- hello.ts --print-hir
```

## Development Workflow

1. Make changes to the relevant crate
2. `cargo build --release` to build
3. `cargo test --workspace --exclude perry-ui-ios` to verify
4. Test with a real TypeScript file: `cargo run --release -- test.ts -o test && ./test`

## Project Structure

```
perry/
├── crates/
│   ├── perry/              # CLI driver
│   ├── perry-parser/       # SWC TypeScript parser
│   ├── perry-types/        # Type definitions
│   ├── perry-hir/          # HIR and lowering
│   ├── perry-transform/    # IR passes
│   ├── perry-codegen-llvm/ # LLVM native codegen
│   ├── perry-codegen-wasm/ # WebAssembly codegen (--target web / --target wasm)
│   ├── perry-codegen-js/   # JS minifier (formerly the web target's codegen)
│   ├── perry-codegen-swiftui/ # Widget codegen
│   ├── perry-runtime/      # Runtime library
│   ├── perry-stdlib/       # npm package implementations
│   ├── perry-ui/           # Shared UI types
│   ├── perry-ui-macos/     # macOS AppKit UI
│   ├── perry-ui-ios/       # iOS UIKit UI
│   └── perry-jsruntime/    # QuickJS integration
├── docs/                   # This documentation (mdBook)
├── CLAUDE.md               # Detailed implementation notes
└── CHANGELOG.md            # Version history
```

## Next Steps

- [Architecture](architecture.md) — Crate map and pipeline overview
- See `CLAUDE.md` for detailed implementation notes and pitfalls
