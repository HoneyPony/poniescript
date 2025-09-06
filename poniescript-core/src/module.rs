use std::path::Path;

use crate::arena::IndexCell;
use crate::db::*;
use crate::expr::{Declare, FunDeclare, ClassDeclare};
use crate::parser::Parser;

pub struct Module {
	pub classes: Vec<ClassDeclare>,
	pub functions: Vec<FunDeclare>,

	// Globals are simply variable declarations that aren't in any other scope.
	pub globals: Vec<Declare>,
}

impl Module {
	pub fn new_empty() -> Self {
		return Module {
			classes: Vec::new(),
			functions: Vec::new(),
			globals: Vec::new(),
		}
	}
}

pub fn parse_module(ast: &mut Ast, db: &mut Db, source_id: SourceId) -> std::io::Result<bool> {
	let reader = ast.sources.get(source_id).to_reader()?;

	let mut parser = Parser::new(reader, source_id, db, ast)?;
	parser.parse()?;

	let had_error = parser.had_error;

	Ok(had_error)
}