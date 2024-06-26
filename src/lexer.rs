use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;

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

	Semicolon, Colon,

	Question, Percent, Ampersand, VerticalBar,

	Bang, BangEqual,
	Equal, EqualEqual,
	Greater, GreaterEqual,
	Less, LessEqual,

	LeftArrow,

	Identifier, String, WholeNumber, DecimalNumber,

	And, Class, Else, False, Fun, For, If, In, Null, Or,
	Range, Return, Super, KeySelf, True, Using, Var, While,

	Print,

	Eof
}

pub struct Token {
	pub typ: Tok,
	pub lexeme: StrId,
	pub location: SourceLocation,
}

pub fn build_key_lookup_map(db: &mut Db) -> HashMap<StrId, Tok> {
	let mut map = HashMap::new();

	let mut add = |key, value: Tok| {
		let key = db.put_str(key);
		map.insert(key, value);
	};

	add("and"   , Tok::And);
	add("class" , Tok::Class);
	add("else"  , Tok::Else);
	add("false" , Tok::False);
	add("fun"   , Tok::Fun);
	add("for"   , Tok::For);
	add("if"    , Tok::If);
	add("in"    , Tok::In);
	add("null"  , Tok::Null);
	add("or"    , Tok::Or);
	add("range" , Tok::Range);
	add("return", Tok::Return);
	add("super" , Tok::Super);
	add("self"  , Tok::KeySelf);
	add("true"  , Tok::True);
	add("using" , Tok::Using);
	add("var"   , Tok::Var);
	add("while" , Tok::While);

	add("print", Tok::Print);

	return map;
}

pub struct Lexer {
	input: BufReader<File>,
	source_id: SourceId,

	// Current offset in the source file.
	current: u64,
	// Offset of the start of the token in the source file.
	start: u64,

	// Buffer holding the currently-scanned token
	buffer: String,

	next_char: char,

	at_eof: bool,
}

fn is_whitespace(c: char) -> bool {
	return c == ' ' || c == '\r' || c == '\n' || c == '\t';
}

fn is_num(c: char) -> bool {
	return c >= '0' && c <= '9';
}

fn is_alpha(c: char) -> bool {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
}

fn is_ident(c: char) -> bool {
	return is_alpha(c) || is_num(c) || c == '_';
}

impl Lexer {
	pub fn new(input: File, source_id: SourceId) -> Self {
		return Lexer {
			input: BufReader::new(input),
			source_id,

			current: 0,
			start: 0,

			buffer: String::new(),

			next_char: ' ',

			at_eof: false,
		}
	}

	fn mk_token(&self, db: &mut Db, ty: Tok) -> Token {
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

	fn mk_token_res(&self, db: &mut Db, ty: Tok) -> std::io::Result<Token> {
		Ok(self.mk_token(db, ty))
	}

	fn advance(&mut self) -> std::io::Result<char> {
		let result = self.next_char;
		self.current += 1;
		self.buffer.push(self.next_char);
		
		let mut buf = [0u8];
		match self.input.read(&mut buf)? {
			0 => { self.next_char = '\0'; self.at_eof = true; },
			1 => {
				

				// TODO: Consider reading utf-8 data better. For now, because Rust
				// doesn't support it, we will just read ASCII -- we can easily support
				// utf8 later by changing this function.
				self.next_char = buf[0] as char;
			},
			_ => unreachable!()
		}

		return Ok(result);
	}

	fn peek(&mut self) -> char {
		return self.next_char;
	}

	/// Advances past all the whitespace, THEN advances 1 character.
	fn advance_past_whitespace(&mut self) -> std::io::Result<char> {
		while is_whitespace(self.peek()) {
			self.advance()?;
		}

		// Reset the start and buffer
		self.start = self.current;
		self.buffer.clear();

		return self.advance();
	}

	fn advance_if(&mut self, at: char) -> std::io::Result<bool> {
		if self.peek() == at {
			self.advance()?;
			return Ok(true);
		}
		return Ok(false);
	}

	fn tok_eq(&mut self, non_equal: Tok, with_equal: Tok) -> std::io::Result<Tok> {
		Ok(match self.advance_if('=')? {
			true => with_equal,
			false => non_equal
		})
	}

	fn error(&self, db: &mut Db, message: String) {
		eprintln!("Parse error: {message}");
	}

	fn string(&mut self, db: &mut Db) -> std::io::Result<Token> {
		while self.advance()? != '"' {
			// TODO: Implement string escapes, etc..
			if self.at_eof {
				self.error(db, "Unterminated string".into());
				break;
			}
		}
		return self.mk_token_res(db, Tok::String);
	}

	fn ident(&mut self, db: &mut Db) -> std::io::Result<Token> {
		// The dummy next char at eof will terminate this automatically.
		while is_ident(self.peek()) { self.advance()?; }

		let mut token = self.mk_token(db, Tok::Identifier);

		// Replace token with keyword if it matches one
		if let Some(key_ty) = db.lookup_key(token.lexeme) {
			token.typ = key_ty;
		}

		return Ok(token);
	}

	fn number(&mut self, db: &mut Db) -> std::io::Result<Token> {
		while is_num(self.peek()) { self.advance()?; }

		let ty = if self.peek() == '.' {
			// Eat the dot
			self.advance()?;

			while is_num(self.peek()) { self.advance()?; }

			Tok::DecimalNumber
		} else { Tok::WholeNumber };
		return self.mk_token_res(db, ty);
	}

	pub fn next_token(&mut self, db: &mut Db) -> std::io::Result<Token> {
		if self.at_eof {
			return self.mk_token_res(db, Tok::Eof);
		}

		let c = self.advance_past_whitespace()?;

		// In terms of code structure, we check the identifier and numerical
		// case first, so that we can have a big match at the end.

		let ty = match c {
			'(' => Tok::LeftParen,
			')' => Tok::RightParen,
			'{' => Tok::LeftBrace,
			'}' => Tok::RightBrace,
			'[' => Tok::LeftSquare,
			']' => Tok::RightSquare,
			',' => Tok::Comma,
			'.' => Tok::Dot,

			';' => Tok::Semicolon,
			':' => Tok::Colon,
			'?' => Tok::Question,
			'%' => Tok::Percent,
			'&' => Tok::Ampersand,
			'|' => Tok::VerticalBar,

			'-' => {
				if self.advance_if('=')? {
					Tok::MinusEqual
				}
				else if self.advance_if('>')? {
					Tok::LeftArrow
				}
				else {
					Tok::Minus
				}
			},
			'+' => self.tok_eq(Tok::Plus, Tok::PlusEqual)?,
			'/' => self.tok_eq(Tok::Slash, Tok::SlashEqual)?,
			'*' => self.tok_eq(Tok::Star, Tok::StarEqual)?,

			'!' => self.tok_eq(Tok::Bang, Tok::BangEqual)?,
			'=' => self.tok_eq(Tok::Equal, Tok::EqualEqual)?,
			'>' => self.tok_eq(Tok::Greater, Tok::GreaterEqual)?,
			'<' => self.tok_eq(Tok::Less, Tok::LessEqual)?,

			'"' => {
				return self.string(db);
			},

			'a'..='z' | 'A'..='Z' | '_' => {
				return self.ident(db);
			},

			'0'..='9' => {
				return self.number(db);
			},

			_ => {
				// If we have flagged EOF, there's no more undefined characters.
				// Otherwise, there's an error.
				if !self.at_eof {
					self.error(db, format!("Unrecognized character '{c}'"));
				}
				
				// TODO: Do we want to introduce a separate "error token" here?
				Tok::Eof
			}
		};

		return self.mk_token_res(db, ty);
	}
}