//! LLVM IR function builder.
//!
//! Port of `anvil/src/llvm/function.ts`. A function owns a `RegCounter` shared
//! by all its blocks (see `block.rs`), an ordered list of blocks, and emits
//! itself as an LLVM `define` when serialized.

use std::rc::Rc;

use crate::block::{LlBlock, RegCounter};
use crate::types::LlvmType;

pub struct LlFunction {
    pub name: String,
    pub return_type: LlvmType,
    pub params: Vec<(LlvmType, String)>,
    /// Optional LLVM linkage string, e.g. `"internal"` or `"private"`. Empty
    /// string means external (default) linkage.
    pub linkage: String,
    /// When true, the function body contains a `try` statement (setjmp/longjmp).
    /// We must emit `#1` (noinline optnone) on the definition so LLVM doesn't
    /// promote allocas to SSA registers across the setjmp call — otherwise
    /// mutations performed in the try body are invisible in the catch block
    /// after longjmp returns. `returns_twice` alone on the setjmp call is not
    /// sufficient at -O2 on aarch64.
    pub has_try: bool,
    blocks: Vec<LlBlock>,
    block_counter: u32,
    reg_counter: Rc<RegCounter>,
}

impl LlFunction {
    pub fn new(name: impl Into<String>, return_type: LlvmType, params: Vec<(LlvmType, String)>) -> Self {
        Self {
            name: name.into(),
            return_type,
            params,
            linkage: String::new(),
            has_try: false,
            blocks: Vec::new(),
            block_counter: 0,
            reg_counter: Rc::new(RegCounter::new()),
        }
    }

    /// Create a new basic block with the given semantic name (e.g. "entry",
    /// "if.then"). A numeric suffix is appended to make the label unique
    /// across the function.
    pub fn create_block(&mut self, name: &str) -> &mut LlBlock {
        let label = format!("{}.{}", name, self.block_counter);
        self.block_counter += 1;
        let block = LlBlock::new(label, self.reg_counter.clone());
        self.blocks.push(block);
        // Safe unwrap: we just pushed.
        self.blocks.last_mut().unwrap()
    }

    /// Accessor for an earlier block by index — needed when codegen has to
    /// come back and append to a predecessor (e.g. patching an unreachable
    /// fallthrough).
    pub fn block_mut(&mut self, idx: usize) -> Option<&mut LlBlock> {
        self.blocks.get_mut(idx)
    }

    pub fn blocks(&self) -> &[LlBlock] {
        &self.blocks
    }

    pub fn num_blocks(&self) -> usize {
        self.blocks.len()
    }

    /// Label of the last-created block — convenience for expression codegen
    /// that needs to feed a phi node the predecessor label after compiling a
    /// sub-expression whose control flow may have split.
    pub fn last_block_label(&self) -> Option<&str> {
        self.blocks.last().map(|b| b.label.as_str())
    }

    pub fn to_ir(&self) -> String {
        let param_str = self
            .params
            .iter()
            .map(|(t, n)| format!("{} {}", t, n))
            .collect::<Vec<_>>()
            .join(", ");

        let linkage = if self.linkage.is_empty() {
            String::new()
        } else {
            format!("{} ", self.linkage)
        };

        let attrs = if self.has_try { " #1" } else { "" };
        let mut ir = format!(
            "define {}{} @{}({}){} {{\n",
            linkage, self.return_type, self.name, param_str, attrs
        );

        for (i, blk) in self.blocks.iter().enumerate() {
            if i > 0 {
                ir.push('\n');
            }
            ir.push_str(&blk.to_ir());
            ir.push('\n');
        }

        ir.push_str("}\n");
        ir
    }
}
