use crate::ast::{
    Closure, EnumDef, Expr, ExprEnum, FnDef, MainDef, Op, ParamDef, Party, Pattern, PatternEnum,
    Program, Type, UnaryOp, Variant, VariantExpr, VariantExprEnum,
};

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct MetaInfo {
    pub start: (usize, usize),
    pub end: (usize, usize),
}

impl std::fmt::Debug for MetaInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            format!(
                "{}:{}-{}:{}",
                self.start.0, self.start.1, self.end.0, self.end.1
            )
            .as_str(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct ParseError(pub ParseErrorEnum, pub MetaInfo);

#[derive(Debug, Clone)]
pub enum ParseErrorEnum {
    EmptySexpr,
    UnexpectedToken,
    UnclosedSexpr,
    InvalidDef,
    InvalidEnumDef,
    InvalidFnDef,
    InvalidMainFnDef,
    InvalidEnumVariant,
    InvalidEnumPattern,
    InvalidEnumPatternEnum,
    InvalidRangePattern,
    InvalidPattern,
    ExpectedIdentifier,
    ExpectedListOfLength(usize),
    ExpectedKeyword(String),
    ExpectedType,
    InvalidParty,
    InvalidArity(usize),
    InvalidExpr,
    ArrayMaxSizeExceeded(u128),
    TupleMaxSizeExceeded(u128),
}

pub fn parse(prg: &str) -> Result<Program, ParseError> {
    let sexprs = parse_into_sexprs(prg)?;
    let ast = parse_into_ast(sexprs)?;
    Ok(ast)
}

#[derive(Debug, Clone)]
struct Sexpr(SexprEnum, MetaInfo);

#[derive(Debug, Clone)]
enum SexprEnum {
    True,
    False,
    NumUnsigned(u128),
    NumSigned(i128),
    List(Vec<Sexpr>),
    Identifier(String),
}

#[derive(Debug, Clone)]
enum ParsedDef {
    Enum(EnumDef),
    Fn(FnDef),
}

fn parse_into_ast(mut sexprs: Vec<Sexpr>) -> Result<Program, ParseError> {
    let main = sexprs.pop().unwrap();
    let mut enum_defs = Vec::new();
    let mut fn_defs = Vec::new();
    for sexpr in sexprs.into_iter() {
        let parsed = parse_def(sexpr)?;
        match parsed {
            ParsedDef::Enum(def) => enum_defs.push(def),
            ParsedDef::Fn(def) => fn_defs.push(def),
        }
    }
    let main = parse_main_def(main)?;
    Ok(Program {
        enum_defs,
        fn_defs,
        main,
    })
}

fn parse_def(sexpr: Sexpr) -> Result<ParsedDef, ParseError> {
    let Sexpr(sexpr_enum, meta) = sexpr.clone();
    match sexpr_enum {
        SexprEnum::List(sexprs) => {
            if sexprs.is_empty() {
                return Err(ParseError(ParseErrorEnum::InvalidDef, meta));
            }
            let (keyword, meta) = expect_identifier(sexprs[0].clone())?;
            match keyword.as_str() {
                "enum" => Ok(ParsedDef::Enum(parse_enum_def(sexpr)?)),
                "fn" => Ok(ParsedDef::Fn(parse_fn_def(sexpr)?)),
                _ => Err(ParseError(ParseErrorEnum::InvalidDef, meta)),
            }
        }
        _ => Err(ParseError(ParseErrorEnum::InvalidDef, meta)),
    }
}

fn parse_enum_def(sexpr: Sexpr) -> Result<EnumDef, ParseError> {
    let Sexpr(sexpr, meta) = sexpr;
    if let SexprEnum::List(sexprs) = sexpr {
        let mut sexprs = sexprs.into_iter();
        if let (Some(keyword_fn), Some(identifier)) = (sexprs.next(), sexprs.next()) {
            expect_keyword(keyword_fn, "enum")?;
            let (identifier, _) = expect_identifier(identifier)?;
            let mut variants = Vec::new();
            while let Some(variant) = sexprs.next() {
                let Sexpr(variant, meta) = variant;
                if let SexprEnum::List(variant) = variant {
                    if variant.len() < 2 {
                        return Err(ParseError(ParseErrorEnum::InvalidEnumDef, meta));
                    }
                    let (variant_type, meta) = expect_identifier(variant[0].clone())?;
                    let (identifier, _) = expect_identifier(variant[1].clone())?;
                    let variant = match variant_type.as_str() {
                        "unit-variant" => Variant::Unit(identifier),
                        "tuple-variant" => {
                            let mut types = Vec::with_capacity(variant.len() - 2);
                            for ty in variant[2..].iter() {
                                let (ty, _) = expect_type(ty.clone())?;
                                types.push(ty);
                            }
                            Variant::Tuple(identifier, types)
                        }
                        _ => return Err(ParseError(ParseErrorEnum::InvalidEnumDef, meta)),
                    };
                    variants.push(variant);
                } else {
                    return Err(ParseError(ParseErrorEnum::InvalidEnumDef, meta));
                }
            }
            Ok(EnumDef {
                identifier,
                variants,
                meta,
            })
        } else {
            return Err(ParseError(ParseErrorEnum::InvalidEnumDef, meta));
        }
    } else {
        Err(ParseError(ParseErrorEnum::InvalidEnumDef, meta))
    }
}

fn parse_closure_def(sexpr: Sexpr) -> Result<Closure, ParseError> {
    let Sexpr(sexpr, meta) = sexpr;
    match sexpr {
        SexprEnum::List(mut sexprs) => {
            let body = parse_expr(sexprs.pop().unwrap())?;
            let mut sexprs = sexprs.into_iter();
            if let (Some(keyword_fn), Some(ty)) = (sexprs.next(), sexprs.next()) {
                expect_keyword(keyword_fn, "lambda")?;
                let (ty, _) = expect_type(ty)?;
                let mut params = Vec::new();
                while let Some(param_def) = sexprs.next() {
                    let (param_def, _) = expect_fixed_list(param_def, 3)?;
                    let mut param_def = param_def.into_iter();
                    expect_keyword(param_def.next().unwrap(), "param")?;
                    let (identifier, _) = expect_identifier(param_def.next().unwrap())?;
                    let (ty, _) = expect_type(param_def.next().unwrap())?;
                    params.push(ParamDef(identifier, ty));
                }
                Ok(Closure {
                    ty,
                    params,
                    body,
                    meta,
                })
            } else {
                return Err(ParseError(ParseErrorEnum::InvalidFnDef, meta));
            }
        }
        _ => Err(ParseError(ParseErrorEnum::InvalidFnDef, meta)),
    }
}

fn parse_fn_def(sexpr: Sexpr) -> Result<FnDef, ParseError> {
    let Sexpr(sexpr, meta) = sexpr;
    match sexpr {
        SexprEnum::List(mut sexprs) => {
            let body = parse_expr(sexprs.pop().unwrap())?;
            let mut sexprs = sexprs.into_iter();
            if let (Some(keyword_fn), Some(identifier), Some(ty)) =
                (sexprs.next(), sexprs.next(), sexprs.next())
            {
                expect_keyword(keyword_fn, "fn")?;
                let (identifier, _) = expect_identifier(identifier)?;
                let (ty, _) = expect_type(ty)?;
                let mut params = Vec::new();
                while let Some(param_def) = sexprs.next() {
                    let (param_def, _) = expect_fixed_list(param_def, 3)?;
                    let mut param_def = param_def.into_iter();
                    expect_keyword(param_def.next().unwrap(), "param")?;
                    let (identifier, _) = expect_identifier(param_def.next().unwrap())?;
                    let (ty, _) = expect_type(param_def.next().unwrap())?;
                    params.push(ParamDef(identifier, ty));
                }
                Ok(FnDef {
                    identifier,
                    ty,
                    params,
                    body,
                    meta,
                })
            } else {
                return Err(ParseError(ParseErrorEnum::InvalidFnDef, meta));
            }
        }
        _ => Err(ParseError(ParseErrorEnum::InvalidFnDef, meta)),
    }
}

fn parse_main_def(sexpr: Sexpr) -> Result<MainDef, ParseError> {
    let Sexpr(sexpr, meta) = sexpr;
    match sexpr {
        SexprEnum::List(mut sexprs) => {
            let body = parse_expr(sexprs.pop().unwrap())?;
            let mut sexprs = sexprs.into_iter();
            if let (Some(keyword_fn), Some(identifier), Some(ty)) =
                (sexprs.next(), sexprs.next(), sexprs.next())
            {
                expect_keyword(keyword_fn, "fn")?;
                expect_keyword(identifier, "main")?;
                let (ty, _) = expect_type(ty)?;
                let mut params = Vec::new();
                while let Some(param_def) = sexprs.next() {
                    let (param_def, meta) = expect_fixed_list(param_def, 4)?;
                    let mut param_def = param_def.into_iter();
                    expect_keyword(param_def.next().unwrap(), "param")?;
                    let (identifier, _) = expect_identifier(param_def.next().unwrap())?;
                    let (party, _) = expect_identifier(param_def.next().unwrap())?;
                    let party = match party.as_str() {
                        "A" => Party::A,
                        "B" => Party::B,
                        _ => {
                            return Err(ParseError(ParseErrorEnum::InvalidParty, meta));
                        }
                    };
                    let (ty, _) = expect_type(param_def.next().unwrap())?;
                    params.push((party, ParamDef(identifier, ty)));
                }
                Ok(MainDef {
                    ty,
                    params,
                    body,
                    meta,
                })
            } else {
                return Err(ParseError(ParseErrorEnum::InvalidMainFnDef, meta));
            }
        }
        _ => Err(ParseError(ParseErrorEnum::InvalidMainFnDef, meta)),
    }
}

fn parse_expr(sexpr: Sexpr) -> Result<Expr, ParseError> {
    let Sexpr(sexpr, meta) = sexpr;
    let expr = match sexpr {
        SexprEnum::True => ExprEnum::True,
        SexprEnum::False => ExprEnum::False,
        SexprEnum::NumUnsigned(n) => ExprEnum::NumUnsigned(n),
        SexprEnum::NumSigned(n) => ExprEnum::NumSigned(n),
        SexprEnum::Identifier(s) => ExprEnum::Identifier(s),
        SexprEnum::List(sexprs) => {
            let arity = sexprs.len() - 1;
            let mut sexprs = sexprs.into_iter();
            let (f, _meta) = expect_identifier(sexprs.next().unwrap())?;

            match f.as_str() {
                "-" | "!" if arity == 1 => {
                    let op = match f.as_str() {
                        "-" => UnaryOp::Neg,
                        "!" => UnaryOp::Not,
                        _ => unreachable!(),
                    };
                    let x = parse_expr(sexprs.next().unwrap())?;
                    ExprEnum::UnaryOp(op, Box::new(x))
                }
                "+" | "-" | "*" | "/" | "%" | "&" | "^" | "|" | ">" | "<" | "==" | "!=" | "<<"
                | ">>" => {
                    if arity == 2 {
                        let op = match f.as_str() {
                            "+" => Op::Add,
                            "-" => Op::Sub,
                            "*" => Op::Mul,
                            "/" => Op::Div,
                            "%" => Op::Mod,
                            "&" => Op::BitAnd,
                            "^" => Op::BitXor,
                            "|" => Op::BitOr,
                            ">" => Op::GreaterThan,
                            "<" => Op::LessThan,
                            "==" => Op::Eq,
                            "!=" => Op::NotEq,
                            "<<" => Op::ShiftLeft,
                            ">>" => Op::ShiftRight,
                            _ => unreachable!(),
                        };
                        let x = parse_expr(sexprs.next().unwrap())?;
                        let y = parse_expr(sexprs.next().unwrap())?;
                        ExprEnum::Op(op, Box::new(x), Box::new(y))
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "let" => {
                    if arity == 3 {
                        let (identifier, _) = expect_identifier(sexprs.next().unwrap())?;
                        let binding = parse_expr(sexprs.next().unwrap())?;
                        let body = parse_expr(sexprs.next().unwrap())?;
                        ExprEnum::Let(identifier, Box::new(binding), Box::new(body))
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "if" => {
                    if arity == 3 {
                        let condition = parse_expr(sexprs.next().unwrap())?;
                        let case_true = parse_expr(sexprs.next().unwrap())?;
                        let case_false = parse_expr(sexprs.next().unwrap())?;
                        ExprEnum::If(
                            Box::new(condition),
                            Box::new(case_true),
                            Box::new(case_false),
                        )
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "call" => {
                    if arity > 0 {
                        let (identifier, _) = expect_identifier(sexprs.next().unwrap())?;
                        let mut exprs = Vec::new();
                        while let Some(sexpr) = sexprs.next() {
                            exprs.push(parse_expr(sexpr)?);
                        }
                        ExprEnum::FnCall(identifier, exprs)
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "cast" => {
                    if arity == 2 {
                        let (ty, _) = expect_type(sexprs.next().unwrap())?;
                        let expr = parse_expr(sexprs.next().unwrap())?;
                        ExprEnum::Cast(ty, Box::new(expr))
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "array" => {
                    if arity == 2 {
                        let value = parse_expr(sexprs.next().unwrap())?;
                        let (size, size_meta) = expect_unsigned_num(sexprs.next().unwrap())?;
                        if size <= usize::MAX as u128 {
                            ExprEnum::ArrayLiteral(Box::new(value), size as usize)
                        } else {
                            return Err(ParseError(
                                ParseErrorEnum::ArrayMaxSizeExceeded(size),
                                size_meta,
                            ));
                        }
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "array-get" => {
                    if arity == 2 {
                        let arr = parse_expr(sexprs.next().unwrap())?;
                        let index = parse_expr(sexprs.next().unwrap())?;
                        ExprEnum::ArrayAccess(Box::new(arr), Box::new(index))
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "array-set" => {
                    if arity == 3 {
                        let arr = parse_expr(sexprs.next().unwrap())?;
                        let index = parse_expr(sexprs.next().unwrap())?;
                        let value = parse_expr(sexprs.next().unwrap())?;
                        ExprEnum::ArrayAssignment(Box::new(arr), Box::new(index), Box::new(value))
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "fold" => {
                    if arity == 3 {
                        let arr = parse_expr(sexprs.next().unwrap())?;
                        let init_value = parse_expr(sexprs.next().unwrap())?;
                        let closure = parse_closure_def(sexprs.next().unwrap())?;
                        ExprEnum::Fold(Box::new(arr), Box::new(init_value), Box::new(closure))
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "map" => {
                    if arity == 2 {
                        let arr = parse_expr(sexprs.next().unwrap())?;
                        let closure = parse_closure_def(sexprs.next().unwrap())?;
                        ExprEnum::Map(Box::new(arr), Box::new(closure))
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "range" => {
                    if arity == 2 {
                        let (from, from_meta) = expect_unsigned_num(sexprs.next().unwrap())?;
                        if from <= usize::MAX as u128 {
                            let (to, to_meta) = expect_unsigned_num(sexprs.next().unwrap())?;
                            if to <= usize::MAX as u128 {
                                ExprEnum::Range(from as usize, to as usize)
                            } else {
                                return Err(ParseError(
                                    ParseErrorEnum::ArrayMaxSizeExceeded(to),
                                    to_meta,
                                ));
                            }
                        } else {
                            return Err(ParseError(
                                ParseErrorEnum::ArrayMaxSizeExceeded(from),
                                from_meta,
                            ));
                        }
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "tuple" => {
                    let mut parsed = Vec::with_capacity(arity);
                    for sexpr in sexprs {
                        parsed.push(parse_expr(sexpr)?);
                    }
                    ExprEnum::TupleLiteral(parsed)
                }
                "tuple-get" => {
                    if arity == 2 {
                        let tuple = parse_expr(sexprs.next().unwrap())?;
                        let (size, size_meta) = expect_unsigned_num(sexprs.next().unwrap())?;
                        if size <= usize::MAX as u128 {
                            ExprEnum::TupleAccess(Box::new(tuple), size as usize)
                        } else {
                            return Err(ParseError(
                                ParseErrorEnum::TupleMaxSizeExceeded(size),
                                size_meta,
                            ));
                        }
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "enum" => {
                    if arity == 2 {
                        let (identifier, _) = expect_identifier(sexprs.next().unwrap())?;
                        if let Sexpr(SexprEnum::List(variant), meta) = sexprs.next().unwrap() {
                            if variant.len() < 2 {
                                return Err(ParseError(ParseErrorEnum::InvalidEnumVariant, meta));
                            } else {
                                let mut variant = variant.into_iter();
                                let (variant_type, _) = expect_identifier(variant.next().unwrap())?;
                                let (variant_identifier, _) =
                                    expect_identifier(variant.next().unwrap())?;
                                let mut values = Vec::new();
                                for v in variant {
                                    values.push(parse_expr(v)?);
                                }
                                let variant_expr = match variant_type.as_str() {
                                    "unit-variant" => {
                                        if values.is_empty() {
                                            VariantExpr(
                                                variant_identifier,
                                                VariantExprEnum::Unit,
                                                meta,
                                            )
                                        } else {
                                            return Err(ParseError(
                                                ParseErrorEnum::InvalidEnumVariant,
                                                meta,
                                            ));
                                        }
                                    }
                                    "tuple-variant" => VariantExpr(
                                        variant_identifier,
                                        VariantExprEnum::Tuple(values),
                                        meta,
                                    ),
                                    _ => {
                                        return Err(ParseError(
                                            ParseErrorEnum::InvalidEnumVariant,
                                            meta,
                                        ))
                                    }
                                };
                                ExprEnum::EnumLiteral(identifier, Box::new(variant_expr))
                            }
                        } else {
                            return Err(ParseError(ParseErrorEnum::InvalidEnumVariant, meta));
                        }
                    } else {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                }
                "match" => {
                    if arity < 2 {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                    let expr = parse_expr(sexprs.next().unwrap())?;
                    let mut clauses = Vec::new();
                    while let Some(clause) = sexprs.next() {
                        let (clause, _) = expect_fixed_list(clause, 3)?;
                        let mut clause = clause.into_iter();
                        expect_keyword(clause.next().unwrap(), "clause")?;
                        let pattern = parse_pattern(clause.next().unwrap())?;
                        let body = parse_expr(clause.next().unwrap())?;
                        clauses.push((pattern, body));
                    }
                    ExprEnum::Match(Box::new(expr), clauses)
                }
                _ => {
                    return Err(ParseError(ParseErrorEnum::InvalidExpr, meta));
                }
            }
        }
    };
    Ok(Expr(expr, meta))
}

fn parse_pattern(sexpr: Sexpr) -> Result<Pattern, ParseError> {
    let Sexpr(sexpr, meta) = sexpr;
    let pattern = match sexpr {
        SexprEnum::True => PatternEnum::True,
        SexprEnum::False => PatternEnum::False,
        SexprEnum::NumUnsigned(n) => PatternEnum::NumUnsigned(n),
        SexprEnum::NumSigned(n) => PatternEnum::NumSigned(n),
        SexprEnum::Identifier(s) => PatternEnum::Identifier(s),
        SexprEnum::List(sexprs) => {
            let arity = sexprs.len();
            if arity < 2 {
                return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
            }
            let mut pattern = sexprs.into_iter();
            let (pattern_kind, pattern_kind_meta) = expect_identifier(pattern.next().unwrap())?;
            match pattern_kind.as_str() {
                "range" => {
                    if arity != 3 {
                        return Err(ParseError(ParseErrorEnum::InvalidArity(arity), meta));
                    }
                    let Sexpr(min, _) = pattern.next().unwrap();
                    let Sexpr(max, _) = pattern.next().unwrap();
                    match (min, max) {
                        (SexprEnum::NumUnsigned(min), SexprEnum::NumUnsigned(max)) => {
                            PatternEnum::UnsignedInclusiveRange(min, max - 1)
                        }
                        (SexprEnum::NumSigned(min), SexprEnum::NumUnsigned(max))
                            if max <= i128::MAX as u128 + 1 =>
                        {
                            PatternEnum::SignedInclusiveRange(min, (max - 1) as i128)
                        }
                        (SexprEnum::NumSigned(min), SexprEnum::NumSigned(max)) => {
                            PatternEnum::SignedInclusiveRange(min, max - 1)
                        }
                        _ => return Err(ParseError(ParseErrorEnum::InvalidRangePattern, meta)),
                    }
                }
                "tuple" => {
                    let mut fields = Vec::new();
                    for field in pattern {
                        fields.push(parse_pattern(field)?);
                    }
                    PatternEnum::Tuple(fields)
                },
                "unit-variant" | "tuple-variant" => {
                    let (variant_identifier, _) = expect_identifier(pattern.next().unwrap())?;
                    let mut fields = Vec::new();
                    for field in pattern {
                        fields.push(parse_pattern(field)?);
                    }
                    match (pattern_kind.as_str(), fields.len()) {
                        ("unit-variant", 0) => PatternEnum::EnumUnit(variant_identifier),
                        ("unit-variant", _) => {
                            return Err(ParseError(ParseErrorEnum::InvalidEnumVariant, meta))
                        }
                        ("tuple-variant", _) => PatternEnum::EnumTuple(variant_identifier, fields),
                        _ => return Err(ParseError(ParseErrorEnum::InvalidEnumPattern, meta)),
                    }
                }
                _ => {
                    return Err(ParseError(
                        ParseErrorEnum::InvalidPattern,
                        pattern_kind_meta,
                    ))
                }
            }
        }
    };
    Ok(Pattern(pattern, meta))
}

fn expect_fixed_list(sexpr: Sexpr, n: usize) -> Result<(Vec<Sexpr>, MetaInfo), ParseError> {
    let Sexpr(sexpr, meta) = sexpr;
    match sexpr {
        SexprEnum::List(sexprs) => {
            if sexprs.len() == n {
                Ok((sexprs, meta))
            } else {
                Err(ParseError(ParseErrorEnum::ExpectedListOfLength(n), meta))
            }
        }
        _ => Err(ParseError(ParseErrorEnum::ExpectedListOfLength(n), meta)),
    }
}

fn expect_identifier(sexpr: Sexpr) -> Result<(String, MetaInfo), ParseError> {
    let Sexpr(sexpr, meta) = sexpr;
    match sexpr {
        SexprEnum::Identifier(s) => Ok((s, meta)),
        _ => Err(ParseError(ParseErrorEnum::ExpectedIdentifier, meta)),
    }
}

fn expect_keyword(sexpr: Sexpr, keyword: &str) -> Result<MetaInfo, ParseError> {
    match expect_identifier(sexpr) {
        Ok((identifier, meta)) => {
            if identifier == keyword {
                Ok(meta)
            } else {
                Err(ParseError(
                    ParseErrorEnum::ExpectedKeyword(keyword.to_string()),
                    meta,
                ))
            }
        }
        Err(ParseError(_, meta)) => Err(ParseError(
            ParseErrorEnum::ExpectedKeyword(keyword.to_string()),
            meta,
        )),
    }
}

fn expect_type(sexpr: Sexpr) -> Result<(Type, MetaInfo), ParseError> {
    match expect_identifier(sexpr) {
        Ok((identifier, meta)) => {
            let ty = match identifier.as_str() {
                "bool" => Type::Bool,
                "usize" => Type::Usize,
                "u8" => Type::U8,
                "u16" => Type::U16,
                "u32" => Type::U32,
                "u64" => Type::U64,
                "u128" => Type::U128,
                "i8" => Type::I8,
                "i16" => Type::I16,
                "i32" => Type::I32,
                "i64" => Type::I64,
                "i128" => Type::I128,
                _ => return Err(ParseError(ParseErrorEnum::ExpectedType, meta)),
            };
            Ok((ty, meta))
        }
        Err(ParseError(_, meta)) => Err(ParseError(ParseErrorEnum::ExpectedType, meta)),
    }
}

fn expect_unsigned_num(sexpr: Sexpr) -> Result<(u128, MetaInfo), ParseError> {
    let Sexpr(sexpr, meta) = sexpr;
    match sexpr {
        SexprEnum::NumUnsigned(n) => Ok((n, meta)),
        _ => Err(ParseError(ParseErrorEnum::ExpectedIdentifier, meta)),
    }
}

fn parse_into_sexprs(prg: &str) -> Result<Vec<Sexpr>, ParseError> {
    let mut stack = vec![vec![]];
    let mut stack_meta_start = vec![(0, 0)];
    let mut current_token = Vec::new();
    let mut current_token_start = (0, 0);
    for (l, line) in prg.lines().enumerate() {
        for (c, char) in line.chars().enumerate() {
            if char == '(' || char == ')' || char.is_whitespace() {
                if char == '(' {
                    stack.push(Vec::new());
                    stack_meta_start.push((l, c));
                }
                if let Some(sexprs) = stack.last_mut() {
                    if !current_token.is_empty() {
                        let token: String = current_token.iter().collect();
                        current_token.clear();
                        let meta = MetaInfo {
                            start: current_token_start,
                            end: (l, c),
                        };
                        let sexpr = if let Ok(n) = token.parse::<u128>() {
                            SexprEnum::NumUnsigned(n)
                        } else if let Ok(n) = token.parse::<i128>() {
                            SexprEnum::NumSigned(n)
                        } else if token.as_str() == "true" {
                            SexprEnum::True
                        } else if token.as_str() == "false" {
                            SexprEnum::False
                        } else {
                            SexprEnum::Identifier(token)
                        };
                        sexprs.push(Sexpr(sexpr, meta));
                    }
                } else {
                    let meta = MetaInfo {
                        start: current_token_start,
                        end: (l, c),
                    };
                    return Err(ParseError(ParseErrorEnum::UnexpectedToken, meta));
                }
                if char == ')' {
                    if stack.len() > 1 {
                        if let (Some(mut sexprs), Some(parent)) = (stack.pop(), stack.last_mut()) {
                            let meta = MetaInfo {
                                start: stack_meta_start.pop().unwrap(),
                                end: (l, c + 1),
                            };
                            if sexprs.is_empty() {
                                return Err(ParseError(ParseErrorEnum::EmptySexpr, meta));
                            } else if sexprs.len() == 1 {
                                let Sexpr(sexpr, _) = sexprs.pop().unwrap();
                                parent.push(Sexpr(sexpr, meta));
                            } else {
                                parent.push(Sexpr(SexprEnum::List(sexprs), meta));
                            }
                        }
                    }
                }
            } else {
                if current_token.is_empty() {
                    current_token_start = (l, c);
                }
                current_token.push(char);
            }
        }
    }
    let meta = MetaInfo {
        start: (0, 0),
        end: (0, 1),
    };
    if stack.len() == 1 {
        let sexprs = stack.pop().unwrap();
        Ok(sexprs)
    } else {
        Err(ParseError(ParseErrorEnum::UnclosedSexpr, meta))
    }
}
