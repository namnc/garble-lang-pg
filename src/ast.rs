//! The untyped Abstract Syntax Tree (AST).

use std::collections::HashMap;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::token::{MetaInfo, SignedNumType, UnsignedNumType};

/// A program, consisting of top level definitions (enums or functions).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Program<T> {
    /// Top level struct type definitions.
    pub struct_defs: HashMap<String, StructDef>,
    /// Top level enum type definitions.
    pub enum_defs: HashMap<String, EnumDef>,
    /// Top level function definitions.
    pub fn_defs: HashMap<String, FnDef<T>>,
}

/// A top level struct type definition.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct StructDef {
    /// The variants of the enum type.
    pub fields: Vec<(String, Type)>,
    /// The location in the source code.
    pub meta: MetaInfo,
}

/// A top level enum type definition.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EnumDef {
    /// The variants of the enum type.
    pub variants: Vec<Variant>,
    /// The location in the source code.
    pub meta: MetaInfo,
}

impl EnumDef {
    pub(crate) fn get_variant(&self, variant_name: &str) -> Option<&Variant> {
        for variant in self.variants.iter() {
            if variant.variant_name() == variant_name {
                return Some(variant);
            }
        }
        None
    }
}

/// An enum variant.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Variant {
    /// A unit variant with the specified name, but containing no fields.
    Unit(String),
    /// A tuple variant with the specified name, containing positional fields.
    Tuple(String, Vec<Type>),
}

impl Variant {
    pub(crate) fn variant_name(&self) -> &str {
        match self {
            Variant::Unit(name) => name.as_str(),
            Variant::Tuple(name, _) => name.as_str(),
        }
    }

    pub(crate) fn types(&self) -> Option<Vec<Type>> {
        match self {
            Variant::Unit(_) => None,
            Variant::Tuple(_, types) => Some(types.clone()),
        }
    }
}

/// A top level function definition.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FnDef<T> {
    /// Whether or not the function is public.
    pub is_pub: bool,
    /// The name of the function.
    pub identifier: String,
    /// The return type of the function.
    pub ty: Type,
    /// The parameters of the function.
    pub params: Vec<ParamDef>,
    /// The body expression that the function evaluates to.
    pub body: Vec<Stmt<T>>,
    /// The location in the source code.
    pub meta: MetaInfo,
}

/// A parameter definition (mutability flag, parameter name and type).
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ParamDef(pub Mutability, pub String, pub Type);

/// Indicates whether a variable is declared as mutable.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Mutability {
    /// The variable is declared as mutable.
    Mutable,
    /// The variable is declared as immutable.
    Immutable,
}

impl From<bool> for Mutability {
    fn from(b: bool) -> Self {
        if b {
            Mutability::Mutable
        } else {
            Mutability::Immutable
        }
    }
}

/// Either a concrete type or a struct/enum that needs to be looked up in the definitions.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Type {
    /// Boolean type with the values true and false.
    Bool,
    /// Unsigned number types
    Unsigned(UnsignedNumType),
    /// Signed number types
    Signed(SignedNumType),
    /// Function type with the specified parameters and the specified return type.
    Fn(Vec<Type>, Box<Type>),
    /// Array type of a fixed size, containing elements of the specified type.
    Array(Box<Type>, usize),
    /// Tuple type containing fields of the specified types.
    Tuple(Vec<Type>),
    /// A struct or an enum, depending on the top level definitions (used only before typechecking).
    UntypedTopLevelDefinition(String, MetaInfo),
    /// Struct type of the specified name, needs to be looked up in struct defs for its field types.
    Struct(String),
    /// Enum type of the specified name, needs to be looked up in enum defs for its variant types.
    Enum(String),
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Bool => f.write_str("bool"),
            Type::Unsigned(n) => n.fmt(f),
            Type::Signed(n) => n.fmt(f),
            Type::Fn(params, ret_ty) => {
                f.write_str("(")?;
                let mut params = params.iter();
                if let Some(param) = params.next() {
                    param.fmt(f)?;
                }
                for param in params {
                    f.write_str(", ")?;
                    param.fmt(f)?;
                }
                f.write_str(") -> ")?;
                ret_ty.fmt(f)
            }
            Type::Array(ty, size) => {
                f.write_str("[")?;
                ty.fmt(f)?;
                f.write_str("; ")?;
                size.fmt(f)?;
                f.write_str("]")
            }
            Type::Tuple(fields) => {
                f.write_str("(")?;
                let mut fields = fields.iter();
                if let Some(field) = fields.next() {
                    field.fmt(f)?;
                }
                for field in fields {
                    f.write_str(", ")?;
                    field.fmt(f)?;
                }
                f.write_str(")")
            }
            Type::UntypedTopLevelDefinition(name, _) => f.write_str(name),
            Type::Struct(name) => f.write_str(name),
            Type::Enum(name) => f.write_str(name),
        }
    }
}

/// A statement and its location in the source code.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Stmt<T>(pub StmtEnum<T>, pub MetaInfo);

/// The different kinds of statements.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum StmtEnum<T> {
    /// Let expression, binds variables to exprs.
    Let(Pattern<T>, Expr<T>),
    /// Mutable let expression, bind a single variable to an expr.
    LetMut(String, Expr<T>),
    /// Assignment of a (previously as mutable declared) variable.
    VarAssign(String, Expr<T>),
    /// Assignment of an index in a (mutable) array.
    ArrayAssign(String, Expr<T>, Expr<T>),
    /// Binds an identifier to each value of an array expr, evaluating the body.
    ForEachLoop(String, Expr<T>, Vec<Stmt<T>>),
    /// An expression (all expressions are statements, but not all statements expressions).
    Expr(Expr<T>),
}

/// An expression and its location in the source code.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Expr<T>(pub ExprEnum<T>, pub MetaInfo, pub T);

impl Expr<()> {
    /// Constructs an expression without any associated type information.
    pub fn untyped(expr: ExprEnum<()>, meta: MetaInfo) -> Self {
        Self(expr, meta, ())
    }
}

impl Expr<Type> {
    /// Constructs an expression with an associated type.
    pub fn typed(expr: ExprEnum<Type>, ty: Type, meta: MetaInfo) -> Self {
        Self(expr, meta, ty)
    }
}

/// The different kinds of expressions.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExprEnum<T> {
    /// Literal `true`.
    True,
    /// Literal `false`.
    False,
    /// Unsigned number literal.
    NumUnsigned(u64, UnsignedNumType),
    /// Signed number literal.
    NumSigned(i64, SignedNumType),
    /// Identifier (either a variable or a function).
    Identifier(String),
    /// Array literal which explicitly specifies all of its elements.
    ArrayLiteral(Vec<Expr<T>>),
    /// Array "repeat expression", which specifies 1 element, to be repeated a number of times.
    ArrayRepeatLiteral(Box<Expr<T>>, usize),
    /// Access of an array at the specified index, returning its element.
    ArrayAccess(Box<Expr<T>>, Box<Expr<T>>),
    /// Tuple literal containing the specified fields.
    TupleLiteral(Vec<Expr<T>>),
    /// Access of a tuple at the specified position.
    TupleAccess(Box<Expr<T>>, usize),
    /// Access of a struct at the specified field.
    StructAccess(Box<Expr<T>>, String),
    /// Struct literal with the specified fields.
    StructLiteral(String, Vec<(String, Expr<T>)>),
    /// Enum literal of the specified variant, possibly with fields.
    EnumLiteral(String, Box<VariantExpr<T>>),
    /// Matching the specified expression with a list of clauses (pattern + expression).
    Match(Box<Expr<T>>, Vec<(Pattern<T>, Expr<T>)>),
    /// Application of a unary operator.
    UnaryOp(UnaryOp, Box<Expr<T>>),
    /// Application of a binary operator.
    Op(Op, Box<Expr<T>>, Box<Expr<T>>),
    /// A block that lexically scopes any bindings introduced within it.
    Block(Vec<Stmt<T>>),
    /// Call of the specified function with a list of arguments.
    FnCall(String, Vec<Expr<T>>),
    /// If-else expression for the specified condition, if-expr and else-expr.
    If(Box<Expr<T>>, Box<Expr<T>>, Box<Expr<T>>),
    /// Explicit cast of an expression to the specified type.
    Cast(Type, Box<Expr<T>>),
    /// Range of numbers from the specified min (inclusive) to the specified max (exclusive).
    Range((u64, UnsignedNumType), (u64, UnsignedNumType)),
}

/// A variant literal, used by [`ExprEnum::EnumLiteral`], with its location in the source code.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct VariantExpr<T>(pub String, pub VariantExprEnum<T>, pub MetaInfo);

/// The different kinds of variant literals.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum VariantExprEnum<T> {
    /// A unit variant, containing no fields.
    Unit,
    /// A tuple variant, containing positional fields (but can be empty).
    Tuple(Vec<Expr<T>>),
}

/// A (possibly nested) pattern used by [`ExprEnum::Match`], with its location in the source code.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Pattern<T>(pub PatternEnum<T>, pub MetaInfo, pub T);

impl Pattern<()> {
    /// Constructs a pattern without any associated type information.
    pub fn untyped(pattern: PatternEnum<()>, meta: MetaInfo) -> Self {
        Self(pattern, meta, ())
    }
}

impl Pattern<Type> {
    /// Constructs a pattern with an associated type.
    pub fn typed(pattern: PatternEnum<Type>, ty: Type, meta: MetaInfo) -> Self {
        Self(pattern, meta, ty)
    }
}

/// The different kinds of patterns.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PatternEnum<T> {
    /// A variable, always matches.
    Identifier(String),
    /// Matches `true`.
    True,
    /// Matches `false`.
    False,
    /// Matches the specified unsigned number.
    NumUnsigned(u64, UnsignedNumType),
    /// Matches the specified signed number.
    NumSigned(i64, SignedNumType),
    /// Matches a tuple if all of its fields match their respective patterns.
    Tuple(Vec<Pattern<T>>),
    /// Matches a struct if all of its fields match their respective patterns.
    Struct(String, Vec<(String, Pattern<T>)>),
    /// Matches a struct if its fields match their respective patterns, ignoring remaining fields.
    StructIgnoreRemaining(String, Vec<(String, Pattern<T>)>),
    /// Matches an enum with the specified name and variant.
    EnumUnit(String, String),
    /// Matches an enum with the specified name and variant, if all fields match.
    EnumTuple(String, String, Vec<Pattern<T>>),
    /// Matches any number inside the unsigned range between min (inclusive) and max (inclusive).
    UnsignedInclusiveRange(u64, u64, UnsignedNumType),
    /// Matches any number inside the signed range between min (inclusive) and max (inclusive).
    SignedInclusiveRange(i64, i64, SignedNumType),
}

impl<T> std::fmt::Display for Pattern<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            PatternEnum::Identifier(name) => f.write_str(name),
            PatternEnum::True => f.write_str("true"),
            PatternEnum::False => f.write_str("false"),
            PatternEnum::NumUnsigned(n, suffix) => f.write_fmt(format_args!("{n}{suffix}")),
            PatternEnum::NumSigned(n, suffix) => f.write_fmt(format_args!("{n}{suffix}")),
            PatternEnum::Struct(struct_name, fields) => {
                f.write_fmt(format_args!("{struct_name} {{ "))?;
                let mut fields = fields.iter();
                if let Some((field_name, field)) = fields.next() {
                    f.write_fmt(format_args!("{field_name}: {field}"))?;
                }
                for (field_name, field) in fields {
                    f.write_str(", ")?;
                    f.write_fmt(format_args!("{field_name}: {field}"))?;
                }
                f.write_str("}")
            }
            PatternEnum::StructIgnoreRemaining(struct_name, fields) => {
                f.write_fmt(format_args!("{struct_name} {{ "))?;
                for (field_name, field) in fields.iter() {
                    f.write_fmt(format_args!("{field_name}: {field}"))?;
                    f.write_str(", ")?;
                }
                f.write_str(".. }")
            }
            PatternEnum::Tuple(fields) => {
                f.write_str("(")?;
                let mut fields = fields.iter();
                if let Some(field) = fields.next() {
                    field.fmt(f)?;
                }
                for field in fields {
                    f.write_str(", ")?;
                    field.fmt(f)?;
                }
                f.write_str(")")
            }
            PatternEnum::EnumUnit(enum_name, variant_name) => {
                f.write_fmt(format_args!("{enum_name}::{variant_name}"))
            }
            PatternEnum::EnumTuple(enum_name, variant_name, fields) => {
                f.write_fmt(format_args!("{enum_name}::{variant_name}("))?;
                let mut fields = fields.iter();
                if let Some(field) = fields.next() {
                    field.fmt(f)?;
                }
                for field in fields {
                    f.write_str(", ")?;
                    field.fmt(f)?;
                }
                f.write_str(")")
            }
            PatternEnum::UnsignedInclusiveRange(min, max, suffix) => {
                if min == max {
                    f.write_fmt(format_args!("{min}{suffix}"))
                } else if *min == 0 && *max == suffix.max() {
                    f.write_str("_")
                } else {
                    f.write_fmt(format_args!("{min}{suffix}..={max}{suffix}"))
                }
            }
            PatternEnum::SignedInclusiveRange(min, max, suffix) => {
                if min == max {
                    f.write_fmt(format_args!("{min}{suffix}"))
                } else if *min == suffix.min() && *max == suffix.max() {
                    f.write_str("_")
                } else {
                    f.write_fmt(format_args!("{min}{suffix}..={max}{suffix}"))
                }
            }
        }
    }
}

/// The different kinds of unary operator.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum UnaryOp {
    /// Bitwise / logical negation (`!`).
    Not,
    /// Arithmetic negation (`-`).
    Neg,
}

/// the different kinds of binary operators.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Op {
    /// Addition (`+`).
    Add,
    /// Subtraction (`-`).
    Sub,
    /// Multiplication (`*`).
    Mul,
    /// Division (`/`).
    Div,
    /// Modulo (`%`).
    Mod,
    /// Bitwise and (`&`).
    BitAnd,
    /// Bitwise xor (`^`).
    BitXor,
    /// Bitwise or (`|`).
    BitOr,
    /// Greater-than (`>`).
    GreaterThan,
    /// Less-than (`<`).
    LessThan,
    /// Equals (`==`).
    Eq,
    /// Not-equals (`!=`).
    NotEq,
    /// Bitwise shift-left (`<<`).
    ShiftLeft,
    /// Bitwise shift-right (`>>`).
    ShiftRight,
    /// Short-circuiting and (`&&`).
    ShortCircuitAnd,
    /// Short-circuiting or (`||`).
    ShortCircuitOr,
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Op::Add => f.write_str("+"),
            Op::Sub => f.write_str("-"),
            Op::Mul => f.write_str("*"),
            Op::Div => f.write_str("/"),
            Op::Mod => f.write_str("%"),
            Op::BitAnd => f.write_str("&"),
            Op::BitXor => f.write_str("^"),
            Op::BitOr => f.write_str("|"),
            Op::GreaterThan => f.write_str(">"),
            Op::LessThan => f.write_str("<"),
            Op::Eq => f.write_str("=="),
            Op::NotEq => f.write_str("!="),
            Op::ShiftLeft => f.write_str("<<"),
            Op::ShiftRight => f.write_str(">>"),
            Op::ShortCircuitAnd => f.write_str("&&"),
            Op::ShortCircuitOr => f.write_str("||"),
        }
    }
}
