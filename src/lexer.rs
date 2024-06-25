use std::fs::File;

use crate::db::*;
use crate::source::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tok {
	LeftParen, RightParen,
	LeftBrace, RightBrace,
	LeftSquare, RightSquare,
	Comma, Dot,
	
	Minus, Plus, Slash, Star,
	MinusEqual, PlusEqual, SlashEqual, StarEqual,

	Semicolon,

	Bang, BangEqual,
	Equal, EqualEqual,
	Greater, GreaterEqual,
	Less, LessEqual,

	Identifier, String, Number,

	And, Class, Else, False, Fun, For, If, Null, Or,
	Return, Super, This, True, Var, While,

	Print,

	Eof
}

pub struct Token {
	pub typ: Tok,
	pub lexeme: StrId,
	pub location: SourceLocation,
}

pub struct Lexer {
	input: File,
	source_id: SourceId,

	// Current offset in the source file.
	current: u64,
	// Offset of the start of the token in the source file.
	start: u64,

	// Buffer holding the currently-scanned token
	buffer: String,
}

impl Lexer {
	pub fn new(input: File, source_id: SourceId) -> Self {
		return Lexer {
			input, source_id,

			current: 0,
			start: 0,

			buffer: String::new(),
		}
	}

	fn make_token(&self, db: &mut Db, ty: Tok) -> Token {
		let location = SourceLocation {
			source: self.source_id,
			offset: self.start,
			length: (self.current - self.start)
		};

		return Token {
			typ: ty,
			lexeme: db.put_str(&self.buffer),
			location
		}
	}

	pub fn next_token(&mut self, db: &mut Db) -> Token {
		self.make_token(db, Tok::And)
	}
}