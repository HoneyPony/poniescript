use std::fs::File;
use std::io;

use crate::db::*;

use crate::lexer::*;
use crate::module::Module;

use crate::expr::*;

pub struct Parser<'a> {
	lexer: Lexer,
	module: &'a mut Module,

	current: Token
}

enum ParseErr {
	SyntaxErr,
	IoErr(std::io::Error)
}

type Result<T> = std::result::Result<T, ParseErr>;

macro_rules! consume {
    ($parser:ident, $db:ident, $ty:expr, $($arg:tt)*) => {
        if($parser.peek_typ() != $ty) {
			eprintln!($($arg)*);
			return Err(ParseErr::SyntaxErr);
        }
		else {
			$parser.advance($db)
		}
    };
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

	fn advance(&mut self, db: &mut Db) -> Result<Token> {
		let next = self.lexer
			.next_token(db)
			.map_err(|err| ParseErr::IoErr(err))?;
		Ok(std::mem::replace(&mut self.current, next))
	}

	fn is_at_end(&self) -> bool {
		return self.current.typ == Tok::Eof;
	}

	fn match_(&mut self, db: &mut Db, ty: Tok) -> bool {
		if self.peek_typ() == ty {
			self.advance(db);
			return true;
		}

		return false;
	}

	fn expression(&mut self) -> Result<Expr> {
		return Err(ParseErr::SyntaxErr);
	}

	fn var_declaration(&mut self, db: &mut Db) -> Result<Declare> {
		consume!(self, db, Tok::Var, "Expect 'var'")?;

		let name = consume!(self, db, Tok::Identifier,
			"Expected variable name after 'var'")?;

		consume!(self, db, "Expect '=' in var declaration.")?;

		let initializer = self.expression()?;

		return Ok(Declare { initi 	a})
	}

	fn parse_top_level(&mut self, db: &mut Db) -> Result<()> {
		match self.peek_typ() {
			Tok::Eof => { },

			Tok::Var => {
				self.var_declaration(db)?;
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
	pub fn parse(&mut self, db: &mut Db) -> Result<()> {
		while !self.is_at_end() {
			self.parse_top_level(db)?;
		}

		Ok(())
	}
}