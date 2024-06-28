use std::path::Path;

use crate::db::*;
use crate::expr::{Declare, FunDeclare};
use crate::parser::Parser;

pub struct Module {
	//classes: Vec<ClassId>,
	pub functions: Vec<FunDeclare>,

	// Globals are simply variable declarations that aren't in any other scope.
	pub globals: Vec<Declare>,
}

impl Module {
	pub fn new_empty() -> Self {
		return Module {
			functions: Vec::new(),
			globals: Vec::new(),
		}
	}
}

pub fn parse_module(db: &mut Db, path: &Path) -> std::io::Result<(Module, bool)> {
	let source_id = db.put_source_path(path);

	let file = db.get(source_id).to_file()?;
	
	let mut module = Module::new_empty();

	let mut parser = Parser::new(file, source_id, db, &mut module)?;
	parser.parse()?;

	let had_error = parser.had_error;

	Ok((module, had_error))
}