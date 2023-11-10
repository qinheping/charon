//! Implementations for [crate::gast]
#![allow(dead_code)]

use crate::common::TAB_INCR;
pub use crate::expressions_utils::ExprFormatter;
use crate::formatter::Formatter;
use crate::gast::*;
use crate::names::Name;
use crate::types::*;
use crate::values::*;
use serde::Serialize;
use std::cmp::max;

/// Iterate on the declarations' non-empty bodies with their corresponding name and type.
/// TODO: generalize this with visitors
pub fn iter_function_bodies<T>(
    funs: &mut FunDeclId::Map<GFunDecl<T>>,
) -> impl Iterator<Item = (&Name, &mut GExprBody<T>)> {
    funs.iter_mut().flat_map(|f| match f.body.as_mut() {
        None => None, // Option::map was complaining about borrowing f
        Some(b) => Some((&f.name, b)),
    })
}

/// Iterate on the declarations' non-empty bodies with their corresponding name and type.
/// Same as [iter_function_bodies] (but the `flat_map` lambda cannot be generic).
/// TODO: generalize this with visitors
pub fn iter_global_bodies<T>(
    globals: &mut GlobalDeclId::Map<GGlobalDecl<T>>,
) -> impl Iterator<Item = (&Name, &mut GExprBody<T>)> {
    globals.iter_mut().flat_map(|g| match g.body.as_mut() {
        None => None, // Option::map was complaining about borrowing g
        Some(b) => Some((&g.name, b)),
    })
}

/// Makes a lambda that generates a new variable id, pushes a new variable in
/// the body locals with the given type and returns its id.
pub fn make_locals_generator(locals: &mut VarId::Vector<Var>) -> impl FnMut(ETy) -> VarId::Id + '_ {
    let mut next_id = locals.iter().fold(VarId::ZERO, |id, v| max(id, v.index));
    move |ty| {
        next_id.incr();
        let id = next_id;
        locals.push_back(Var {
            index: id,
            name: None,
            ty,
        });
        id
    }
}

impl FunDeclId::Id {
    pub fn to_pretty_string(&self) -> String {
        format!("@Fun{self}")
    }
}

impl std::string::ToString for Var {
    fn to_string(&self) -> String {
        let id = self.index.to_pretty_string();
        match &self.name {
            // We display both the variable name and its id because some
            // variables may have the same name (in different scopes)
            Some(name) => format!("{name}{id}"),
            None => id,
        }
    }
}

impl VarId::Vector<Var> {
    pub fn fresh_var(&mut self, name: Option<String>, ty: ETy) -> VarId::Id {
        let index = VarId::Id::new(self.len());
        self.push_back(Var { index, name, ty });
        index
    }
}

impl Var {
    /// Substitute the region parameters and type variables and return
    /// the resulting variable
    pub fn substitute(&self, subst: &ETypeSubst, cgsubst: &ConstGenericSubst) -> Var {
        Var {
            index: self.index,
            name: self.name.clone(),
            ty: self.ty.substitute_types(subst, cgsubst),
        }
    }
}

impl FunKind {
    pub(crate) fn is_trait_method(&self) -> bool {
        matches!(self, FunKind::Regular)
    }
}

impl TraitDecl {
    pub fn fmt_with_ctx<'a, C>(&'a self, ctx: &C) -> String
    where
        C: TypeFormatter<'a, Region<RegionVarId::Id>>
            + Formatter<&'a ErasedRegion>
            + Formatter<RegionVarId::Id>,
    {
        let name = self.name.to_string();
        let (generics, trait_clauses) = self.generics.fmt_with_ctx_with_trait_clauses(ctx);
        let clauses = fmt_where_clauses_with_ctx(ctx, "", &None, trait_clauses, &self.preds);

        let items = {
            let items = self
                .parent_clauses
                .iter()
                .map(|c| {
                    format!(
                        "{TAB_INCR}parent_clause_{} : {}\n",
                        c.clause_id.to_string(),
                        c.fmt_with_ctx(ctx)
                    )
                })
                .chain(
                    self.consts
                        .iter()
                        .map(|(name, (ty, opt_id))| {
                            let ty = ty.fmt_with_ctx(ctx);
                            match opt_id {
                                None => format!("{TAB_INCR}const {name} : {ty}\n"),
                                Some(id) => {
                                    format!(
                                        "{TAB_INCR}const {name} : {ty} = {}\n",
                                        ctx.format_object(*id)
                                    )
                                }
                            }
                        })
                        .chain(self.types.iter().map(|(name, (trait_clauses, opt_ty))| {
                            let trait_clauses: Vec<_> =
                                trait_clauses.iter().map(|x| x.fmt_with_ctx(ctx)).collect();
                            let clauses = fmt_where_clauses(
                                &format!("{TAB_INCR}{TAB_INCR}"),
                                0,
                                trait_clauses,
                            );
                            match opt_ty {
                                None => format!("{TAB_INCR}type {name}{clauses}\n"),
                                Some(ty) => {
                                    format!(
                                        "{TAB_INCR}type {name} = {}{clauses}\n",
                                        ty.fmt_with_ctx(ctx)
                                    )
                                }
                            }
                        }))
                        .chain(self.required_methods.iter().map(|(name, f)| {
                            format!("{TAB_INCR}fn {name} : {}\n", ctx.format_object(*f))
                        }))
                        .chain(self.provided_methods.iter().map(|(name, f)| match f {
                            None => format!("{TAB_INCR}fn {name}\n"),
                            Some(f) => format!("{TAB_INCR}fn {name} : {}\n", ctx.format_object(*f)),
                        })),
                )
                .collect::<Vec<String>>();
            if items.is_empty() {
                "".to_string()
            } else {
                format!("\n{{\n{}}}", items.join(""))
            }
        };

        format!("trait {name}{generics}{clauses}{items}")
    }
}

impl TraitImpl {
    pub fn fmt_with_ctx<'a, C>(&'a self, ctx: &C) -> String
    where
        C: TypeFormatter<'a, Region<RegionVarId::Id>>
            + Formatter<&'a ErasedRegion>
            + Formatter<RegionVarId::Id>,
    {
        let name = self.name.to_string();
        let (generics, trait_clauses) = self.generics.fmt_with_ctx_with_trait_clauses(ctx);
        let clauses = fmt_where_clauses_with_ctx(ctx, "", &None, trait_clauses, &self.preds);

        let items = {
            let items = self
                .parent_trait_refs
                .iter()
                .enumerate()
                .map(|(i, clause)| {
                    let i = TraitClauseId::Id::new(i);
                    format!(
                        "{TAB_INCR}parent_clause{i} = {}\n",
                        clause.fmt_with_ctx(ctx)
                    )
                })
                .chain(self.consts.iter().map(|(name, (ty, id))| {
                    format!(
                        "{TAB_INCR}const {name} : {} = {}\n",
                        ty.fmt_with_ctx(ctx),
                        ctx.format_object(*id)
                    )
                }))
                .chain(self.types.iter().map(|(name, (trait_refs, ty))| {
                    let trait_refs = trait_refs
                        .iter()
                        .map(|x| x.fmt_with_ctx(ctx))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "{TAB_INCR}type {name} = {} with [{}]\n",
                        ty.fmt_with_ctx(ctx),
                        trait_refs
                    )
                }))
                .chain(
                    self.required_methods
                        .iter()
                        .chain(self.provided_methods.iter())
                        .map(|(name, f)| {
                            format!("{TAB_INCR}fn {name} = {}\n", ctx.format_object(*f))
                        }),
                )
                .collect::<Vec<String>>();
            if items.is_empty() {
                "".to_string()
            } else {
                format!("\n{{\n{}}}", items.join(""))
            }
        };

        let impl_trait = self.impl_trait.fmt_with_ctx(ctx);
        format!("impl{generics} {name}{generics} : {impl_trait}{clauses}{items}")
    }
}

/// Format a function call.
/// We return the pair: (function call, comment)
pub fn fmt_call<'a, 'b, T>(ctx: &'b T, call: &'a Call) -> (String, Option<String>)
where
    T: ExprFormatter<'a>,
{
    let trait_and_method_generic_args =
        if let Some(generics) = &call.func.trait_and_method_generic_args {
            Option::Some(generics.fmt_with_ctx_split_trait_refs(ctx))
        } else {
            None
        };

    let f = call.func.fmt_with_ctx(ctx);

    let args: Vec<String> = call.args.iter().map(|x| x.fmt_with_ctx(ctx)).collect();
    let args = args.join(", ");

    (format!("{f}({args})"), trait_and_method_generic_args)
}

impl<T> GExprBody<T> {
    /// This is an auxiliary function for printing definitions. One may wonder
    /// why we require a formatter to format, for instance, (type) var ids,
    /// because the function definition already has the information to print
    /// variables. The reason is that it is easier for us to write this very
    /// generic auxiliary function, then apply it on an evaluation context
    /// properly initialized (with the information contained in the function
    /// definition). See [`fmt_with_decls`](crate::ullbc_ast::FunDecl::fmt_with_decls).
    pub fn fmt_with_ctx<'a, 'b, 'c, C>(&'a self, tab: &'b str, ctx: &'c C) -> String
    where
        C: ExprFormatter<'a> + Formatter<&'a T>,
    {
        // Format the local variables
        let mut locals: Vec<String> = Vec::new();
        for v in &self.locals {
            use crate::id_vector::ToUsize;
            let index = v.index.to_usize();
            let comment = if index == 0 {
                "// return".to_string()
            } else if index <= self.arg_count {
                format!("// arg #{index}").to_string()
            } else {
                match &v.name {
                    Some(_) => "// local".to_string(),
                    None => "// anonymous local".to_string(),
                }
            };

            let var_id = v.index.to_pretty_string();
            let var_name = match &v.name {
                Some(name) => format!("{name}{var_id}"),
                None => var_id,
            };

            locals.push(
                format!(
                    "{tab}let {var_name}: {}; {comment}\n",
                    v.ty.fmt_with_ctx(ctx),
                )
                .to_string(),
            );
        }

        let mut locals = locals.join("");
        locals.push('\n');

        // Format the body blocks - TODO: we don't take the indentation
        // into account, here
        let body = ctx.format_object(&self.body);

        // Put everything together
        let mut out = locals;
        out.push_str(&body);
        out
    }
}

pub trait GFunDeclFormatter<'a, Body: 'a> = ExprFormatter<'a>
    + Formatter<&'a Body>
    + Formatter<&'a Region<RegionVarId::Id>>
    + Formatter<RegionVarId::Id>;

impl<T> GFunDecl<T> {
    /// This is an auxiliary function for printing definitions. One may wonder
    /// why we require a formatter to format, for instance, (type) var ids,
    /// because the function definition already has the information to print
    /// variables. The reason is that it is easier for us to write this very
    /// generic auxiliary function, then apply it on an evaluation context
    /// properly initialized (with the information contained in the function
    /// definition). See [`fmt_with_decls`](crate::ullbc_ast::FunDecl::fmt_with_decls).
    pub fn gfmt_with_ctx<'a, 'b, 'c, C>(&'a self, tab: &'b str, ctx: &'c C) -> String
    where
        C: GFunDeclFormatter<'a, T>,
    {
        // Unsafe keyword
        let unsafe_kw = if self.signature.is_unsafe {
            "unsafe ".to_string()
        } else {
            "".to_string()
        };

        // Function name
        let name = self.name.to_string();

        // Generic parameters
        let (params, trait_clauses) = self.signature.generics.fmt_with_ctx_with_trait_clauses(ctx);

        // Arguments
        let mut args: Vec<String> = Vec::new();
        for i in 0..self.signature.inputs.len() {
            // The input variables start at index 1
            let id = VarId::Id::new(i + 1);
            let arg_ty = &self.signature.inputs.get(i).unwrap();
            args.push(
                format!("{}: {}", id.to_pretty_string(), arg_ty.fmt_with_ctx(ctx)).to_string(),
            );
        }
        let args = args.join(", ");

        // Return type
        let ret_ty = &self.signature.output;
        let ret_ty = if ret_ty.is_unit() {
            "".to_string()
        } else {
            format!(" -> {}", ret_ty.fmt_with_ctx(ctx))
        };

        // Predicates
        let preds = fmt_where_clauses_with_ctx(
            ctx,
            tab,
            &self.signature.parent_params_info,
            trait_clauses,
            &self.signature.preds,
        );

        // Case disjunction on the presence of a body (transparent/opaque definition)
        match &self.body {
            Option::None => {
                // Put everything together
                format!("{tab}{unsafe_kw}fn {name}{params}({args}){ret_ty}{preds}")
            }
            Option::Some(body) => {
                // Body
                let body_tab = format!("{tab}{TAB_INCR}");
                let body = body.fmt_with_ctx(&body_tab, ctx);

                // Put everything together
                format!(
                    "{tab}{unsafe_kw}fn {name}{params}({args}){ret_ty}{preds}\n{tab}{{\n{body}\n{tab}}}",
                )
            }
        }
    }
}

pub trait GGlobalDeclFormatter<'a, Body: 'a> = ExprFormatter<'a> + Formatter<&'a Body>;

impl<T> GGlobalDecl<T> {
    /// This is an auxiliary function for printing definitions. One may wonder
    /// why we require a formatter to format, for instance, (type) var ids,
    /// because the global definition already has the information to print
    /// variables. The reason is that it is easier for us to write this very
    /// generic auxiliary function, then apply it on an evaluation context
    /// properly initialized (with the information contained in the global
    /// definition). See [`fmt_with_decls`](crate::ullbc_ast::FunDecl::fmt_with_decls).
    pub fn gfmt_with_ctx<'a, C>(&'a self, tab: &str, ctx: &C) -> String
    where
        C: GGlobalDeclFormatter<'a, T>,
    {
        // Decl name
        let name = self.name.to_string();

        // Case disjunction on the presence of a body (transparent/opaque definition)
        match &self.body {
            Option::None => {
                // Put everything together
                format!("{tab}global {name}")
            }
            Option::Some(body) => {
                // Body
                let body_tab = format!("{tab}{TAB_INCR}");
                let body = body.fmt_with_ctx(&body_tab, ctx);

                // Put everything together
                format!("{tab}global {name} {{\n{body}\n{tab}}}")
            }
        }
    }
}

impl<T: std::fmt::Debug + Clone + Serialize> GGlobalDecl<T> {
    fn is_opaque(&self) -> bool {
        self.body.is_none()
    }
}

impl FunIdOrTraitMethodRef {
    pub(crate) fn mk_assumed(aid: AssumedFunId) -> Self {
        Self::Fun(FunId::Assumed(aid))
    }
}

impl std::fmt::Display for TraitItemName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(f, "{}", self.0)
    }
}
