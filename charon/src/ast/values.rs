//! Contains definitions for variables and constant values.

use crate::ast::FloatTy;
use core::hash::Hash;
use derive_visitor::{Drive, DriveMut};
use macros::{EnumAsGetters, EnumIsA, VariantIndexArity, VariantName};
use serde::{Deserialize, Serialize};

// We need to manipulate a lot of indices for the types, variables, definitions,
// etc. In order not to confuse them, we define an index type for every one of
// them (which is just a struct with a unique usize field), together with some
// utilities like a fresh index generator. Those structures and utilities are
// generated by using macros.
// TODO: move
generate_index_type!(VarId, "");

/// A primitive value.
///
/// Those are for instance used for the constant operands [crate::expressions::Operand::Const]
#[derive(
    Debug,
    PartialEq,
    Eq,
    Clone,
    VariantName,
    EnumIsA,
    EnumAsGetters,
    Serialize,
    Deserialize,
    Drive,
    DriveMut,
    Hash,
    PartialOrd,
    Ord,
)]
#[charon::variants_prefix("V")]
pub enum Literal {
    Scalar(ScalarValue),
    Float(FloatValue),
    Bool(bool),
    Char(char),
    ByteStr(Vec<u8>),
    Str(String),
}

/// It might be a good idea to use a structure:
/// `{ value: ??; int_ty: IntegerTy; }`
/// But then it is not obvious how to naturally store the integer (for instance,
/// in OCaml it is possible to use big integers).
///
/// Also, we don't automatically derive the serializer, because it would serialize
/// the values to integers, leading to potential overflows: we implement a custom
/// serialization, which serializes the values to strings.
#[derive(
    Debug,
    PartialEq,
    Eq,
    Copy,
    Clone,
    EnumIsA,
    EnumAsGetters,
    VariantName,
    VariantIndexArity,
    Hash,
    PartialOrd,
    Ord,
    Drive,
    DriveMut,
)]
pub enum ScalarValue {
    /// Using i64 to be safe
    Isize(i64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    /// Using u64 to be safe
    Usize(u64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
}

/// This is simlar to the Scalar value above. However, instead of storing
/// the float value itself, we store its String representation. This allows
/// to derive the Eq and Ord traits, which are not implemented for floats
#[derive(
    Debug, PartialEq, Eq, Clone, Serialize, Deserialize, Hash, PartialOrd, Ord, Drive, DriveMut,
)]
pub struct FloatValue {
    #[charon::rename("float_value")]
    pub value: String,
    #[charon::rename("float_ty")]
    pub ty: FloatTy,
}
