use crate::{
    ast::{
        parse::symbol::Ident,
        types::{self as ty, Generic, Path, Spany, Ty, DUMMY},
    },
    error::Error,
    typeck::{
        generic::{GenericArgument, Node},
        TyCheckRes,
    },
    visit::VisitMut,
};

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

struct GenSubstitution<'a, 'b> {
    generic: &'a Generic,
    ty: &'a Ty,
    tcxt: &'a TyCheckRes<'a, 'b>,
}

impl<'ast, 'b> VisitMut<'ast> for GenSubstitution<'ast, 'b> {
    fn visit_func(&mut self, func: &'ast mut ty::Func) {
        // TODO: this is REALLY bad since we lock anytime we do something like this, and making a
        // bunch of thrown away allocations to the interner isn't ideal
        func.ident = Ident::new(
            func.ident.span(),
            // TODO: @name-cleanup
            &format!("{}{}", func.ident.name(), self.ty.to_string().replace(' ', "")),
        );
        crate::visit::walk_mut_func(self, func);
    }

    fn visit_ty(&mut self, ty: &mut ty::Type) {
        // println!("{:?} {:?} {:?}", self.generic, self.ty, ty);
        ty.val.subst_generic(self.generic.ident, self.ty)
    }

    fn visit_expr(&mut self, expr: &'ast mut ty::Expression) {
        if let Some(t) = self.tcxt.expr_ty.get(expr) {
            if t.has_generics() && t.generics().contains(&&self.generic.ident) {
                self.tcxt.mono_expr_ty.borrow_mut().insert(expr.clone(), self.ty.clone());
            } else if let ty::Expr::Builtin(ty::Builtin::SizeOf(t)) = &mut expr.val {
                t.set(self.ty.clone().into_spanned(DUMMY));
            }
        }
        crate::visit::walk_mut_expr(self, expr);
    }
}

crate struct TraitRes<'ast, 'a> {
    type_args: Vec<&'ast Ty>,
    tcxt: &'ast TyCheckRes<'ast, 'a>,
}

impl<'ast, 'b> TraitRes<'ast, 'b> {
    crate fn new(tcxt: &'ast TyCheckRes<'ast, 'b>, type_args: Vec<&'ast Ty>) -> Self {
        Self { tcxt, type_args }
    }
}

impl<'ast, 'a> VisitMut<'ast> for TraitRes<'ast, 'a> {
    fn visit_stmt(&mut self, stmt: &'ast mut ty::Statement) {
        let mut x = None;
        if let ty::Stmt::TraitMeth(ty::Spanned {
            val: ty::Expr::TraitMeth { trait_, type_args: _, args },
            ..
        }) = &mut stmt.val
        {
            let ident = trait_.segs.last().unwrap();
            if let Some(i) =
                self.tcxt.trait_solve.impls.get(trait_).and_then(|imp| imp.get(&self.type_args))
            {
                // TODO: fucking terrible
                let args2: &'static mut [_] = args.clone().leak();
                for arg in args2 {
                    self.visit_expr(arg);
                }
                // TODO: @name-cleanup
                let ident = format!(
                    "{}{}",
                    trait_,
                    self.type_args.iter().map(|t| t.to_string()).collect::<Vec<_>>().join("0"),
                );
                x = Some(ty::Stmt::Call(
                    ty::Expr::Call {
                        path: trait_.clone(),
                        args: args.to_vec(),
                        type_args: crate::raw_vec![i.method.ret.get().clone()],
                    }
                    .into_spanned(DUMMY),
                ));
            } else {
                panic!(
                    "{}",
                    Error::error_with_span(
                        self.tcxt,
                        stmt.span,
                        &format!(
                            "`{}` is not implemented for `<{}>`",
                            trait_,
                            self.type_args
                                .iter()
                                .map(|t| t.to_string())
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                    )
                )
            }
        }
        if let Some(replace) = x {
            stmt.val = replace;
        }
        // `walk_stmt` here to recurse into `Expr`, `visit_expr` stops (no walk call)
        crate::visit::walk_mut_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast mut ty::Expression) {
        let mut x = None;
        if let ty::Expr::TraitMeth { trait_, type_args: _, args } = &expr.val {
            let ident = trait_.segs.last().unwrap();
            if let Some(i) =
                self.tcxt.trait_solve.impls.get(trait_).and_then(|imp| imp.get(&self.type_args))
            {
                // TODO: fucking terrible
                let args2: &'static mut [_] = args.clone().leak();
                for arg in args2 {
                    // Incase there is a function/trait method call as an argument
                    self.visit_expr(arg);
                }
                // TODO: @name-cleanup
                let ident = Ident::new(
                    DUMMY,
                    &format!(
                        "{}{}",
                        trait_,
                        self.type_args
                            .iter()
                            .map(|t| t.to_string().replace(' ', ""))
                            .collect::<Vec<_>>()
                            .join("0"),
                    ),
                );
                x = Some(ty::Expr::Call {
                    path: Path::single(ident),
                    args: args.to_vec(),
                    type_args: crate::raw_vec![i.method.ret.get().clone()],
                });
            } else {
                panic!(
                    "{}",
                    Error::error_with_span(
                        self.tcxt,
                        expr.span,
                        &format!(
                            "`{}` is not implemented for `<{}>`",
                            trait_,
                            self.type_args
                                .iter()
                                .map(|t| t.to_string())
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                    )
                )
            }
        }

        if let Some(replace) = x {
            expr.val = replace;
        }
    }
}

impl TyCheckRes<'_, '_> {
    crate fn mono_func(&self, func: &ty::Func) -> Vec<ty::Func> {
        let node = Node::Func(func.ident);
        let mut mono_items = vec![];
        // Resolved type mono's so `T` -> `int` for function `foo`
        if let Some(res_list) = self.generic_res.resolved(&node) {
            // Mono the original function
            let mono_funcs = sub_mono_generic(func, res_list, self);
            mono_items.extend_from_slice(&mono_funcs);
            // If `foo` was generic itself then any calls to generic functions inside of `foo`
            // are dependent on the mono of `foo`
            let relations = self.generic_res.generic_dag().get(&node).unwrap();
            for node in relations.child_iter().filter(|n| matches!(n, Node::Func(_))) {
                let dep_func = self.var_func.name_func.get(&node.name()).unwrap();
                let mono_dep_funcs = sub_mono_generic(dep_func, res_list, self);
                mono_items.extend_from_slice(&mono_dep_funcs)
            }
        }
        // println!("{:#?}", mono_items);
        mono_items
    }
}

/// Monomorphize `foo` and dependent functions with the known types `res_list`.
fn sub_mono_generic(
    func: &ty::Func,
    res_list: &HashMap<usize, HashSet<GenericArgument<'_>>>,
    tcxt: &TyCheckRes<'_, '_>,
) -> Vec<ty::Func> {
    // We know that there is at least one generic so getting the zeroth value should be fine
    let number_of_specializations = res_list.get(&0).map_or(0, |m| m.len());
    let mut map: HashMap<_, Vec<_>> = HashMap::default();
    for arg in res_list.iter().flat_map(|(_, a)| a) {
        map.entry(arg.instance_id).or_default().push(arg);
    }

    let mut functions = vec![func.clone(); number_of_specializations];
    for (idx, mut generics) in map.into_values().enumerate() {
        generics.sort_by(|a, b| a.gen_idx.cmp(&b.gen_idx));

        for (i, gen) in generics.iter().enumerate() {
            let fn_gens = functions[idx].generics.len() - 1;
            let safeidx = fn_gens.min(gen.gen_idx);
            // if fn_gens < (gen.gen_idx + 1) { fn_gens.min(gen.gen_idx) } else { gen.gen_idx };
            let gen_param = functions[idx]
                .generics
                .get(safeidx)
                .unwrap_or_else(|| panic!("{:?} {}", functions[idx], gen.gen_idx))
                .clone();
            // Replace ALL uses of this generic and remove the generic parameters
            let mut subs = GenSubstitution { generic: &gen_param, ty: &gen.ty, tcxt };
            subs.visit_func(&mut functions[idx]);

            // TODO: @name-cleanup
            // @cleanup: build the string then make a new ident so we aren't doing many little
            // allocs
            if i != generics.len() - 1 {
                functions[idx].ident =
                    Ident::new(DUMMY, &format!("{}0", functions[idx].ident.name()));
            }
        }

        let mut trait_res =
            TraitRes { type_args: generics.iter().map(|g| &g.ty).collect::<Vec<_>>(), tcxt };
        trait_res.visit_func(&mut functions[idx]);
    }

    for f in &mut functions {
        if f.generics.len() == res_list.len() {
            f.generics.clear();
        }
    }

    functions
}
