use std::{fs::File, path::Path};

use crate::{db::*, glue::lexer::{GlueTok, GlueToken, Lexer}, source::SourceLocation};
use crate::error::Error;

pub struct Parser<'b> {
	lexer: Lexer,
	db: &'b mut Db,

	current: GlueToken,
	last_location: SourceLocation,

	pub had_error: bool,
}

// TODO: Deduplicate this with the other parser? Maybe build a small general
// parsing framework?
pub enum ParseErr {
	SyntaxErr,
	IoErr(std::io::Error)
}

pub type Result<T> = std::result::Result<T, ParseErr>;

// TODO: Deduplicate macros.

macro_rules! begin_error {
	($parser:ident, $($arg:tt)*) => {
		Error::simple(
			format!($($arg)*),
			$parser.current.location.clone()
		)
	}
}

macro_rules! note {
	($error:ident, $location:expr, $($arg:tt)*) => {
		$error = $error.add_note(
			format!($($arg)*),
			$location
		);
	}
}

// NOTE: We should either come up with a way to easily do Error::simple() when
// the passed expr is not an error, or come up with a better name than "with".
macro_rules! semantic_error_with {
	($parser:ident, $error:expr) => {
		$parser.had_error = true;

		// Semantic errors are always reported, because they should not be able
		// to cascade, generally.
		$parser.db.report_error($error);
    };
}

macro_rules! parse_error {
	($parser:ident, $($arg:tt)*) => {
		// For now, just eprintln()... TODO Implement error handling system
		$parser.had_error = true;

		if $parser.should_report_errors() {
			$parser.db.report_error(Error::simple(
				format!($($arg)*),
				$parser.current.location.clone()
			)) 
		}
    };
}

macro_rules! consume {
    ($parser:ident, $ty:expr, $($arg:tt)*) => {
        if($parser.peek_typ() != $ty) {
			parse_error!($parser, $($arg)*);
			return Err(ParseErr::SyntaxErr);
        }
		else {
			$parser.advance()
		}
    };
}

// Note: A somewhat helpful regex for finding places where we forgot the question
// mark:
//    expected(_after)?!\([^;]+\);
macro_rules! expected {
	($parser:ident, $ty:expr, $($arg:tt)*) => {
		consume!($parser, $ty, "Expected {}, got '{}'", format!($($arg)*), $parser.db.get($parser.peek_lexeme()))
	}
}

macro_rules! got {
	($parser:ident, $($arg:tt)*) => {
		{
			parse_error!($parser, "{}, got '{}'", format!($($arg)*), $parser.db.get($parser.peek_lexeme()));
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

impl<'b> Parser<'b> {
	pub fn new(input: File, source_id: SourceId, db: &'b mut Db) -> std::io::Result<Self> {
		let mut lexer = Lexer::new(input, source_id);

		// TODO: Move File initialization to Lexer

		// Prime the parser with the first token in the file.
		let current = lexer.next_token(db)?;
		
		let parser = Parser {
			lexer,

			db,

			current,
			last_location: SourceLocation {
				source: source_id,
				offset: 0,
				length: 0,
			},

			had_error: false,
		};

		Ok(parser)
	}

	fn should_report_errors(&self) -> bool {
		// TODO: panic mode, etc

		// Don't report parse errors if the lexer has an error
		!self.lexer.had_error
	}
    
	fn start(&mut self) -> SourceLocation {
		self.current.location.clone()
	}

	fn end(&self, mut location: SourceLocation) -> SourceLocation {
		location.length = (self.last_location.offset - location.offset) + self.last_location.length;
		location
	}

	fn peek_typ(&self) -> GlueTok {
		return self.current.typ;
	}

	fn peek_lexeme(&self) -> StrId {
		return self.current.lexeme;
	}

	fn advance(&mut self) -> Result<GlueToken> {
		self.last_location = self.current.location.clone();
		let next = self.lexer
			.next_token(self.db)
			.map_err(|err| ParseErr::IoErr(err))?;
		Ok(std::mem::replace(&mut self.current, next))
	}

	fn is_at_end(&self) -> bool {
		return self.current.typ == GlueTok::Eof;
	}

	fn match_(&mut self, ty: GlueTok) -> Result<Option<GlueToken>> {
		if self.peek_typ() == ty {
			return Ok(Some(self.advance()?));
		}

		return Ok(None)
	}

	fn at(&mut self, ty: GlueTok) -> bool {
		return self.peek_typ() == ty;
	}

    fn member(&mut self) -> Result<()> {
        todo!()
    }

    fn fun(&mut self) -> Result<()> {
        todo!()
    }

    // TODO: This won't be able to return TypId forever, it will have to actually
    // resolve types. But this works for now...?
    fn c_type(&mut self) -> Result<TypId> {
        let id = expected!(self, GlueTok::Identifier, "C type expression")?;

        if id.lexeme == self.db.put_str("ps_int") {
            return Ok(self.db.types.int)
        }
        if id.lexeme == self.db.put_str("ps_float") {
            return Ok(self.db.types.float)
        }
        if id.lexeme == self.db.put_str("ps_bool") {
            return Ok(self.db.types.bool)
        }

        parse_error!(self, "Unknown C type");
        Err(ParseErr::SyntaxErr)
    }

    fn var(&mut self) -> Result<()> {
        let location = self.start();
        expected!(self, GlueTok::AnnotateVar, "PS_VAR")?;

        expected!(self, GlueTok::LeftParen, "'(' after PS_VAR")?;
        let name_override = self.match_(GlueTok::String)?;
        expected!(self, GlueTok::RightParen, "')' after PS_VAR")?;

        let c_type = self.c_type()?;
        let c_name = expected!(self, GlueTok::Identifier, "Variable name")?;

        self.eat_until(GlueTok::Semicolon)?;

        let var_name = match name_override {
            Some(name) => {
                // Chop off the quotes around the name
                let str = self.db.get(name.lexeme);
                let str = &str[1..str.len() - 1];
                self.db.put_str(str)
            },
            None => c_name.lexeme
        };

        let var = self.db.new_var(var_name, c_type, None, false, None, self.end(location));
        self.db.know_var_cname(var, self.db.get(c_name.lexeme));

        // TODO: Handle name collisions here as well?
        self.db.add_full_name(self.db.get(var_name), ScopeEntry::Var(var));
        

        Ok(())
    }

    fn eat_until(&mut self, tok: GlueTok) -> Result<()> {
        loop {
            let next = self.advance()?;
            if next.typ == tok {
                return Ok(());
            }
            if next.typ == GlueTok::Eof {
                return Ok(());
            }
            self.do_ignore(next)?;
        }
    }

    fn do_ignore(&mut self, tok: GlueToken) -> Result<()> {
        match tok.typ {
            GlueTok::LeftParen => self.ignore_until_rparen(),
            GlueTok::LeftBrace => self.ignore_until_rbrace(),
            GlueTok::LeftSquare => self.ignore_until_rsquare(),
            _ => Ok(())
        }
    }

    fn ignore_until_rparen(&mut self) -> Result<()> {
        loop {
            let next = self.advance()?;
            match next.typ {
                GlueTok::RightParen => return Ok(()),
                GlueTok::Eof => return Ok(()),
                _ => self.do_ignore(next)?
            }
        }
    }

    fn ignore_until_rsquare(&mut self) -> Result<()> {
        loop {
            let next = self.advance()?;
            match next.typ {
                GlueTok::RightSquare => return Ok(()),
                GlueTok::Eof => return Ok(()),
                _ => self.do_ignore(next)?
            }
        }
    }

    fn ignore_until_rbrace(&mut self) -> Result<()> {
        loop {
            let next = self.advance()?;
            match next.typ {
                GlueTok::RightBrace => return Ok(()),
                GlueTok::Eof => return Ok(()),
                _ => self.do_ignore(next)?
            }
        }
    }

    fn top_level(&mut self) -> Result<()> {
        loop {
            match self.peek_typ() {    
                GlueTok::Eof => return Ok(()),
                GlueTok::AnnotateVar => self.var()?,    
                GlueTok::AnnotateFun => self.fun()?,
                GlueTok::AnnotateMember => self.member()?,
                _ => {
                    // Ignore all other tokens.
                    let next = self.advance()?;
                    self.do_ignore(next)?
                }
            }
        }
    }

    pub fn parse(&mut self) -> std::io::Result<()> {
        match self.top_level() {
            // Syntax errors are only used to unwind the parser. No
            // need to report them to the caller here (we will report them
            // through a more sophisticated mechanism later).
            Ok(_) | Err(ParseErr::SyntaxErr) => return Ok(()),
            Err(ParseErr::IoErr(err)) => return Err(err)
        }
	}
}

pub fn parse_import(db: &mut Db, path: &Path) -> std::io::Result<bool> {
	let source_id = db.put_source_path(path);

	let file = db.get(source_id).to_file()?;

	let mut parser = Parser::new(file, source_id, db)?;
	parser.parse()?;

	let had_error = parser.had_error;

	Ok(had_error)
}