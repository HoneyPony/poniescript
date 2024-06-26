use crate::db::{Db, StrId};

#[derive(Clone, Hash, PartialEq, Eq)]
pub enum Type {
	Int,
	Float,

	//Class(ClassId),
	//Function(SignatureId),
	//ListOf(TypId),
	
	Unassigned,
	UnassignedNumeric,
	// For floating point numbers, we have to use a different Unassigned type.
	// This is because numerics can be assigned to either Int or Float, but
	// Decimals cannot be assigned to Int. (But if we add fixed point types,
	// they can be assigned to those).
	UnassignedDecimal,

	// A type that only exists before the name-binding pass.
	UnboundIdent(StrId),
}

impl Type {
	pub fn to_string(&self) -> String {
		match self {
			Type::Int => "int".to_string(),
			Type::Float => "float".to_string(),
			Type::Unassigned => "<unknown>".to_string(),
			Type::UnassignedNumeric => "a number".to_string(),
			Type::UnassignedDecimal => "a decimal number".to_string(),

			Type::UnboundIdent(id) => "<unknown named>".to_string(),
		}
	}

	pub fn gen_ctype(&self, db: &Db) -> String {
		match self {
			Type::Int => return "ps_int".into(),
			Type::Float => return "ps_float".into(),


			Type::Unassigned | Type::UnassignedNumeric 
			| Type::UnassignedDecimal | Type::UnboundIdent(_)
			=> {
				panic!("Trying to generate ctype for invalid type");
			}
		}
	}
}
