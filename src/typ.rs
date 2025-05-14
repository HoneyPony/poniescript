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

	/// A function type that cannot have a closure or bound this.
	FunRaw(SigId),

	/// A function type that can store any function with a matching signature.
	/// This includes closures and bound this.
	Fun(SigId),
	//Str,

	// StrSlice, // Maybe we need three/four String types:
	// - Str = a raw block of chars, fixed length.
	// - StrSlice = slice pointing into a Str.
	// - String / StrBuf = a string that can be pushed/popped/written.
	// - StringSlice = slice pointing into a String.

	// Block,
	// 

	Bottom,

	Class(ClassId),
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
	pub fn to_string(&self, db: &Db) -> String {
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

			Type::FunRaw(sig) => {
				let mut result = "fun*(".to_string();
				let mut comma = false;
				for ty in &db.get(*sig).parameters {
					if comma { result.push_str(", "); }
					comma = true;

					let ty = db.get(*ty);
					result.push_str(&ty.to_string(db));
				}
				result.push(')');

				result
			},

			// TODO: Deduplicate the code
			Type::Fun(sig) => {
				let mut result = "fun(".to_string();
				let mut comma = false;
				for ty in &db.get(*sig).parameters {
					if comma { result.push_str(", "); }
					comma = true;

					let ty = db.get(*ty);
					result.push_str(&ty.to_string(db));
				}
				result.push_str(") -> ");
				result.push_str(&db.get(db.get(*sig).return_type).to_string(db));

				result
			},

			Type::Class(class) => {
				db.get(db.get(*class).name.lexeme).to_string()
			}

			Type::AssumeInt => "a number".to_string(),
			Type::AssumeFloat => "a decimal number".to_string(),

			Type::UnboundIdent(_) => "<unknown named>".to_string(),
		}
	}

	pub fn gen_ctype(&self, db: &mut Db) -> String {
		match self {
			Type::Int => "ps_int".into(),
			Type::Float => "ps_float".into(),

			Type::Void => "void".into(),
			Type::Bool => "ps_bool".into(),
			Type::StrConst => "const ps_str*".into(),
			Type::Str => "ps_str*".into(),
			Type::StrBuf => "ps_strbuf*".into(),

			// TODO: MAybe take &mut db, and then we can use format! and such
			Type::FunRaw(sig) => String::from(db.gen_sig_raw_ctype(*sig)),
			Type::Fun(sig) => String::from(db.gen_sig_ctype(*sig)),

			// IMPORTANT: We must generate class_cnames before ctypes
			Type::Class(class_id) => format!("struct {}*", db.get_class_cname(*class_id)),

			Type::Bottom => "<pony:compiler-err:bottom-type>".into(),

			Type::Unassigned => "<pony:compiler-err:unassigned-type>".into(),
			Type::AssumeInt => "<pony:compiler-err:unassigned-int-type>".into(),
			Type::AssumeFloat => "<pony:compiler-err:unassigned-float-type>".into(),
			Type::UnboundIdent(name) =>
				format!("<pony:compiler-err:unassigned-named-type[{}]>", db.get(*name)),
		}
	}
}
