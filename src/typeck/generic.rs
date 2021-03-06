use std::{
    collections::VecDeque,
    fmt,
    hash::{Hash, Hasher},
};

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::ast::{
    parse::symbol::Ident,
    types::{Const, Expr, Path, Ty},
};

#[derive(Clone, PartialEq, Eq, Hash)]
crate enum TyRegion<'ast> {
    Expr(&'ast Expr),
    Const(&'ast Const),
}

impl fmt::Debug for TyRegion<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expr(_e) => write!(f, "Expr(..)"),
            Self::Const(e) => write!(f, "Const({})", e.ident),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
crate enum Node {
    Func(Ident),
    Trait(Ident),
    Enum(Ident),
    Struct(Ident),
    Builtin(Ident),
}

impl Node {
    crate fn name(&self) -> Ident {
        match self {
            Node::Func(s) => *s,
            Node::Trait(s) => *s,
            Node::Enum(s) => *s,
            Node::Struct(s) => *s,
            Node::Builtin(s) => *s,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
crate struct GenericArgument<'ast> {
    crate ty: Ty,
    exprs: Vec<TyRegion<'ast>>,
    /// The index of this arguments generic parameter.
    ///
    /// `<T, U, V>` here T = 0, U = 1, V = 2
    crate gen_idx: usize,
    crate instance_id: usize,
}

impl Hash for GenericArgument<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.ty.hash(state);
        self.gen_idx.hash(state);
        self.instance_id.hash(state);
    }
}

impl PartialEq for GenericArgument<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.ty.eq(&other.ty)
            && self.gen_idx.eq(&other.gen_idx)
            && self.instance_id.eq(&other.instance_id)
    }
}
impl Eq for GenericArgument<'_> {}

#[derive(Debug, Default, PartialEq, Eq)]
crate struct GenericParam {
    /// Generic type name `T` to possible bounds `T: add`.
    generics: HashMap<Ident, Option<Path>>,
    /// Any dependent generic types. When monomorphizing these will be walked to create
    /// mono variants of each type.
    ///
    /// We need to keep copies of the same function called with different args. we can dedup during
    /// monomorphization.
    children: Vec<(Node, GenericParam)>,
}

crate struct GenericParamIter<'a> {
    curr: &'a GenericParam,
    stack: VecDeque<(&'a Node, &'a GenericParam)>,
}

impl<'a> Iterator for GenericParamIter<'a> {
    type Item = &'a Node;
    fn next(&mut self) -> Option<Self::Item> {
        self.stack.extend(self.curr.children.iter().map(|(a, b)| (a, b)));
        let next = self.stack.pop_front()?;
        self.curr = next.1;
        Some(next.0)
    }
}

impl GenericParam {
    fn insert_generic(&mut self, id: Ident, bound: Option<Path>) {
        self.generics.insert(id, bound);
    }

    crate fn child_iter(&self) -> GenericParamIter {
        GenericParamIter { curr: self, stack: VecDeque::new() }
    }
}

#[derive(Debug, Default)]
crate struct GenericResolver<'ast> {
    /// Mapping of region name (function or struct/enum) to the generic arguments.
    ///
    /// These are the "resolved" types.
    node_resolved: HashMap<Node, HashMap<usize, HashSet<GenericArgument<'ast>>>>,
    /// Mapping of declaration (function or struct or enum) to the generic parameter.
    ///
    /// If a function defines a dependent statement that relationship is preserved.
    /// ```c
    /// enum option<T> foo<T>(T x) {
    ///     enum option<T> abc;
    ///     abc = option::some(x);
    ///     return abc;
    /// }
    /// ```
    item_generics: HashMap<Node, GenericParam>,
}

impl<'ast> GenericResolver<'ast> {
    crate fn resolved(
        &self,
        node: &Node,
    ) -> Option<&HashMap<usize, HashSet<GenericArgument<'ast>>>> {
        self.node_resolved.get(node)
    }

    crate fn generic_dag(&self) -> &HashMap<Node, GenericParam> {
        &self.item_generics
    }

    crate fn has_generics(&self, node: &Node) -> bool {
        self.item_generics.contains_key(node)
    }

    crate fn collect_generic_params(&mut self, node: &Node, ty: &Ty) {
        match ty {
            Ty::Generic { ident, bound } => {
                self.item_generics.entry(*node).or_default().insert_generic(*ident, bound.clone());
            }
            Ty::Array { size: _, ty: _ } => todo!(),
            Ty::Struct { ident: _, gen } => {
                for t in gen {
                    self.collect_generic_params(node, &t.val);
                }
            }
            Ty::Enum { ident: _, gen } => {
                for t in gen {
                    self.collect_generic_params(node, &t.val);
                }
            }
            Ty::Func { ident: _, ret, params } => {
                if let Ty::Generic { .. } = &**ret {
                    self.collect_generic_params(node, ret);
                }
                for t in params {
                    self.collect_generic_params(node, t);
                }
            }
            _ => {
                panic!("walk {:?}", ty);
            }
        }
    }

    fn push_generic_child(
        &mut self,
        stack: &[Node],
        _expr: &[TyRegion<'ast>],
        id: Ident,
        bound: Option<Path>,
    ) -> Option<()> {
        // TODO: can this be more than 2 deep??
        let mut iter = stack.iter();
        let gp = self.item_generics.get_mut(iter.next()?)?;

        let mut generics = HashMap::default();
        generics.insert(id.to_owned(), bound);

        gp.children.push((*iter.next()?, GenericParam { generics, children: vec![] }));

        Some(())
    }

    crate fn push_resolved_child(
        &mut self,
        stack: &[Node],
        ty: &Ty,
        instance_id: usize,
        gen_idx: usize,
        exprs: Vec<TyRegion<'ast>>,
    ) {
        for node in stack.iter().rev() {
            self.node_resolved
                // The map of function name -> indexed generic arguments
                .entry(*node)
                .or_default()
                // The map of indexed generic args -> each generic mono substitution
                .entry(gen_idx)
                .or_default()
                .insert(GenericArgument {
                    ty: ty.clone(),
                    exprs: exprs.clone(),
                    gen_idx,
                    instance_id,
                });
        }
    }

    /// Collect all the generics to track resolved and dependent sites/uses.
    ///
    /// This also converts any type arguments to their correct type.
    crate fn collect_generic_usage(
        &mut self,
        ty: &Ty,
        instance_id: usize,
        gen_idx: usize,
        exprs: &[TyRegion<'ast>],
        stack: &mut Vec<Node>,
    ) {
        // println!("collect {:?} {:?}", ty, stack);
        match &ty {
            Ty::Generic { ident, bound } => {
                self.push_generic_child(stack, exprs, *ident, bound.clone());
            }
            Ty::Array { size: _, ty } => {
                self.collect_generic_usage(&ty.val, instance_id, gen_idx, exprs, stack)
            }
            Ty::Struct { ident: struct_name, gen } => {
                if gen.iter().any(|t| t.val.has_generics()) {
                    for t in gen.iter() {
                        if let Ty::Generic { ident, bound } = &t.val {
                            stack.push(Node::Struct(*struct_name));
                            self.push_generic_child(stack, exprs, *ident, bound.clone());
                        } else {
                            self.collect_generic_usage(&t.val, instance_id, gen_idx, exprs, stack);
                        }
                    }
                } else {
                    self.push_resolved_child(stack, ty, instance_id, gen_idx, exprs.to_vec());
                }
                stack.pop();
            }
            Ty::Enum { ident: enum_name, gen } => {
                if gen.iter().any(|t| t.val.has_generics()) {
                    for t in gen.iter() {
                        if let Ty::Generic { ident, bound } = &t.val {
                            stack.push(Node::Enum(*enum_name));
                            self.push_generic_child(stack, exprs, *ident, bound.clone());
                        } else {
                            self.collect_generic_usage(&t.val, instance_id, gen_idx, exprs, stack);
                        }
                    }
                } else {
                    self.push_resolved_child(stack, ty, instance_id, gen_idx, exprs.to_vec());
                }

                stack.pop();
            }
            Ty::Func { ident: _, ret: _, params: _ } => {
                // stack.push(Node::Func(ident.clone()));
                todo!()
            }
            Ty::Ptr(t) => {
                self.collect_generic_usage(&t.val, instance_id, gen_idx, exprs, stack);
            }
            Ty::Ref(t) => {
                self.collect_generic_usage(&t.val, instance_id, gen_idx, exprs, stack);
            }
            _ => {
                self.push_resolved_child(stack, ty, instance_id, gen_idx, exprs.to_vec());
            }
        }
    }
}
