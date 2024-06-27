use crate::db::*;

#[derive(Clone, Hash, PartialEq, Eq)]
pub enum Type {
	Int,
	Float,
	Void,

	Bottom,

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
			Type::Void => "void".to_string(),
			Type::Bottom => "<bottom>".to_string(),
			Type::Unassigned => "<unknown>".to_string(),
			Type::UnassignedNumeric => "a number".to_string(),
			Type::UnassignedDecimal => "a decimal number".to_string(),

			Type::UnboundIdent(id) => "<unknown named>".to_string(),
		}
	}

	pub fn gen_ctype(&self, db: &Db) -> String {
		match self {
			Type::Int => "ps_int".into(),
			Type::Float => "ps_float".into(),

			Type::Void => "void".into(),

			Type::Bottom => "<pony:compiler-err:bottom-type>".into(),

			Type::Unassigned => "<pony:compiler-err:unassigned-type>".into(),
			Type::UnassignedNumeric => "<pony:compiler-err:unassigned-int-type>".into(),
			Type::UnassignedDecimal => "<pony:compiler-err:unassigned-float-type>".into(),
			Type::UnboundIdent(name) =>
				format!("<pony:compiler-err:unassigned-named-type[{}]>", db.get(*name)),
		}
	}
}
