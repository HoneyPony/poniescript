use std::fs::File;
use std::io;

use crate::db::*;

use crate::lexer::*;
use crate::module::Module;

use crate::expr::*;

pub struct Parser<'a, 'b> {
	lexer: Lexer,
	module: &'a mut Module,
	db: &'b mut Db,

	current: Token
}

pub enum ParseErr {
	SyntaxErr,
	IoErr(std::io::Error)
}

pub type Result<T> = std::result::Result<T, ParseErr>;

macro_rules! consume {
    ($parser:ident, $ty:expr, $($arg:tt)*) => {
        if($parser.peek_typ() != $ty) {
			eprintln!($($arg)*);
			return Err(ParseErr::SyntaxErr);
        }
		else {
			$parser.advance()
		}
    };
}

macro_rules! expected {
	($parser:ident, $ty:expr, $($arg:tt)*) => {
		consume!($parser, $ty, "Expected {}, got {}", format!($($arg)*), $parser.db.get($parser.peek_lexeme()))
	}
}

macro_rules! expected_after {
	($parser:ident, $ty:expr, $prev_tok:expr, $($arg:tt)*) => {
		consume!($parser, $ty, "Expected {} after {}, got {}",
			format!($($arg)*),
			$parser.db.get($prev_tok.lexeme),
			$parser.db.get($parser.peek_lexeme()),
		)
	}
}

impl<'a, 'b> Parser<'a, 'b> {
	pub fn new(input: File, source_id: SourceId, db: &'b mut Db, module: &'a mut Module) -> std::io::Result<Self> {
		let mut lexer = Lexer::new(input, source_id);

		// TODO: Move File initialization to Lexer

		// Prime the parser with the first token in the file.
		let current = lexer.next_token(db)?;
		
		let parser = Parser {
			lexer,

			module,
			db,

			current
		};

		Ok(parser)
	}

	fn peek_typ(&self) -> Tok {
		return self.current.typ;
	}

	fn peek_lexeme(&self) -> StrId {
		return self.current.lexeme;
	}

	fn advance(&mut self) -> Result<Token> {
		let next = self.lexer
			.next_token(self.db)
			.map_err(|err| ParseErr::IoErr(err))?;
		Ok(std::mem::replace(&mut self.current, next))
	}

	fn is_at_end(&self) -> bool {
		return self.current.typ == Tok::Eof;
	}

	fn match_(&mut self, ty: Tok) -> bool {
		if self.peek_typ() == ty {
			self.advance();
			return true;
		}

		return false;
	}

	fn expression(&mut self) -> Result<Expr> {
		return Err(ParseErr::SyntaxErr);
	}

	fn var_declaration(&mut self) -> Result<Declare> {
		let key_var = expected!(self, Tok::Var, "'var''")?;

		let name = expected_after!(self, Tok::Identifier, key_var,
			"variable name")?;

		expected_after!(self, Tok::Equal, name, "'=' in declaration")?;

		let initializer = self.expression()?;
		
		let identity = self.db.new_var(name);

		return Stmt::new_declare_ok(identity, initializer);
	}

	fn parse_top_level(&mut self) -> Result<()> {
		match self.peek_typ() {
			Tok::Eof => { },

			Tok::Var => {
				self.var_declaration()?;
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
	pub fn parse(&mut self) -> Result<()> {
		while !self.is_at_end() {
			self.parse_top_level()?;
		}

		Ok(())
	}
}