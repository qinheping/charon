use crate::ast::*;
use crate::formatter::{FmtCtx, Formatter, IntoFormatter};
use crate::ids::Vector;
use crate::reorder_decls::DeclarationsGroups;
use derive_visitor::{Drive, DriveMut};
use hashlink::LinkedHashSet;
use macros::{EnumAsGetters, EnumIsA, VariantIndexArity, VariantName};
use serde::{Deserialize, Serialize};
use serde_map_to_array::HashMapToArray;
use std::cmp::{Ord, PartialOrd};
use std::collections::HashMap;
use std::fmt;
use std::ops::{Index, IndexMut};

/// The id of a translated item.
#[derive(
    Copy,
    Clone,
    Debug,
    PartialOrd,
    Ord,
    PartialEq,
    Eq,
    Hash,
    EnumIsA,
    EnumAsGetters,
    VariantName,
    VariantIndexArity,
    Serialize,
    Deserialize,
    Drive,
    DriveMut,
)]
#[charon::rename("AnyDeclId")]
#[charon::variants_prefix("Id")]
pub enum AnyTransId {
    Type(TypeDeclId),
    Fun(FunDeclId),
    Global(GlobalDeclId),
    TraitDecl(TraitDeclId),
    TraitImpl(TraitImplId),
}

/// Implement `TryFrom`  and `From` to convert between an enum and its variants.
macro_rules! wrap_unwrap_enum {
    ($enum:ident::$variant:ident($variant_ty:ident)) => {
        impl TryFrom<$enum> for $variant_ty {
            type Error = ();
            fn try_from(x: $enum) -> Result<Self, Self::Error> {
                match x {
                    $enum::$variant(x) => Ok(x),
                    _ => Err(()),
                }
            }
        }

        impl From<$variant_ty> for $enum {
            fn from(x: $variant_ty) -> Self {
                $enum::$variant(x)
            }
        }
    };
}

wrap_unwrap_enum!(AnyTransId::Fun(FunDeclId));
wrap_unwrap_enum!(AnyTransId::Global(GlobalDeclId));
wrap_unwrap_enum!(AnyTransId::Type(TypeDeclId));
wrap_unwrap_enum!(AnyTransId::TraitDecl(TraitDeclId));
wrap_unwrap_enum!(AnyTransId::TraitImpl(TraitImplId));

/// A translated item.
#[derive(Debug, Clone, Copy, EnumIsA, EnumAsGetters, VariantName, VariantIndexArity)]
pub enum AnyTransItem<'ctx> {
    Type(&'ctx TypeDecl),
    Fun(&'ctx FunDecl),
    Global(&'ctx GlobalDecl),
    TraitDecl(&'ctx TraitDecl),
    TraitImpl(&'ctx TraitImpl),
}

/// The data of a translated crate.
#[derive(Default, Clone, Drive, DriveMut, Serialize, Deserialize)]
pub struct TranslatedCrate {
    /// The name that the user requested for the crate. This may differ from what rustc reports as
    /// the name of the crate.
    pub crate_name: String,
    /// The name of the crate according to rustc.
    pub real_crate_name: String,

    /// File names to ids and vice-versa
    #[drive(skip)]
    pub id_to_file: Vector<FileId, FileName>,
    #[drive(skip)]
    #[serde(skip)]
    #[charon::opaque]
    pub file_to_id: HashMap<FileName, FileId>,

    /// File id to content.
    ///
    /// Note that some files may be missing, if they are not "real" files.
    #[drive(skip)]
    #[serde(with = "HashMapToArray::<FileId, String>")]
    pub file_id_to_content: HashMap<FileId, String>,

    /// All the ids, in the order in which we encountered them
    #[drive(skip)]
    pub all_ids: LinkedHashSet<AnyTransId>,
    /// The names of all registered items. Available so we can know the names even of items that
    /// failed to translate.
    #[serde(with = "HashMapToArray::<AnyTransId, Name>")]
    pub item_names: HashMap<AnyTransId, Name>,

    /// The translated type definitions
    pub type_decls: Vector<TypeDeclId, TypeDecl>,
    /// The translated function definitions
    pub fun_decls: Vector<FunDeclId, FunDecl>,
    /// The translated global definitions
    pub global_decls: Vector<GlobalDeclId, GlobalDecl>,
    /// The bodies of functions and constants
    pub bodies: Vector<BodyId, Body>,
    /// The translated trait declarations
    pub trait_decls: Vector<TraitDeclId, TraitDecl>,
    /// The translated trait declarations
    pub trait_impls: Vector<TraitImplId, TraitImpl>,
    /// The re-ordered groups of declarations, initialized as empty.
    #[drive(skip)]
    pub ordered_decls: Option<DeclarationsGroups>,
}

impl TranslatedCrate {
    pub fn get_item(&self, trans_id: impl Into<AnyTransId>) -> Option<AnyTransItem<'_>> {
        match trans_id.into() {
            AnyTransId::Type(id) => self.type_decls.get(id).map(AnyTransItem::Type),
            AnyTransId::Fun(id) => self.fun_decls.get(id).map(AnyTransItem::Fun),
            AnyTransId::Global(id) => self.global_decls.get(id).map(AnyTransItem::Global),
            AnyTransId::TraitDecl(id) => self.trait_decls.get(id).map(AnyTransItem::TraitDecl),
            AnyTransId::TraitImpl(id) => self.trait_impls.get(id).map(AnyTransItem::TraitImpl),
        }
    }

    pub fn item_name(&self, trans_id: impl Into<AnyTransId>) -> Option<&Name> {
        self.item_names.get(&trans_id.into())
    }

    pub fn all_items(&self) -> impl Iterator<Item = AnyTransItem<'_>> {
        self.all_items_with_ids().map(|(_, item)| item)
    }
    pub fn all_items_with_ids(&self) -> impl Iterator<Item = (AnyTransId, AnyTransItem<'_>)> {
        self.all_ids
            .iter()
            .flat_map(|id| Some((*id, self.get_item(*id)?)))
    }
}

impl<'ctx> AnyTransItem<'ctx> {
    pub fn id(&self) -> AnyTransId {
        match self {
            AnyTransItem::Type(d) => d.def_id.into(),
            AnyTransItem::Fun(d) => d.def_id.into(),
            AnyTransItem::Global(d) => d.def_id.into(),
            AnyTransItem::TraitDecl(d) => d.def_id.into(),
            AnyTransItem::TraitImpl(d) => d.def_id.into(),
        }
    }

    pub fn item_meta(&self) -> &'ctx ItemMeta {
        match self {
            AnyTransItem::Type(d) => &d.item_meta,
            AnyTransItem::Fun(d) => &d.item_meta,
            AnyTransItem::Global(d) => &d.item_meta,
            AnyTransItem::TraitDecl(d) => &d.item_meta,
            AnyTransItem::TraitImpl(d) => &d.item_meta,
        }
    }

    /// The generic parameters of this item.
    pub fn generic_params(&self) -> &'ctx GenericParams {
        match self {
            AnyTransItem::Type(d) => &d.generics,
            AnyTransItem::Fun(d) => &d.signature.generics,
            AnyTransItem::Global(d) => &d.generics,
            AnyTransItem::TraitDecl(d) => &d.generics,
            AnyTransItem::TraitImpl(d) => &d.generics,
        }
    }

    /// We can't implement the `Drive` type because of the `'static` constraint, but it's ok
    /// because `AnyTransItem` isn't contained in any of our types.
    pub fn drive<V: derive_visitor::Visitor>(&self, visitor: &mut V) {
        match self {
            AnyTransItem::Type(d) => d.drive(visitor),
            AnyTransItem::Fun(d) => d.drive(visitor),
            AnyTransItem::Global(d) => d.drive(visitor),
            AnyTransItem::TraitDecl(d) => d.drive(visitor),
            AnyTransItem::TraitImpl(d) => d.drive(visitor),
        }
    }
}

impl fmt::Display for TranslatedCrate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let fmt: FmtCtx = self.into_fmt();
        match &self.ordered_decls {
            None => {
                // We do simple: types, globals, traits, functions
                for d in &self.type_decls {
                    writeln!(f, "{}\n", fmt.format_object(d))?
                }
                for d in &self.global_decls {
                    writeln!(f, "{}\n", fmt.format_object(d))?
                }
                for d in &self.trait_decls {
                    writeln!(f, "{}\n", fmt.format_object(d))?
                }
                for d in &self.trait_impls {
                    writeln!(f, "{}\n", fmt.format_object(d))?
                }
                for d in &self.fun_decls {
                    writeln!(f, "{}\n", fmt.format_object(d))?
                }
            }
            Some(ordered_decls) => {
                for gr in ordered_decls {
                    for id in gr.get_ids() {
                        writeln!(f, "{}\n", fmt.format_decl_id(id))?
                    }
                }
            }
        }
        fmt::Result::Ok(())
    }
}

impl<'tcx, 'ctx, 'a> IntoFormatter for &'a TranslatedCrate {
    type C = FmtCtx<'a>;

    fn into_fmt(self) -> Self::C {
        FmtCtx {
            translated: Some(self),
            ..Default::default()
        }
    }
}

/// Delegate `Index` implementations to subfields.
macro_rules! mk_index_impls {
    ($ty:ident.$field:ident[$idx:ty]: $output:ty) => {
        impl Index<$idx> for $ty {
            type Output = $output;
            fn index(&self, index: $idx) -> &Self::Output {
                &self.$field[index]
            }
        }
        impl IndexMut<$idx> for $ty {
            fn index_mut(&mut self, index: $idx) -> &mut Self::Output {
                &mut self.$field[index]
            }
        }
    };
}
mk_index_impls!(TranslatedCrate.type_decls[TypeDeclId]: TypeDecl);
mk_index_impls!(TranslatedCrate.fun_decls[FunDeclId]: FunDecl);
mk_index_impls!(TranslatedCrate.global_decls[GlobalDeclId]: GlobalDecl);
mk_index_impls!(TranslatedCrate.bodies[BodyId]: Body);
mk_index_impls!(TranslatedCrate.trait_decls[TraitDeclId]: TraitDecl);
mk_index_impls!(TranslatedCrate.trait_impls[TraitImplId]: TraitImpl);
