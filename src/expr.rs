include!(concat!(env!("OUT_DIR"), "/expr.gen.rs"));

use crate::{db::*, lexer::Token};
use crate::source::SourceLocation;
use crate::lexer::Tok;
use crate::module::ExprId;
use crate::module::StmtId;
use crate::module::Module;

/// Information for a variable.
pub struct Var {
	pub name: Token,
	pub typ: TypId,
}