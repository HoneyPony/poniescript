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
}

impl Type {
	pub fn to_string(&self) -> String {
		match self {
			Type::Int => "int".to_string(),
			Type::Float => "float".to_string(),
			Type::Unassigned => "<unknown>".to_string(),
			Type::UnassignedNumeric => "a number".to_string(),
			Type::UnassignedDecimal => "a decimal number".to_string(),
		}
	}
}
