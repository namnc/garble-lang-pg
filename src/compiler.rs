use std::{cmp::max, collections::HashMap};

use crate::{
    ast::{Op, ParamDef, Party, Type, UnaryOp},
    circuit::{Circuit, Gate, GateIndex},
    env::Env,
    typed_ast::{Expr, ExprEnum, FnDef, Program},
};

impl Program {
    pub fn compile(&self) -> Circuit {
        let mut fns = HashMap::new();
        for fn_def in self.fn_defs.iter() {
            fns.insert(fn_def.identifier.clone(), fn_def);
        }
        let mut env = Env::new();
        let mut gates = vec![];
        let mut wire = 2;
        for (party, ParamDef(identifier, ty)) in self.main.params.iter() {
            let type_size = ty.size_in_bits();
            let mut wires = Vec::with_capacity(type_size);
            for _i in 0..type_size {
                match party {
                    Party::A => gates.push(Gate::InA),
                    Party::B => gates.push(Gate::InB),
                };
                wires.push(wire);
                wire += 1;
            }
            env.set(identifier.clone(), wires);
        }
        let output_gates = self.main.body.compile(&fns, &mut env, &mut gates);
        Circuit {
            gates,
            output_gates,
        }
    }
}

fn push_gate(gates: &mut Vec<Gate>, gate: Gate) -> GateIndex {
    gates.push(gate);
    gates.len() + 1 // because index 0 and 1 are reserved for const true/false
}

fn push_not(gates: &mut Vec<Gate>, x: GateIndex) -> GateIndex {
    push_gate(gates, Gate::Xor(x, 1))
}

fn push_or(gates: &mut Vec<Gate>, x: GateIndex, y: GateIndex) -> GateIndex {
    let xor = push_gate(gates, Gate::Xor(x, y));
    let and = push_gate(gates, Gate::And(x, y));
    push_gate(gates, Gate::Xor(xor, and))
}

fn push_mux(gates: &mut Vec<Gate>, s: GateIndex, x0: GateIndex, x1: GateIndex) -> GateIndex {
    if x0 == x1 {
        return x0;
    }
    let not_s = push_not(gates, s);
    let x0_selected = push_gate(gates, Gate::And(x0, s));
    let x1_selected = push_gate(gates, Gate::And(x1, not_s));
    push_gate(gates, Gate::Xor(x0_selected, x1_selected))
}

fn push_adder(
    gates: &mut Vec<Gate>,
    x: GateIndex,
    y: GateIndex,
    carry: GateIndex,
) -> (GateIndex, GateIndex) {
    // first half-adder:
    let u = push_gate(gates, Gate::Xor(x, y));
    let v = push_gate(gates, Gate::And(x, y));
    // second half-adder:
    let s = push_gate(gates, Gate::Xor(u, carry));
    let w = push_gate(gates, Gate::And(u, carry));

    let carry = push_or(gates, v, w);
    (s, carry)
}

fn push_multiplier(
    gates: &mut Vec<Gate>,
    x: GateIndex,
    y: GateIndex,
    z: GateIndex,
    carry: GateIndex,
) -> (GateIndex, GateIndex) {
    let x_and_y = push_gate(gates, Gate::And(x, y));
    push_adder(gates, x_and_y, z, carry)
}

fn push_addition_circuit(
    gates: &mut Vec<Gate>,
    x: &Vec<GateIndex>,
    y: &Vec<GateIndex>,
) -> (Vec<GateIndex>, GateIndex) {
    assert_eq!(x.len(), y.len());
    let bits = x.len();

    let mut carry = 0;
    let mut sum = vec![0; bits];
    // sequence of full adders:
    for i in (0..bits).rev() {
        let (s, c) = push_adder(gates, x[i], y[i], carry);
        sum[i] = s;
        carry = c;
    }
    (sum, carry)
}

fn push_negation_circuit(gates: &mut Vec<Gate>, x: &Vec<GateIndex>) -> Vec<GateIndex> {
    // flip bits and increment to get negate:
    let mut carry = 1;
    let mut neg = vec![0; x.len()];
    for i in (0..x.len()).rev() {
        let x = push_not(gates, x[i]);
        // half-adder:
        neg[i] = push_gate(gates, Gate::Xor(carry, x));
        carry = push_gate(gates, Gate::And(carry, x));
    }
    neg
}

fn push_subtraction_circuit(
    gates: &mut Vec<Gate>,
    x: &Vec<GateIndex>,
    y: &Vec<GateIndex>,
) -> (Vec<GateIndex>, GateIndex) {
    assert_eq!(x.len(), y.len());
    let bits = x.len();

    // flip bits of y and increment y to get negative y:
    let mut carry = 1;
    let mut x_extended = vec![0; bits + 1];
    x_extended[1..].copy_from_slice(&x);
    let mut z = vec![0; bits + 1];
    for i in (0..bits + 1).rev() {
        let y = if i == 0 { 1 } else { push_not(gates, y[i - 1]) };
        // half-adder:
        z[i] = push_gate(gates, Gate::Xor(carry, y));
        carry = push_gate(gates, Gate::And(carry, y));
    }

    let (mut sum_extended, _carry) = push_addition_circuit(gates, &x_extended, &z);
    let sum = sum_extended.split_off(1);
    (sum, sum_extended[0])
}

fn push_unsigned_division_circuit(
    gates: &mut Vec<Gate>,
    x: &Vec<GateIndex>,
    y: &Vec<GateIndex>,
) -> (Vec<GateIndex>, Vec<GateIndex>) {
    assert_eq!(x.len(), y.len());
    let bits = x.len();

    let mut quotient = vec![0; bits];
    let mut remainder = x.clone();
    for shift_amount in (0..bits).rev() {
        let mut overflow = 0;
        let mut y_shifted = vec![0; bits];
        for shift in 0..shift_amount {
            overflow = push_or(gates, overflow, y[shift]);
        }
        for j in 0..(bits - shift_amount) {
            y_shifted[j] = y[j + shift_amount];
        }

        let (x_sub, carry) = push_subtraction_circuit(gates, &remainder, &y_shifted);
        let carry_or_overflow = push_or(gates, carry, overflow);
        for i in 0..bits {
            remainder[i] = push_mux(gates, carry_or_overflow, remainder[i], x_sub[i]);
        }
        let quotient_bit = push_not(gates, carry);
        quotient[bits - shift_amount - 1] = push_mux(gates, overflow, 0, quotient_bit);
    }
    (quotient, remainder)
}

fn push_signed_division_circuit(
    gates: &mut Vec<Gate>,
    x: &mut Vec<GateIndex>,
    y: &mut Vec<GateIndex>,
) -> (Vec<GateIndex>, Vec<GateIndex>) {
    assert_eq!(x.len(), y.len());
    let bits = x.len();

    let is_result_neg = push_gate(gates, Gate::Xor(x[0], y[0]));
    let x_negated = push_negation_circuit(gates, &x);
    let x_sign_bit = x[0];
    for i in 0..bits {
        x[i] = push_mux(gates, x_sign_bit, x_negated[i], x[i]);
    }
    let y_negated = push_negation_circuit(gates, &y);
    let y_sign_bit = y[0];
    for i in 0..bits {
        y[i] = push_mux(gates, y_sign_bit, y_negated[i], y[i]);
    }
    let (mut quotient, mut remainder) = push_unsigned_division_circuit(gates, &x, &y);
    let quot_neg = push_negation_circuit(gates, &quotient);
    for i in 0..bits {
        quotient[i] = push_mux(gates, is_result_neg, quot_neg[i], quotient[i]);
    }
    let rem_neg = push_negation_circuit(gates, &remainder);
    for i in 0..bits {
        remainder[i] = push_mux(gates, x_sign_bit, rem_neg[i], remainder[i]);
    }
    (quotient, remainder)
}

fn extend_to_bits(v: &mut Vec<usize>, ty: &Type, bits: usize) {
    if v.len() != bits {
        let old_size = v.len();
        v.resize(bits, 0);
        v.copy_within(0..old_size, bits - old_size);
        if let Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 = ty {
            v[0..old_size].fill(1);
        } else {
            v[0..old_size].fill(0);
        }
    }
}

impl Expr {
    fn compile(
        &self,
        fns: &HashMap<String, &FnDef>,
        env: &mut Env<Vec<GateIndex>>,
        gates: &mut Vec<Gate>,
    ) -> Vec<GateIndex> {
        let Expr(expr, ty, meta) = self;
        match expr {
            ExprEnum::True => {
                vec![1]
            }
            ExprEnum::False => {
                vec![0]
            }
            ExprEnum::NumUnsigned(n) => {
                let mut bits = Vec::with_capacity(ty.size_in_bits());
                unsigned_to_bits(*n, ty.size_in_bits(), &mut bits);
                bits.into_iter().map(|b| b as usize).collect()
            }
            ExprEnum::NumSigned(n) => {
                let mut bits = Vec::with_capacity(ty.size_in_bits());
                signed_to_bits(*n, ty.size_in_bits(), &mut bits);
                bits.into_iter().map(|b| b as usize).collect()
            }
            ExprEnum::Identifier(s) => env.get(s).unwrap(),
            ExprEnum::ArrayLiteral(elem, size) => {
                let elem_ty = elem.1.clone();
                let mut elem = elem.compile(fns, env, gates);
                extend_to_bits(&mut elem, &elem_ty, elem_ty.size_in_bits());
                let bits = ty.size_in_bits();
                let mut array = Vec::with_capacity(bits);
                for _ in 0..*size {
                    array.extend_from_slice(&elem);
                }
                array
            }
            ExprEnum::ArrayAccess(array, index) => {
                let elem_bits = ty.size_in_bits();
                let mut array = array.compile(fns, env, gates);
                let mut index = index.compile(fns, env, gates);
                extend_to_bits(&mut index, &Type::Usize, Type::Usize.size_in_bits());
                let out_of_bounds_elem = 1;
                for mux_layer in (0..index.len()).rev() {
                    let mut muxed_array = Vec::new();
                    let s = index[mux_layer];
                    let mut i = 0;
                    while i < array.len() {
                        for _ in 0..elem_bits {
                            if i + elem_bits < array.len() {
                                let a0 = array[i];
                                let a1 = array[i + elem_bits];
                                muxed_array.push(push_mux(gates, s, a1, a0));
                            } else if i < array.len() {
                                let a0 = array[i];
                                muxed_array.push(push_mux(gates, s, out_of_bounds_elem, a0));
                            }
                            i += 1;
                        }
                        i += elem_bits;
                    }
                    array = muxed_array;
                }
                array
            }
            ExprEnum::ArrayAssignment(array, index, value) => {
                let (elem_ty, size) = match ty {
                    Type::Array(elem_ty, size) => (elem_ty, size),
                    _ => panic!(
                        "Expected array assignment to have an array type, but found {:?}",
                        ty
                    ),
                };
                let mut array = array.compile(fns, env, gates);
                let mut index = index.compile(fns, env, gates);
                let value = value.compile(fns, env, gates);
                extend_to_bits(&mut index, &Type::Usize, Type::Usize.size_in_bits());
                let elem_bits = elem_ty.size_in_bits();

                let mut index_negated = vec![0; index.len()];
                for (i, index) in index.iter().copied().enumerate() {
                    index_negated[i] = push_not(gates, index);
                }
                // for each array element...
                for i in 0..*size {
                    // ...and each bit of that array element...
                    for b in 0..elem_bits {
                        // ...use a index-length chain of mux, select the value if index == i
                        let mut x1 = value[b];
                        for s in 0..index.len() {
                            let s_must_be_negated = ((i >> (index.len() - s - 1)) & 1) > 0;
                            let s = if s_must_be_negated {
                                index_negated[s]
                            } else {
                                index[s]
                            };
                            // x0 is selected by the mux-chain whenever a single bit of index != i
                            let x0 = array[i * elem_bits + b];
                            // x1 is value[b] only if index == i in all bits
                            x1 = push_mux(gates, s, x0, x1);
                        }
                        array[i * elem_bits + b] = x1;
                    }
                }
                array
            }
            ExprEnum::UnaryOp(UnaryOp::Neg, x) => {
                let x = x.compile(fns, env, gates);
                push_negation_circuit(gates, &x)
            }
            ExprEnum::UnaryOp(UnaryOp::Not, x) => {
                let x = x.compile(fns, env, gates);
                let mut flipped = vec![0; x.len()];
                for (i, x) in x.iter().enumerate() {
                    flipped[i] = push_not(gates, *x);
                }
                flipped
            }
            ExprEnum::Op(op @ (Op::ShiftLeft | Op::ShiftRight), x, y) => {
                let x = x.compile(fns, env, gates);
                let y = y.compile(fns, env, gates);
                assert_eq!(y.len(), 8);
                let bits = x.len();
                let bit_to_shift_in = x[0];
                let mut shift = 1;
                let mut bits_unshifted = x;
                for layer in (0..8).rev() {
                    let s = y[layer];
                    let mut bits_shifted = vec![0; bits];
                    for i in 0..bits {
                        let unshifted = bits_unshifted[i];
                        let shifted = if op == &Op::ShiftLeft {
                            if i + shift >= bits {
                                0
                            } else {
                                bits_unshifted[i + shift]
                            }
                        } else {
                            if i < shift {
                                bit_to_shift_in
                            } else {
                                bits_unshifted[i - shift]
                            }
                        };
                        bits_shifted[i] = push_mux(gates, s, shifted, unshifted);
                    }
                    shift *= 2;
                    bits_unshifted = bits_shifted;
                }
                bits_unshifted
            }
            ExprEnum::Op(op, x, y) => {
                let ty_x = &x.1;
                let ty_y = &y.1;
                let mut x = x.compile(fns, env, gates);
                let mut y = y.compile(fns, env, gates);
                let bits = max(x.len(), y.len());
                extend_to_bits(&mut x, ty_x, bits);
                extend_to_bits(&mut y, ty_y, bits);
                match op {
                    Op::BitAnd => {
                        let mut output_bits = vec![0; bits];
                        for i in 0..bits {
                            output_bits[i] = push_gate(gates, Gate::And(x[i], y[i]));
                        }
                        output_bits
                    }
                    Op::BitXor => {
                        let mut output_bits = vec![0; bits];
                        for i in 0..bits {
                            output_bits[i] = push_gate(gates, Gate::Xor(x[i], y[i]));
                        }
                        output_bits
                    }
                    Op::BitOr => {
                        let mut output_bits = vec![0; bits];
                        for i in 0..bits {
                            let xor = push_gate(gates, Gate::Xor(x[i], y[i]));
                            let and = push_gate(gates, Gate::And(x[i], y[i]));
                            let or = push_gate(gates, Gate::Xor(xor, and));
                            output_bits[i] = or;
                        }
                        output_bits
                    }
                    Op::Sub => push_subtraction_circuit(gates, &x, &y).0,
                    Op::Add => push_addition_circuit(gates, &x, &y).0,
                    Op::Mul => {
                        let mut sums: Vec<Vec<GateIndex>> = vec![vec![0; bits]; bits];
                        let mut carries: Vec<Vec<GateIndex>> = vec![vec![0; bits]; bits];
                        let lsb_index = bits - 1;
                        for i in (0..bits).rev() {
                            for j in (0..bits).rev() {
                                let carry = if j == lsb_index { 0 } else { carries[i][j + 1] };
                                let z = if i == lsb_index {
                                    0
                                } else if j == 0 {
                                    carries[i + 1][j]
                                } else {
                                    sums[i + 1][j - 1]
                                };
                                let (s, c) = push_multiplier(gates, x[i], y[j], z, carry);
                                sums[i][j] = s;
                                carries[i][j] = c;
                            }
                        }
                        let mut result = vec![0; bits];
                        for (i, s) in sums.into_iter().enumerate() {
                            result[i] = s[lsb_index];
                        }
                        result
                    }
                    Op::Div => {
                        if is_signed(ty) {
                            push_signed_division_circuit(gates, &mut x, &mut y).0
                        } else {
                            push_unsigned_division_circuit(gates, &x, &y).0
                        }
                    }
                    Op::Mod => {
                        if is_signed(ty) {
                            push_signed_division_circuit(gates, &mut x, &mut y).1
                        } else {
                            push_unsigned_division_circuit(gates, &x, &y).1
                        }
                    }
                    Op::GreaterThan | Op::LessThan => {
                        let mut acc_gt = 0;
                        let mut acc_lt = 0;
                        let is_x_signed = is_signed(ty_x);
                        let is_y_signed = is_signed(ty_y);
                        for i in 0..bits {
                            let xor = push_gate(gates, Gate::Xor(x[i], y[i]));

                            let (gt, lt) = if i == 0 && (is_x_signed || is_y_signed) {
                                let lt = push_gate(gates, Gate::And(xor, x[i]));
                                let gt = push_gate(gates, Gate::And(xor, y[i]));
                                (gt, lt)
                            } else {
                                let gt = push_gate(gates, Gate::And(xor, x[i]));
                                let lt = push_gate(gates, Gate::And(xor, y[i]));
                                (gt, lt)
                            };

                            let gt = push_or(gates, gt, acc_gt);
                            let lt = push_or(gates, lt, acc_lt);

                            let not_acc_gt = push_not(gates, acc_gt);
                            let not_acc_lt = push_not(gates, acc_lt);

                            acc_gt = push_gate(gates, Gate::And(gt, not_acc_lt));
                            acc_lt = push_gate(gates, Gate::And(lt, not_acc_gt))
                        }

                        match op {
                            Op::GreaterThan => vec![acc_gt],
                            Op::LessThan => vec![acc_lt],
                            _ => unreachable!(),
                        }
                    }
                    Op::Eq | Op::NotEq => {
                        let mut acc = 1;
                        for i in 0..bits {
                            let xor = push_gate(gates, Gate::Xor(x[i], y[i]));
                            let eq = push_gate(gates, Gate::Xor(xor, 1));
                            acc = push_gate(gates, Gate::And(acc, eq));
                        }
                        match op {
                            Op::Eq => vec![acc],
                            Op::NotEq => vec![push_gate(gates, Gate::Xor(acc, 1))],
                            _ => unreachable!(),
                        }
                    }
                    Op::ShiftLeft => {
                        unreachable!("handled in the match clause one layer up")
                    }
                    Op::ShiftRight => {
                        unreachable!("handled in the match clause one layer up")
                    }
                }
            }
            ExprEnum::Let(var, binding, body) => {
                let binding = binding.compile(fns, env, gates);
                env.push();
                env.set(var.clone(), binding);
                let body = body.compile(fns, env, gates);
                env.pop();
                body
            }
            ExprEnum::FnCall(identifier, args) => {
                let fn_def = *fns.get(identifier).unwrap();
                let mut body = fn_def.body.clone();
                for (ParamDef(identifier, ty), arg) in fn_def.params.iter().zip(args) {
                    let let_expr =
                        ExprEnum::Let(identifier.clone(), Box::new(arg.clone()), Box::new(body));
                    body = Expr(let_expr, ty.clone(), meta.clone())
                }
                env.push();
                let body = body.compile(fns, env, gates);
                env.pop();
                body
            }
            ExprEnum::If(condition, case_true, case_false) => {
                let condition = condition.compile(fns, env, gates);
                let case_true = case_true.compile(fns, env, gates);
                let case_false = case_false.compile(fns, env, gates);
                assert_eq!(condition.len(), 1);
                assert_eq!(case_true.len(), case_false.len());
                let condition = condition[0];
                let not_condition = push_gate(gates, Gate::Xor(condition, 1));
                let mut gate_indexes = Vec::with_capacity(case_true.len());
                for i in 0..case_true.len() {
                    let case_true = case_true[i];
                    let case_false = case_false[i];
                    let gate_if_true = push_gate(gates, Gate::And(case_true, condition));
                    let gate_if_false = push_gate(gates, Gate::And(case_false, not_condition));
                    gate_indexes.push(push_gate(gates, Gate::Xor(gate_if_true, gate_if_false)));
                }
                gate_indexes
            }
            ExprEnum::Cast(ty, expr) => {
                let ty_expr = &expr.1;
                let mut expr = expr.compile(fns, env, gates);
                let size_after_cast = ty.size_in_bits();
                let result = match size_after_cast.cmp(&expr.len()) {
                    std::cmp::Ordering::Equal => expr,
                    std::cmp::Ordering::Less => expr[(expr.len() - size_after_cast)..].to_vec(),
                    std::cmp::Ordering::Greater => {
                        extend_to_bits(&mut expr, ty_expr, size_after_cast);
                        expr
                    }
                };
                result
            }
            ExprEnum::Fold(array, init, closure) => {
                let array = array.compile(fns, env, gates);
                let mut init = init.compile(fns, env, gates);

                let ParamDef(init_identifier, init_ty) = &closure.params[0];
                let ParamDef(elem_identifier, elem_ty) = &closure.params[1];
                let elem_bits = elem_ty.size_in_bits();
                extend_to_bits(&mut init, init_ty, init_ty.size_in_bits());

                let mut acc = init;
                let mut i = 0;
                while i < array.len() {
                    let elem = &array[i..i + elem_bits];
                    env.push();
                    env.set(init_identifier.clone(), acc);
                    env.set(elem_identifier.clone(), elem.to_vec());
                    acc = closure.body.compile(fns, env, gates);
                    env.pop();
                    i += elem_bits;
                }
                acc
            }
            ExprEnum::Map(array, closure) => {
                let array = array.compile(fns, env, gates);

                let ParamDef(elem_identifier, elem_ty) = &closure.params[0];
                let elem_in_bits = elem_ty.size_in_bits();
                let elem_out_bits = closure.ty.size_in_bits();
                let size = array.len() / elem_in_bits;

                let mut i = 0;
                let mut result = Vec::with_capacity(elem_out_bits * size);
                while i < array.len() {
                    let elem_in = &array[i..i + elem_in_bits];
                    env.push();
                    env.set(elem_identifier.clone(), elem_in.to_vec());
                    let elem_out = closure.body.compile(fns, env, gates);
                    result.extend(elem_out);
                    env.pop();
                    i += elem_in_bits;
                }
                result
            }
        }
    }
}

fn is_signed(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128
    )
}

impl Type {
    fn size_in_bits(&self) -> usize {
        match self {
            Type::Bool => 1,
            Type::Usize => usize::BITS as usize,
            Type::U8 | Type::I8 => 8,
            Type::U16 | Type::I16 => 16,
            Type::U32 | Type::I32 => 32,
            Type::U64 | Type::I64 => 64,
            Type::U128 | Type::I128 => 128,
            Type::Array(elem, size) => elem.size_in_bits() * size,
            Type::Fn(_, _) => panic!("Fn types cannot be directly mapped to bits"),
        }
    }
}

pub struct Computation {
    circuit: Circuit,
    in_a: Vec<bool>,
    in_b: Vec<bool>,
    output: Option<Vec<Option<bool>>>,
}

impl Into<Computation> for Circuit {
    fn into(self) -> Computation {
        Computation {
            circuit: self,
            in_a: vec![],
            in_b: vec![],
            output: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ComputeError {
    UnexpectedNumberOfInputsFromPartyA(usize),
    UnexpectedNumberOfInputsFromPartyB(usize),
    OutputHasNotBeenComputed,
    OutputTypeMismatch { expected: Type, actual_bits: usize },
}

impl Computation {
    pub fn run(&mut self) -> Result<(), ComputeError> {
        let mut expected_in_a_len = 0;
        let mut expected_in_b_len = 0;
        for gate in self.circuit.gates.iter() {
            match gate {
                Gate::InA => expected_in_a_len += 1,
                Gate::InB => expected_in_b_len += 1,
                _ => {}
            }
        }
        if self.in_a.len() != expected_in_a_len {
            return Err(ComputeError::UnexpectedNumberOfInputsFromPartyA(
                self.in_a.len(),
            ));
        }
        if self.in_b.len() != expected_in_b_len {
            return Err(ComputeError::UnexpectedNumberOfInputsFromPartyB(
                self.in_b.len(),
            ));
        }
        self.output = Some(self.circuit.eval(&self.in_a, &self.in_b));
        self.in_a.clear();
        self.in_b.clear();
        Ok(())
    }

    fn get_input(&mut self, p: Party) -> &mut Vec<bool> {
        match p {
            Party::A => &mut self.in_a,
            Party::B => &mut self.in_b,
        }
    }

    fn get_output(&self) -> Result<Vec<bool>, ComputeError> {
        if let Some(output) = &self.output {
            let mut output_bits = Vec::with_capacity(self.circuit.output_gates.len());
            for output_gate in &self.circuit.output_gates {
                output_bits.push(output[*output_gate].unwrap());
            }
            Ok(output_bits)
        } else {
            Err(ComputeError::OutputHasNotBeenComputed)
        }
    }

    pub fn set_bool(&mut self, p: Party, b: bool) {
        let inputs = self.get_input(p);
        inputs.push(b);
    }

    pub fn get_bool(&self) -> Result<bool, ComputeError> {
        let output = self.get_output()?;
        if output.len() == 1 {
            Ok(output[0])
        } else {
            Err(ComputeError::OutputTypeMismatch {
                expected: Type::Bool,
                actual_bits: output.len(),
            })
        }
    }

    pub fn set_usize(&mut self, p: Party, n: usize) {
        let inputs = self.get_input(p);
        unsigned_to_bits(n as u128, usize::BITS as usize, inputs);
    }

    pub fn set_u8(&mut self, p: Party, n: u8) {
        let inputs = self.get_input(p);
        unsigned_to_bits(n as u128, 8, inputs);
    }

    pub fn set_u16(&mut self, p: Party, n: u16) {
        let inputs = self.get_input(p);
        unsigned_to_bits(n as u128, 16, inputs);
    }

    pub fn set_u32(&mut self, p: Party, n: u32) {
        let inputs = self.get_input(p);
        unsigned_to_bits(n as u128, 32, inputs);
    }

    pub fn set_u64(&mut self, p: Party, n: u64) {
        let inputs = self.get_input(p);
        unsigned_to_bits(n as u128, 64, inputs);
    }

    pub fn set_u128(&mut self, p: Party, n: u128) {
        let inputs = self.get_input(p);
        unsigned_to_bits(n, 128, inputs);
    }

    pub fn set_i8(&mut self, p: Party, n: i8) {
        let inputs = self.get_input(p);
        signed_to_bits(n as i128, 8, inputs);
    }

    pub fn set_i16(&mut self, p: Party, n: i16) {
        let inputs = self.get_input(p);
        signed_to_bits(n as i128, 16, inputs);
    }

    pub fn set_i32(&mut self, p: Party, n: i32) {
        let inputs = self.get_input(p);
        signed_to_bits(n as i128, 32, inputs);
    }

    pub fn set_i64(&mut self, p: Party, n: i64) {
        let inputs = self.get_input(p);
        signed_to_bits(n as i128, 64, inputs);
    }

    pub fn set_i128(&mut self, p: Party, n: i128) {
        let inputs = self.get_input(p);
        signed_to_bits(n, 128, inputs);
    }

    fn get_unsigned(&self, ty: Type) -> Result<u128, ComputeError> {
        let output = self.get_output()?;
        let size = ty.size_in_bits();
        if output.len() == size {
            let mut n = 0;
            for i in 0..size {
                n += (output[i] as u128) << (size - 1 - i);
            }
            Ok(n)
        } else {
            Err(ComputeError::OutputTypeMismatch {
                expected: ty,
                actual_bits: output.len(),
            })
        }
    }

    fn get_signed(&self, ty: Type) -> Result<i128, ComputeError> {
        let output = self.get_output()?;
        let size = ty.size_in_bits();
        if output.len() == size {
            let mut n = 0;
            for i in 0..size {
                n += (output[i] as i128) << (size - 1 - i);
            }
            Ok(n)
        } else {
            Err(ComputeError::OutputTypeMismatch {
                expected: ty,
                actual_bits: output.len(),
            })
        }
    }

    pub fn get_usize(&self) -> Result<usize, ComputeError> {
        self.get_unsigned(Type::Usize).map(|n| n as usize)
    }

    pub fn get_u8(&self) -> Result<u8, ComputeError> {
        self.get_unsigned(Type::U8).map(|n| n as u8)
    }

    pub fn get_u16(&self) -> Result<u16, ComputeError> {
        self.get_unsigned(Type::U16).map(|n| n as u16)
    }

    pub fn get_u32(&self) -> Result<u32, ComputeError> {
        self.get_unsigned(Type::U32).map(|n| n as u32)
    }

    pub fn get_u64(&self) -> Result<u64, ComputeError> {
        self.get_unsigned(Type::U64).map(|n| n as u64)
    }

    pub fn get_u128(&self) -> Result<u128, ComputeError> {
        self.get_unsigned(Type::U128)
    }

    pub fn get_i8(&self) -> Result<i8, ComputeError> {
        self.get_signed(Type::I8).map(|n| n as i8)
    }

    pub fn get_i16(&self) -> Result<i16, ComputeError> {
        self.get_signed(Type::I16).map(|n| n as i16)
    }

    pub fn get_i32(&self) -> Result<i32, ComputeError> {
        self.get_signed(Type::I32).map(|n| n as i32)
    }

    pub fn get_i64(&self) -> Result<i64, ComputeError> {
        self.get_signed(Type::I64).map(|n| n as i64)
    }

    pub fn get_i128(&self) -> Result<i128, ComputeError> {
        self.get_signed(Type::I128)
    }
}

fn unsigned_to_bits(n: u128, size: usize, bits: &mut Vec<bool>) {
    for i in 0..size {
        bits.push((n >> (size - 1 - i) & 1) == 1);
    }
}

fn signed_to_bits(n: i128, size: usize, bits: &mut Vec<bool>) {
    for i in 0..size {
        bits.push((n >> (size - 1 - i) & 1) == 1);
    }
}
