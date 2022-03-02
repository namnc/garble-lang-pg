//! The circuit representation used by the compiler.

use crate::{compile::wires_as_unsigned, token::MetaInfo};
use std::collections::HashMap;

// This module currently implements 4 very basic types of circuit optimizations:
//
// 1. Constant evaluation (e.g. x ^ 0 == x; x & 1 == x; x & 0 == 0)
// 2. Sub-expression sharing (wires are re-used if a gate with the same type and inputs exists)
// 3. Pruning of useless gates (gates that are not part of the output nor used by other gates)
// 4. Rewriting of equivalences up to a max depth (e.g. x & (!x | y) == x & y)
const MAX_OPTIMIZATION_DEPTH: u32 = 4;
const PRINT_OPTIMIZATION_RATIO: bool = false;

/// Data type to uniquely identify gates.
pub type GateIndex = usize;

/// Description of a gate executed under S-MPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// A logical XOR gate attached to the two specified input wires.
    Xor(GateIndex, GateIndex),
    /// A logical AND gate attached to the two specified input wires.
    And(GateIndex, GateIndex),
    /// A logical NOT gate attached to the specified input wire.
    Not(GateIndex),
}

/// A decoded panic, indicating why and where a panic occurred.
#[derive(Debug, Clone)]
pub struct EvalPanic {
    /// The reason why the panic occurred.
    pub reason: PanicReason,
    /// The location in the source code where the panic occurred.
    pub panicked_at: MetaInfo,
}

/// Representation of a circuit evaluated by an S-MPC engine.
#[derive(Debug, Clone)]
pub struct Circuit {
    /// The different parties, with `usize` at index `i` as the number of input bits for party `i`.
    pub input_gates: Vec<usize>,
    /// The non-input intermediary gates.
    pub gates: Vec<Gate>,
    /// The indices of the gates in [`Circuit::gates`] that produce output bits.
    pub output_gates: Vec<GateIndex>,
    /// The output gates that carry information about a panic, if any occurred.
    pub panic_gates: PanicResult,
}

impl Circuit {
    /// Evaluates the circuit with the specified inputs (with one `Vec<bool>` per party).
    ///
    /// Assumes that the inputs have been previously type-checked and **panics** if the number of
    /// parties or the bits of a particular party do not match the circuit.
    pub fn eval(&self, inputs: &[Vec<bool>]) -> Result<Vec<Option<bool>>, EvalPanic> {
        let mut input_len = 0;
        for p in self.input_gates.iter() {
            input_len += p;
        }
        let mut output = vec![None; input_len + self.gates.len()];
        let inputs: Vec<_> = inputs.iter().map(|inputs| inputs.iter()).collect();
        let mut i = 0;
        if self.input_gates.len() != inputs.len() {
            panic!(
                "Circuit was built for {} parties, but found {} inputs",
                self.input_gates.len(),
                inputs.len()
            );
        }
        for (p, &input_gates) in self.input_gates.iter().enumerate() {
            if input_gates != inputs[p].len() {
                panic!(
                    "Expected {} input bits for party {}, but found {}",
                    input_gates,
                    p,
                    inputs[p].len()
                );
            }
            for bit in inputs[p].as_slice() {
                output[i] = Some(*bit);
                i += 1;
            }
        }
        for (w, gate) in self.gates.iter().enumerate() {
            let w = w + i;
            let output_bit = match gate {
                Gate::Xor(x, y) => output[*x].unwrap() ^ output[*y].unwrap(),
                Gate::And(x, y) => output[*x].unwrap() & output[*y].unwrap(),
                Gate::Not(x) => !output[*x].unwrap(),
            };
            output[w] = Some(output_bit);
        }
        let resolve_wires_as_usize = |wires: &[usize]| -> usize {
            let resolved: Vec<bool> = wires.iter().map(|w| output[*w].unwrap()).collect();
            wires_as_unsigned(&resolved) as usize
        };
        if output[self.panic_gates.has_panicked].unwrap() {
            let reason =
                PanicReason::from_num(resolve_wires_as_usize(&self.panic_gates.panic_type));
            let start_line = resolve_wires_as_usize(&self.panic_gates.start_line) as usize;
            let start_column = resolve_wires_as_usize(&self.panic_gates.start_column) as usize;
            let end_line = resolve_wires_as_usize(&self.panic_gates.end_line) as usize;
            let end_column = resolve_wires_as_usize(&self.panic_gates.end_column) as usize;
            Err(EvalPanic {
                reason,
                panicked_at: MetaInfo {
                    start: (start_line, start_column),
                    end: (end_line, end_column),
                },
            })
        } else {
            for (w, output) in output.iter_mut().enumerate() {
                if !self.output_gates.contains(&w) {
                    *output = None;
                }
            }
            Ok(output)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BuilderGate {
    Xor(GateIndex, GateIndex),
    And(GateIndex, GateIndex),
}

#[derive(Debug, Clone)]
pub(crate) struct CircuitBuilder {
    shift: usize,
    input_gates: Vec<usize>,
    gates: Vec<BuilderGate>,
    cache: HashMap<BuilderGate, GateIndex>,
    negated: HashMap<GateIndex, GateIndex>,
    gates_optimized: usize,
    gate_counter: usize,
    panic_gates: PanicResult,
}

const USIZE_BITS: usize = usize::BITS as usize;

/// A collection of wires that carry information about whether and where a panic occurred.
#[derive(Debug, Clone)]
pub struct PanicResult {
    /// A boolean wire indicating whether a panic has occurred.
    pub has_panicked: GateIndex,
    /// The (encoded) reason why the panic occurred (overflow, div-by-zero, etc).
    pub panic_type: [GateIndex; USIZE_BITS],
    /// The (encoded) first line in the source code where the panic occurred.
    pub start_line: [GateIndex; USIZE_BITS],
    /// The (encoded) first column of the first line in the source code where the panic occurred.
    pub start_column: [GateIndex; USIZE_BITS],
    /// The (encoded) last line in the source code where the panic occurred.
    pub end_line: [GateIndex; USIZE_BITS],
    /// The (encoded) last column of the last line in the source code where the panic occurred.
    pub end_column: [GateIndex; USIZE_BITS],
}

impl PanicResult {
    /// Returns a `PanicResult` indicating that no panic has occurred.
    pub fn ok() -> Self {
        Self {
            has_panicked: 0,
            panic_type: PanicReason::Overflow.as_bits(),
            start_line: [0; USIZE_BITS],
            start_column: [0; USIZE_BITS],
            end_line: [0; USIZE_BITS],
            end_column: [0; USIZE_BITS],
        }
    }
}

/// The reason why a panic occurred.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PanicReason {
    /// Arithmetic overflow.
    Overflow,
    /// Division by zero.
    DivByZero,
    /// Array out of bounds access.
    OutOfBounds,
}

impl PanicReason {
    fn from_num(n: usize) -> Self {
        match n {
            1 => PanicReason::Overflow,
            2 => PanicReason::DivByZero,
            3 => PanicReason::OutOfBounds,
            r => panic!("Invalid panic reason: {}", r),
        }
    }

    fn as_bits(&self) -> [GateIndex; USIZE_BITS] {
        let n = match self {
            PanicReason::Overflow => 1,
            PanicReason::DivByZero => 2,
            PanicReason::OutOfBounds => 3,
        };
        unsigned_as_usize_bits(n)
    }
}

impl CircuitBuilder {
    pub fn new(input_gates: Vec<usize>) -> Self {
        let mut gate_counter = 2; // for const true and false
        for input_gates_of_party in input_gates.iter() {
            gate_counter += input_gates_of_party;
        }
        Self {
            shift: gate_counter,
            input_gates,
            gates: vec![],
            cache: HashMap::new(),
            negated: HashMap::new(),
            gates_optimized: 0,
            gate_counter,
            panic_gates: PanicResult::ok(),
        }
    }

    // Pruning of useless gates (gates that are not part of the output nor used by other gates):
    fn remove_unused_gates(&mut self, output_gates: Vec<GateIndex>) -> Vec<GateIndex> {
        // To find all unused gates, we start at the output gates and recursively mark all their
        // inputs (and their inputs, etc.) as used:
        let shift = self.shift;
        let mut output_gate_stack = output_gates.clone();
        output_gate_stack.push(self.panic_gates.has_panicked);
        output_gate_stack.extend(self.panic_gates.panic_type.iter());
        output_gate_stack.extend(self.panic_gates.start_line.iter());
        output_gate_stack.extend(self.panic_gates.start_column.iter());
        output_gate_stack.extend(self.panic_gates.end_line.iter());
        output_gate_stack.extend(self.panic_gates.end_column.iter());
        let mut used_gates = vec![false; self.gates.len()];
        while let Some(gate_index) = output_gate_stack.pop() {
            if gate_index >= shift {
                let shifted_index = gate_index - shift;
                used_gates[shifted_index] = true;
                let (x, y) = match self.gates[shifted_index] {
                    BuilderGate::Xor(x, y) => (x, y),
                    BuilderGate::And(x, y) => (x, y),
                };
                if x >= shift && !used_gates[x - shift] {
                    output_gate_stack.push(x);
                }
                if y >= shift && !used_gates[y - shift] {
                    output_gate_stack.push(y);
                }
            }
        }
        let mut unused_gates = 0;
        let mut unused_before_gate = vec![0; self.gates.len()];
        for (w, used) in used_gates.iter().enumerate() {
            if !used {
                unused_gates += 1;
            }
            unused_before_gate[w] = unused_gates;
        }
        // Now that we know which gates are useless, iterate through the circuit and shift the
        // indices of the useful gates to reuse the freed space:
        let shift_gate_index_if_necessary = |index: usize| {
            if index > shift {
                index - unused_before_gate[index - shift]
            } else {
                index
            }
        };
        for gate in self.gates.iter_mut() {
            let (x, y) = match gate {
                BuilderGate::Xor(x, y) => (x, y),
                BuilderGate::And(x, y) => (x, y),
            };
            *x = shift_gate_index_if_necessary(*x);
            *y = shift_gate_index_if_necessary(*y);
        }
        self.panic_gates.has_panicked =
            shift_gate_index_if_necessary(self.panic_gates.has_panicked);
        for w in self.panic_gates.panic_type.iter_mut() {
            *w = shift_gate_index_if_necessary(*w);
        }
        for w in self.panic_gates.start_line.iter_mut() {
            *w = shift_gate_index_if_necessary(*w);
        }
        for w in self.panic_gates.start_column.iter_mut() {
            *w = shift_gate_index_if_necessary(*w);
        }
        for w in self.panic_gates.end_line.iter_mut() {
            *w = shift_gate_index_if_necessary(*w);
        }
        for w in self.panic_gates.end_column.iter_mut() {
            *w = shift_gate_index_if_necessary(*w);
        }
        let mut without_unused_gates = Vec::with_capacity(self.gates.len() - unused_gates);
        for (w, &used) in used_gates.iter().enumerate() {
            if used {
                without_unused_gates.push(self.gates[w]);
            }
        }
        self.gates_optimized += unused_gates;
        self.gates = without_unused_gates;
        // The indices of the output gates might have become invalid due to shifting the gates
        // around, so we need to shift the output indices as well:
        output_gates
            .into_iter()
            .map(|w| {
                if w > shift {
                    w - unused_before_gate[w - shift]
                } else {
                    w
                }
            })
            .collect()
    }

    pub fn build(mut self, output_gates: Vec<GateIndex>) -> Circuit {
        let output_gates = self.remove_unused_gates(output_gates);

        if PRINT_OPTIMIZATION_RATIO && self.gates_optimized > 0 {
            let optimized = self.gates_optimized * 100 / (self.gates.len() + self.gates_optimized);
            println!("Optimizations removed {optimized}% of all generated gates");
        }

        // All of the following shifts are necessary to translate between the intermediate circuit
        // representation (where indices 0 and 1 always refer to constant false and true values) and
        // the final representation (where there are no constant values and the constants have to be
        // 'built' by XOR'ing two identical input wire + using NOT):
        let input_shift = self.shift - 2;
        let shift_gate_index_if_necessary = |i: GateIndex| {
            if i <= 1 {
                i + input_shift
            } else if i < input_shift + 2 {
                i - 2
            } else {
                i
            }
        };
        let shift_gate_if_necessary = |gate: BuilderGate| match gate {
            BuilderGate::Xor(x, y) => {
                if x == 1 {
                    let y = shift_gate_index_if_necessary(y);
                    Gate::Not(y)
                } else if y == 1 {
                    let x = shift_gate_index_if_necessary(x);
                    Gate::Not(x)
                } else {
                    let x = shift_gate_index_if_necessary(x);
                    let y = shift_gate_index_if_necessary(y);
                    Gate::Xor(x, y)
                }
            }
            BuilderGate::And(x, y) => {
                let x = shift_gate_index_if_necessary(x);
                let y = shift_gate_index_if_necessary(y);
                Gate::And(x, y)
            }
        };
        let mut gates: Vec<Gate> = self
            .gates
            .into_iter()
            .map(shift_gate_if_necessary)
            .collect();
        gates.insert(0, Gate::Xor(0, 0)); // constant false
        gates.insert(1, Gate::Not(input_shift)); // constant true
        let output_gates = output_gates
            .into_iter()
            .map(shift_gate_index_if_necessary)
            .collect();

        let shift_gate_indexes_if_necessary = |mut indexes: [usize; USIZE_BITS]| -> [usize; USIZE_BITS] {
            for wire in indexes.iter_mut() {
                *wire = shift_gate_index_if_necessary(*wire);
            }
            indexes
        };
        let panic_gates = PanicResult {
            has_panicked: shift_gate_index_if_necessary(self.panic_gates.has_panicked),
            panic_type: shift_gate_indexes_if_necessary(self.panic_gates.panic_type),
            start_line: shift_gate_indexes_if_necessary(self.panic_gates.start_line),
            start_column: shift_gate_indexes_if_necessary(self.panic_gates.start_column),
            end_line: shift_gate_indexes_if_necessary(self.panic_gates.end_line),
            end_column: shift_gate_indexes_if_necessary(self.panic_gates.end_column),
        };
        Circuit {
            input_gates: self.input_gates,
            gates,
            output_gates,
            panic_gates,
        }
    }

    pub fn push_panic_if(&mut self, cond: GateIndex, reason: PanicReason, meta: MetaInfo) {
        let already_panicked = self.panic_gates.has_panicked;
        self.panic_gates.has_panicked = self.push_or(self.panic_gates.has_panicked, cond);
        let current = PanicResult {
            has_panicked: 1,
            panic_type: reason.as_bits(),
            start_line: unsigned_as_usize_bits(meta.start.0 as u128),
            start_column: unsigned_as_usize_bits(meta.start.1 as u128),
            end_line: unsigned_as_usize_bits(meta.end.0 as u128),
            end_column: unsigned_as_usize_bits(meta.end.1 as u128),
        };
        for i in 0..self.panic_gates.start_line.len() {
            self.panic_gates.start_line[i] = self.push_mux(
                already_panicked,
                self.panic_gates.start_line[i],
                current.start_line[i],
            );
            self.panic_gates.start_column[i] = self.push_mux(
                already_panicked,
                self.panic_gates.start_column[i],
                current.start_column[i],
            );
            self.panic_gates.end_line[i] = self.push_mux(
                already_panicked,
                self.panic_gates.end_line[i],
                current.end_line[i],
            );
            self.panic_gates.end_column[i] = self.push_mux(
                already_panicked,
                self.panic_gates.end_column[i],
                current.end_column[i],
            );
        }
        for i in 0..current.panic_type.len() {
            self.panic_gates.panic_type[i] = self.push_mux(
                already_panicked,
                self.panic_gates.panic_type[i],
                current.panic_type[i],
            );
        }
    }

    pub fn peek_panic(&self) -> &PanicResult {
        &self.panic_gates
    }

    pub fn replace_panic_with(&mut self, p: PanicResult) -> PanicResult {
        std::mem::replace(&mut self.panic_gates, p)
    }

    pub fn mux_panic(
        &mut self,
        condition: GateIndex,
        t: &PanicResult,
        f: &PanicResult,
    ) -> PanicResult {
        let mut panic_gates = PanicResult::ok();
        panic_gates.has_panicked = self.push_mux(condition, t.has_panicked, f.has_panicked);
        for (i, (&if_true, &if_false)) in t.panic_type.iter().zip(f.panic_type.iter()).enumerate() {
            panic_gates.panic_type[i] = self.push_mux(condition, if_true, if_false);
        }
        for (i, (&if_true, &if_false)) in t.start_line.iter().zip(f.start_line.iter()).enumerate() {
            panic_gates.start_line[i] = self.push_mux(condition, if_true, if_false);
        }
        for (i, (&if_true, &if_false)) in
            t.start_column.iter().zip(f.start_column.iter()).enumerate()
        {
            panic_gates.start_column[i] = self.push_mux(condition, if_true, if_false);
        }
        for (i, (&if_true, &if_false)) in t.end_line.iter().zip(f.end_line.iter()).enumerate() {
            panic_gates.end_line[i] = self.push_mux(condition, if_true, if_false);
        }
        for (i, (&if_true, &if_false)) in t.end_column.iter().zip(f.end_column.iter()).enumerate() {
            panic_gates.end_column[i] = self.push_mux(condition, if_true, if_false);
        }
        panic_gates
    }

    // - Constant evaluation (e.g. x ^ 0 == x; x & 1 == x; x & 0 == 0)
    // - Rewriting of equivalences up to a max depth (e.g. x & (!x | y) == x & y)
    //
    // `is_true` is set by `push_and` to simplify AND sub-exprs
    fn optimize_gate(&self, w: GateIndex, is_true: GateIndex, depth: u32) -> GateIndex {
        if depth >= MAX_OPTIMIZATION_DEPTH {
            return w;
        } else if w == is_true {
            return 1;
        } else if w >= self.shift {
            match self.gates[w - self.shift] {
                BuilderGate::Xor(x, y) => {
                    if let Some(optimized) = self.optimize_xor(x, y, is_true, depth + 1) {
                        return optimized;
                    }
                }
                BuilderGate::And(x, y) => {
                    if let Some(optimized) = self.optimize_and(x, y, is_true, depth + 1) {
                        return optimized;
                    }
                }
            }
        }
        w
    }

    // - Constant evaluation (e.g. x ^ 0 == x; x ^ x == 0)
    // - Rewriting of equivalences up to a max depth (e.g. x & (!x | y) == x & y)
    // - Sub-expression sharing (wires are re-used if a gate with the same type and inputs exists)
    //
    // `is_true` is set by `push_and` to simplify AND sub-exprs
    fn optimize_xor(
        &self,
        x: GateIndex,
        y: GateIndex,
        is_true: GateIndex,
        depth: u32,
    ) -> Option<GateIndex> {
        let x = self.optimize_gate(x, is_true, depth);
        let y = self.optimize_gate(y, is_true, depth);
        if x == 0 {
            return Some(y);
        } else if y == 0 {
            return Some(x);
        } else if x == y {
            return Some(0);
        } else if let Some(&x_negated) = self.negated.get(&x) {
            if x_negated == y {
                return Some(1);
            } else if y == 1 {
                return Some(x_negated);
            }
        } else if let Some(&y_negated) = self.negated.get(&y) {
            if y_negated == x {
                return Some(1);
            } else if x == 1 {
                return Some(y_negated);
            }
        }
        // Sub-expression sharing:
        if let Some(&wire) = self.cache.get(&BuilderGate::Xor(x, y)) {
            return Some(wire);
        }
        None
    }

    // - Constant evaluation (e.g. x & x == x; x & 1 == x; x & 0 == 0)
    // - Rewriting of equivalences up to a max depth (e.g. x & (!x | y) == x & y)
    // - Sub-expression sharing (wires are re-used if a gate with the same type and inputs exists)
    //
    // `is_true` is set by `push_and` to simplify AND sub-exprs
    fn optimize_and(
        &self,
        x: GateIndex,
        y: GateIndex,
        is_true: GateIndex,
        depth: u32,
    ) -> Option<GateIndex> {
        let x = self.optimize_gate(x, is_true, depth);
        let y = self.optimize_gate(y, is_true, depth);
        if x == 0 || y == 0 {
            return Some(0);
        } else if x == 1 {
            return Some(y);
        } else if y == 1 || x == y {
            return Some(x);
        } else if let Some(&x_negated) = self.negated.get(&x) {
            if x_negated == y {
                return Some(0);
            }
        } else if let Some(&y_negated) = self.negated.get(&y) {
            if y_negated == x {
                return Some(0);
            }
        }
        // Sub-expression sharing:
        if let Some(&wire) = self.cache.get(&BuilderGate::And(x, y)) {
            return Some(wire);
        }
        None
    }

    pub fn push_xor(&mut self, x: GateIndex, y: GateIndex) -> GateIndex {
        if let Some(optimized) = self.optimize_xor(x, y, 1, 0) {
            self.gates_optimized += 1;
            optimized
        } else {
            let gate = BuilderGate::Xor(x, y);
            self.gate_counter += 1;
            self.gates.push(gate);
            let gate_index = self.gate_counter - 1;
            self.cache.insert(gate, gate_index);
            if x == 1 {
                self.negated.insert(y, gate_index);
                self.negated.insert(gate_index, y);
            }
            if y == 1 {
                self.negated.insert(x, gate_index);
                self.negated.insert(gate_index, x);
            }
            gate_index
        }
    }

    pub fn push_and(&mut self, x: GateIndex, y: GateIndex) -> GateIndex {
        // if we have (x & y) and x and/or y are sub-expressions, we can simplify each
        // sub-expression while assuming that the other sub-expression is true (or the and as a
        // whole would evaluate to false), e.g.:
        //
        // x & (!x | y)
        // -> evaluate x while assuming that (!x | y) is true => no optimization possible
        // -> evaluate (x! | y) while assuming that x is true => simplify (x! | y) to (0 | y) == y
        // ==> whole expression is simplified to x & y
        let x = self.optimize_gate(x, y, 0);
        let y = self.optimize_gate(y, x, 0);
        if let Some(optimized) = self.optimize_and(x, y, 1, MAX_OPTIMIZATION_DEPTH) {
            self.gates_optimized += 1;
            optimized
        } else {
            let gate = BuilderGate::And(x, y);
            self.gate_counter += 1;
            self.gates.push(gate);
            self.cache.insert(gate, self.gate_counter - 1);
            self.gate_counter - 1
        }
    }

    pub fn push_not(&mut self, x: GateIndex) -> GateIndex {
        self.push_xor(x, 1)
    }

    pub fn push_or(&mut self, x: GateIndex, y: GateIndex) -> GateIndex {
        let xor = self.push_xor(x, y);
        let and = self.push_and(x, y);
        self.push_xor(xor, and)
    }

    pub fn push_eq(&mut self, x: GateIndex, y: GateIndex) -> GateIndex {
        let xor = self.push_xor(x, y);
        self.push_xor(xor, 1)
    }

    pub fn push_mux(&mut self, s: GateIndex, x0: GateIndex, x1: GateIndex) -> GateIndex {
        if x0 == x1 {
            return x0;
        }
        let not_s = self.push_not(s);
        let x0_selected = self.push_and(x0, s);
        let x1_selected = self.push_and(x1, not_s);
        self.push_xor(x0_selected, x1_selected)
    }

    pub fn push_adder(
        &mut self,
        x: GateIndex,
        y: GateIndex,
        carry: GateIndex,
    ) -> (GateIndex, GateIndex) {
        // first half-adder:
        let wire_u = self.push_xor(x, y);
        let wire_v = self.push_and(x, y);
        // second half-adder:
        let wire_s = self.push_xor(wire_u, carry);
        let wire_w = self.push_and(wire_u, carry);

        let carry = self.push_or(wire_v, wire_w);
        (wire_s, carry)
    }

    pub fn push_multiplier(
        &mut self,
        x: GateIndex,
        y: GateIndex,
        z: GateIndex,
        carry: GateIndex,
    ) -> (GateIndex, GateIndex) {
        let x_and_y = self.push_and(x, y);
        self.push_adder(x_and_y, z, carry)
    }

    pub fn push_addition_circuit(
        &mut self,
        x: &[GateIndex],
        y: &[GateIndex],
    ) -> (Vec<GateIndex>, GateIndex, GateIndex) {
        assert_eq!(x.len(), y.len());
        let bits = x.len();

        let mut carry_prev = 0;
        let mut carry = 0;
        let mut sum = vec![0; bits];
        // sequence of full adders:
        for i in (0..bits).rev() {
            let (s, c) = self.push_adder(x[i], y[i], carry);
            sum[i] = s;
            carry_prev = carry;
            carry = c;
        }
        (sum, carry, carry_prev)
    }

    pub fn push_negation_circuit(&mut self, x: &[GateIndex]) -> Vec<GateIndex> {
        // flip bits and increment to get negate:
        let mut carry = 1;
        let mut neg = vec![0; x.len()];
        for i in (0..x.len()).rev() {
            let x = self.push_not(x[i]);
            // half-adder:
            neg[i] = self.push_xor(carry, x);
            carry = self.push_and(carry, x);
        }
        neg
    }

    pub fn push_subtraction_circuit(
        &mut self,
        x: &[GateIndex],
        y: &[GateIndex],
    ) -> (Vec<GateIndex>, GateIndex) {
        assert_eq!(x.len(), y.len());
        let bits = x.len();

        // flip bits of y and increment y to get negative y:
        let mut carry = 1;
        let mut x_extended = vec![0; bits + 1];
        x_extended[1..].copy_from_slice(x);
        let mut z = vec![0; bits + 1];
        for i in (0..bits + 1).rev() {
            let y = if i == 0 { 1 } else { self.push_not(y[i - 1]) };
            // half-adder:
            z[i] = self.push_xor(carry, y);
            carry = self.push_and(carry, y);
        }

        let (mut sum_extended, _, _) = self.push_addition_circuit(&x_extended, &z);
        let sum = sum_extended.split_off(1);
        (sum, sum_extended[0])
    }

    pub fn push_unsigned_division_circuit(
        &mut self,
        x: &[GateIndex],
        y: &[GateIndex],
    ) -> (Vec<GateIndex>, Vec<GateIndex>) {
        assert_eq!(x.len(), y.len());
        let bits = x.len();

        let mut quotient = vec![0; bits];
        let mut remainder = x.to_vec();
        for shift_amount in (0..bits).rev() {
            let mut overflow = 0;
            let mut y_shifted = vec![0; bits];
            for y in y.iter().copied().take(shift_amount) {
                overflow = self.push_or(overflow, y);
            }
            y_shifted[..(bits - shift_amount)]
                .clone_from_slice(&y[shift_amount..((bits - shift_amount) + shift_amount)]);

            let (x_sub, carry) = self.push_subtraction_circuit(&remainder, &y_shifted);
            let carry_or_overflow = self.push_or(carry, overflow);
            for i in 0..bits {
                remainder[i] = self.push_mux(carry_or_overflow, remainder[i], x_sub[i]);
            }
            let quotient_bit = self.push_not(carry);
            quotient[bits - shift_amount - 1] = self.push_mux(overflow, 0, quotient_bit);
        }
        (quotient, remainder)
    }

    pub fn push_signed_division_circuit(
        &mut self,
        x: &mut Vec<GateIndex>,
        y: &mut Vec<GateIndex>,
    ) -> (Vec<GateIndex>, Vec<GateIndex>) {
        assert_eq!(x.len(), y.len());
        let bits = x.len();

        let is_result_neg = self.push_xor(x[0], y[0]);
        let x_negated = self.push_negation_circuit(x);
        let x_sign_bit = x[0];
        for i in 0..bits {
            x[i] = self.push_mux(x_sign_bit, x_negated[i], x[i]);
        }
        let y_negated = self.push_negation_circuit(y);
        let y_sign_bit = y[0];
        for i in 0..bits {
            y[i] = self.push_mux(y_sign_bit, y_negated[i], y[i]);
        }
        let (mut quotient, mut remainder) = self.push_unsigned_division_circuit(x, y);
        let quot_neg = self.push_negation_circuit(&quotient);
        for i in 0..bits {
            quotient[i] = self.push_mux(is_result_neg, quot_neg[i], quotient[i]);
        }
        let rem_neg = self.push_negation_circuit(&remainder);
        for i in 0..bits {
            remainder[i] = self.push_mux(x_sign_bit, rem_neg[i], remainder[i]);
        }
        (quotient, remainder)
    }

    pub fn push_comparator_circuit(
        &mut self,
        bits: usize,
        x: &[GateIndex],
        is_x_signed: bool,
        y: &[GateIndex],
        is_y_signed: bool,
    ) -> (GateIndex, GateIndex) {
        let mut acc_gt = 0;
        let mut acc_lt = 0;
        for i in 0..bits {
            let xor = self.push_xor(x[i], y[i]);

            let xor_and_x = self.push_and(xor, x[i]);
            let xor_and_y = self.push_and(xor, y[i]);
            let (gt, lt) = if i == 0 && (is_x_signed || is_y_signed) {
                (xor_and_y, xor_and_x)
            } else {
                (xor_and_x, xor_and_y)
            };

            let gt = self.push_or(gt, acc_gt);
            let lt = self.push_or(lt, acc_lt);

            let not_acc_gt = self.push_not(acc_gt);
            let not_acc_lt = self.push_not(acc_lt);

            acc_gt = self.push_and(gt, not_acc_lt);
            acc_lt = self.push_and(lt, not_acc_gt)
        }
        (acc_lt, acc_gt)
    }
}

fn unsigned_as_usize_bits(n: u128) -> [usize; USIZE_BITS] {
    let mut bits = [0; USIZE_BITS];
    for i in 0..USIZE_BITS {
        bits[i] = (n >> (USIZE_BITS - 1 - i) & 1) as usize;
    }
    bits
}
