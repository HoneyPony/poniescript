use crate::db::*;

#[derive(Clone, Hash, PartialEq, Eq)]
pub enum Type {
	Int,
	Float,
	Void,
	Bool,

	/// The constant version of Str. Must be copied, etc to be modified.
	StrConst,

	/// Essentially a constant-sized array of characters. The characters may
	/// be modified, but the array must be reallocated to be resized.
	Str,

	/// A string with a dynamic size. Can have characters added and removed.
	StrBuf,

	
	//Str,

	// StrSlice, // Maybe we need three/four String types:
	// - Str = a raw block of chars, fixed length.
	// - StrSlice = slice pointing into a Str.
	// - String / StrBuf = a string that can be pushed/popped/written.
	// - StringSlice = slice pointing into a String.

	// Block,
	// 

	Bottom,

	//Class(ClassId),
	//Function(SignatureId),
	//ListOf(TypId),
	
	Unassigned,
	AssumeInt,
	// For floating point numbers, we have to use a different Unassigned type.
	// This is because numerics can be assigned to either Int or Float, but
	// Decimals cannot be assigned to Int. (But if we add fixed point types,
	// they can be assigned to those).
	AssumeFloat,

	// A type that only exists before the name-binding pass.
	UnboundIdent(StrId),
}

impl Type {
	pub fn to_string(&self) -> String {
		match self {
			Type::Int => "int".to_string(),
			Type::Float => "float".to_string(),
			Type::Void => "void".to_string(),
			Type::Bool => "bool".to_string(),
			Type::StrConst => "StrConst".to_string(),
			Type::Str => "Str".to_string(),
			Type::StrBuf => "StrBuf".to_string(),
			Type::Bottom => "<bottom>".to_string(),
			Type::Unassigned => "<unknown>".to_string(),
			Type::AssumeInt => "a number".to_string(),
			Type::AssumeFloat => "a decimal number".to_string(),

			Type::UnboundIdent(id) => "<unknown named>".to_string(),
		}
	}

	pub fn gen_ctype(&self, db: &Db) -> String {
		match self {
			Type::Int => "ps_int".into(),
			Type::Float => "ps_float".into(),

			Type::Void => "void".into(),
			Type::Bool => "ps_bool".into(),
			Type::StrConst => "const ps_str*".into(),
			Type::Str => "ps_str*".into(),
			Type::StrBuf => "ps_strbuf*".into(),

			Type::Bottom => "<pony:compiler-err:bottom-type>".into(),

			Type::Unassigned => "<pony:compiler-err:unassigned-type>".into(),
			Type::AssumeInt => "<pony:compiler-err:unassigned-int-type>".into(),
			Type::AssumeFloat => "<pony:compiler-err:unassigned-float-type>".into(),
			Type::UnboundIdent(name) =>
				format!("<pony:compiler-err:unassigned-named-type[{}]>", db.get(*name)),
		}
	}
}
