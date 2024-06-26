#[derive(Clone, Hash, PartialEq, Eq)]
pub enum Type {
	Int,
	Float,

	//Class(ClassId),
	//Function(SignatureId),
	//ListOf(TypId),
	
	Unassigned,
	UnassignedNumeric,
}

impl Type {
	pub fn to_string(&self) -> String {
		match self {
			Type::Int => "int".to_string(),
			Type::Float => "float".to_string(),
			Type::Unassigned => "<unknown>".to_string(),
			Type::UnassignedNumeric => "a number".to_string(),
		}
	}
}
