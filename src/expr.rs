include!(concat!(env!("OUT_DIR"), "/expr.gen.rs"));

use crate::db::*;

/// Information for a variable.
pub struct Var {
	id: VarId,
	name: StrId,
	typ: TypId,
	initializer: Expr,
}