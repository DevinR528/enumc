use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
};

use crate::{
    ast::types::{
        Adt, BinOp, Binding, Block, Decl, Enum, Expr, Expression, Field, FieldInit, Func, Impl,
        MatchArm, Param, Pat, Range, Spany, Statement, Stmt, Struct, Trait, Ty, Type, TypeEquality,
        UnOp, Val, Var, Variant, DUMMY,
    },
    error::Error,
    typeck::generic::TyRegion,
    visit::Visit,
};

crate mod generic;
crate mod trait_solver;

use generic::{GenericResolver, Node};
use trait_solver::TraitSolve;

#[derive(Debug, Default)]
crate struct VarInFunction<'ast> {
    /// A backwards mapping of variable span -> function name.
    func_spans: BTreeMap<Range, String>,
    /// The variables in functions, mapped fn name -> variables.
    func_refs: HashMap<String, HashMap<String, Ty>>,
    /// Name to the function it represents.
    crate name_func: HashMap<String, &'ast Func>,
    /// Does this function have any return statements.
    func_return: HashSet<String>,
    /// All of the variables in a scope that are used.
    unsed_vars: HashMap<String, (Range, Cell<bool>)>,
}

impl VarInFunction<'_> {
    crate fn get_fn_by_span(&self, span: Range) -> Option<&str> {
        self.func_spans.iter().find_map(|(k, v)| {
            if k.start <= span.start && k.end >= span.end {
                Some(&**v)
            } else {
                None
            }
        })
    }

    fn insert(&mut self, rng: Range, name: String) -> Option<String> {
        self.func_spans.insert(rng, name)
    }
}
#[derive(Default)]
crate struct TyCheckRes<'ast, 'input> {
    /// The name of the file being checked.
    crate name: &'input str,
    /// The content of the file as a string.
    crate input: &'input str,

    /// The name of the function currently in or `None` if global.
    curr_fn: Option<String>,
    /// Global variables declared outside of functions.
    global: HashMap<String, Ty>,

    /// All the info about variables local to a specific function.
    ///
    /// Parameters are included in the locals.
    crate var_func: VarInFunction<'ast>,

    /// A mapping of expression -> type, this is the main inference table.
    crate expr_ty: HashMap<&'ast Expression, Ty>,

    /// An `Expression` -> `Ty` mapping made after monomorphization.
    ///
    /// Types reflect specializations that happens to the expressions. This
    /// only effects expressions where parameters are used (as far as I can tell) since
    /// `GenSubstitution` removes all the typed statements and expressions.
    crate mono_expr_ty: RefCell<HashMap<Expression, Ty>>,

    /// A mapping of identities -> val, this is how const folding keeps track of `Expr::Ident`s.
    crate consts: HashMap<&'ast str, &'ast Val>,

    /// A mapping of struct name to the fields of that struct.
    struct_fields: HashMap<String, (Vec<Type>, Vec<Field>)>,
    /// A mapping of enum name to the variants of that enum.
    enum_fields: HashMap<String, (Vec<Type>, Vec<Variant>)>,
    /// A mapping of struct name to struct def.
    crate name_struct: HashMap<String, &'ast Struct>,
    /// A mapping of enum name to enum def.
    crate name_enum: HashMap<String, &'ast Enum>,

    /// Resolve generic types at the end of type checking.
    crate generic_res: GenericResolver<'ast>,
    /// Trait resolver for checking the bounds on generic types.
    crate trait_solve: TraitSolve<'ast>,

    uniq_generic_instance_id: Cell<usize>,

    /// Errors collected during parsing and type checking.
    errors: Vec<Error<'input>>,
}

impl fmt::Debug for TyCheckRes<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TyCheckResult")
            // .field("global", &self.global)
            // .field("curr_fn", &self.curr_fn)
            // .field("func_refs", &self.var_func.func_refs)
            // .field("func_params", &self.func_params)
            // .field("expr_ty", &self.expr_ty)
            // .field("struct_fields", &self.struct_fields)
            // .field("enum_fields", &self.enum_fields)
            .field("generic_res", &self.generic_res)
            .field("trait_solve", &self.trait_solve)
            .finish()
    }
}

impl<'input> TyCheckRes<'_, 'input> {
    crate fn new(input: &'input str, name: &'input str) -> Self {
        Self { name, input, ..Self::default() }
    }

    crate fn report_errors(&self) -> Result<(), &'static str> {
        if !self.errors.is_empty() {
            for e in &self.errors {
                eprintln!("{}", e)
            }
            // println!("{:?}", self);
            return Err("errors");
        }
        Ok(())
    }

    crate fn unique_id(&self) -> usize {
        let x = self.uniq_generic_instance_id.get();
        self.uniq_generic_instance_id.set(x + 1);
        x
    }

    crate fn type_of_ident(&self, id: &str, span: Range) -> Option<Ty> {
        // TODO: unused leaks into other scope
        if let Some((_, b)) = self.var_func.unsed_vars.get(id) {
            b.set(true);
        }

        self.var_func
            .get_fn_by_span(span)
            .and_then(|f| self.var_func.func_refs.get(f).and_then(|s| s.get(id)))
            .or_else(|| self.global.get(id))
            .cloned()
    }
}

impl<'ast, 'input> Visit<'ast> for TyCheckRes<'ast, 'input> {
    fn visit_prog(&mut self, items: &'ast [crate::ast::types::Declaration]) {
        let mut funcs = vec![];
        let mut impls = vec![];
        for item in items {
            match &item.val {
                Decl::Func(func) => {
                    self.visit_func(func);
                    funcs.push(func);
                }
                Decl::Var(var) => {
                    self.visit_var(var);
                }
                Decl::Trait(trait_) => self.visit_trait(trait_),
                Decl::Impl(imp) => {
                    self.visit_impl(imp);
                    self.visit_func(&imp.method);
                    impls.push(imp);
                }
                Decl::Adt(adt) => self.visit_adt(adt),
                Decl::Const(co) => {}
            }
        }
        // Stabilize order which I'm not sure how it gets unordered
        funcs.sort_by(|a, b| a.span.start.cmp(&b.span.start));
        for func in funcs {
            self.curr_fn = Some(func.ident.clone());

            crate::visit::walk_func(self, func);

            if !matches!(func.ret.val, Ty::Void) && !self.var_func.func_return.contains(&func.ident)
            {
                panic!(
                    "{}",
                    Error::error_with_span(
                        self,
                        func.span,
                        &format!(
                            "function `{}` has return type `{}` but no return statement",
                            func.ident, func.ret.val
                        ),
                    )
                )
            }
            self.curr_fn.take();
        }

        // stabilize order
        impls.sort_by(|a, b| a.span.start.cmp(&b.span.start));
        for trait_ in impls {
            self.curr_fn = Some(trait_.method.ident.clone());
            crate::visit::walk_func(self, &trait_.method);
            self.curr_fn.take();
        }

        let mut unused = self
            .var_func
            .unsed_vars
            .iter()
            .filter(|(_id, (_, used))| !used.get())
            .map(|(id, (sp, _))| (id, *sp))
            .collect::<Vec<_>>();
        unused.sort_by(|a, b| a.1.cmp(&b.1));
        // After all checking then we can check for unused vars
        for (unused, span) in unused {
            self.errors.push(Error::error_with_span(
                self,
                span,
                &format!("unused variable `{}`, remove or reference", unused),
            ));
        }
    }

    fn visit_trait(&mut self, t: &'ast Trait) {
        if self.trait_solve.add_trait(t).is_some() {
            panic!(
                "{}",
                Error::error_with_span(
                    self,
                    t.span,
                    &format!("duplicate trait `{}` found", t.ident)
                )
            )
        }
    }

    fn visit_impl(&mut self, imp: &'ast Impl) {
        if let Err(e) = self.trait_solve.add_impl(imp) {
            panic!(
                "{}\n{}",
                e,
                Error::error_with_span(
                    self,
                    imp.span,
                    &format!("no trait `{}` found for this implementation", imp.ident)
                )
            )
        }
    }

    fn visit_func(&mut self, func: &'ast Func) {
        if self.curr_fn.is_none() {
            // Current function scope (also the name)
            self.curr_fn = Some(func.ident.clone());

            if self.var_func.insert(func.span, func.ident.clone()).is_some() {
                self.errors.push(Error::error_with_span(
                    self,
                    func.span,
                    "function takes up same span as other function",
                ));
            }

            assert!(
                !(func.generics.is_empty() && func.ret.val.has_generics()),
                "{}",
                Error::error_with_span(self, func.span, "generic type used without being declared",)
            );

            if !func.generics.is_empty() {
                self.generic_res.collect_generic_params(
                    &Node::Func(func.ident.clone()),
                    &Ty::Func {
                        ident: func.ident.clone(),
                        ret: box func.ret.val.clone(),
                        params: func.generics.iter().map(|t| t.val.clone()).collect(),
                    },
                );
            }

            // Now we can check the return value incase it was generic we did that ^^
            //
            // We take from the `generics` to get bound info
            let ty = &func.ret;
            if func.ret.val.has_generics() {
                self.generic_res.collect_generic_usage(
                    &ty.val,
                    self.unique_id(),
                    0,
                    &[],
                    &mut vec![Node::Func(func.ident.clone())],
                );

                let matching_gen = func.generics.iter().any(
                    |g| matches!(&g.val, Ty::Generic {ident: id, ..} if id == ty.val.generics()[0]),
                );
                assert!(
                    matching_gen,
                    "{}",
                    Error::error_with_span(
                        self,
                        func.span,
                        &format!("found `{}` which is not a declared generic type", func.ret.val),
                    ),
                );
            };

            assert!(
                self.var_func.name_func.insert(func.ident.to_owned(), func).is_none(),
                "{}",
                Error::error_with_span(
                    self,
                    func.span,
                    &format!("multiple function declaration `{}`", func.ident),
                )
            );
        } else {
            panic!(
                "{}",
                Error::error_with_span(self, func.span, "function defined within function")
            )
        }
        // We have left this functions scope
        self.curr_fn.take();
    }

    fn visit_adt(&mut self, adt: &'ast Adt) {
        match adt {
            Adt::Struct(struc) => {
                if self
                    .struct_fields
                    .insert(struc.ident.clone(), (struc.generics.clone(), struc.fields.clone()))
                    .is_some()
                {
                    self.errors.push(Error::error_with_span(
                        self,
                        struc.span,
                        "duplicate struct names",
                    ));
                }

                self.name_struct.insert(struc.ident.clone(), struc);

                if !struc.generics.is_empty() {
                    self.generic_res.collect_generic_params(
                        &Node::Struct(struc.ident.clone()),
                        &Ty::Struct { ident: struc.ident.to_string(), gen: struc.generics.clone() },
                    );
                }
            }
            Adt::Enum(en) => {
                if self
                    .enum_fields
                    .insert(en.ident.clone(), (en.generics.clone(), en.variants.clone()))
                    .is_some()
                {
                    self.errors.push(Error::error_with_span(
                        self,
                        en.span,
                        "duplicate struct names",
                    ));
                }

                self.name_enum.insert(en.ident.clone(), en);

                if !en.generics.is_empty() {
                    self.generic_res.collect_generic_params(
                        &Node::Enum(en.ident.clone()),
                        &Ty::Enum { ident: en.ident.to_string(), gen: en.generics.clone() },
                    );
                }
            }
        }
    }

    fn visit_var(&mut self, var: &'ast Var) {
        #[allow(clippy::if_then_panic)]
        if let Some(fn_id) = self.curr_fn.clone() {
            let node = Node::Func(fn_id.clone());
            let mut stack = if self.generic_res.has_generics(&node) { vec![node] } else { vec![] };

            self.generic_res.collect_generic_usage(
                &var.ty.val,
                self.unique_id(),
                0,
                &[TyRegion::VarDecl(var)],
                &mut stack,
            );

            if self
                .var_func
                .func_refs
                .entry(fn_id)
                .or_default()
                .insert(var.ident.clone(), var.ty.val.clone())
                .is_some()
            {
                panic!(
                    "{}",
                    Error::error_with_span(
                        self,
                        var.span,
                        &format!("duplicate variable name `{}`", var.ident),
                    )
                );
            }
        } else if self.global.insert(var.ident.clone(), var.ty.val.clone()).is_some() {
            panic!(
                "{}",
                Error::error_with_span(
                    self,
                    var.span,
                    &format!("global variable `{}` is already declared", var.ident)
                )
            );
        }
        self.var_func.unsed_vars.insert(var.ident.clone(), (var.span, Cell::new(false)));
    }

    fn visit_params(&mut self, params: &[Param]) {
        if let Some(fn_id) = self.curr_fn.clone() {
            for Param { ident, ty, span } in params {
                // TODO: Do this for returns and any place we match for Ty::Generic {..}
                if ty.val.has_generics() {
                    self.generic_res.collect_generic_usage(
                        &ty.val,
                        self.unique_id(),
                        0,
                        &[],
                        &mut vec![Node::Func(fn_id.clone())],
                    );

                    let matching_gen = self.var_func
                        .name_func
                        .get(&fn_id)
                        .and_then(|f| {
                            // TODO: this doesn't work for something like `enum result<T, E>`
                            // only checks `T` now
                            f.generics.iter().find(
                                |g| matches!(&g.val, Ty::Generic {ident: id, ..} if id == ty.val.generics()[0]),
                            )
                        }).is_some();
                    assert!(
                        matching_gen,
                        "{}",
                        Error::error_with_span(
                            self,
                            *span,
                            &format!("found {} which is not a declared generic type", ty.val),
                        ),
                    );
                };
                if self
                    .var_func
                    .func_refs
                    .entry(fn_id.clone())
                    .or_default()
                    .insert(ident.clone(), ty.val.clone())
                    .is_some()
                {
                    self.errors.push(Error::error_with_span(
                        self,
                        *span,
                        &format!("duplicate param name `{}`", ident),
                    ));
                }
                self.var_func.unsed_vars.insert(ident.clone(), (*span, Cell::new(false)));
            }
        } else {
            panic!("{}", Error::error_with_span(self, DUMMY, &format!("{:?}", params)))
        }
    }

    /// We overwrite this so that no type checking of the arm statements happens until we
    /// gather the nested scope from binding in match arms.
    ///
    /// See `StmtCheck::visit_stmt` for what happens.
    fn visit_match_arm(&mut self, _arms: &'ast [MatchArm]) {}

    fn visit_stmt(&mut self, stmt: &'ast Statement) {
        crate::visit::walk_stmt(self, stmt);

        // check the statement after walking incase there were var declarations
        let mut check = StmtCheck { tcxt: self };
        check.visit_stmt(stmt);
    }

    #[allow(clippy::if_then_panic)]
    fn visit_expr(&mut self, expr: &'ast Expression) {
        match &expr.val {
            Expr::Ident(var_name) => {
                if let Some(ty) = self.type_of_ident(var_name, expr.span) {
                    self.expr_ty.insert(expr, ty);
                    // Ok because of `x += 1;` turns into `x = x + 1;`
                } else {
                    panic!(
                        "{}",
                        Error::error_with_span(self, expr.span, "no type found for ident expr",)
                    );
                }
            }
            Expr::Array { ident, exprs } => {
                for expr in exprs {
                    self.visit_expr(expr);
                }

                for e in exprs {
                    let ty = self.expr_ty.get(e);
                    if !matches!(ty, Some(Ty::Int)) {
                        panic!(
                            "{}",
                            Error::error_with_span(
                                self,
                                expr.span,
                                &format!(
                                    "cannot index array with {}",
                                    ty.map_or("<unknown>".to_owned(), |t| t.to_string())
                                )
                            )
                        );
                    }
                }
                if let Some(ty) = self.type_of_ident(ident, expr.span) {
                    if self.expr_ty.insert(expr, ty).is_some() {
                        // Ok because of `x[0] += 1;` turns into `x[0] = x[0] + 1;`
                    }
                } else {
                    self.errors.push(Error::error_with_span(
                        self,
                        expr.span,
                        "no type found for array expr",
                    ));
                }
            }
            Expr::Urnary { op, expr: inner_expr } => {
                self.visit_expr(inner_expr);
                let ty = self.expr_ty.get(&**inner_expr);
                match op {
                    UnOp::Not => {
                        if is_truthy(ty) {
                            self.expr_ty.insert(expr, Ty::Bool);
                        } else {
                            self.errors.push(Error::error_with_span(
                                self,
                                expr.span,
                                "cannot negate non bool type",
                            ));
                        }
                    }
                    UnOp::OnesComp => {
                        // TODO: think about pointer maths
                        if let Some(Ty::Int | Ty::Ptr(_)) = ty {
                            self.expr_ty.insert(expr, Ty::Int);
                        } else {
                            self.errors.push(Error::error_with_span(
                                self,
                                expr.span,
                                "cannot negate non bool type",
                            ));
                        }
                    }
                }
            }
            Expr::Deref { indir, expr: inner_expr } => {
                self.visit_expr(inner_expr);

                let ty = self.expr_ty.get(&**inner_expr).expect("expression to be walked already");
                let ty = ty.dereference(*indir);

                check_dereference(self, inner_expr);

                self.expr_ty.insert(expr, ty);
            }
            Expr::AddrOf(inner_expr) => {
                self.visit_expr(inner_expr);

                // TODO: if inner_expr isn't found likely that var isn't declared if ident
                let ty = self.expr_ty.get(&**inner_expr).expect("type for address of").clone();
                self.expr_ty.insert(expr, Ty::Ptr(box ty.into_spanned(DUMMY)));
            }
            Expr::Binary { op, lhs, rhs } => {
                self.visit_expr(lhs);
                self.visit_expr(rhs);

                let lhs_ty = self.expr_ty.get(&**lhs);
                let rhs_ty = self.expr_ty.get(&**rhs);

                // println!("BINOP {:?} == {:?}", lhs_ty, rhs_ty);
                if let Some(ty) = fold_ty(
                    self,
                    resolve_ty(self, lhs, lhs_ty).as_ref(),
                    resolve_ty(self, rhs, rhs_ty).as_ref(),
                    op,
                    expr.span,
                ) {
                    if let Some(t2) = self.expr_ty.insert(expr, ty.clone()) {
                        assert!(
                            ty.is_ty_eq(&t2),
                            "{}",
                            format!(
                                "{}",
                                Error::error_with_span(
                                    self,
                                    expr.span,
                                    "ICE: something went wrong in the compiler",
                                )
                            )
                        )
                    }
                } else {
                    self.errors.push(Error::error_with_span(
                        self,
                        expr.span,
                        &format!("no type found for bin expr {:?} != {:?}", lhs_ty, rhs_ty),
                    ));
                }
            }
            Expr::Parens(inner_expr) => {
                self.visit_expr(inner_expr);
                if let Some(ty) = self.expr_ty.get(&**inner_expr).cloned() {
                    if let Some(t2) = self.expr_ty.insert(expr, ty.clone()) {
                        assert!(
                            ty.is_ty_eq(&t2),
                            "{}",
                            format!(
                                "{}",
                                Error::error_with_span(
                                    self,
                                    expr.span,
                                    "ICE: something went wrong in the compiler",
                                )
                            )
                        )
                    }
                } else {
                    self.errors.push(Error::error_with_span(
                        self,
                        expr.span,
                        "no type found for paren expr",
                    ));
                }
            }
            Expr::Call { ident, args, type_args } => {
                if self.var_func.name_func.get(ident).is_none() {
                    panic!(
                        "{}",
                        Error::error_with_span(
                            self,
                            expr.span,
                            &format!("no function named `{}`", ident)
                        )
                    )
                }

                for arg in args {
                    self.visit_expr(arg);
                }

                // Check type_args agrees
                let stack = build_stack(self, Node::Func(ident.to_string()));

                let gen_arg_set_id = self.unique_id();
                let mut gen_arg_map = HashMap::new();
                // Iter the type arguments at the call site
                for (gen_arg_idx, ty_arg) in type_args.iter().enumerate() {
                    // Don't use the same stack for each iteration
                    let mut stack = stack.clone();

                    let func =
                        self.var_func.name_func.get(ident).expect("all functions are collected");
                    let gen = &func.generics[gen_arg_idx];
                    // Find the param that is the "generic" and check against type argument
                    let arguments = func
                        .params
                        .iter()
                        .enumerate()
                        .filter(|(_i, p)| p.ty.val.is_ty_eq(&gen.val))
                        .map(|(i, _)| TyRegion::Expr(&args[i].val))
                        .collect::<Vec<_>>();

                    self.generic_res.collect_generic_usage(
                        &ty_arg.val,
                        gen_arg_set_id,
                        gen_arg_idx,
                        &arguments,
                        &mut stack,
                    );

                    // println!("CALL IN CALL {:?} == {:?} {:?}", ty, gen, stack);

                    gen_arg_map.insert(gen.val.generic().to_string(), ty_arg.val.clone());
                }

                let func_params = self
                    .var_func
                    .name_func
                    .get(ident)
                    .map(|f| &f.params)
                    .expect("function is known with params");

                if args.len() != func_params.len() {
                    panic!(
                        "{}",
                        Error::error_with_span(self, expr.span, "wrong number of arguments",)
                    );
                }

                for (idx, arg) in args.iter().enumerate() {
                    let mut param_ty = func_params.get(idx).map(|p| p.ty.val.clone());
                    let mut arg_ty = self.expr_ty.get(arg).cloned();

                    if let Some(Ty::Generic { ident, .. }) = &param_ty {
                        if let Some(ty_arg) = gen_arg_map.get(ident).cloned() {
                            param_ty = Some(ty_arg);
                        }
                    }

                    // TODO: remove
                    coercion(param_ty.as_ref(), arg_ty.as_mut());

                    if !param_ty.as_ref().is_ty_eq(&arg_ty.as_ref()) {
                        self.errors.push(Error::error_with_span(
                            self,
                            arg.span,
                            &format!(
                                "call with wrong argument type\nfound `{}` expected `{}`",
                                arg_ty.map_or("<unknown>".to_owned(), |t| t.to_string()),
                                param_ty.map_or("<unknown>".to_owned(), |t| t.to_string()),
                            ),
                        ));
                    }
                }

                let f = self.var_func.name_func.get(ident).expect("function is defined");
                let t = &f.ret.val;
                let ret_ty = if t.has_generics() {
                    subs_type_args(t, type_args, &f.generics)
                } else {
                    t.clone()
                };
                self.expr_ty.insert(expr, ret_ty);
                // because of x += 1;
            }
            Expr::TraitMeth { trait_, args, type_args } => {
                if self.trait_solve.traits.get(trait_).is_none() {
                    panic!(
                        "{}",
                        Error::error_with_span(
                            self,
                            expr.span,
                            &format!("no trait named `{}`", trait_)
                        )
                    )
                }
                for expr in args {
                    self.visit_expr(expr);
                }

                let trait_def =
                    self.trait_solve.traits.get(trait_).cloned().expect("trait is defined");

                let mut stack = build_stack(self, Node::Trait(trait_.to_string()));

                let gen_arg_set_id = self.unique_id();
                let mut gen_arg_map = HashMap::new();
                for (gen_arg_idx, ty_arg) in type_args.iter().enumerate() {
                    let gen = &trait_def.generics[gen_arg_idx];

                    let arguments = trait_def
                        .method
                        .function()
                        .params
                        .iter()
                        .enumerate()
                        .filter(|(_i, p)| p.ty.val.is_ty_eq(&gen.val))
                        .map(|(i, _)| TyRegion::Expr(&args[i].val))
                        .collect::<Vec<_>>();

                    self.generic_res.collect_generic_usage(
                        &ty_arg.val,
                        gen_arg_set_id,
                        gen_arg_idx,
                        &arguments,
                        &mut stack,
                    );

                    gen_arg_map.insert(gen.val.generic().to_string(), ty_arg.val.clone());
                }

                let mut has_generic = false;
                let func_params = &trait_def.method.function().params;
                for (idx, arg) in args.iter().enumerate() {
                    let mut param_ty = func_params.get(idx).map(|p| p.ty.val.clone());
                    let arg_ty = self.expr_ty.get(arg).cloned();

                    if let Some(Ty::Generic { ident, .. }) = &param_ty {
                        has_generic = true;
                        if let Some(ty_arg) = gen_arg_map.get(ident).cloned() {
                            param_ty = Some(ty_arg);
                        }
                    }

                    if !param_ty.as_ref().is_ty_eq(&arg_ty.as_ref()) {
                        self.errors.push(Error::error_with_span(
                            self,
                            arg.span,
                            &format!(
                                "trait call with wrong argument type\nfound `{}` expected `{}`",
                                arg_ty.map_or("<unknown>".to_owned(), |t| t.to_string()),
                                param_ty.map_or("<unknown>".to_owned(), |t| t.to_string()),
                            ),
                        ));
                    }
                }

                let generic_dependence = if has_generic { Some(stack) } else { None };
                self.trait_solve.to_solve(
                    trait_,
                    type_args.iter().map(|t| &t.val).collect::<Vec<_>>(),
                    generic_dependence,
                );

                let def_fn = self.trait_solve.traits.get(trait_).expect("trait is defined");
                let t = &def_fn.method.return_ty().val;
                let ret_ty = if t.has_generics() {
                    subs_type_args(t, type_args, &trait_def.generics)
                } else {
                    t.clone()
                };
                self.expr_ty.insert(expr, ret_ty);
            }
            Expr::Value(val) => {
                if self.expr_ty.insert(expr, lit_to_type(&val.val)).is_some() {
                    self.errors.push(Error::error_with_span(
                        self,
                        expr.span,
                        &format!("duplicate value expr {:?}\n{:?}", self.expr_ty, expr),
                    ));
                }
            }
            Expr::StructInit { name, fields } => {
                let (generics, field_tys) =
                    self.struct_fields.get(name).expect("initialized undefined struct").clone();

                let gen_arg_set_id = self.unique_id();
                let mut gen_args = BTreeMap::new();
                for FieldInit { ident, init, .. } in fields {
                    self.visit_expr(init);

                    let field_ty = field_tys
                        .iter()
                        .find_map(|f| if f.ident == *ident { Some(&f.ty.val) } else { None })
                        .expect("no field with that name found");

                    let exprty = self.expr_ty.get(&*init).cloned();

                    let mut stack = build_stack(self, Node::Struct(name.to_string()));

                    // Collect the generic parameter `struct list<T> vec;` (this has to be a
                    // dependent parameter) or a type argument `struct list<int> vec;`
                    for gen in field_ty.generics().into_iter() {
                        if let Some(idx) = generics.iter().enumerate().find_map(|(i, t)| {
                            if matches!(&t.val, Ty::Generic { ident: id, .. } if id == gen) {
                                Some(i)
                            } else {
                                None
                            }
                        }) {
                            self.generic_res.collect_generic_usage(
                                exprty.as_ref().unwrap(),
                                gen_arg_set_id,
                                idx,
                                &[TyRegion::Expr(&init.val)],
                                &mut stack,
                            );

                            gen_args.insert(gen, exprty.clone().unwrap().into_spanned(DUMMY));
                        } else {
                            panic!("undefined generic type used")
                        }
                    }

                    // Skip checking type equivalence
                    if field_ty.has_generics() {
                        continue;
                    }

                    if !exprty.as_ref().is_ty_eq(&Some(field_ty)) {
                        self.errors.push(Error::error_with_span(
                            self,
                            init.span,
                            &format!(
                                "field initialized with mismatched type\nfound `{}` expected `{}`",
                                exprty.map_or("<unknown>".to_owned(), |t| t.to_string()),
                                field_ty,
                            ),
                        ));
                    }
                }

                if self
                    .expr_ty
                    .insert(
                        expr,
                        Ty::Struct {
                            ident: name.clone(),
                            gen: generics
                                .iter()
                                .filter_map(|g| gen_args.remove(g.val.generic()))
                                .collect(),
                        },
                    )
                    .is_some()
                {
                    unimplemented!("No duplicates")
                }
            }
            Expr::EnumInit { ident, variant, items } => {
                let (generics, variant_tys) =
                    self.enum_fields.get(ident).expect("initialized undefined enum").clone();

                let found_variant =
                    variant_tys.iter().find(|v| v.ident == *variant).unwrap_or_else(|| {
                        panic!(
                            "{}",
                            Error::error_with_span(
                                self,
                                expr.span,
                                &format!("enum `{}` has no variant `{}`", ident, variant),
                            )
                        )
                    });

                let mut gen_args = BTreeMap::new();
                for (_idx, (item, variant_ty)) in items.iter().zip(&found_variant.types).enumerate()
                {
                    // Visit inner expressions
                    self.visit_expr(item);

                    // Gather expression and expected (declared) type
                    let exprty = self.expr_ty.get(&*item).cloned();

                    let _stack = build_stack(self, Node::Enum(ident.to_string()));

                    // Collect the generic parameter `enum option<T> opt;` (this has to be a
                    // dependent parameter) or a type argument `enum option<int>
                    // opt;`
                    for gen in variant_ty.val.generics().into_iter() {
                        if let Some(_idx) = generics.iter().enumerate().find_map(|(i, t)| {
                            if matches!(&t.val, Ty::Generic { ident: id, .. } if id == gen) {
                                Some(i)
                            } else {
                                None
                            }
                        }) {
                            // We do NOT call `generic_res.collect_generic_usage(..)` because we
                            // will only collect a variants worth of generic type info.
                            // We instead wait for the assignment to collect all generic arguments.

                            gen_args.insert(gen, exprty.clone().unwrap().into_spanned(DUMMY));
                        } else {
                            panic!("undefined generic type used")
                        }
                    }

                    // Skip checking type equivalence
                    if variant_ty.val.has_generics() {
                        continue;
                    }

                    if !exprty.as_ref().is_ty_eq(&Some(&variant_ty.val)) {
                        self.errors.push(Error::error_with_span(
                            self,
                            item.span,
                            &format!(
                                "enum tuple initialized with mismatched type\nfound `{}` expected `{}`",
                                exprty.map_or("<unknown>".to_owned(), |t| t.to_string()),
                                variant_ty.val,
                            ),
                        ));
                    }
                }

                if self
                    .expr_ty
                    .insert(
                        expr,
                        Ty::Enum {
                            ident: ident.clone(),
                            gen: generics
                                .iter()
                                .filter_map(|g| gen_args.remove(g.val.generic()))
                                .collect(),
                        },
                    )
                    .is_some()
                {
                    unimplemented!("No duplicates")
                }
            }
            Expr::ArrayInit { items } => {
                for item in items {
                    self.visit_expr(item);
                }

                let arr_ty = items.chunks(2).fold(
                    Option::<Ty>::None,
                    // this might be overkill, but `{1 + 1, 2, call()}` all need to be checked
                    |ty, arr| match arr {
                        [] => None,
                        [a] if ty.is_none() => self.expr_ty.get(a).cloned(),
                        [a] => fold_ty(self, ty.as_ref(), self.expr_ty.get(a), &BinOp::Add, a.span),
                        [a, b] if ty.is_none() => fold_ty(
                            self,
                            self.expr_ty.get(a),
                            self.expr_ty.get(b),
                            &BinOp::Add,
                            (a.span.start..b.span.end).into(),
                        ),
                        [a, b] => fold_ty(
                            self,
                            fold_ty(self, ty.as_ref(), self.expr_ty.get(a), &BinOp::Add, a.span)
                                .as_ref(),
                            self.expr_ty.get(b),
                            &BinOp::Add,
                            b.span,
                        ),
                        [..] => unreachable!("{:?}", arr),
                    },
                );

                self.expr_ty.insert(
                    expr,
                    Ty::Array { size: items.len(), ty: box arr_ty.unwrap().into_spanned(DUMMY) },
                );
                // no is_some check: because of `x[0] += 1;` being lowered to `x[0] = w[0] + 1;`
            }
            Expr::FieldAccess { lhs, rhs } => {
                self.visit_expr(lhs);

                // rhs is saved in `check_field_access`
                let field_ty = check_field_access(self, lhs, rhs);
                if let Some(ty) = field_ty {
                    self.expr_ty.insert(expr, ty);
                    // no is_some check: because of `x.y += 1;` being lowered to `x.y = w.y + 1;`
                } else {
                    // TODO: this error is crappy
                    self.errors.push(Error::error_with_span(
                        self,
                        expr.span,
                        "no type found for field access",
                    ));
                }
            }
        }
        // We do NOT call walk_expr here since we recursively walk the exprs
        // when ever found so we have folded the expr types depth first
    }
}

/// Return a type that has been substituted as much as possible at this stage.
///
/// If any generics are left it is because the variable/call they come from has a
/// un-substituted/unresolved generic parameter.
fn subs_type_args(ty: &Ty, ty_args: &[Type], generics: &[Type]) -> Ty {
    let mut typ = ty.clone();
    for gen in ty.generics() {
        let pos =
            generics.iter().position(|g| g.val.generic() == gen).expect("no matching generic");
        typ.subst_generic(gen, &ty_args[pos].val);
    }
    typ
}

/// Create a stack for the current generic location.
///
/// Filters out function calls with no generic arguments (remove main).
fn build_stack(tcxt: &TyCheckRes<'_, '_>, kind: Node) -> Vec<Node> {
    if let Some((def, ident)) =
        tcxt.curr_fn.as_ref().and_then(|f| Some((tcxt.var_func.name_func.get(f)?, f.to_string())))
    {
        if def.generics.is_empty() {
            vec![kind]
        } else {
            vec![Node::Func(ident), kind]
        }
    } else {
        vec![kind]
    }
}

/// The left hand side of field access has been collected calling this collects the right side.
///
/// The is used in the collection of expressions.
fn check_field_access<'ast>(
    tcxt: &mut TyCheckRes<'ast, '_>,
    lhs: &'ast Expression,
    rhs: &'ast Expression,
) -> Option<Ty> {
    let lhs_ty = tcxt.expr_ty.get(lhs);

    let (name, (_generics, fields)) =
        if let Some(Ty::Struct { ident, .. }) = lhs_ty.and_then(|t| t.resolve()) {
            (
                ident.clone(),
                tcxt.struct_fields.get(&ident).cloned().expect("no struct definition found"),
            )
        } else {
            panic!("{}", Error::error_with_span(tcxt, lhs.span, "not valid field access"));
        };

    match &rhs.val {
        Expr::Ident(ident) => {
            let rty = fields
                .iter()
                .find_map(|f| if f.ident == *ident { Some(f.ty.val.clone()) } else { None })
                .unwrap_or_else(|| panic!("no field `{}` found for struct `{}`", ident, name));
            tcxt.expr_ty.insert(rhs, rty.clone());
            Some(rty)
        }
        Expr::Array { ident, exprs } => {
            for expr in exprs {
                tcxt.visit_expr(expr);
            }

            let rty = fields
                .iter()
                .find_map(|f| if f.ident == *ident { Some(f.ty.val.clone()) } else { None })
                .unwrap_or_else(|| panic!("no field `{}` found for struct `{}`", ident, name));
            tcxt.expr_ty.insert(rhs, rty.clone());
            rty.index_dim(tcxt, exprs, rhs.span)
        }
        Expr::FieldAccess { lhs, rhs } => {
            tcxt.visit_expr(lhs);

            let accty = check_field_access(tcxt, lhs, rhs);
            if let Some(ty) = &accty {
                tcxt.expr_ty.insert(rhs, ty.clone());
            }
            accty
        }
        _ => unreachable!("access struct with non ident"),
    }
}

/// The is used in the collection of expressions.
/// TODO: return an error
fn check_dereference(tcxt: &mut TyCheckRes<'_, '_>, expr: &Expression) {
    match &expr.val {
        Expr::Ident(id) => {
            let ty = tcxt.type_of_ident(id, expr.span).or_else(|| tcxt.expr_ty.get(expr).cloned());
            if let Some(_ty) = ty {
                // println!("{:?} == {:?}", ty, tcxt.expr_ty.get(expr))
            } else {
                // panic!("{:?}", expr);
                tcxt.errors.push(Error::error_with_span(
                    tcxt,
                    expr.span,
                    &format!(
                        "cannot dereference `{}`",
                        ty.map_or("<unknown>".to_owned(), |t| t.to_string())
                    ),
                ));
            }
        }
        Expr::Deref { indir: _, expr } => check_dereference(tcxt, expr),
        Expr::AddrOf(expr) => check_dereference(tcxt, expr),
        Expr::FieldAccess { lhs, rhs } => {
            check_dereference(tcxt, lhs);
            check_dereference(tcxt, rhs);
        }
        Expr::Array { ident, exprs } => {
            let ty = tcxt
                .type_of_ident(ident, expr.span)
                .and_then(|ty| ty.index_dim(tcxt, exprs, expr.span));
            if let Some(_ty) = ty {
                // println!("{:?} == {:?}", ty, tcxt.expr_ty.get(expr))
            } else {
                tcxt.errors.push(Error::error_with_span(
                    tcxt,
                    expr.span,
                    &format!(
                        "cannot dereference array `{}`",
                        ty.map_or("<unknown>".to_owned(), |t| t.to_string())
                    ),
                ));
            }
        }

        Expr::Urnary { .. }
        | Expr::Binary { .. }
        | Expr::Parens(_)
        | Expr::Call { .. }
        | Expr::TraitMeth { .. }
        | Expr::StructInit { .. }
        | Expr::EnumInit { .. }
        | Expr::ArrayInit { .. }
        | Expr::Value(_) => todo!(),
    }
}

//
//
//
// All the following is used for actual type checking after the collection phase.

#[derive(Debug)]
crate struct StmtCheck<'v, 'ast, 'input> {
    tcxt: &'v mut TyCheckRes<'ast, 'input>,
}

impl<'ast> Visit<'ast> for StmtCheck<'_, 'ast, '_> {
    fn visit_prog(&mut self, items: &'ast [crate::ast::types::Declaration]) {
        crate::visit::walk_items(self, items);
    }

    fn visit_stmt(&mut self, stmt: &'ast Statement) {
        match &stmt.val {
            Stmt::VarDecl(_) => {}
            Stmt::Assign { lval, rval } => {
                let orig_lty = lvalue_type(self.tcxt, lval, stmt.span);
                let lval_ty = resolve_ty(self.tcxt, lval, orig_lty.as_ref());

                let mut stack = if let Some((def, ident)) = self
                    .tcxt
                    .curr_fn
                    .as_ref()
                    .and_then(|f| Some((self.tcxt.var_func.name_func.get(f)?, f.to_string())))
                {
                    if def.generics.is_empty() {
                        vec![]
                    } else {
                        vec![Node::Func(ident)]
                    }
                } else {
                    vec![]
                };
                collect_enum_generics(self.tcxt, lval_ty.as_ref(), &rval.val, &mut stack);

                let orig_rty = self.tcxt.expr_ty.get(rval);
                let mut rval_ty = resolve_ty(self.tcxt, rval, orig_rty);

                check_used_enum_generics(
                    self.tcxt,
                    lval_ty.as_ref(),
                    rval_ty.as_mut(),
                    rval.span,
                    &rval.val,
                );

                coercion(lval_ty.as_ref(), rval_ty.as_mut());

                if !lval_ty.as_ref().is_ty_eq(&rval_ty.as_ref()) {
                    self.tcxt.errors.push(Error::error_with_span(
                        self.tcxt,
                        stmt.span,
                        &format!(
                            "assign to expression of wrong type\nfound `{}` expected `{}`",
                            orig_rty.map_or("<unknown>".to_owned(), |t| t.to_string()),
                            orig_lty.map_or("<unknown>".to_owned(), |t| t.to_string()),
                        ),
                    ));
                } else if let Expr::Ident(id) = &lval.val {
                    if let Expr::Value(val) = &rval.val {
                        self.tcxt.consts.insert(id, &val.val);
                    }
                }
            }
            Stmt::Call(_expr) => {
                // Hmm we need something here?
            }
            Stmt::TraitMeth(_e) => {
                // TODO:
            }
            Stmt::If { cond, blk: Block { stmts, .. }, els } => {
                let cond_ty =
                    self.tcxt.expr_ty.get(cond).and_then(|t| resolve_ty(self.tcxt, cond, Some(t)));

                // TODO: type coercions :( REMOVE
                if !is_truthy(cond_ty.as_ref()) {
                    panic!(
                        "{}",
                        Error::error_with_span(
                            self.tcxt,
                            stmt.span,
                            "condition of if must be of type bool",
                        )
                    );
                }

                for stmt in stmts {
                    self.visit_stmt(stmt);
                }

                if let Some(Block { stmts, .. }) = els {
                    for stmt in stmts {
                        self.visit_stmt(stmt);
                    }
                }
            }
            Stmt::While { cond, stmt } => {
                let cond_ty =
                    self.tcxt.expr_ty.get(cond).and_then(|t| resolve_ty(self.tcxt, cond, Some(t)));

                // TODO: type coercions :( REMOVE
                if !is_truthy(cond_ty.as_ref()) {
                    panic!(
                        "{}",
                        Error::error_with_span(
                            self.tcxt,
                            stmt.span,
                            &format!(
                                "condition of while must be of truthy, got `{}`",
                                cond_ty.map_or("<unknown>".to_owned(), |t| t.to_string())
                            ),
                        )
                    );
                }
                self.visit_stmt(stmt);
            }
            Stmt::Match { expr, arms } => {
                let match_ty = resolve_ty(self.tcxt, expr, self.tcxt.expr_ty.get(expr));

                // TODO: more
                match match_ty.as_ref().unwrap() {
                    Ty::Array { size: _, ty: _ } => todo!(),
                    Ty::Enum { ident, gen: _ } => {
                        let (_generics, _variant_tys) = self
                            .tcxt
                            .enum_fields
                            .get(ident)
                            .expect("matched undefined enum")
                            .clone();
                        let mut bound_vars = BTreeMap::new();
                        for arm in arms {
                            check_pattern_type(
                                self.tcxt,
                                &arm.pat.val,
                                match_ty.as_ref(),
                                arm.span,
                                &mut bound_vars,
                            );

                            let fn_name = self
                                .tcxt
                                .var_func
                                .get_fn_by_span(stmt.span)
                                .expect("in a function")
                                .to_string();

                            // Add the bound locals if any
                            for (variable, ty) in &bound_vars {
                                self.tcxt
                                    .var_func
                                    .func_refs
                                    .entry(fn_name.clone())
                                    .or_default()
                                    .insert(variable.to_string(), ty.clone());
                            }

                            for stmt in &arm.blk.stmts {
                                self.tcxt.visit_stmt(stmt);
                                // self.visit_stmt(stmt);
                            }

                            // // Remove the bound locals after the arm leaves scope
                            // for (id, _) in bound_vars.drain_filter(|_, _| true) {
                            //     self.tcxt
                            //         .var_func
                            //         .func_refs
                            //         .get_mut(&fn_name)
                            //         .map(|map| map.remove(&id));
                            // }
                        }
                    }
                    Ty::Int => {
                        let mut bound_vars = BTreeMap::new();
                        for arm in arms {
                            check_pattern_type(
                                self.tcxt,
                                &arm.pat.val,
                                match_ty.as_ref(),
                                arm.span,
                                &mut bound_vars,
                            );
                            let fn_name = self
                                .tcxt
                                .var_func
                                .get_fn_by_span(stmt.span)
                                .expect("in a function")
                                .to_string();

                            // Add the bound locals if any
                            for (variable, ty) in &bound_vars {
                                self.tcxt
                                    .var_func
                                    .func_refs
                                    .entry(fn_name.clone())
                                    .or_default()
                                    .insert(variable.to_string(), ty.clone());
                            }

                            // println!("{} {:?} {}", fn_name, bound_vars, arm);

                            for stmt in &arm.blk.stmts {
                                self.tcxt.visit_stmt(stmt);
                                // self.visit_stmt(stmt);
                            }

                            // Remove the bound locals after the arm leaves scope
                            for (id, _) in bound_vars.drain_filter(|_, _| true) {
                                self.tcxt
                                    .var_func
                                    .func_refs
                                    .get_mut(&fn_name)
                                    .map(|map| map.remove(&id));
                            }
                        }
                    }
                    Ty::Char => todo!(),
                    Ty::Float => todo!(),
                    Ty::Bool => todo!(),
                    _ => panic!(
                        "{}",
                        Error::error_with_span(
                            self.tcxt,
                            stmt.span,
                            &format!(
                                "not a valid match type found: `{}`",
                                match_ty.map_or("<unknown>".to_owned(), |t| t.to_string())
                            ),
                        )
                    ),
                }
            }
            Stmt::Read(expr) => {
                if let Some(_read_ty) = self.tcxt.expr_ty.get(expr) {
                    // TODO: check for readable type
                } else {
                    self.tcxt.errors.push(Error::error_with_span(
                        self.tcxt,
                        stmt.span,
                        &format!("variable `{}` not found", expr.val.as_ident_string()),
                    ));
                }
                // TODO: writable trait
                // id must be something that can be from_string or something
            }
            Stmt::Write { expr: _ } => {
                // TODO: display trait?
            }
            Stmt::Ret(expr) => {
                let mut ret_ty = resolve_ty(self.tcxt, expr, self.tcxt.expr_ty.get(expr));
                let mut name = String::new();
                let func_ret_ty = self.tcxt.var_func.get_fn_by_span(expr.span).and_then(|fname| {
                    name = fname.to_string();
                    self.tcxt.var_func.name_func.get(fname).map(|f| f.ret.val.clone())
                });
                self.tcxt.var_func.func_return.insert(name);

                let mut stack = if let Some((def, ident)) = self
                    .tcxt
                    .curr_fn
                    .as_ref()
                    .and_then(|f| Some((self.tcxt.var_func.name_func.get(f)?, f.to_string())))
                {
                    if def.generics.is_empty() {
                        vec![]
                    } else {
                        vec![Node::Func(ident)]
                    }
                } else {
                    vec![]
                };
                collect_enum_generics(self.tcxt, ret_ty.as_ref(), &expr.val, &mut stack);
                check_used_enum_generics(
                    self.tcxt,
                    func_ret_ty.as_ref(),
                    ret_ty.as_mut(),
                    expr.span,
                    &expr.val,
                );

                if !ret_ty.as_ref().is_ty_eq(&func_ret_ty.as_ref()) {
                    self.tcxt.errors.push(Error::error_with_span(
                        self.tcxt,
                        stmt.span,
                        &format!(
                            "call with wrong return type\nfound `{}` expected `{}`",
                            ret_ty.map_or("<unknown>".to_owned(), |t| t.to_string()),
                            func_ret_ty.map_or("<unknown>".to_owned(), |t| t.to_string()),
                        ),
                    ));
                }
            }
            Stmt::Exit => {
                let func_ret_ty =
                    self.tcxt.var_func.get_fn_by_span(stmt.span).and_then(|fname| {
                        self.tcxt.var_func.name_func.get(fname).map(|f| &f.ret.val)
                    });
                if !func_ret_ty.is_ty_eq(&Some(&Ty::Void)) {
                    self.tcxt.errors.push(Error::error_with_span(
                        self.tcxt,
                        stmt.span,
                        "return type must be void",
                    ));
                }
            }
            Stmt::Block(Block { stmts, .. }) => {
                for stmt in stmts {
                    self.visit_stmt(stmt);
                }
            }
        }
    }
}

/// TODO: remove coercion
fn is_truthy(ty: Option<&Ty>) -> bool {
    if let Some(t) = ty {
        match t {
            Ty::Ptr(_) | Ty::Ref(_) | Ty::String | Ty::Int | Ty::Char | Ty::Float | Ty::Bool => {
                true
            }
            _ => false,
        }
    } else {
        false
    }
}

fn coercion(lhs: Option<&Ty>, rhs: Option<&mut Ty>) -> Option<()> {
    match lhs? {
        Ty::Int => match rhs? {
            r @ Ty::Float => {
                *r = Ty::Int;
            }
            r @ Ty::Bool => {
                *r = Ty::Int;
            }
            _ => return None,
        },
        Ty::Float => match rhs? {
            r @ Ty::Int => {
                *r = Ty::Float;
            }
            r @ Ty::Bool => {
                *r = Ty::Float;
            }
            _ => return None,
        },
        Ty::Bool => match rhs? {
            _r @ Ty::Int => {
                // anything but 0 is true ..
            }
            _r @ Ty::Float => {
                // anything but 0 is true ..
            }
            _ => return None,
        },
        Ty::Ptr(_) => match rhs? {
            _r @ Ty::Int => {
                // pointer maths
            }
            _ => return None,
        },
        Ty::Ref(_) => todo!(),
        Ty::Generic { ident: _, bound: _ } => return None,
        // TODO: char has no coercion as of now
        // array has no coercion
        _ => return None,
    }
    Some(())
}

/// Fill the unused generic types if a variant is missing some.
///
/// `enum result<int, string> foo = result::error("blah");` is an example of generic args that
/// need to be filled, the expression would be typed as `result<string>`.
fn check_used_enum_generics(
    tcxt: &TyCheckRes<'_, '_>,
    lty: Option<&Ty>,
    rty: Option<&mut Ty>,
    span: Range,
    rexpr: &Expr,
) {
    let dumb = rty.as_ref().map(|x| (*x).clone());
    let _: Option<()> = try {
        if let (Ty::Enum { ident, gen }, Ty::Enum { ident: _rid, gen: rgen }) = (lty?, rty?) {
            // Oops we don't collect anything, if we continue after here the inner `else` panics
            if gen.is_empty() && rgen.is_empty() {
                return;
            }
            let def = tcxt.name_enum.get(ident)?;
            if let Expr::EnumInit { variant, .. } = rexpr {
                let var = def.variants.iter().find(|v| v.ident == *variant)?;
                let pos = def
                    .generics
                    .iter()
                    .enumerate()
                    // Find the generic type position that this variant needs to satisfy
                    // `result<int, char> foo = result::error('c');` works because
                    // the generic types of result are `generics = [int, char]` where
                    // `error` variant generic type index is 1.
                    .filter(|(_, g)| var.types.iter().any(|t| t.is_ty_eq(g)))
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>();

                if !pos.is_empty()
                    && pos.iter().all(|idx| rgen.iter().any(|t| gen.get(*idx).is_ty_eq(&Some(t))))
                {
                    *rgen = gen.clone();
                } else {
                    panic!(
                        "{}",
                        Error::error_with_span(
                            tcxt,
                            span,
                            &format!(
                                "enum `{}::{}` found with wrong items \nfound `{}` expected `{}`",
                                ident,
                                variant,
                                dumb.map_or("<unknown>".to_owned(), |t| t.to_string()),
                                lty.map_or("<unknown>".to_owned(), |t| t.to_string()),
                            ),
                        )
                    );
                }
            } else {
                return;
            }
        } else {
            return;
        };
    };
}

/// If variants with generics are not constructed the generic parameters are never resolved,
/// this will collect them based on the generic arguments in the type def.
fn collect_enum_generics<'ast>(
    tcxt: &mut TyCheckRes<'ast, '_>,
    lty: Option<&Ty>,
    expr: &'ast Expr,
    stack: &mut Vec<Node>,
) {
    let _: Option<()> = try {
        if let Ty::Enum { ident, gen } = lty? {
            stack.push(Node::Enum(ident.to_owned()));
            let gen_param_id = tcxt.unique_id();
            for (idx, gen_arg) in gen.iter().enumerate() {
                tcxt.generic_res.collect_generic_usage(
                    &gen_arg.val,
                    gen_param_id,
                    idx,
                    &[TyRegion::Expr(expr)],
                    stack,
                );
            }
        } else {
            return;
        };
    };
}

/// Type check the patterns of a match arm.
///
/// Panic's with a good compiler error if types do not match.
fn check_pattern_type(
    tcxt: &TyCheckRes<'_, '_>,
    pat: &Pat,
    ty: Option<&Ty>,
    span: Range,
    bound_vars: &mut BTreeMap<String, Ty>,
) {
    match ty.as_ref().unwrap() {
        Ty::Array { size, ty: t } => match pat {
            Pat::Enum { ident, variant, .. } => panic!(
                "{}",
                Error::error_with_span(
                    tcxt,
                    span,
                    &format!("expected array found `{}::{}`", ident, variant),
                )
            ),
            Pat::Array { size: p_size, items } => {
                assert_eq!(
                    size,
                    p_size,
                    "{}",
                    Error::error_with_span(
                        tcxt,
                        span,
                        &format!(
                            "found array of different sizes\nexpected `{}` found `{}`",
                            size, p_size
                        ),
                    )
                );
                for item in items {
                    check_pattern_type(tcxt, item, Some(&t.val), span, bound_vars);
                }
            }
            Pat::Bind(bind) => match bind {
                Binding::Wild(id) => {
                    bound_vars.insert(id.to_string(), ty.cloned().unwrap());
                }
                Binding::Value(val) => {
                    panic!(
                        "{}",
                        Error::error_with_span(
                            tcxt,
                            span,
                            &format!("expected array found `{}`", val),
                        )
                    );
                }
            },
        },
        Ty::Struct { ident: _, gen: _ } => todo!(),
        Ty::Enum { ident, gen } => {
            let (_generics, variant_tys) =
                tcxt.enum_fields.get(ident).expect("matched undefined enum").clone();
            match pat {
                Pat::Enum { ident: pat_name, variant, items, .. } => {
                    assert_eq!(
                        ident,
                        pat_name,
                        "{}",
                        Error::error_with_span(
                            tcxt,
                            span,
                            &format!(
                                "no enum variant `{}::{}` found for `{}`",
                                pat_name, variant, ident
                            ),
                        )
                    );
                    let var_ty =
                        variant_tys.iter().find(|v| v.ident == *variant).unwrap_or_else(|| {
                            panic!(
                                "{}",
                                Error::error_with_span(
                                    tcxt,
                                    span,
                                    &format!(
                                        "no enum variant `{}::{}` found for `{}`",
                                        pat_name, variant, ident
                                    ),
                                )
                            )
                        });

                    for (idx, it) in items.iter().enumerate() {
                        let var_ty = var_ty.types.get(idx).map(|t| {
                            if let Ty::Generic { .. } = &t.val {
                                &gen[idx].val
                            } else {
                                &t.val
                            }
                        });

                        check_pattern_type(tcxt, it, var_ty, span, bound_vars);
                    }
                }
                Pat::Array { size: _, items: _ } => todo!(),
                Pat::Bind(bind) => match bind {
                    Binding::Wild(id) => {
                        bound_vars.insert(id.to_string(), ty.cloned().unwrap());
                    }
                    Binding::Value(val) => {
                        panic!(
                            "{}",
                            Error::error_with_span(
                                tcxt,
                                span,
                                &format!("expected enum found `{}`", val),
                            )
                        );
                    }
                },
            }
        }
        Ty::String => check_val_pat(tcxt, pat, ty, "string", span, bound_vars),
        Ty::Float => check_val_pat(tcxt, pat, ty, "float", span, bound_vars),
        Ty::Int => check_val_pat(tcxt, pat, ty, "int", span, bound_vars),
        Ty::Char => check_val_pat(tcxt, pat, ty, "char", span, bound_vars),
        Ty::Bool => check_val_pat(tcxt, pat, ty, "bool", span, bound_vars),
        _ => panic!(
            "{}",
            Error::error_with_span(
                tcxt,
                span,
                &format!(
                    "must match a valid enum found: `{}`",
                    ty.map_or("<unknown>".to_owned(), |t| t.to_string())
                ),
            )
        ),
    }
}

/// Panic with a good compiler error if the type of `Pat` is not the correct `Binding::Value`.
fn check_val_pat(
    tcxt: &TyCheckRes<'_, '_>,
    pat: &Pat,
    ty: Option<&Ty>,
    expected: &str,
    span: Range,
    bound_vars: &mut BTreeMap<String, Ty>,
) {
    match pat {
        Pat::Enum { ident, variant, .. } => panic!(
            "{}",
            Error::error_with_span(
                tcxt,
                span,
                &format!("expected `{}` found `{}::{}`", expected, ident, variant)
            )
        ),
        Pat::Array { .. } => panic!(
            "{}",
            Error::error_with_span(tcxt, span, &format!("expected `{}` found `{}`", expected, pat))
        ),
        Pat::Bind(bind) => match bind {
            Binding::Wild(id) => {
                bound_vars.insert(id.to_string(), ty.cloned().unwrap());
            }
            Binding::Value(val) => {
                assert_eq!(
                    Some(&lit_to_type(&val.val)),
                    ty,
                    "{}",
                    Error::error_with_span(
                        tcxt,
                        span,
                        &format!("expected `{}` found `{}`", expected, val)
                    )
                );
            }
        },
    }
}

fn resolve_ty(tcxt: &TyCheckRes<'_, '_>, expr: &Expression, ty: Option<&Ty>) -> Option<Ty> {
    match &expr.val {
        Expr::Deref { indir: _, expr: _ } => ty.and_then(|t| t.resolve()),
        Expr::Array { ident: _, exprs } => ty.and_then(|t| t.index_dim(tcxt, exprs, expr.span)),
        Expr::AddrOf(_) => ty.cloned(),
        Expr::FieldAccess { lhs: _, rhs } => resolve_ty(tcxt, rhs, ty),
        Expr::Ident(_)
        | Expr::Urnary { .. }
        | Expr::Binary { .. }
        | Expr::Parens(_)
        | Expr::Call { .. }
        | Expr::TraitMeth { .. }
        | Expr::StructInit { .. }
        | Expr::EnumInit { .. }
        | Expr::ArrayInit { .. }
        | Expr::Value(_) => ty.cloned(),
    }
}

fn lvalue_type(tcxt: &mut TyCheckRes<'_, '_>, lval: &Expression, stmt_span: Range) -> Option<Ty> {
    let lval_ty = match &lval.val {
        Expr::Ident(_id) => tcxt.expr_ty.get(lval).cloned(),
        Expr::Deref { indir, expr } => {
            lvalue_type(tcxt, expr, stmt_span)
                .map(|t| t.dereference(*indir))
        }
        Expr::Array { ident, exprs } => {
            if let Some(ty @ Ty::Array { .. }) = &tcxt.type_of_ident(ident, stmt_span) {
                let dim = ty.array_dim();
                if exprs.len() != dim {
                    tcxt.errors.push(Error::error_with_span(
                        tcxt,
                        stmt_span,
                        &format!("mismatched array dimension\nfound `{}` expected `{}`", exprs.len(), dim),
                    ));
                    None
                } else {
                    ty.index_dim(tcxt, exprs, stmt_span)
                }
            } else {
                panic!("ICE: todo `{:?}`", lval);
                // TODO: specific error here?
                // None
            }
        },
        Expr::FieldAccess { lhs, rhs } => {
            if let Some(Ty::Struct { ident, .. }) = tcxt.expr_ty.get(&**lhs).and_then(|t| t.resolve()) {
                let fields = tcxt.struct_fields.get(&ident).map(|(_g, f)| f.clone()).unwrap_or_default();

                walk_field_access(tcxt, &fields, rhs)
            } else {
                tcxt.errors.push(Error::error_with_span(
                    tcxt,
                    stmt_span,
                    &format!(
                        "no struct `{}` found",
                        tcxt.type_of_ident(&lhs.val.as_ident_string(), lhs.span)
                            .map_or("<unknown>".to_owned(), |t| t.to_string()),
                    ),
                ));
                None
            }
        },
        Expr::AddrOf(_)
        // invalid lval
        | Expr::Urnary { .. }
        | Expr::Binary { .. }
        | Expr::Parens(_)
        | Expr::Call { .. }
        | Expr::TraitMeth { .. }
        | Expr::StructInit { .. }
        | Expr::EnumInit { .. }
        | Expr::ArrayInit { .. }
        | Expr::Value(_) => {
            panic!(
                "{}",
                Error::error_with_span(tcxt, stmt_span, "invalid lValue")
            )
        }
    };
    lval_ty
}

fn walk_field_access(
    tcxt: &mut TyCheckRes<'_, '_>,
    fields: &[Field],
    expr: &Expression,
) -> Option<Ty> {
    match &expr.val {
        Expr::Ident(id) => fields.iter().find_map(|f| if f.ident == *id { Some(f.ty.val.clone()) } else { None }),
        Expr::Deref { indir, expr } => {
            if let Some(ty) = walk_field_access(tcxt, fields, expr) {
                Some(ty.dereference(*indir))
            } else {
                unreachable!("no type for dereference {:?}", expr)
            }
        }
        Expr::Array { ident, exprs } => {
            if let arr @ Some(ty @ Ty::Array { .. }) = fields
                .iter()
                .find_map(|f| if f.ident == *ident { Some(&f.ty.val) } else { None })
            {
                let dim = ty.array_dim();
                if exprs.len() != dim {
                    tcxt.errors.push(Error::error_with_span(
                        tcxt,
                        expr.span,
                        &format!("mismatched array dimension\nfound `{}` expected `{}`", exprs.len(), dim),
                    ));
                    None
                } else {
                    arr.cloned()
                }
            } else {
                tcxt.errors.push(Error::error_with_span(
                    tcxt,
                    expr.span,
                    &format!("ident `{}` not array", ident),
                ));
                // TODO: specific error here?
                None
            }
        },
        Expr::FieldAccess { lhs, rhs } => {
            let id = lhs.val.as_ident_string();
            if let Some(Ty::Struct { ident: name, .. }) = tcxt.type_of_ident(&id, expr.span).and_then(|t| t.resolve()) {
                // TODO: this is kinda ugly because of the clone but it complains about tcxt otherwise
                // or default not being impl'ed \o/
                let fields = tcxt.struct_fields.get(&name).map(|(_g, f)| f.clone()).unwrap_or_default();
                walk_field_access(tcxt, &fields, rhs)
            } else {
                tcxt.errors.push(Error::error_with_span(
                    tcxt,
                    expr.span,
                    &format!("no struct `{}` found", id),
                ));
                None
            }
        },
        Expr::AddrOf(_)
        // invalid lval
        | Expr::Urnary { .. }
        | Expr::Binary { .. }
        | Expr::Parens(_)
        | Expr::Call { .. }
        | Expr::TraitMeth { .. }
        | Expr::StructInit { .. }
        | Expr::EnumInit { .. }
        | Expr::ArrayInit { .. }
        | Expr::Value(_) => {
            panic!(
                "{}",
                Error::error_with_span(tcxt, expr.span, "invalid lValue")
            )
        }
    }
}

// TODO: finish the type folding
fn fold_ty(
    tcxt: &TyCheckRes<'_, '_>,
    lhs: Option<&Ty>,
    rhs: Option<&Ty>,
    op: &BinOp,
    span: Range,
) -> Option<Ty> {
    let res = match (lhs?, rhs?) {
        (Ty::Int, Ty::Int) => math_ops(tcxt, op, Ty::Int, span),
        (Ty::Float, Ty::Float) => math_ops(tcxt, op, Ty::Float, span),
        // TODO: remove Carr's rules
        (Ty::Int, Ty::Float) => math_ops(tcxt, op, Ty::Float, span),
        (Ty::Float, Ty::Int) => math_ops(tcxt, op, Ty::Float, span),
        (Ty::Char, Ty::Char) => match op {
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Some(Ty::Bool),
            BinOp::AddAssign | BinOp::SubAssign => {
                panic!(
                    "{}",
                    Error::error_with_span(
                        tcxt,
                        span,
                        "cannot assign to a statement, this isn't Rust ;)"
                    )
                )
            }
            _ => {
                panic!("{}", Error::error_with_span(tcxt, span, "not a legal operation for `char`"))
            }
        },
        (Ty::String, Ty::String) => todo!(),
        (Ty::Ptr(t), Ty::Int) => match op {
            BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Rem
            | BinOp::LeftShift
            | BinOp::RightShift
            | BinOp::BitAnd
            | BinOp::BitXor
            | BinOp::BitOr => Some(Ty::Ptr(t.clone())),
            _ => panic!("illegal operation"),
        },
        // swap left and write so the above arm catches
        (l @ Ty::Int, r @ Ty::Ptr(_)) => fold_ty(tcxt, Some(r), Some(l), op, span),
        (Ty::Array { size, ty: t1 }, Ty::Array { size: s, ty: t2 }) if size == s => {
            Some(Ty::Array {
                size: *size,
                ty: box fold_ty(tcxt, Some(&t1.val), Some(&t2.val), op, span)?.into_spanned(DUMMY),
            })
        }
        (Ty::Void, Ty::Void) => Some(Ty::Void),
        (Ty::Bool, Ty::Bool) => match op {
            BinOp::And | BinOp::Or => Some(Ty::Bool),
            _ => panic!("illegal boolean operation"),
        },
        // TODO: deal with structs/enums
        (Ty::Struct { .. }, _) => todo!(""),
        (Ty::Enum { .. }, _) => todo!(""),
        (Ty::Ptr(_), _) => todo!("{:?} {:?}", lhs?, rhs?),
        (r @ Ty::Ref(_), t @ Ty::Ref(_)) => {
            fold_ty(tcxt, r.resolve().as_ref(), t.resolve().as_ref(), op, span)
        }
        (r @ Ty::Ref(_), t) => fold_ty(tcxt, r.resolve().as_ref(), Some(t), op, span),
        (r, t @ Ty::Ref(_)) => fold_ty(tcxt, Some(r), t.resolve().as_ref(), op, span),

        (Ty::Generic { .. }, _) => {
            unreachable!("since no unresolved generic item will ever be in maths")
        }
        (Ty::Func { .. }, _) => unreachable!("Func should never be folded"),
        _ => None,
    };
    res
}

fn math_ops(tcxt: &TyCheckRes<'_, '_>, op: &BinOp, ret_ty: Ty, span: Range) -> Option<Ty> {
    match op {
        BinOp::Add
        | BinOp::Sub
        | BinOp::Mul
        | BinOp::Div
        | BinOp::Rem
        | BinOp::LeftShift
        | BinOp::RightShift
        | BinOp::BitAnd
        | BinOp::BitXor
        | BinOp::BitOr => Some(ret_ty),
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Some(Ty::Bool),
        // TODO: Carr's rules remove
        BinOp::And | BinOp::Or => Some(Ty::Bool),
        BinOp::AddAssign | BinOp::SubAssign => {
            panic!(
                "{}",
                Error::error_with_span(
                    tcxt,
                    span,
                    "cannot assign to a statement, this isn't Rust ;)"
                )
            )
        }
    }
}

fn lit_to_type(lit: &Val) -> Ty {
    match lit {
        Val::Float(_) => Ty::Float,
        Val::Int(_) => Ty::Int,
        Val::Char(_) => Ty::Char,
        Val::Bool(_) => Ty::Bool,
        Val::Str(_) => Ty::String,
    }
}
