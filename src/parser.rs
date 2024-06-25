use std::fs::File;
use std::io;

use crate::db::*;

use crate::lexer::*;
use crate::module::Module;

pub struct Parser<'a> {
	lexer: Lexer,
	module: &'a mut Module,

	current: Token
}

impl<'a> Parser<'a> {
	pub fn new(input: File, source_id: SourceId, db: &mut Db, module: &'a mut Module) -> std::io::Result<Self> {
		let mut lexer = Lexer::new(input, source_id);

		// TODO: Move File initialization to Lexer

		// Prime the parser with the first token in the file.
		let current = lexer.next_token(db)?;
		
		let parser = Parser {
			lexer,
			module,

			current
		};

		Ok(parser)
	}

	fn peek_typ(&self) -> Tok {
		return self.current.typ;
	}

	fn is_at_end(&self) -> bool {
		return self.current.typ == Tok::Eof;
	}

	fn parse_top_level(&mut self, db: &mut Db) -> io::Result<()> {
		match self.peek_typ() {
			Tok::Eof => { },

			Tok::Var => {

			},

			_ => {
				// Report error...
			}
		}

		Ok(())
	}

	// TODO: Consider creating parse error struct..?
	// Some design thoughts...
	//
	// We do want to be able to use the Result system to synchronize the parser
	// in some cases.
	// But, when we get an io::Result, we might want to just propagate it all
	// the way up.
	// 
	// That's a bit hard to do with ?. Although, I guess at sync points is
	// the only place we have to check.
	pub fn parse(&mut self, db: &mut Db) -> io::Result<()> {
		while !self.is_at_end() {
			self.parse_top_level(db)?;
		}

		Ok(())
	}
}