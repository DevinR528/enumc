use std::fmt::{self, Display, Write};

use crate::ast::types::{
    Adt, Block, Decl, Declaration, Enum, Expr, Expression, Field, FieldInit, Func, Generic,
    MatchArm, Param, Statement, Stmt, Struct, Ty, Type, Value, Var, Variant, DUMMY,
};

pub trait Visit<'ast>: Sized {
    fn visit_prog(&mut self, items: &'ast [Declaration]) {
        walk_items(self, items)
    }

    fn visit_decl(&mut self, item: &'ast Declaration) {
        walk_decl(self, item)
    }

    fn visit_func(&mut self, func: &'ast Func) {
        walk_func(self, func)
    }

    fn visit_adt(&mut self, adt: &'ast Adt) {
        walk_adt(self, adt)
    }

    fn visit_var(&mut self, var: &'ast Var) {
        walk_var(self, var)
    }

    fn visit_params(&mut self, params: &[Param]) {
        walk_params(self, params)
    }

    fn visit_generics(&mut self, generics: &[Type]) {}

    fn visit_ty(&mut self, ty: &Type) {
        // done
    }

    fn visit_stmt(&mut self, stmt: &'ast Statement) {
        walk_stmt(self, stmt)
    }

    fn visit_match_arm(&mut self, arms: &'ast [MatchArm]) {
        walk_match_arm(self, arms)
    }

    fn visit_expr(&mut self, expr: &'ast Expression) {
        walk_expr(self, expr)
    }
}

crate fn walk_items<'ast, V: Visit<'ast>>(visit: &mut V, items: &'ast [Declaration]) {
    for item in items {
        visit.visit_decl(item);
    }
}

crate fn walk_decl<'ast, V: Visit<'ast>>(visit: &mut V, item: &'ast Declaration) {
    match &item.val {
        Decl::Func(func) => {
            visit.visit_func(func);
        }
        Decl::Var(var) => {
            visit.visit_var(var);
        }
        Decl::Adt(struc) => visit.visit_adt(struc),
    }
}

crate fn walk_func<'ast, V: Visit<'ast>>(visit: &mut V, func: &'ast Func) {
    let Func { ident, params, stmts, ret, generics, span: _ } = func;
    // visit.visit_ident(ident);
    visit.visit_generics(generics);
    visit.visit_params(params);
    visit.visit_ty(ret);
    for stmt in stmts {
        visit.visit_stmt(stmt);
    }
}

crate fn walk_adt<'ast, V: Visit<'ast>>(visit: &mut V, adt: &'ast Adt) {
    match adt {
        Adt::Struct(Struct { ident, fields, generics, span: _ }) => {
            // visit.visit_ident(ident);
            for Field { ident, ty, span: _ } in fields {
                visit.visit_ty(ty);
            }
        }
        Adt::Enum(Enum { ident, variants, generics, .. }) => {
            for Variant { ident, types, span: _ } in variants {
                for ty in types {
                    visit.visit_ty(ty);
                }
            }
        }
    }
}

crate fn walk_var<'ast, V: Visit<'ast>>(visit: &mut V, var: &'ast Var) {
    // visit.visit_ident(&var.ident);
    visit.visit_ty(&var.ty);
}

crate fn walk_params<'ast, V: Visit<'ast>>(visit: &mut V, params: &[Param]) {
    for Param { ident, ty, .. } in params {
        visit.visit_ty(ty);
    }
}

crate fn walk_match_arm<'ast, V: Visit<'ast>>(visit: &mut V, arms: &'ast [MatchArm]) {
    for MatchArm { pat, blk: Block { stmts, .. }, .. } in arms {
        for stmt in stmts {
            visit.visit_stmt(stmt);
        }
    }
}

crate fn walk_stmt<'ast, V: Visit<'ast>>(visit: &mut V, stmt: &'ast Statement) {
    match &stmt.val {
        Stmt::VarDecl(vars) => {
            for var in vars {
                visit.visit_var(var)
            }
        }
        Stmt::Assign { lval, rval, .. } => {
            // visit.visit_ident(ident);
            visit.visit_expr(lval);
            visit.visit_expr(rval);
        }
        Stmt::Call { ident, args } => {
            // visit.visit_ident(ident);
            for expr in args {
                visit.visit_expr(expr);
            }
        }
        Stmt::If { cond, blk: Block { stmts, .. }, els } => {
            visit.visit_expr(cond);
            for stmt in stmts {
                visit.visit_stmt(stmt);
            }
            if let Some(Block { stmts, .. }) = els {
                for stmt in stmts {
                    visit.visit_stmt(stmt);
                }
            }
        }
        Stmt::While { cond, stmt } => {
            visit.visit_expr(cond);
            visit.visit_stmt(stmt);
        }
        Stmt::Match { expr, arms } => {
            visit.visit_expr(expr);
            visit.visit_match_arm(arms);
        }
        Stmt::Read(_) => {
            // variable ident
        }
        Stmt::Write { expr } => visit.visit_expr(expr),
        Stmt::Ret(expr) => visit.visit_expr(expr),
        Stmt::Exit => {}
        Stmt::Block(Block { stmts, .. }) => {
            for stmt in stmts {
                visit.visit_stmt(stmt);
            }
        }
    }
}

crate fn walk_expr<'ast, V: Visit<'ast>>(visit: &mut V, expr: &'ast Expression) {
    match &expr.val {
        Expr::Ident(id) => {
            // visit.visit_ident(id)
        }
        Expr::Array { ident, exprs } => {
            // visit.visit_ident(ident);
            for expr in exprs {
                visit.visit_expr(expr)
            }
        }
        Expr::Urnary { op, expr } => {
            visit.visit_expr(expr);
        }
        Expr::Deref { indir, expr } => {
            visit.visit_expr(expr);
        }
        Expr::AddrOf(expr) => {
            visit.visit_expr(expr);
        }
        Expr::Binary { op, lhs, rhs } => {
            visit.visit_expr(lhs);
            visit.visit_expr(rhs)
        }
        Expr::Parens(expr) => visit.visit_expr(expr),
        Expr::StructInit { name, fields } => {
            for FieldInit { ident, init, span: _ } in fields {
                visit.visit_expr(init);
            }
        }
        Expr::EnumInit { items, .. } => {
            for expr in items {
                visit.visit_expr(expr);
            }
        }
        Expr::ArrayInit { items } => {
            for expr in items {
                visit.visit_expr(expr);
            }
        }
        Expr::FieldAccess { lhs, rhs } => {
            visit.visit_expr(lhs);
            visit.visit_expr(rhs)
        }
        Expr::Call { ident, args, type_args } => {
            for expr in args {
                visit.visit_expr(expr);
            }
        }
        Expr::Value(_) => {
            // visit.visit_value(val);
        }
    }
}

#[derive(Default, Debug)]
crate struct DotWalker {
    buf: String,
    node_id: usize,
    prev_id: usize,
}

type Res = std::fmt::Result;

impl DotWalker {
    crate fn new() -> Self {
        Self { buf: String::from("digraph ast {\n"), ..DotWalker::default() }
    }

    fn walk_deeper<P: Fn(&mut Self) -> Res, F: Fn(&mut Self) -> Res>(&mut self, pre: P, calls: F) {
        self.node_id += 1;
        pre(self).unwrap();
        let tmp = self.prev_id;
        self.prev_id = self.node_id;
        calls(self).unwrap();
        self.prev_id = tmp;
    }
}

impl Display for DotWalker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}}}", self.buf)
    }
}

impl<'ast> Visit<'ast> for DotWalker {
    fn visit_prog(&mut self, items: &[Declaration]) {
        writeln!(&mut self.buf, "{}[label = PGM, shape = ellipse]", self.node_id);
        for item in items {
            match &item.val {
                Decl::Func(func) => {
                    self.visit_func(func);
                }
                Decl::Var(var) => {
                    self.visit_var(var);
                }
                Decl::Adt(struc) => {
                    self.visit_adt(struc);
                }
            }
        }
    }

    fn visit_func(&mut self, func: &Func) {
        self.walk_deeper(
            |this| {
                writeln!(
                    this.buf,
                    "{}[label = \"func {}({})\", shape = ellipse]",
                    this.node_id,
                    func.ident,
                    func.params
                        .iter()
                        .map(|p| format!("{} {}", p.ty.val, p.ident))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
                writeln!(this.buf, "{} -> {}", this.prev_id, this.node_id)
            },
            |this| {
                for stmt in &func.stmts {
                    this.visit_stmt(stmt);
                }
                Ok(())
            },
        );
    }

    fn visit_adt(&mut self, adt: &'ast Adt) {
        let (ident, fields): (_, Vec<_>) = match adt {
            Adt::Struct(Struct { ident, fields, .. }) => (
                ident.clone(),
                fields.iter().map(|f| format!("field {}{}", f.ty.val, f.ident)).collect(),
            ),
            Adt::Enum(Enum { ident, variants, .. }) => (
                ident.clone(),
                variants
                    .iter()
                    .map(|v| {
                        format!(
                            "variant {}({})",
                            v.ident,
                            v.types
                                .iter()
                                .map(|t| t.val.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })
                    .collect(),
            ),
        };
        self.walk_deeper(
            |this| {
                writeln!(
                    this.buf,
                    "{}[label = \"struct {}\", shape = ellipse]",
                    this.node_id, ident
                )?;
                writeln!(this.buf, "{} -> {}", this.prev_id, this.node_id)
            },
            |this| {
                for ident in &fields {
                    this.node_id += 1;
                    writeln!(this.buf, "{}[label = \"{}\", shape = ellipse]", this.node_id, ident)?;
                    writeln!(this.buf, "{} -> {}", this.prev_id, this.node_id)?;
                }
                Ok(())
            },
        );
    }

    fn visit_var(&mut self, var: &Var) {
        self.node_id += 1;
        writeln!(
            &mut self.buf,
            "{}[label = \"var {} {}\", shape = ellipse]",
            self.node_id, var.ty.val, var.ident
        );
        writeln!(&mut self.buf, "{} -> {}", self.prev_id, self.node_id);
    }

    fn visit_match_arm(&mut self, arms: &'ast [MatchArm]) {
        for MatchArm { pat, blk, span } in arms {
            self.walk_deeper(
                |this| {
                    writeln!(
                        &mut this.buf,
                        "{}[label = \"arm {}\", shape = ellipse]",
                        this.node_id, pat
                    )?;
                    writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                },
                |this| {
                    this.visit_stmt(&Stmt::Block(blk.clone()).into_spanned(DUMMY));
                    Ok(())
                },
            );
        }
    }

    fn visit_ty(&mut self, ty: &Type) {
        // done
    }

    fn visit_stmt(&mut self, stmt: &Statement) {
        match &stmt.val {
            Stmt::VarDecl(vars) => {
                for var in vars {
                    self.visit_var(var);
                }
            }
            Stmt::Assign { lval, rval } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"assign {}\", shape = ellipse]",
                            this.node_id,
                            lval.val.as_ident_string(),
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(rval);
                        Ok(())
                    },
                );
            }
            Stmt::Call { ident, args } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"call {}\", shape = ellipse]",
                            this.node_id, ident
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        for expr in args {
                            this.visit_expr(expr);
                        }
                        Ok(())
                    },
                );
            }
            Stmt::If { cond, blk, els } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"if call\", shape = ellipse]",
                            this.node_id
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(cond);
                        this.walk_deeper(
                            |me| {
                                writeln!(
                                    &mut me.buf,
                                    "{}[label = \"block\", shape = ellipse]",
                                    me.node_id
                                )?;
                                writeln!(&mut me.buf, "{} -> {}", me.prev_id, me.node_id)
                            },
                            |me| {
                                for stmt in &blk.stmts {
                                    me.visit_stmt(stmt);
                                }
                                Ok(())
                            },
                        );
                        if let Some(Block { stmts, .. }) = els {
                            this.walk_deeper(
                                |me| {
                                    writeln!(
                                        &mut me.buf,
                                        "{}[label = \"else block\", shape = ellipse]",
                                        me.node_id
                                    )?;
                                    writeln!(&mut me.buf, "{} -> {}", me.prev_id, me.node_id)
                                },
                                |me| {
                                    for stmt in stmts {
                                        me.visit_stmt(stmt);
                                    }
                                    Ok(())
                                },
                            );
                        }
                        Ok(())
                    },
                );
            }
            Stmt::While { cond, stmt } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"while loop\", shape = ellipse]",
                            this.node_id
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(cond);
                        this.visit_stmt(stmt);
                        Ok(())
                    },
                );
            }
            Stmt::Match { expr, arms } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"match stmt\", shape = ellipse]",
                            this.node_id
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(expr);
                        this.visit_match_arm(arms);
                        Ok(())
                    },
                );
            }
            Stmt::Read(ident) => {
                self.node_id += 1;
                writeln!(
                    &mut self.buf,
                    "{}[label = \"read({})\", shape = ellipse]",
                    self.node_id, ident
                );
                writeln!(&mut self.buf, "{} -> {}", self.prev_id, self.node_id);
            }
            Stmt::Write { expr } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"write call\", shape = ellipse]",
                            this.node_id
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(expr);
                        Ok(())
                    },
                );
            }
            Stmt::Ret(expr) => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"return\", shape = ellipse]",
                            this.node_id
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(expr);
                        Ok(())
                    },
                );
            }
            Stmt::Exit => {
                self.node_id += 1;
                writeln!(&mut self.buf, "{}[label = \"exit\", shape = ellipse]", self.node_id);
                writeln!(&mut self.buf, "{} -> {}", self.prev_id, self.node_id);
            }
            Stmt::Block(Block { stmts, .. }) => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"block\", shape = ellipse]",
                            this.node_id
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        for stmt in stmts {
                            this.visit_stmt(stmt);
                        }
                        Ok(())
                    },
                );
            }
        }
    }

    fn visit_expr(&mut self, expr: &Expression) {
        match &expr.val {
            Expr::Ident(name) => {
                self.node_id += 1;
                writeln!(
                    &mut self.buf,
                    "{}[label = \"ident {}\", shape = ellipse]",
                    self.node_id, name
                );
                writeln!(&mut self.buf, "{} -> {}", self.prev_id, self.node_id);
            }
            Expr::Array { ident, exprs } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"array {}{}\", shape = ellipse]",
                            this.node_id,
                            ident,
                            "[]".repeat(exprs.len())
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        for expr in exprs {
                            this.visit_expr(expr);
                        }
                        Ok(())
                    },
                );
            }
            Expr::Urnary { op, expr } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"expr UrnOp {:?}\", shape = ellipse]",
                            this.node_id, op
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(expr);
                        Ok(())
                    },
                );
            }
            Expr::Deref { indir, expr } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"expr deref of {} times\", shape = ellipse]",
                            this.node_id, indir,
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(expr);
                        Ok(())
                    },
                );
            }
            Expr::AddrOf(expr) => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"expr address of\", shape = ellipse]",
                            this.node_id,
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(expr);
                        Ok(())
                    },
                );
            }
            Expr::Binary { op, lhs, rhs } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"expr BinOp {:?}\", shape = ellipse]",
                            this.node_id, op
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(lhs);
                        this.visit_expr(rhs);
                        Ok(())
                    },
                );
            }
            Expr::Parens(expr) => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"parenthesis\", shape = ellipse]",
                            this.node_id,
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(expr);
                        Ok(())
                    },
                );
            }
            Expr::StructInit { name, fields } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"struct initializer {}\", shape = ellipse]",
                            this.node_id, name,
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        for FieldInit { ident, init, span } in fields {
                            this.visit_expr(init);
                        }
                        Ok(())
                    },
                );
            }
            Expr::EnumInit { ident, variant, items } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"enum initializer {}::{}\", shape = ellipse]",
                            this.node_id, ident, variant
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        for expr in items {
                            this.visit_expr(expr);
                        }
                        Ok(())
                    },
                );
            }
            Expr::ArrayInit { items } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"array initializer\", shape = ellipse]",
                            this.node_id,
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        for expr in items {
                            this.visit_expr(expr);
                        }
                        Ok(())
                    },
                );
            }
            Expr::FieldAccess { lhs, rhs } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"expr field access\", shape = ellipse]",
                            this.node_id
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        this.visit_expr(lhs);
                        this.visit_expr(rhs);
                        Ok(())
                    },
                );
            }
            Expr::Call { ident, args, type_args } => {
                self.walk_deeper(
                    |this| {
                        writeln!(
                            &mut this.buf,
                            "{}[label = \"call {}\", shape = ellipse]",
                            this.node_id, ident
                        )?;
                        writeln!(&mut this.buf, "{} -> {}", this.prev_id, this.node_id)
                    },
                    |this| {
                        for expr in args {
                            this.visit_expr(expr);
                        }
                        Ok(())
                    },
                );
            }
            Expr::Value(val) => {
                self.node_id += 1;
                writeln!(
                    &mut self.buf,
                    "{}[label = \"value {}\", shape = ellipse]",
                    self.node_id, val
                );
                writeln!(&mut self.buf, "{} -> {}", self.prev_id, self.node_id);
            }
        }
    }
}
