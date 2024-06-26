use std::path::Path;

// Include arenas
include!(concat!(env!("OUT_DIR"), "/module.arenas.rs"));

use crate::db::*;
use crate::expr::Declare;
use crate::parser::Parser;

use crate::expr::Expr;
use crate::expr::Stmt;

pub struct Module {
	//classes: Vec<ClassId>,
	//functions: Vec<FunctionId>,

	// Globals are simply variable declarations that aren't in any other scope.
	pub globals: Vec<StmtId>,

	pub arenas: ModuleArenas
}

impl Module {
	pub fn new_empty() -> Self {
		return Module {
			arenas: ModuleArenas::new(),
			globals: Vec::new(),
		}
	}
}

pub fn parse_module(db: &mut Db, path: &Path) -> std::io::Result<Module> {
	let source_id = db.put_source_path(path);

	let file = db.get(source_id).to_file()?;
	
	let mut module = Module::new_empty();

	let mut parser = Parser::new(file, source_id, db, &mut module)?;
	parser.parse()?;

	return Ok(module);
}