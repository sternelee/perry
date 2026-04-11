//! LLVM IR basic-block builder.
//!
//! Direct port of `anvil/src/llvm/block.ts`. Each method appends one textual
//! LLVM IR instruction to an internal buffer; `to_ir` produces the final text.
//!
//! We use `alloca` + `load`/`store` for locals and rely on LLVM's `mem2reg`
//! pass (run automatically by `clang -O2` or higher) to promote them to SSA
//! form. This avoids the phi-node bookkeeping the Cranelift backend needs —
//! locals just become stack slots at codegen time and LLVM's optimizer sorts
//! out the registers. Explicit `phi` nodes are still emitted for control-flow
//! merges (if/else value context, short-circuit logical ops).

use std::cell::Cell;
use std::rc::Rc;

use crate::types::LlvmType;

/// Function-wide register counter shared between all blocks in a function.
///
/// Registers are `%r1`, `%r2`, … unique across the entire function body —
/// LLVM requires SSA value names to be unique per function, not per block.
#[derive(Default)]
pub struct RegCounter {
    value: Cell<u32>,
}

impl RegCounter {
    pub fn new() -> Self {
        Self { value: Cell::new(0) }
    }

    pub fn next(&self) -> u32 {
        let v = self.value.get() + 1;
        self.value.set(v);
        v
    }
}

pub struct LlBlock {
    pub label: String,
    instructions: Vec<String>,
    terminated: bool,
    counter: Rc<RegCounter>,
}

impl LlBlock {
    pub fn new(label: impl Into<String>, counter: Rc<RegCounter>) -> Self {
        Self {
            label: label.into(),
            instructions: Vec::new(),
            terminated: false,
            counter,
        }
    }

    pub fn is_terminated(&self) -> bool {
        self.terminated
    }

    fn emit(&mut self, line: impl Into<String>) {
        // Never emit instructions after a terminator — LLVM rejects them and
        // the symptom is a confusing `clang` parse error many lines later.
        // We silently drop them: this mirrors a common bug pattern in anvil
        // where catch-all statement visitors occasionally fall through past
        // an already-emitted `ret`/`br`.
        if self.terminated {
            return;
        }
        self.instructions.push(format!("  {}", line.into()));
    }

    fn reg(&self) -> String {
        format!("%r{}", self.counter.next())
    }

    pub fn next_reg(&self) -> String {
        self.reg()
    }

    pub fn emit_raw(&mut self, line: impl Into<String>) {
        self.emit(line);
    }

    // -------- Arithmetic (double) --------

    pub fn fadd(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fadd double {}, {}", r, a, b));
        r
    }

    pub fn fsub(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fsub double {}, {}", r, a, b));
        r
    }

    pub fn fmul(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fmul double {}, {}", r, a, b));
        r
    }

    pub fn fdiv(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fdiv double {}, {}", r, a, b));
        r
    }

    pub fn frem(&mut self, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = frem double {}, {}", r, a, b));
        r
    }

    pub fn fneg(&mut self, a: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fneg double {}", r, a));
        r
    }

    // -------- Comparisons --------

    /// Float comparison. `cond` is an LLVM predicate string: `olt`, `ole`,
    /// `ogt`, `oge`, `oeq`, `one`, `ord`, `uno`, …
    pub fn fcmp(&mut self, cond: &str, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = fcmp {} double {}, {}", r, cond, a, b));
        r
    }

    pub fn icmp_eq(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp eq {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_ne(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp ne {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_slt(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp slt {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_sgt(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp sgt {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_sle(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp sle {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_ult(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp ult {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_ule(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp ule {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_ugt(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp ugt {} {}, {}", r, ty, a, b));
        r
    }

    pub fn icmp_sge(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = icmp sge {} {}, {}", r, ty, a, b));
        r
    }

    // -------- Memory --------

    pub fn alloca(&mut self, ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = alloca {}", r, ty));
        r
    }

    pub fn load(&mut self, ty: LlvmType, ptr: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = load {}, ptr {}", r, ty, ptr));
        r
    }

    pub fn store(&mut self, ty: LlvmType, val: &str, ptr: &str) {
        self.emit(format!("store {} {}, ptr {}", ty, val, ptr));
    }

    // -------- Conversions / bitcasts --------

    pub fn bitcast_i64_to_double(&mut self, val: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = bitcast i64 {} to double", r, val));
        r
    }

    pub fn bitcast_double_to_i64(&mut self, val: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = bitcast double {} to i64", r, val));
        r
    }

    pub fn sitofp(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = sitofp {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn uitofp(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = uitofp {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn fptosi(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = fptosi {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn trunc(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = trunc {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn zext(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = zext {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn sext(&mut self, from_ty: LlvmType, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = sext {} {} to {}", r, from_ty, val, to_ty));
        r
    }

    pub fn inttoptr(&mut self, from_ty: LlvmType, val: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = inttoptr {} {} to ptr", r, from_ty, val));
        r
    }

    pub fn ptrtoint(&mut self, val: &str, to_ty: LlvmType) -> String {
        let r = self.reg();
        self.emit(format!("{} = ptrtoint ptr {} to {}", r, val, to_ty));
        r
    }

    // -------- Integer arithmetic --------

    pub fn add(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = add {} {}, {}", r, ty, a, b));
        r
    }

    pub fn sub(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = sub {} {}, {}", r, ty, a, b));
        r
    }

    pub fn mul(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = mul {} {}, {}", r, ty, a, b));
        r
    }

    pub fn and(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = and {} {}, {}", r, ty, a, b));
        r
    }

    pub fn or(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = or {} {}, {}", r, ty, a, b));
        r
    }

    pub fn xor(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = xor {} {}, {}", r, ty, a, b));
        r
    }

    pub fn shl(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = shl {} {}, {}", r, ty, a, b));
        r
    }

    pub fn ashr(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = ashr {} {}, {}", r, ty, a, b));
        r
    }

    pub fn lshr(&mut self, ty: LlvmType, a: &str, b: &str) -> String {
        let r = self.reg();
        self.emit(format!("{} = lshr {} {}, {}", r, ty, a, b));
        r
    }

    // -------- Select --------

    pub fn select(
        &mut self,
        cond_ty: LlvmType,
        cond: &str,
        ty: LlvmType,
        true_val: &str,
        false_val: &str,
    ) -> String {
        let r = self.reg();
        self.emit(format!(
            "{} = select {} {}, {} {}, {} {}",
            r, cond_ty, cond, ty, true_val, ty, false_val
        ));
        r
    }

    // -------- Function calls --------

    pub fn call(&mut self, ret_ty: LlvmType, func_name: &str, args: &[(LlvmType, &str)]) -> String {
        let r = self.reg();
        let arg_str = format_args(args);
        self.emit(format!("{} = call {} @{}({})", r, ret_ty, func_name, arg_str));
        r
    }

    pub fn call_void(&mut self, func_name: &str, args: &[(LlvmType, &str)]) {
        let arg_str = format_args(args);
        self.emit(format!("call void @{}({})", func_name, arg_str));
    }

    pub fn call_indirect(
        &mut self,
        ret_ty: LlvmType,
        fn_ptr: &str,
        args: &[(LlvmType, &str)],
    ) -> String {
        let r = self.reg();
        let arg_str = format_args(args);
        let param_types: Vec<&str> = args.iter().map(|(t, _)| *t).collect();
        self.emit(format!(
            "{} = call {} ({})* {}({})",
            r,
            ret_ty,
            param_types.join(", "),
            fn_ptr,
            arg_str
        ));
        r
    }

    // -------- Control flow --------

    pub fn br(&mut self, target: &str) {
        self.emit(format!("br label %{}", target));
        self.terminated = true;
    }

    pub fn cond_br(&mut self, cond: &str, true_label: &str, false_label: &str) {
        self.emit(format!(
            "br i1 {}, label %{}, label %{}",
            cond, true_label, false_label
        ));
        self.terminated = true;
    }

    pub fn ret(&mut self, ty: LlvmType, val: &str) {
        self.emit(format!("ret {} {}", ty, val));
        self.terminated = true;
    }

    pub fn ret_void(&mut self) {
        self.emit("ret void");
        self.terminated = true;
    }

    pub fn unreachable(&mut self) {
        self.emit("unreachable");
        self.terminated = true;
    }

    // -------- GEP / Phi --------

    pub fn gep(&mut self, base_ty: LlvmType, ptr: &str, indices: &[(LlvmType, &str)]) -> String {
        let r = self.reg();
        let idx_str = indices
            .iter()
            .map(|(t, v)| format!("{} {}", t, v))
            .collect::<Vec<_>>()
            .join(", ");
        self.emit(format!(
            "{} = getelementptr {}, ptr {}, {}",
            r, base_ty, ptr, idx_str
        ));
        r
    }

    pub fn phi(&mut self, ty: LlvmType, incoming: &[(&str, &str)]) -> String {
        let r = self.reg();
        let pairs = incoming
            .iter()
            .map(|(val, label)| format!("[ {}, %{} ]", val, label))
            .collect::<Vec<_>>()
            .join(", ");
        self.emit(format!("{} = phi {} {}", r, ty, pairs));
        r
    }

    pub fn to_ir(&self) -> String {
        let mut out = String::with_capacity(self.instructions.iter().map(|l| l.len() + 1).sum::<usize>() + self.label.len() + 2);
        out.push_str(&self.label);
        out.push_str(":\n");
        out.push_str(&self.instructions.join("\n"));
        out
    }
}

fn format_args(args: &[(LlvmType, &str)]) -> String {
    args.iter()
        .map(|(t, v)| format!("{} {}", t, v))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DOUBLE, I64};

    fn fresh() -> LlBlock {
        LlBlock::new("entry.0", Rc::new(RegCounter::new()))
    }

    #[test]
    fn fadd_emits_expected_ir() {
        let mut b = fresh();
        let r = b.fadd("1.0", "2.0");
        assert_eq!(r, "%r1");
        assert_eq!(b.to_ir(), "entry.0:\n  %r1 = fadd double 1.0, 2.0");
    }

    #[test]
    fn call_with_args() {
        let mut b = fresh();
        let r = b.call(DOUBLE, "js_nanbox_string", &[(I64, "%handle")]);
        assert_eq!(r, "%r1");
        assert!(b.to_ir().contains("call double @js_nanbox_string(i64 %handle)"));
    }

    #[test]
    fn terminator_blocks_further_emits() {
        let mut b = fresh();
        b.ret(DOUBLE, "0.0");
        // This would silently drop; we don't want extra lines after ret.
        let _ = b.fadd("1.0", "2.0");
        let ir = b.to_ir();
        assert!(ir.contains("ret double 0.0"));
        assert!(!ir.contains("fadd"));
    }

    #[test]
    fn regs_are_function_unique_not_block_unique() {
        let counter = Rc::new(RegCounter::new());
        let mut b1 = LlBlock::new("a", counter.clone());
        let mut b2 = LlBlock::new("b", counter);
        let r1 = b1.fadd("1.0", "2.0");
        let r2 = b2.fadd("3.0", "4.0");
        assert_eq!(r1, "%r1");
        assert_eq!(r2, "%r2");
    }
}
