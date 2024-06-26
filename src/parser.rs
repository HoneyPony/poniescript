use std::fs::File;
use std::io;

use crate::db::*;

use crate::lexer::*;
use crate::module::Module;

use crate::expr::*;
use crate::typ::Type;

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

macro_rules! parse_error {
	($($arg:tt)*) => {
		// For now, just eprintln()... TODO Implement error handling system
		eprintln!($($arg)*); 
    };
}

macro_rules! consume {
    ($parser:ident, $ty:expr, $($arg:tt)*) => {
        if($parser.peek_typ() != $ty) {
			parse_error!($($arg)*);
			return Err(ParseErr::SyntaxErr);
        }
		else {
			$parser.advance()
		}
    };
}

macro_rules! expected {
	($parser:ident, $ty:expr, $($arg:tt)*) => {
		consume!($parser, $ty, "Expected {}, got '{}'", format!($($arg)*), $parser.db.get($parser.peek_lexeme()))
	}
}

macro_rules! got {
	($parser:ident, $($arg:tt)*) => {
		{
			parse_error!("{}, got '{}'", format!($($arg)*), $parser.db.get($parser.peek_lexeme()));
			return Err(ParseErr::SyntaxErr)
		}
	}
}

macro_rules! expected_after {
	($parser:ident, $ty:expr, $prev_tok:expr, $($arg:tt)*) => {
		consume!($parser, $ty, "Expected {} after '{}', got '{}'",
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

	fn match_(&mut self, ty: Tok) -> Result<bool> {
		if self.peek_typ() == ty {
			self.advance()?;
			return Ok(true);
		}

		return Ok(false);
	}

	fn number(&mut self) -> Result<Expr> {
		let number = self.advance()?; // Eat numerical token... TODO expected! with multiple
		// types..?
		//let number = expected!(self, Tok::Number, "number literal")?;

		let typ = match number.typ {
			Tok::DecimalNumber => Type::UnassignedDecimal,
			Tok::WholeNumber => Type::UnassignedNumeric,
			_ => unreachable!()
		};

		return Expr::mk_literal_ok(number.location.clone(),
			number,
			self.db.put_type(typ));
	}

	fn expression(&mut self) -> Result<Expr> {
		match self.peek_typ() {
			Tok::DecimalNumber | Tok::WholeNumber => {
				self.number()
			},

			_ => {
				got!(self, "Expected identifier")
			}
		}
	}

	fn var_declaration(&mut self) -> Result<Declare> {
		let key_var = expected!(self, Tok::Var, "'var''")?;

		let name = expected_after!(self, Tok::Identifier, key_var,
			"variable name")?;

		let equal = expected_after!(self, Tok::Equal, name, "'=' in declaration")?;

		let initializer = self.expression()?;

		expected!(self, Tok::Semicolon, "';' after initializer expression")?;
		
		let identity = self.db.new_var(name);

		return Stmt::new_declare_ok(equal.location, identity, initializer);
	}

	fn parse_top_level(&mut self) -> Result<()> {
		match self.peek_typ() {
			Tok::Eof => { },

			Tok::Var => {
				let global = self.var_declaration()?;
				self.module.globals.push(global);
			},

			_ => {
				// Skip the erroneous token, as nothing else will drive
				// parsing forward.
				self.advance()?;
				got!(self, "Expected 'var', 'const', 'class', or 'fun'")
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
	pub fn parse(&mut self) -> io::Result<()> {
		while !self.is_at_end() {
			match self.parse_top_level() {
				// Syntax errors are only used to unwind the parser. No
				// need to report them to the caller here (we will report them
				// through a more sophisticated mechanism later).
				Ok(_) | Err(ParseErr::SyntaxErr) => continue,
				Err(ParseErr::IoErr(err)) => return Err(err)
			}
		}

		Ok(())
	}
}