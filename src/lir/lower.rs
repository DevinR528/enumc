use std::fmt;

use crate::{
    ast::{
        parse::symbol::Ident,
        types::{self as ty, Path, Spanned, DUMMY},
    },
    error::Error,
    lir::{const_fold::Folder, mono::TraitRes},
    typeck::TyCheckRes,
    visit::VisitMut,
};

#[derive(Clone, Debug)]
pub enum Val {
    Float(f64),
    Int(isize),
    Char(char),
    Bool(bool),
    Str(Ident),
}

impl Val {
    crate fn lower(val: ty::Val) -> Self {
        match val {
            ty::Val::Float(v) => Val::Float(v),
            ty::Val::Int(v) => Val::Int(v),
            ty::Val::Char(v) => Val::Char(v),
            ty::Val::Bool(v) => Val::Bool(v),
            ty::Val::Str(v) => Val::Str(v),
        }
    }

    fn type_of(&self) -> Ty {
        match self {
            Val::Float(_) => Ty::Float,
            Val::Int(_) => Ty::Int,
            Val::Char(_) => Ty::Char,
            Val::Bool(_) => Ty::Bool,
            Val::Str(_) => Ty::String,
        }
    }

    #[allow(dead_code)]
    crate fn size_of(&self) -> usize {
        match self {
            Val::Float(_) => 8,
            Val::Int(_) => 8,
            Val::Char(_) => 4,
            Val::Bool(_) => 1,
            Val::Str(_) => 8,
        }
    }
}

impl fmt::Display for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Val::Float(v) => v.fmt(f),
            Val::Int(v) => v.fmt(f),
            Val::Char(v) => v.fmt(f),
            Val::Bool(v) => v.fmt(f),
            Val::Str(v) => v.fmt(f),
        }
    }
}

impl PartialEq for Val {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Val::Float(a), Val::Float(b)) => a.to_string().eq(&b.to_string()),
            (Val::Float(_), _) => false,
            (Val::Int(a), Val::Int(b)) => a.eq(b),
            (Val::Int(_), _) => false,
            (Val::Char(a), Val::Char(b)) => a.eq(b),
            (Val::Char(_), _) => false,
            (Val::Str(a), Val::Str(b)) => a.eq(b),
            (Val::Str(_), _) => false,
            (Val::Bool(a), Val::Bool(b)) => a.eq(b),
            (Val::Bool(_), _) => false,
        }
    }
}

impl Eq for Val {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum UnOp {
    Not,
    OnesComp,
}

impl UnOp {
    fn lower(op: ty::UnOp) -> Self {
        match op {
            ty::UnOp::Not => UnOp::Not,
            ty::UnOp::OnesComp => UnOp::OnesComp,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    /// The `*` operator.
    Mul,
    /// The `/` operator.
    Div,
    /// The `%` operator.
    Rem,
    /// The `+` operator.
    Add,
    /// The `-` operator.
    Sub,

    /// The `<<` left shift operator.
    LeftShift,
    /// The `>>` right shift operator.
    RightShift,

    /// The `<` operator.
    Lt,
    /// The `<=` operator.
    Le,
    /// The `>=` operator.
    Ge,
    /// The `>` operator.
    Gt,

    /// The `==` operator.
    Eq,
    /// The `!=` operator.
    Ne,

    /// The `&` bitwise and operator.
    BitAnd,
    /// The `^` bitwise and operator.
    BitXor,
    /// The `|` bitwise and operator.
    BitOr,

    /// The `&&` operator.
    And,
    /// The `||` operator.
    Or,

    /// The `+=` operator.
    AddAssign,
    /// The `-=` operator.
    SubAssign,
}

impl BinOp {
    fn lower(op: ty::BinOp) -> Self {
        match op {
            ty::BinOp::Mul => BinOp::Mul,
            ty::BinOp::Div => BinOp::Div,
            ty::BinOp::Rem => BinOp::Rem,
            ty::BinOp::Add => BinOp::Add,
            ty::BinOp::Sub => BinOp::Sub,
            ty::BinOp::LeftShift => BinOp::LeftShift,
            ty::BinOp::RightShift => BinOp::RightShift,
            ty::BinOp::Lt => BinOp::Lt,
            ty::BinOp::Le => BinOp::Le,
            ty::BinOp::Ge => BinOp::Ge,
            ty::BinOp::Gt => BinOp::Gt,
            ty::BinOp::Eq => BinOp::Eq,
            ty::BinOp::Ne => BinOp::Ne,
            ty::BinOp::BitAnd => BinOp::BitAnd,
            ty::BinOp::BitXor => BinOp::BitXor,
            ty::BinOp::BitOr => BinOp::BitOr,
            ty::BinOp::And => BinOp::And,
            ty::BinOp::Or => BinOp::Or,
            ty::BinOp::AddAssign => BinOp::AddAssign,
            ty::BinOp::SubAssign => BinOp::SubAssign,
        }
    }

    crate fn as_instruction(&self) -> &'static str {
        match self {
            BinOp::Mul => "imul",
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::LeftShift => "shl",
            BinOp::RightShift => "shr",
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::BitAnd => "and",
            BinOp::BitXor => "xor",
            BinOp::BitOr => "or",
            // FIXME: this is not ideal, we only use this for `Instruction::FloatMath` so problem
            // could arise. also how do we know when to take remainder rdx:rax /
            // location remainder is rdx
            BinOp::Div | BinOp::Rem => "div",
            _ => unreachable!("handle differently {:?}", self),
        }
    }

    crate fn is_cmp(&self) -> bool {
        matches!(self, Self::Lt | Self::Le | Self::Ge | Self::Gt | Self::Eq | Self::Ne)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldInit {
    pub ident: Ident,
    pub init: Expr,
    pub ty: Ty,
}

impl FieldInit {
    fn lower(tyctx: &TyCheckRes<'_, '_>, fold: &Folder, def: &ty::Field, f: ty::FieldInit) -> Self {
        FieldInit {
            ident: f.ident,
            init: Expr::lower(tyctx, fold, f.init),
            ty: Ty::lower(tyctx, &def.ty.val),
        }
    }
}

#[derive(Clone, derive_help::Debug, PartialEq, Eq)]
pub enum Expr {
    /// Access a named variable `a`.
    Ident { ident: Ident, ty: Ty },
    /// Remove indirection, follow a pointer to it's pointee.
    Deref { indir: usize, expr: Box<Expr>, ty: Ty },
    /// Add indirection, refer to a variable by it's memory address (pointer).
    AddrOf(Box<Expr>),
    /// Access an array by index `[expr][expr]`.
    ///
    /// Each `exprs` represents an access of a dimension of the array.
    Array { ident: Ident, exprs: Vec<Expr>, ty: Ty },
    /// A urnary operation `!expr`.
    Urnary { op: UnOp, expr: Box<Expr>, ty: Ty },
    /// A binary operation `1 + 1`.
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr>, ty: Ty },
    /// An expression wrapped in parantheses (expr).
    Parens(Box<Expr>),
    /// A function call with possible expression arguments `call(expr)`.
    Call {
        path: Path,
        args: Vec<Expr>,
        type_args: Vec<Ty>,
        #[dbg_ignore]
        def: Func,
    },
    /// A call to a trait method with possible expression arguments `<<T>::trait>(expr)`.
    TraitMeth {
        trait_: Path,
        args: Vec<Expr>,
        type_args: Vec<Ty>,
        #[dbg_ignore]
        def: Impl,
    },
    /// Access the fields of a struct `expr.expr.expr;`.
    FieldAccess { lhs: Box<Expr>, def: Struct, rhs: Box<Expr>, field_idx: u32 },
    /// An ADT is initialized with field values.
    StructInit { path: Path, fields: Vec<FieldInit>, def: Struct },
    /// An ADT is initialized with field values.
    EnumInit { path: Path, variant: Ident, items: Vec<Expr>, def: Enum },
    /// An array initializer `{0, 1, 2}`
    ArrayInit { items: Vec<Expr>, ty: Ty },
    /// A literal value `1, "hello", true`
    Value(Val),
}

impl Expr {
    fn lower(tyctx: &TyCheckRes<'_, '_>, fold: &Folder, mut ex: ty::Expression) -> Self {
        let mut typ = tyctx.expr_ty.get(&ex).cloned().unwrap_or_else(|| match &mut ex.val {
            // HACK: pass the return value to lower via `type_args` see `TraitRes::visit_expr`
            ty::Expr::Call { path: _, args: _, type_args } => type_args.remove(0).val,
            ty::Expr::TraitMeth { trait_: _, args: _, type_args } => type_args.remove(0).val,
            _ => unreachable!("only trait impl calls and function calls are replaced"),
        });

        if typ.has_generics() {
            if let Some(ty) = tyctx.mono_expr_ty.borrow().get(&ex) {
                typ = ty.clone();
            }
        }
        let ty = Ty::lower(tyctx, &typ);

        let mut lowered = match ex.val {
            ty::Expr::Ident(ident) => Expr::Ident { ident, ty },
            ty::Expr::AddrOf(expr) => Expr::AddrOf(box Expr::lower(tyctx, fold, *expr)),
            ty::Expr::Deref { indir, expr } => {
                Expr::Deref { indir, expr: box Expr::lower(tyctx, fold, *expr), ty }
            }
            ty::Expr::Array { ident, exprs } => Expr::Array {
                ident,
                ty,
                exprs: exprs.into_iter().map(|e| Expr::lower(tyctx, fold, e)).collect(),
            },
            ty::Expr::FieldAccess { lhs, rhs } => {
                let left = Expr::lower(tyctx, fold, *lhs);
                let right = Expr::lower(tyctx, fold, *rhs);

                let def = match &left {
                    Expr::Ident { ty: Ty::Struct { def, .. }, .. } => def.clone(),
                    Expr::Deref { indir: _, expr: _, ty } => {
                        let mut peel = ty;
                        while let Ty::Ptr(t) | Ty::Ref(t) = peel {
                            peel = t;
                        }
                        if let Ty::Struct { def, .. } = peel {
                            def.clone()
                        } else {
                            unreachable!("lhs of field access must be struct {:?}", left)
                        }
                    }
                    Expr::AddrOf(_) => todo!(),
                    Expr::Array { ident: _, exprs: _, ty: _ } => todo!(),

                    _ => unreachable!("lhs of field access must be struct {:?}", left),
                };

                let field_idx = if let Expr::Ident { ident, .. } | Expr::Array { ident, .. } =
                    &right
                {
                    def.fields
                        .iter()
                        .enumerate()
                        .find_map(|(i, f)| if f.ident == *ident { Some(i as u32) } else { None })
                        .expect("field access of unknown field")
                } else {
                    unreachable!("rhs of field access must be struct field")
                };

                Expr::FieldAccess { lhs: box left, def, rhs: box right, field_idx }
            }
            ty::Expr::Urnary { op, expr } => {
                Expr::Urnary { op: UnOp::lower(op), expr: box Expr::lower(tyctx, fold, *expr), ty }
            }
            ty::Expr::Binary { op, lhs, rhs } => Expr::Binary {
                op: BinOp::lower(op),
                lhs: box Expr::lower(tyctx, fold, *lhs),
                rhs: box Expr::lower(tyctx, fold, *rhs),
                ty,
            },
            ty::Expr::Parens(expr) => Expr::Parens(box Expr::lower(tyctx, fold, *expr)),
            ty::Expr::Call { path, args, type_args } => {
                let ident = path.segs.last().unwrap();
                if type_args.iter().any(|arg| !arg.val.has_generics()) {}

                let func = tyctx.var_func.name_func.get(ident).expect("function is defined");
                Expr::Call {
                    path,
                    args: args.into_iter().map(|a| Expr::lower(tyctx, fold, a)).collect(),
                    type_args: type_args.into_iter().map(|t| Ty::lower(tyctx, &t.val)).collect(),
                    def: Func::lower(tyctx, fold, func),
                }
            }
            ty::Expr::TraitMeth { trait_, args, type_args } => {
                if type_args.iter().any(|arg| arg.val.has_generics()) {}

                let f = ty::Impl {
                    path: trait_.clone(),
                    type_arguments: type_args.clone(),
                    method: ty::Func::default(),
                    span: DUMMY,
                };
                let ident = trait_.segs.last().unwrap();
                let func = tyctx
                    .trait_solve
                    .impls
                    .get(&trait_)
                    .expect("function is defined")
                    .get(&type_args.iter().map(|t| &t.val).collect::<Vec<_>>())
                    .cloned()
                    .unwrap_or(&f);
                // .expect(&format!("types have impl {:?}", tyctx.trait_solve));
                Expr::TraitMeth {
                    trait_,
                    args: args.into_iter().map(|a| Expr::lower(tyctx, fold, a)).collect(),
                    type_args: type_args.into_iter().map(|t| Ty::lower(tyctx, &t.val)).collect(),
                    def: Impl::lower(tyctx, fold, func),
                }
            }
            ty::Expr::StructInit { path, fields } => {
                let ident = path.segs.last().unwrap();
                let struc = tyctx.name_struct.get(ident).expect("struct is defined");
                Expr::StructInit {
                    path,
                    fields: fields
                        .into_iter()
                        .zip(&struc.fields)
                        .map(|(finit, fdef)| FieldInit::lower(tyctx, fold, fdef, finit))
                        .collect(),
                    def: Struct::lower(tyctx, (*struc).clone()),
                }
            }
            ty::Expr::EnumInit { path, variant, items } => {
                let ident = path.segs.last().unwrap();
                let enu = tyctx.name_enum.get(ident).expect("struct is defined");
                Expr::EnumInit {
                    path,
                    variant,
                    items: items.into_iter().map(|f| Expr::lower(tyctx, fold, f)).collect(),
                    def: Enum::lower(tyctx, (*enu).clone()),
                }
            }
            ty::Expr::ArrayInit { items } => Expr::ArrayInit {
                items: items.into_iter().map(|f| Expr::lower(tyctx, fold, f)).collect(),
                ty,
            },
            ty::Expr::Value(v) => Expr::Value(Val::lower(v.val)),
        };
        // Evaluate any constant expressions, since this is the lowered Expr we don't have to worry
        // about destroying spans or hashes since we gather types for everything
        lowered.const_fold(tyctx);
        lowered
    }

    crate fn type_of(&self) -> Ty {
        match self {
            Expr::Ident { ident: _, ty } => ty.clone(),
            Expr::Deref { indir: _, expr: _, ty } => ty.clone(),
            Expr::AddrOf(expr) => expr.type_of(),
            Expr::Array { ident: _, exprs: _, ty } => ty.clone(),
            Expr::Urnary { op: _, expr: _, ty } => ty.clone(),
            Expr::Binary { op: _, lhs: _, rhs: _, ty } => ty.clone(),
            Expr::Parens(expr) => expr.type_of(),
            Expr::Call { def, .. } => def.ret.clone(),
            Expr::TraitMeth { trait_: _, args: _, type_args: _, def } => def.method.ret.clone(),
            Expr::FieldAccess { lhs: _, def, rhs: _, field_idx } => {
                def.fields[*field_idx as usize].ty.clone()
            }
            Expr::StructInit { def, .. } => Ty::Struct {
                ident: def.ident,
                gen: def.generics.iter().map(|g| g.to_type()).collect(),
                def: def.clone(),
            },
            Expr::EnumInit { def, .. } => Ty::Enum {
                ident: def.ident,
                gen: def.generics.iter().map(|g| g.to_type()).collect(),
                def: def.clone(),
            },
            Expr::ArrayInit { items: _, ty } => ty.clone(),
            Expr::Value(v) => v.type_of(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LValue {
    /// Access a named variable `a`.
    Ident { ident: Ident, ty: Ty },
    /// Remove indirection, follow a pointer to it's pointee.
    Deref { indir: usize, expr: Box<LValue>, ty: Ty },
    /// Access an array by index `[expr][expr]`.
    ///
    /// Each `exprs` represents an access of a dimension of the array.
    Array { ident: Ident, exprs: Vec<Expr>, ty: Ty },
    /// Access the fields of a struct `expr.expr.expr;`.
    FieldAccess { lhs: Box<LValue>, def: Struct, rhs: Box<LValue>, field_idx: u32 },
}

impl LValue {
    fn lower(tyctx: &TyCheckRes<'_, '_>, fold: &Folder, ex: ty::Expression) -> Self {
        match ex.val {
            ty::Expr::Ident(ident) => {
                let ty = Ty::lower(
                    tyctx,
                    &tyctx.type_of_ident(ident, ex.span).unwrap_or_else(|| {
                        panic!(
                            "type checking missed ident {}",
                            Error::error_with_span(tyctx, ex.span, "foolio")
                        )
                    }),
                );
                LValue::Ident { ident, ty }
            }
            ty::Expr::Deref { indir, expr } => {
                let lvar = LValue::lower(tyctx, fold, *expr);
                let ty = lvar.type_of().clone();
                LValue::Deref { indir, expr: box lvar, ty }
            }
            ty::Expr::Array { ident, exprs } => {
                let ty = Ty::lower(
                    tyctx,
                    &tyctx.type_of_ident(ident, ex.span).expect("type checking missed ident"),
                );
                LValue::Array {
                    ident,
                    exprs: exprs.into_iter().map(|expr| Expr::lower(tyctx, fold, expr)).collect(),
                    ty,
                }
            }
            ty::Expr::FieldAccess { lhs, rhs } => {
                let left = LValue::lower(tyctx, fold, *lhs);

                let def = match &left {
                    LValue::Ident { ty: Ty::Struct { def, .. }, .. } => def.clone(),
                    LValue::Deref { indir: _, expr: _, ty } => {
                        let mut peel = ty;
                        while let Ty::Ptr(t) | Ty::Ref(t) = peel {
                            peel = t;
                        }
                        if let Ty::Struct { def, .. } = peel {
                            def.clone()
                        } else {
                            unreachable!("lhs of field access must be struct {:?}", left)
                        }
                    }
                    LValue::Array { ident: _, exprs: _, ty: _ } => todo!(),

                    _ => unreachable!("lhs of field access must be struct {:?}", left),
                };

                let field = match &rhs.val {
                    ty::Expr::Ident(ident) => ident,
                    ty::Expr::Deref {
                        expr: box Spanned { val: ty::Expr::Ident(ident), .. },
                        ..
                    } => ident,
                    ty::Expr::Array { ident, exprs: _ } => ident,
                    _ => unreachable!("lhs of field access must be struct {:?}", left),
                };
                let (field_idx, right_ty) =
                    def.fields
                        .iter()
                        .enumerate()
                        .find_map(|(i, f)| {
                            if f.ident == *field {
                                Some((i as u32, f.ty.clone()))
                            } else {
                                None
                            }
                        })
                        .expect("field access of unknown field");

                LValue::FieldAccess {
                    lhs: box left,
                    def,
                    rhs: box match rhs.val.clone() {
                        ty::Expr::Ident(ident) => LValue::Ident { ident, ty: right_ty },
                        ty::Expr::Deref {
                            indir,
                            expr: box Spanned { val: ty::Expr::Ident(ident), .. },
                        } => {
                            let inner_ty = tyctx.type_of_ident(ident, rhs.span).unwrap();
                            LValue::Deref {
                                indir,
                                expr: box LValue::Ident { ident, ty: Ty::lower(tyctx, &inner_ty) },
                                ty: right_ty,
                            }
                        }
                        ty::Expr::Array { ident, exprs } => LValue::Array {
                            ident,
                            exprs: exprs.into_iter().map(|e| Expr::lower(tyctx, fold, e)).collect(),
                            ty: right_ty,
                        },
                        _ => unreachable!("lhs of field access must be struct {:?}", rhs),
                    },
                    field_idx,
                }
            }
            _ => unreachable!("not valid lvalue made it all the way to lowering"),
        }
    }

    crate fn as_ident(&self) -> Option<Ident> {
        Some(match self {
            LValue::Ident { ident, ty: _ } => *ident,
            LValue::Deref { indir: _, expr, .. } => expr.as_ident()?,
            LValue::Array { ident, .. } => *ident,
            LValue::FieldAccess { lhs, rhs: _, .. } => lhs.as_ident()?,
        })
    }

    crate fn type_of(&self) -> &Ty {
        match self {
            LValue::Ident { ty, .. } => ty,
            LValue::Deref { expr, .. } => expr.type_of(),
            LValue::Array { ty, .. } => ty,
            // TODO: do we want the final value this would affect array too
            LValue::FieldAccess { rhs, .. } => rhs.type_of(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    /// A generic type parameter `<T>`.
    ///
    /// N.B. This may be used as a type argument but should not be.
    Generic { ident: Ident, bound: Option<Path> },
    /// A static array of `size` containing item of `ty`.
    Array { size: usize, ty: Box<Ty> },
    /// A struct defined by the user.
    ///
    /// The `ident` is the name of the "type" and there are 'gen' generics.
    Struct { ident: Ident, gen: Vec<Ty>, def: Struct },
    /// An enum defined by the user.
    ///
    /// The `ident` is the name of the "type" and there are 'gen' generics.
    Enum { ident: Ident, gen: Vec<Ty>, def: Enum },
    /// A pointer to a type.
    ///
    /// This is equivalent to indirection, for each layer of `Ty::Ptr(..)` we have
    /// to follow a reference to get at the value.
    Ptr(Box<Ty>),
    /// This represents the number of times a pointer has been followed.
    ///
    /// The number of dereferences represented as layers.
    Ref(Box<Ty>),
    /// A string of `char`'s.
    ///
    /// `"hello, world"`
    String,
    /// A positive or negative number.
    Int,
    /// An ascii character.
    ///
    /// todo: Could be bound to between 0-255
    Char,
    /// A positive or negative number with a fractional component.
    Float,
    /// A single bit representing true and false.
    Bool,
    /// The empty/never/uninhabited type.
    Void,
}

impl Ty {
    crate fn size(&self) -> usize {
        match self {
            Ty::Array { size, ty } => ty.size() * size,
            Ty::Struct { ident: _, gen: _, def } => def.fields.iter().map(|f| f.ty.size()).sum(),
            Ty::Enum { ident: _, gen: _, def } => {
                def.variants.iter().map(|v| v.types.iter().map(|t| t.size()).sum::<usize>()).sum()
            }
            Ty::Ptr(_) | Ty::Ref(_) | Ty::String => 8,
            Ty::Int => 8,
            Ty::Char => 8,
            Ty::Float => 8,
            Ty::Bool => 4,
            Ty::Void => 0,
            _ => unreachable!("generic type should be monomorphized"),
        }
    }

    crate fn null_val(&self) -> Val {
        match self {
            Ty::Ptr(_) | Ty::Ref(_) | Ty::String | Ty::Int | Ty::Float => Val::Int(1),
            Ty::Char | Ty::Bool => Val::Int(1),
            _ => unreachable!("generic type should be monomorphized cannot create null value"),
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Array { size, ty } => {
                if let Ty::Array { ty: t, size: s } = &**ty {
                    write!(f, "{}[{}][{}]", t, size, s)
                } else {
                    write!(f, "{}[{}]", ty, size)
                }
            }
            Ty::Generic { ident, .. } => write!(f, "<{}>", ident),
            Ty::Struct { ident, gen, .. } => write!(
                f,
                "struct {}{}",
                ident,
                if gen.is_empty() {
                    "".to_owned()
                } else {
                    format!(
                        "<{}>",
                        gen.iter().map(|g| g.to_string()).collect::<Vec<_>>().join(", ")
                    )
                },
            ),
            Ty::Enum { ident, gen, .. } => write!(
                f,
                "enum {}{}",
                ident,
                if gen.is_empty() {
                    "".to_owned()
                } else {
                    format!(
                        "<{}>",
                        gen.iter().map(|g| g.to_string()).collect::<Vec<_>>().join(", ")
                    )
                }
            ),
            Ty::Ptr(t) => write!(f, "&{}", t),
            Ty::Ref(t) => write!(f, "*{}", t),
            Ty::String => write!(f, "string"),
            Ty::Int => write!(f, "int"),
            Ty::Char => write!(f, "char"),
            Ty::Float => write!(f, "float"),
            Ty::Bool => write!(f, "bool"),
            Ty::Void => write!(f, "void"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub ty: Ty,
    pub ident: Ident,
}

impl Param {
    fn lower(tyctx: &TyCheckRes<'_, '_>, v: ty::Param) -> Self {
        Param { ident: v.ident, ty: Ty::lower(tyctx, &v.ty.val) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

impl Block {
    fn lower(tyctx: &TyCheckRes<'_, '_>, fold: &Folder, b: ty::Block) -> Self {
        Block { stmts: b.stmts.into_iter().map(|s| Stmt::lower(tyctx, fold, s)).collect() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Binding {
    Wild(Ident),
    Value(Val),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pat {
    /// Match an enum variant `option::some(bind)`
    Enum {
        path: Path,
        variant: Ident,
        idx: usize,
        items: Vec<Pat>,
    },
    Array {
        size: usize,
        items: Vec<Pat>,
    },
    Bind(Binding),
}

impl Pat {
    fn lower(tyctx: &TyCheckRes<'_, '_>, fold: &Folder, pat: ty::Pat) -> Self {
        match pat {
            ty::Pat::Enum { path, variant, items } => {
                let ident = path.segs.last().unwrap();
                let idx = tyctx
                    .name_enum
                    .get(ident)
                    .and_then(|e| e.variants.iter().position(|v| variant == v.ident))
                    .unwrap();
                Pat::Enum {
                    path,
                    variant,
                    items: items.into_iter().map(|p| Pat::lower(tyctx, fold, p.val)).collect(),
                    idx,
                }
            }
            ty::Pat::Array { size, items } => Pat::Array {
                size,
                items: items.into_iter().map(|p| Pat::lower(tyctx, fold, p.val)).collect(),
            },
            ty::Pat::Bind(b) => Pat::Bind(match b {
                ty::Binding::Wild(w) => Binding::Wild(w),
                ty::Binding::Value(v) => Binding::Value(Val::lower(v.val)),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchArm {
    pub pat: Pat,
    pub blk: Block,
}

impl MatchArm {
    fn lower(tyctx: &TyCheckRes<'_, '_>, fold: &Folder, arm: ty::MatchArm) -> Self {
        MatchArm {
            pat: Pat::lower(tyctx, fold, arm.pat.val),
            blk: Block::lower(tyctx, fold, arm.blk),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallExpr {
    pub path: Path,
    pub args: Vec<Expr>,
    pub type_args: Vec<Ty>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraitMethExpr {
    pub trait_: Path,
    pub args: Vec<Expr>,
    pub type_args: Vec<Ty>,
}

#[derive(Clone, derive_help::Debug, PartialEq, Eq)]
pub enum Stmt {
    /// Variable declaration `int x;`
    Const(Const),
    /// Assignment `lval = rval;`
    Assign { lval: LValue, rval: Expr },
    /// A call statement `call(arg1, arg2)`
    Call {
        expr: CallExpr,
        #[dbg_ignore]
        def: Func,
    },
    /// A trait method call `<<T>::trait>(args)`
    TraitMeth {
        expr: TraitMethExpr,
        #[dbg_ignore]
        def: Impl,
    },
    /// If statement `if (expr) { stmts }`
    If { cond: Expr, blk: Block, els: Option<Block> },
    /// While loop `while (expr) { stmts }`
    While { cond: Expr, stmts: Block },
    /// A match statement `match expr { variant1 => { stmts }, variant2 => { stmts } }`.
    Match { expr: Expr, arms: Vec<MatchArm>, ty: Ty },
    /// Read statment `read(ident)`
    Read(Expr),
    /// Write statement `write(expr)`
    Write { expr: Expr },
    /// Return statement `return expr`
    Ret(Expr, Ty),
    /// Exit statement `exit`.
    ///
    /// A void return.
    Exit,
    /// A block of statements `{ stmts }`
    Block(Block),
}

impl Stmt {
    fn lower(tyctx: &TyCheckRes<'_, '_>, fold: &Folder, mut s: ty::Statement) -> Self {
        match s.val.clone() {
            ty::Stmt::Const(var) => Stmt::Const(Const {
                ty: Ty::lower(tyctx, &var.ty.val),
                ident: var.ident,
                init: Expr::lower(tyctx, fold, var.init),
                is_global: false,
            }),
            ty::Stmt::Assign { lval, rval } => Stmt::Assign {
                lval: LValue::lower(tyctx, fold, lval),
                rval: Expr::lower(tyctx, fold, rval),
            },
            ty::Stmt::Call(ty::Spanned {
                val: ty::Expr::Call { path, args, type_args }, ..
            }) => {
                let ident = path.segs.last().unwrap();
                if type_args.iter().all(|arg| !arg.val.has_generics()) {
                    TraitRes::new(tyctx, type_args.iter().map(|a| &a.val).collect())
                        .visit_stmt(&mut s);
                }
                let func = tyctx.var_func.name_func.get(ident).expect("function is defined");
                Stmt::Call {
                    expr: CallExpr {
                        path,
                        args: args.into_iter().map(|a| Expr::lower(tyctx, fold, a)).collect(),
                        type_args: type_args
                            .into_iter()
                            .map(|a| Ty::lower(tyctx, &a.val))
                            .collect(),
                    },
                    def: Func::lower(tyctx, fold, func),
                }
            }
            ty::Stmt::Call(_) => unreachable!("call statement without call expression"),
            ty::Stmt::TraitMeth(ty::Spanned {
                val: ty::Expr::TraitMeth { trait_, args, type_args },
                ..
            }) => {
                if type_args.iter().all(|arg| !arg.val.has_generics()) {
                    TraitRes::new(tyctx, type_args.iter().map(|a| &a.val).collect())
                        .visit_stmt(&mut s);
                }

                // TODO: here and in Expr, not sure how OK this is...
                let f = ty::Impl {
                    path: trait_.clone(),
                    type_arguments: type_args.clone(),
                    method: ty::Func::default(),
                    span: DUMMY,
                };
                let func = tyctx
                    .trait_solve
                    .impls
                    .get(&trait_)
                    .expect("function is defined")
                    .get(&type_args.iter().map(|t| &t.val).collect::<Vec<_>>())
                    .cloned()
                    .unwrap_or(&f);

                Stmt::TraitMeth {
                    expr: TraitMethExpr {
                        trait_: trait_.clone(),
                        args: args.iter().map(|a| Expr::lower(tyctx, fold, a.clone())).collect(),
                        type_args: type_args.iter().map(|a| Ty::lower(tyctx, &a.val)).collect(),
                    },
                    def: Impl::lower(tyctx, fold, func),
                }
            }
            ty::Stmt::TraitMeth(_) => {
                unreachable!("trait method call statement without call expression")
            }
            ty::Stmt::If { cond, blk, els } => Stmt::If {
                cond: Expr::lower(tyctx, fold, cond),
                blk: Block::lower(tyctx, fold, blk),
                els: els.map(|e| Block::lower(tyctx, fold, e)),
            },
            ty::Stmt::While { cond, stmts } => Stmt::While {
                cond: Expr::lower(tyctx, fold, cond),
                stmts: Block::lower(tyctx, fold, stmts),
            },
            ty::Stmt::Match { expr, arms } => {
                let expr = Expr::lower(tyctx, fold, expr);
                let ty = expr.type_of();
                Stmt::Match {
                    expr,
                    arms: arms.into_iter().map(|a| MatchArm::lower(tyctx, fold, a)).collect(),
                    ty,
                }
            }
            ty::Stmt::Read(expr) => Stmt::Read(Expr::lower(tyctx, fold, expr)),
            ty::Stmt::Write { expr } => Stmt::Write { expr: Expr::lower(tyctx, fold, expr) },
            ty::Stmt::Ret(expr) => {
                let expr = Expr::lower(tyctx, fold, expr);
                let ty = expr.type_of();
                Stmt::Ret(expr, ty)
            }
            ty::Stmt::Exit => Stmt::Exit,
            ty::Stmt::Block(ty::Block { stmts, .. }) => Stmt::Block(Block {
                stmts: stmts.into_iter().map(|s| Stmt::lower(tyctx, fold, s)).collect(),
            }),
            ty::Stmt::AssignOp { lval, rval, op } => todo!(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub ident: Ident,
    pub ty: Ty,
}

impl Field {
    fn lower(tyctx: &TyCheckRes<'_, '_>, v: ty::Field) -> Self {
        Field { ident: v.ident, ty: Ty::lower(tyctx, &v.ty.val) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Struct {
    pub ident: Ident,
    pub fields: Vec<Field>,
    pub generics: Vec<Generic>,
}

impl Struct {
    fn lower(tyctx: &TyCheckRes<'_, '_>, s: ty::Struct) -> Self {
        Struct {
            ident: s.ident,
            fields: s.fields.into_iter().map(|v| Field::lower(tyctx, v)).collect(),
            // TODO: any generic needs to be gone by this point
            generics: s.generics.into_iter().map(|t| Generic::lower(tyctx, t)).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    /// The name of the variant `some`.
    pub ident: Ident,
    /// The types contained in the variants "tuple".
    pub types: Vec<Ty>,
}

impl Variant {
    fn lower(tyctx: &TyCheckRes<'_, '_>, v: ty::Variant) -> Self {
        Variant {
            ident: v.ident,
            types: v.types.into_iter().map(|t| Ty::lower(tyctx, &t.val)).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enum {
    /// The name of the enum `<option>::none`.
    pub ident: Ident,
    /// The variants of the enum `option::<some(ty, type)>`.
    pub variants: Vec<Variant>,
    pub generics: Vec<Generic>,
}

impl Enum {
    fn lower(tyctx: &TyCheckRes<'_, '_>, e: ty::Enum) -> Self {
        Enum {
            ident: e.ident,
            variants: e.variants.into_iter().map(|v| Variant::lower(tyctx, v)).collect(),
            // TODO: any generic needs to be gone by this point
            generics: e.generics.into_iter().map(|t| Generic::lower(tyctx, t)).collect(),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Adt {
    Struct(Struct),
    Enum(Enum),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generic {
    pub ident: Ident,
    pub bound: Option<Path>,
}

impl Generic {
    fn lower(tyctx: &TyCheckRes<'_, '_>, g: ty::Generic) -> Self {
        Generic { ident: g.ident, bound: g.bound }
    }

    crate fn to_type(&self) -> Ty {
        Ty::Generic { ident: self.ident, bound: self.bound.clone() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Func {
    /// The return type `int name() { stmts }`
    pub ret: Ty,
    /// Name of the function.
    pub ident: Ident,
    /// The generic parameters listed for a function.
    pub generics: Vec<Ty>,
    /// the type and identifier of each parameter.
    pub params: Vec<Param>,
    /// All the crap the function does.
    pub stmts: Vec<Stmt>,
}

impl Func {
    fn lower(tyctx: &TyCheckRes<'_, '_>, fold: &Folder, func: &ty::Func) -> Self {
        Func {
            ret: Ty::lower(tyctx, &func.ret.val),
            ident: func.ident,
            params: func.params.iter().map(|p| Param::lower(tyctx, p.clone())).collect(),
            generics: func.generics.iter().map(|g| Ty::lower(tyctx, &g.to_type())).collect(),
            stmts: func.stmts.stmts.iter().map(|s| Stmt::lower(tyctx, fold, s.clone())).collect(),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraitMethod {
    Default(Func),
    NoBody(Func),
}

impl TraitMethod {
    crate fn function(&self) -> &Func {
        let (Self::Default(f) | Self::NoBody(f)) = self;
        f
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trait {
    pub ident: Ident,
    pub generics: Vec<Ty>,
    pub method: TraitMethod,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Impl {
    pub ident: Path,
    pub type_arguments: Vec<Ty>,
    pub method: Func,
}

impl Impl {
    fn lower(tyctx: &TyCheckRes<'_, '_>, fold: &Folder, imp: &ty::Impl) -> Self {
        Impl {
            ident: imp.path.clone(),
            type_arguments: imp.type_arguments.iter().map(|t| Ty::lower(tyctx, &t.val)).collect(),
            method: Func::lower(tyctx, fold, &imp.method),
        }
    }
}

/// A variable declaration.
///
/// `struct foo x;` or int x[]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Const {
    pub ty: Ty,
    pub ident: Ident,
    pub init: Expr,
    pub is_global: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Adt(Adt),
    Func(Func),
    Trait(Trait),
    Impl(Impl),
    Const(Const),
}

impl Ty {
    fn lower(tyctx: &TyCheckRes<'_, '_>, ty: &ty::Ty) -> Self {
        match ty {
            ty::Ty::Array { size, ty: t } => {
                Ty::Array { ty: box Ty::lower(tyctx, &t.val), size: *size }
            }
            ty::Ty::Struct { ident, gen } => Ty::Struct {
                ident: *ident,
                gen: gen.iter().map(|t| Ty::lower(tyctx, &t.val)).collect(),
                def: tyctx
                    .name_struct
                    .get(ident)
                    .map(|e| Struct::lower(tyctx, (*e).clone()))
                    .unwrap(),
            },
            ty::Ty::Enum { ident, gen } => Ty::Enum {
                ident: *ident,
                gen: gen.iter().map(|t| Ty::lower(tyctx, &t.val)).collect(),
                def: tyctx.name_enum.get(ident).map(|e| Enum::lower(tyctx, (*e).clone())).unwrap(),
            },
            ty::Ty::Ptr(t) => Ty::Ptr(box Ty::lower(tyctx, &t.val)),
            ty::Ty::Ref(t) => Ty::Ref(box Ty::lower(tyctx, &t.val)),
            ty::Ty::String => Ty::String,
            ty::Ty::Int => Ty::Int,
            ty::Ty::Char => Ty::Char,
            ty::Ty::Float => Ty::Float,
            ty::Ty::Bool => Ty::Bool,
            ty::Ty::Void => Ty::Void,
            ty::Ty::Generic { ident, bound } => Ty::Generic { ident: *ident, bound: bound.clone() },
            ty::Ty::Path(_) => todo!(),
            ty::Ty::Func { .. } => {
                todo!("pretty sure this is an error")
            }
        }
    }
}

crate fn lower_items(items: &[ty::Declaration], tyctx: TyCheckRes<'_, '_>) -> Vec<Item> {
    let fold = Folder::default();
    let mut lowered = vec![];
    for item in items {
        match &item.val {
            ty::Decl::Adt(_adt) => {}
            ty::Decl::Func(func) => {
                if func.generics.is_empty() {
                    lowered.push(Item::Func(Func::lower(&tyctx, &fold, func)));
                } else {
                    // Monomorphize
                    for mono in tyctx.mono_func(func) {
                        lowered.push(Item::Func(Func::lower(&tyctx, &fold, &mono)));
                    }
                }
            }
            ty::Decl::Impl(i) => {
                lowered.push(Item::Func(Func::lower(&tyctx, &fold, &i.method)));
            }
            ty::Decl::Const(var) => lowered.push(Item::Const(Const {
                ty: Ty::lower(&tyctx, &var.ty.val),
                ident: var.ident,
                init: Expr::lower(&tyctx, &fold, var.init.clone()),
                is_global: true,
            })),
            _ => {}
        }
    }
    lowered
}
