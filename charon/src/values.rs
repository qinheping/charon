#![allow(dead_code)]

use crate::common::*;
use crate::formatter::Formatter;
use crate::types::*;
use core::hash::Hash;
use im::{OrdSet, Vector};
use macros::{generate_index_type, EnumAsGetters, EnumIsA, VariantName};

pub type VarName = String;

// We need to manipulate a lot of indices for the types, variables, definitions,
// etc. In order not to confuse them, we define an index type for every one of
// them (which is just a struct with a unique usize field), together with some
// utilities like a fresh index generator. Those structures and utilities are
// generated by using macros.
generate_index_type!(DefId);
generate_index_type!(VarId);
generate_index_type!(BorrowId); // Borrow identifier - for loans and borrows
generate_index_type!(SymbolicId);

/// A wrapper for borrow ids, used only for pretty-printing purposes.
/// When implementing formatting, we request formatters implementing the
/// [`Formatter`](Formatter) trait for `BorrowIdFormatWrapper`, rather than
/// `BorrowId`. The reason is that we may want to format borrow ids in quite
/// different manners depending on where they come from (mut borrow, shared
/// borrow, etc.).
/// Also, we use it only to **pretty-print borrows, not loans**.
#[derive(Debug, Clone, Copy)]
pub enum BorrowIdFormatWrapper {
    Shared(BorrowId::Id),
    Mut(BorrowId::Id),
    InactivatedMut(BorrowId::Id),
}

// Value ids are just an implementation detail which we use for convenience:
// we use them as "pointers" to sub-values. For instance, a tuple is not
// encoded as a list of values, but as a list of value ids. Similarly, we use
// value ids for the fields of a structure. More generally, whenever we need
// to manipulate a value, we designate it by its value id (for instance,
// environments map variable ids to value ids).
generate_index_type!(ValueId);

pub type Queue<T> = Vector<T>;

/// Variable
#[derive(Debug, Clone)]
pub struct Var {
    /// Unique index identifying the variable
    pub index: VarId::Id,
    /// Variable name - may be `None` if the variable was introduced by Rust
    /// through desugaring.
    pub name: Option<String>,
    /// The variable type
    pub ty: ETy,
}

impl std::string::ToString for Var {
    fn to_string(&self) -> String {
        let id = var_id_to_pretty_string(self.index);
        match &self.name {
            // We display both the variable name and its id because some
            // variables may have the same name (in different scopes)
            Some(name) => format!("{}({})", name, id),
            None => id,
        }
    }
}

impl Var {
    /// Substitute the region parameters and type variables and return
    /// the resulting variable
    pub fn substitute(&self, subst: &ETypeSubst) -> Var {
        Var {
            index: self.index,
            name: self.name.clone(),
            ty: self.ty.substitute_types(subst),
        }
    }
}

/// An untyped value.
/// "GValue" stands for "Generic Value".
/// We parameterize values with the type of value ids and symbolic value ids.
/// TODO: parameterizing with symbolic value ids may not be necessary anymore.
/// TODO: parameterizing with value ids may not be necessary anymore.
#[derive(Debug, PartialEq, Eq, Clone, VariantName, EnumIsA, EnumAsGetters)]
pub enum GValue<Vid: Copy, Sv: Clone> {
    /// Enumerations and structures
    Adt(AdtValue<Vid>),
    /// Unknown, symbolic value
    Symbolic(Sv),
    /// Concrete (non symbolic) value
    Concrete(ConstantValue),
    /// Tuple. Note that unit is encoded as a 0-tuple.
    Tuple(FieldId::Vector<Vid>),
    /// A value borrowed from another value
    Borrow(BorrowContent<Vid>),
    /// A value loaned to another variable
    Loan(LoanContent<Vid>),
    /// Bottom value: no value (uninitialized value, or moved)
    Bottom,
    /// Assumed types (Box, Vec, Cell...).
    Assumed(AssumedValue<Vid>),
}

/// We could use `()`, but having a dedicated type makes things more explicit
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum UnexpandedSymbolic {
    Unexpanded,
}

/// A symbolic value.
///
/// In the general case, we manipulate projectors over symbolic values, to
/// account for the fact that some regions inside the symbolic value might
/// already have ended.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SymbolicValue {
    /// The set of ended regions. If non-empty, this symbolic value is actually
    /// a projection over a symbolic value.
    pub ended: OrdSet<RegionId::Id>,
    pub id: SymbolicId::Id,
    /// We need to carry the type of the symbolic value **with the non-erased
    /// regions**. This is necessary for proper expansion, when some regions
    /// inside the symbolic value have already ended.
    pub ty: RTy,
}

impl SymbolicValue {
    pub fn ended_intersects(&self, rset: &OrdSet<RegionId::Id>) -> bool {
        self.ended.iter().any(|rid| rset.contains(rid))
    }

    pub fn ended_contains(&self, region: &Region<RegionId::Id>) -> bool {
        match region {
            Region::Static => false,
            Region::Var(rid) => self.ended.contains(rid),
        }
    }
}

/// "Normal" value.
pub type Value = GValue<ValueId::Id, SymbolicValue>;

#[derive(Debug, PartialEq, Eq, Clone, VariantName, EnumIsA, EnumAsGetters)]
pub enum AssumedValue<Vid: Copy> {
    /// A box value: boxes have a special treatment
    Box(Vid),
}

impl<Vid: Copy, Sv: Clone> GValue<Vid, Sv> {
    pub fn mk_unit() -> GValue<Vid, Sv> {
        GValue::Tuple(FieldId::Vector::new())
    }

    pub fn is_box(&self) -> bool {
        match self {
            GValue::Assumed(v) => v.is_box(),
            _ => false,
        }
    }

    pub fn as_box(&self) -> Option<Vid> {
        match self {
            GValue::Assumed(v) => match v {
                AssumedValue::Box(id) => Some(*id),
            },
            _ => None,
        }
    }

    pub fn is_mutable_borrow(&self) -> bool {
        match self {
            GValue::Borrow(b) => b.is_mut(),
            _ => false,
        }
    }

    pub fn as_mutable_borrow(&self) -> Option<(BorrowId::Id, Vid)> {
        match self {
            GValue::Borrow(b) => match b {
                BorrowContent::Mut(bid, vid) => Some((*bid, *vid)),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn is_shared_borrow(&self) -> bool {
        match self {
            GValue::Borrow(b) => b.is_shared(),
            _ => false,
        }
    }

    pub fn as_shared_borrow(&self) -> Option<BorrowId::Id> {
        match self {
            GValue::Borrow(b) => match b {
                BorrowContent::Shared(id) => Some(*id),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn is_shared_loan(&self) -> bool {
        match self {
            GValue::Loan(b) => b.is_shared(),
            _ => false,
        }
    }

    pub fn as_shared_loan(&self) -> Option<(&OrdSet<BorrowId::Id>, Vid)> {
        match self {
            GValue::Loan(b) => match b {
                LoanContent::Shared(ids, vid) => Some((ids, *vid)),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn is_inactivated_mut(&self) -> bool {
        match self {
            GValue::Borrow(b) => match b {
                BorrowContent::InactivatedMut(_) => true,
                _ => false,
            },
            _ => false,
        }
    }

    pub fn as_inactivated_mut(&self) -> Result<BorrowId::Id> {
        match self {
            GValue::Borrow(b) => match b {
                BorrowContent::InactivatedMut(bid) => Ok(*bid),
                _ => Err(()),
            },
            _ => Err(()),
        }
    }
}

/// A typed value.
/// Typed values are parameterized with the same generic parameters as
/// [`Value`](Value).
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct GTypedValue<Ty, Val> {
    pub ty: Ty,
    pub value: Val,
}

pub type TypedValue = GTypedValue<ETy, GValue<ValueId::Id, SymbolicValue>>;

impl<Ty, Val> GTypedValue<Ty, Val> {
    pub fn new(ty: Ty, value: Val) -> Self {
        GTypedValue { ty, value }
    }
}

impl<Vid: Copy, Sv: Clone> GTypedValue<ETy, GValue<Vid, Sv>> {
    /// Return the boolean value of this value, if it is a concrete boolean
    /// value.
    pub fn as_concrete_bool(&self) -> Option<bool> {
        match &self.value {
            GValue::Concrete(v) => match v {
                ConstantValue::Bool(v) => Some(*v),
                _ => None,
            },
            _ => None,
        }
    }
    /// Return the scalar value of this value, if it is a concrete scalar
    /// value.
    pub fn as_concrete_scalar(&self) -> Option<ScalarValue> {
        match &self.value {
            GValue::Concrete(v) => match v {
                ConstantValue::Scalar(v) => Some(*v),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn mk_bottom(ty: ETy) -> Self {
        GTypedValue::new(ty, GValue::Bottom)
    }

    pub fn mk_unit() -> Self {
        GTypedValue::new(Ty::mk_unit(), GValue::mk_unit())
    }
}

pub fn var_id_to_pretty_string(id: VarId::Id) -> String {
    format!("var@{}", id.to_string()).to_owned()
}

pub fn value_id_to_pretty_string(id: ValueId::Id) -> String {
    format!("val@{}", id.to_string()).to_owned()
}

pub fn symbolic_id_to_pretty_string(id: SymbolicId::Id) -> String {
    format!("symb@{}", id.to_string()).to_owned()
}

impl std::fmt::Display for BorrowIdFormatWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match self {
            BorrowIdFormatWrapper::Shared(borrow_id) => {
                write!(f, "⌊shared@{}⌋", borrow_id)
            }
            BorrowIdFormatWrapper::Mut(borrow_id) => {
                write!(f, "&mut@{}", borrow_id)
            }
            BorrowIdFormatWrapper::InactivatedMut(borrow_id) => {
                write!(f, "⌊inactivated_mut@{}⌋", borrow_id)
            }
        }
    }
}

impl<Vid: Copy, Sv: Clone> GTypedValue<ETy, GValue<Vid, Sv>> {
    /// Format the value as a string, provided an appropriate context.
    pub fn fmt_with_ctx<'a, T>(&'a self, ctx: &'a T) -> String
    where
        T: Formatter<TypeVarId::Id>
            + Formatter<Vid>
            + Formatter<&'a Sv>
            + Formatter<BorrowIdFormatWrapper>
            + Formatter<&'a ErasedRegion>
            + Formatter<TypeDefId::Id> // For types and values
            + Formatter<(TypeDefId::Id, VariantId::Id)>, // To translate enum values
    {
        match &self.value {
            GValue::Adt(v) => v.fmt_with_ctx(ctx),
            GValue::Symbolic(sid) => {
                format!("{} : {}", ctx.format_object(sid), self.ty.fmt_with_ctx(ctx)).to_owned()
            }
            GValue::Concrete(v) => v.to_string(),
            GValue::Tuple(v) => {
                let values: Vec<String> = v.iter().map(|v| ctx.format_object(*v)).collect();
                let values = values.join(", ");
                format!("({})", values).to_owned()
            }
            GValue::Borrow(v) => v.fmt_with_ctx(ctx),
            GValue::Loan(v) => v.fmt_with_ctx(ctx),
            GValue::Assumed(v) => match v {
                AssumedValue::Box(v) => format!("@Box({})", ctx.format_object(*v)),
            },
            GValue::Bottom => format!("⊥ : {}", self.ty.fmt_with_ctx(ctx)).to_owned(),
        }
    }
}

pub struct DummyFormatter {}

impl Formatter<VarId::Id> for DummyFormatter {
    fn format_object(&self, id: VarId::Id) -> String {
        var_id_to_pretty_string(id)
    }
}

impl Formatter<TypeVarId::Id> for DummyFormatter {
    fn format_object(&self, id: TypeVarId::Id) -> String {
        type_var_id_to_pretty_string(id)
    }
}

impl Formatter<RegionVarId::Id> for DummyFormatter {
    fn format_object(&self, id: RegionVarId::Id) -> String {
        region_var_id_to_pretty_string(id)
    }
}

impl Formatter<&ErasedRegion> for DummyFormatter {
    fn format_object(&self, _: &ErasedRegion) -> String {
        "".to_owned()
    }
}

impl Formatter<ValueId::Id> for DummyFormatter {
    fn format_object(&self, id: ValueId::Id) -> String {
        value_id_to_pretty_string(id)
    }
}

impl Formatter<&SymbolicId::Id> for DummyFormatter {
    fn format_object(&self, id: &SymbolicId::Id) -> String {
        symbolic_id_to_pretty_string(*id)
    }
}

impl Formatter<BorrowIdFormatWrapper> for DummyFormatter {
    fn format_object(&self, w: BorrowIdFormatWrapper) -> String {
        w.to_string()
    }
}

impl Formatter<TypeDefId::Id> for DummyFormatter {
    fn format_object(&self, id: TypeDefId::Id) -> String {
        type_def_id_to_pretty_string(id)
    }
}

impl Formatter<(TypeDefId::Id, VariantId::Id)> for DummyFormatter {
    fn format_object(&self, id: (TypeDefId::Id, VariantId::Id)) -> String {
        let (def_id, variant_id) = id;
        format!(
            "{}::@Variant{}",
            self.format_object(def_id),
            variant_id.to_string()
        )
        .to_owned()
    }
}

impl Formatter<(TypeDefId::Id, Option<VariantId::Id>, FieldId::Id)> for DummyFormatter {
    fn format_object(&self, id: (TypeDefId::Id, Option<VariantId::Id>, FieldId::Id)) -> String {
        let (_def_id, _opt_variant_id, field_id) = id;
        format!("@field{}", field_id.to_string()).to_owned()
    }
}

impl Formatter<&ETy> for DummyFormatter {
    fn format_object(&self, ty: &ETy) -> String {
        ty.to_string()
    }
}

impl std::string::ToString for GTypedValue<ETy, GValue<ValueId::Id, SymbolicId::Id>> {
    fn to_string(&self) -> String {
        self.fmt_with_ctx(&DummyFormatter {})
    }
}

/// "G" stands for "generic", because the type is parameterized by the region
/// type and others. This is thus a "Generic ADT value", not a "GADT Value".
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct GAdtValue<R: Clone, Ty: Clone, Vid: Copy> {
    pub def_id: TypeDefId::Id,
    /// `Some` if enumeration value, `None` if structure value
    pub variant_id: Option<VariantId::Id>,
    pub regions: Vector<R>,
    pub types: Vector<Ty>,
    pub field_values: FieldId::Vector<Vid>,
}

pub type AdtValue<Vid> = GAdtValue<ErasedRegion, ETy, Vid>;

impl<R: Clone, Ty: Clone, Vid: Copy> GAdtValue<R, Ty, Vid> {
    pub fn is_struct(&self) -> bool {
        self.variant_id.is_none()
    }

    pub fn is_enum(&self) -> bool {
        self.variant_id.is_some()
    }

    /// Format the value as a string, given an appropriate context.
    pub fn fmt_with_ctx<T>(&self, ctx: &T) -> String
    where
        T: Formatter<Vid> + Formatter<TypeDefId::Id> + Formatter<(TypeDefId::Id, VariantId::Id)>,
    {
        // TODO: the formatter should take a pair (TypeDefId, Option(VariantId))
        let adt_ident = match &self.variant_id {
            Some(variant_id) => ctx.format_object((self.def_id, *variant_id)),
            None => ctx.format_object(self.def_id),
        };

        if self.field_values.len() > 0 {
            let fields: Vec<String> = self
                .field_values
                .iter()
                .map(|x| format!("({})", ctx.format_object(*x)).to_owned())
                .collect();
            let fields = fields.join(" ");

            format!("{} {}", adt_ident, fields).to_owned()
        } else {
            adt_ident
        }
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, EnumIsA, EnumAsGetters)]
pub enum BorrowContent<Vid: Copy> {
    /// A shared value.
    Shared(BorrowId::Id),
    /// A mutably borrowed value.
    ///
    /// The value id is for the borrowed value itself. It could be a `Value`,
    /// but book-keeping is simpler by using a `ValueId::Id`.
    Mut(BorrowId::Id, Vid),
    /// An inactived mutable borrow
    ///
    /// This is used to model two-phase borrows. When evaluating a two-phase
    /// mutable borrow, we first introduce an inactivated borrow, which behaves
    /// like a shared borrow, until the moment we actually *use* the borrow:
    /// at this point, we end all the other shared (or inactivated - but there
    /// shouldn't be if the program is well typed) borrows of the value we point
    /// to, then replace the inactivated borrow with a mutable borrow.
    InactivatedMut(BorrowId::Id),
}

impl<Vid: Copy> BorrowContent<Vid> {
    /// Format the value as a string, provided an appropriate context.
    pub fn fmt_with_ctx<T>(&self, ctx: &T) -> String
    where
        T: Formatter<Vid>
            + Formatter<BorrowIdFormatWrapper>
            + Formatter<TypeDefId::Id> // For types and values
            + Formatter<(TypeDefId::Id, VariantId::Id)>, // To translate enum values
    {
        match self {
            BorrowContent::Shared(borrow_id) => {
                ctx.format_object(BorrowIdFormatWrapper::Shared(*borrow_id))
            }
            BorrowContent::Mut(borrow_id, borrowed_value) => {
                format!(
                    "{} ({})",
                    ctx.format_object(BorrowIdFormatWrapper::Mut(*borrow_id)),
                    ctx.format_object(*borrowed_value)
                )
            }
            BorrowContent::InactivatedMut(borrow_id) => {
                ctx.format_object(BorrowIdFormatWrapper::InactivatedMut(*borrow_id))
            }
        }
    }
}

impl std::string::ToString for BorrowContent<ValueId::Id> {
    fn to_string(&self) -> String {
        self.fmt_with_ctx(&DummyFormatter {})
    }
}

#[derive(Debug, PartialEq, Eq, Clone, EnumIsA, EnumAsGetters)]
pub enum LoanContent<Vid: Copy> {
    /// A shared loan. Contains the value itself, and a set of borrow ids.
    Shared(OrdSet<BorrowId::Id>, Vid),
    /// A mutable loan. Only contains the index of the mutable borrow (the value
    /// is "owned" by the mutable borrow as long as it is live).
    Mut(BorrowId::Id),
}

impl<Vid: Copy> LoanContent<Vid> {
    pub fn fmt_with_ctx<T>(&self, ctx: &T) -> String
    where
        T: Formatter<Vid>,
    {
        match self {
            LoanContent::Shared(loans, value) => {
                let loans: Vec<String> = loans.iter().map(|x| x.to_string()).collect();
                let loans = loans.join(",");
                format!("@shared_loan({{{}}}, {})", loans, ctx.format_object(*value)).to_owned()
            }
            LoanContent::Mut(borrower) => format!("⌊mut@{}⌋", borrower.to_string()).to_owned(),
        }
    }
}

impl std::string::ToString for LoanContent<ValueId::Id> {
    fn to_string(&self) -> String {
        self.fmt_with_ctx(&DummyFormatter {})
    }
}

/// Constant value
#[derive(Debug, PartialEq, Eq, Clone, VariantName, EnumIsA, EnumAsGetters)]
pub enum ConstantValue {
    Scalar(ScalarValue),
    Bool(bool),
    Char(char),
    String(String),
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, EnumIsA, EnumAsGetters, VariantName, Hash)]
pub enum ScalarValue {
    Isize(isize),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    Usize(usize),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
}

impl ScalarValue {
    pub fn get_integer_ty(&self) -> IntegerTy {
        match self {
            ScalarValue::Isize(_) => IntegerTy::Isize,
            ScalarValue::I8(_) => IntegerTy::I8,
            ScalarValue::I16(_) => IntegerTy::I16,
            ScalarValue::I32(_) => IntegerTy::I32,
            ScalarValue::I64(_) => IntegerTy::I64,
            ScalarValue::I128(_) => IntegerTy::I128,
            ScalarValue::Usize(_) => IntegerTy::Usize,
            ScalarValue::U8(_) => IntegerTy::U8,
            ScalarValue::U16(_) => IntegerTy::U16,
            ScalarValue::U32(_) => IntegerTy::U32,
            ScalarValue::U64(_) => IntegerTy::U64,
            ScalarValue::U128(_) => IntegerTy::U128,
        }
    }

    pub fn is_int(&self) -> bool {
        match self {
            ScalarValue::Isize(_)
            | ScalarValue::I8(_)
            | ScalarValue::I16(_)
            | ScalarValue::I32(_)
            | ScalarValue::I64(_)
            | ScalarValue::I128(_) => true,
            _ => false,
        }
    }

    pub fn is_uint(&self) -> bool {
        match self {
            ScalarValue::Usize(_)
            | ScalarValue::U8(_)
            | ScalarValue::U16(_)
            | ScalarValue::U32(_)
            | ScalarValue::U64(_)
            | ScalarValue::U128(_) => true,
            _ => false,
        }
    }

    /// When computing the result of binary operations, we convert the values
    /// to u128 then back to the target type (while performing dynamic checks
    /// of course).
    pub fn as_uint(&self) -> Result<u128> {
        match self {
            ScalarValue::Usize(v) => Ok(*v as u128),
            ScalarValue::U8(v) => Ok(*v as u128),
            ScalarValue::U16(v) => Ok(*v as u128),
            ScalarValue::U32(v) => Ok(*v as u128),
            ScalarValue::U64(v) => Ok(*v as u128),
            ScalarValue::U128(v) => Ok(*v),
            _ => Err(()),
        }
    }

    pub fn uint_is_in_bounds(ty: IntegerTy, v: u128) -> bool {
        match ty {
            IntegerTy::Usize => v <= (usize::MAX as u128),
            IntegerTy::U8 => v <= (u8::MAX as u128),
            IntegerTy::U16 => v <= (u16::MAX as u128),
            IntegerTy::U32 => v <= (u32::MAX as u128),
            IntegerTy::U64 => v <= (u64::MAX as u128),
            IntegerTy::U128 => true,
            _ => false,
        }
    }

    pub fn from_unchecked_uint(ty: IntegerTy, v: u128) -> ScalarValue {
        match ty {
            IntegerTy::Usize => ScalarValue::Usize(v as usize),
            IntegerTy::U8 => ScalarValue::U8(v as u8),
            IntegerTy::U16 => ScalarValue::U16(v as u16),
            IntegerTy::U32 => ScalarValue::U32(v as u32),
            IntegerTy::U64 => ScalarValue::U64(v as u64),
            IntegerTy::U128 => ScalarValue::U128(v),
            _ => panic!("Expected an unsigned integer kind"),
        }
    }

    pub fn from_uint(ty: IntegerTy, v: u128) -> Result<ScalarValue> {
        if !ScalarValue::uint_is_in_bounds(ty, v) {
            trace!("Not in bounds for {:?}: {}", ty, v);
            Err(())
        } else {
            Ok(ScalarValue::from_unchecked_uint(ty, v))
        }
    }

    /// When computing the result of binary operations, we convert the values
    /// to i128 then back to the target type (while performing dynamic checks
    /// of course).
    pub fn as_int(&self) -> Result<i128> {
        match self {
            ScalarValue::Isize(v) => Ok(*v as i128),
            ScalarValue::I8(v) => Ok(*v as i128),
            ScalarValue::I16(v) => Ok(*v as i128),
            ScalarValue::I32(v) => Ok(*v as i128),
            ScalarValue::I64(v) => Ok(*v as i128),
            ScalarValue::I128(v) => Ok(*v),
            _ => Err(()),
        }
    }

    pub fn int_is_in_bounds(ty: IntegerTy, v: i128) -> bool {
        match ty {
            IntegerTy::Isize => v >= (isize::MIN as i128) && v <= (isize::MAX as i128),
            IntegerTy::I8 => v >= (i8::MIN as i128) && v <= (i8::MAX as i128),
            IntegerTy::I16 => v >= (i16::MIN as i128) && v <= (i16::MAX as i128),
            IntegerTy::I32 => v >= (i32::MIN as i128) && v <= (i32::MAX as i128),
            IntegerTy::I64 => v >= (i64::MIN as i128) && v <= (i64::MAX as i128),
            IntegerTy::I128 => true,
            _ => false,
        }
    }

    pub fn from_unchecked_int(ty: IntegerTy, v: i128) -> ScalarValue {
        match ty {
            IntegerTy::Isize => ScalarValue::Isize(v as isize),
            IntegerTy::I8 => ScalarValue::I8(v as i8),
            IntegerTy::I16 => ScalarValue::I16(v as i16),
            IntegerTy::I32 => ScalarValue::I32(v as i32),
            IntegerTy::I64 => ScalarValue::I64(v as i64),
            IntegerTy::I128 => ScalarValue::I128(v),
            _ => panic!("Expected a signed integer kind"),
        }
    }

    pub fn from_int(ty: IntegerTy, v: i128) -> Result<ScalarValue> {
        if !ScalarValue::int_is_in_bounds(ty, v) {
            Err(())
        } else {
            Ok(ScalarValue::from_unchecked_int(ty, v))
        }
    }
}

impl std::string::ToString for ScalarValue {
    fn to_string(&self) -> String {
        match self {
            ScalarValue::Isize(v) => format!("{} : isize", v).to_owned(),
            ScalarValue::I8(v) => format!("{} : i8", v).to_owned(),
            ScalarValue::I16(v) => format!("{} : i16", v).to_owned(),
            ScalarValue::I32(v) => format!("{} : i32", v).to_owned(),
            ScalarValue::I64(v) => format!("{} : i64", v).to_owned(),
            ScalarValue::I128(v) => format!("{} : i128", v).to_owned(),
            ScalarValue::Usize(v) => format!("{} : usize", v).to_owned(),
            ScalarValue::U8(v) => format!("{} : u8", v).to_owned(),
            ScalarValue::U16(v) => format!("{} : u16", v).to_owned(),
            ScalarValue::U32(v) => format!("{} : u32", v).to_owned(),
            ScalarValue::U64(v) => format!("{} : u64", v).to_owned(),
            ScalarValue::U128(v) => format!("{} : u128", v).to_owned(),
        }
    }
}

impl ConstantValue {
    pub fn to_value<Vid: Copy, Sv: Clone>(&self) -> GTypedValue<ETy, GValue<Vid, Sv>> {
        let ty = match self {
            ConstantValue::Scalar(v) => Ty::Integer(v.get_integer_ty()),
            ConstantValue::Bool(_) => Ty::Bool,
            ConstantValue::Char(_) => Ty::Char,
            ConstantValue::String(_) => Ty::Str,
        };

        GTypedValue::new(ty, GValue::Concrete(self.clone()))
    }
}

impl std::string::ToString for ConstantValue {
    fn to_string(&self) -> String {
        match self {
            ConstantValue::Scalar(v) => v.to_string(),
            ConstantValue::Bool(v) => v.to_string(),
            ConstantValue::Char(v) => v.to_string(),
            ConstantValue::String(v) => v.to_string(),
        }
    }
}
