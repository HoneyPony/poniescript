include!(concat!(env!("OUT_DIR"), "/expr.gen.rs"));

use crate::{db::*, lexer::Token};
use crate::source::SourceLocation;

/// Information for a variable.
pub struct Var {
	pub name: Token,
	pub typ: TypId,
}