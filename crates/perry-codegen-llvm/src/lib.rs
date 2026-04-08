//! LLVM Code Generation for Perry (experimental)
//!
//! Parallel backend to `perry-codegen` (Cranelift). Produces textual LLVM IR
//! (`.ll`) from Perry's HIR, then shells out to `clang -c` to build an object
//! file whose byte representation matches the contract of the Cranelift backend.
//!
//! The design is a direct Rust port of the approach validated by `anvil`
//! (sibling project `/Users/amlug/projects/perry/anvil`), which compiled
//! TypeScript to LLVM IR text and achieved byte-for-byte parity against Perry
//! on 68 deterministic tests using the identical NaN-boxing value encoding and
//! the same `libperry_runtime.a`.

pub mod types;
pub mod nanbox;
pub mod block;
pub mod function;
pub mod module;
pub mod runtime_decls;
pub mod linker;
pub(crate) mod expr;
pub(crate) mod stmt;
pub mod codegen;

pub use codegen::{compile_module, CompileOptions};
