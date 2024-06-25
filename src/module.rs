use std::path::Path;

use crate::db::*;

pub struct Module {
	//classes: Vec<ClassId>,
	//functions: Vec<FunctionId>,
	globals: Vec<VarId>,
}

impl Module {
	pub fn new_empty() -> Self {
		return Module {
			globals: Vec::new(),
		}
	}
}

pub fn parse_module(db: &mut Db, path: &Path) -> std::io::Result<Module> {
	let source_id = db.put_source_path(path);

	let file = db.get(source_id).to_file()?;

	let mut lexer = crate::lexer::Lexer::new(file, source_id);

	loop {
		let tok = lexer.next_token(db);

		println!("{:>8}: {:>8?} '{}'", tok.location.offset, tok.typ, db.get(tok.lexeme));

		if tok.typ == crate::lexer::Tok::Eof {
			break;
		}
	}
	
	let module = Module::new_empty();

	return Ok(module);
}