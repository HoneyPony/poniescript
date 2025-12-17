use std::io::BufReader;
use std::io::Read;

use rustc_hash::FxHashMap;

use crate::db::*;
use crate::error::Error;
use crate::source::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tok {
	LeftParen, RightParen,
	LeftBrace, RightBrace,
	LeftSquare, RightSquare,
	Comma, Dot,

	// Range types
	// TODO: Also support DotDotLess, LessDotDot, LessDotDotEqual, etc? Might be
	// unnecessary...
	//
	// Also, EqualDotDot and EqualDotDotEqual are actually the wrong ones. Whoops!
	DotDot, DotDotEqual, EqualDotDot, EqualDotDotEqual,
	
	Minus, Plus, Slash, Star,
	MinusEqual, PlusEqual, SlashEqual, StarEqual,

	Semicolon, Colon,

	Question, Percent, Ampersand, VerticalBar,

	Bang, BangEqual,
	Equal, EqualEqual,
	Greater, GreaterEqual,
	Less, LessEqual,

	// TODO: Rename this to RightArrow... Oops...
	LeftArrow,

	Identifier, StringSimple, WholeNumber, DecimalNumber,

	And, Class, Else, False, Fun, For, If, In, Null, Or,
	Return, Super, KeySelf, True, Using, Var, While,
	Loop, Break, Continue,

	New,

	Print, Str,

	Some, Nil,

	DocComment,

	Eof
}

#[derive(Clone)]
pub struct Token {
	pub typ: Tok,
	pub lexeme: StrId,
	pub location: SourceLocation,
}

impl Token {
	pub fn synthesize_ident(db: &Db, str: StrId) -> Token {
		Token {
			typ: Tok::Identifier,
			lexeme: str,
			location: SourceLocation {
				source: db.synthetic,
				offset: 0,
				length: 0,
			}
		}
	}

	pub fn synthesize_ident_from(db: &mut Db, str: &'static str) -> Token {
		let str = db.put_str(str);
		Token::synthesize_ident(db, str)
	}

	pub fn synth_tok(db: &Db, str: StrId, typ: Tok) -> Token {
		Token {
			typ,
			lexeme: str,
			location: SourceLocation {
				source: db.synthetic,
				offset: 0,
				length: 0,
			}
		}
	}

	pub fn synth_tok_from(db: &mut Db, str: &'static str, typ: Tok) -> Token {
		let str = db.put_str(str);
		Token::synth_tok(db, str, typ)
	}
}

pub fn build_key_lookup_map(db: &mut Db) -> FxHashMap<StrId, Tok> {
	let mut map = FxHashMap::default();

	let mut add = |key, value: Tok| {
		let key = db.put_str(key);
		map.insert(key, value);
	};

	add("and"   ,   Tok::And);
	add("break" ,   Tok::Break);
	add("continue", Tok::Continue);
	add("class" ,   Tok::Class);
	add("else"  ,   Tok::Else);
	add("false" ,   Tok::False);
	add("fun"   ,   Tok::Fun);
	add("for"   ,   Tok::For);
	add("if"    ,   Tok::If);
	add("in"    ,   Tok::In);
	add("null"  ,   Tok::Null);
	add("or"    ,   Tok::Or);
	add("return",   Tok::Return);
	add("super" ,   Tok::Super);
	add("self"  ,   Tok::KeySelf);
	add("true"  ,   Tok::True);
	add("using" ,   Tok::Using);
	add("var"   ,   Tok::Var);
	add("while" ,   Tok::While);
	add("loop"  ,   Tok::Loop);
	add("new"   ,   Tok::New);

	add("some"  ,   Tok::Some);
	add("nil"   ,   Tok::Nil);

	add("print",    Tok::Print);
	add("str"  ,    Tok::Str);

	return map;
}

pub struct Lexer {
	input: BufReader<Box<dyn Read>>,
	source_id: SourceId,

	// Current offset in the source file.
	current: u64,
	// Offset of the start of the token in the source file.
	start: u64,

	// Buffer holding the currently-scanned token
	buffer: String,

	next_byte: u8,

	next_char: char,

	at_eof: bool,

	pub had_error: bool,

	// The previously stored token, if any.
	prev: Option<Token>,
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
	pub fn new(input: Box<dyn Read>, source_id: SourceId) -> Self {
		return Lexer {
			input: BufReader::new(input),
			source_id,

			current: 0,
			start: 0,

			buffer: String::new(),

			next_char: ' ',
			next_byte: b' ',

			at_eof: false,

			had_error: false,
			prev: None,
		}
	}

	fn get_current_location(&self) -> SourceLocation {
		return SourceLocation {
			source: self.source_id,

			// Because we use one character of lookahead, the 'start' and 'current'
			// values are always 1 past where they should be.
			offset: self.start - 1,
			length: (self.current - self.start)
		};
	}

	fn mk_token(&mut self, db: &mut Db, ty: Tok) -> Token {
		let location = self.get_current_location();

		// eprintln!("-- trace lexer: {}:[{}] {:?}", location.offset, location.length, ty);

		return Token {
			typ: ty,
			// TODO: Is this legit? It seems like a good idea.
			lexeme: db.put_clear_string(&mut self.buffer),
			location
		}
	}

	fn mk_token_res(&mut self, db: &mut Db, ty: Tok) -> std::io::Result<Token> {
		Ok(self.mk_token(db, ty))
	}

	fn advance_byte(&mut self) -> std::io::Result<u8> {
		let result = self.next_byte;

		let mut buf = [0u8];
		match self.input.read(&mut buf)? {
			0 => { self.next_byte = b'\0'; return Ok(result); }
			1 => {
				self.next_byte = buf[0];
			}
			_ => unreachable!()
		}

		Ok(result)
	}

	fn advance(&mut self, db: &mut Db) -> std::io::Result<char> {
		let result = self.next_char;
		self.current += 1;
		self.buffer.push(self.next_char);
		
		let mut full_buf = [0u8, 0u8, 0u8, 0u8, 0u8, 0u8];
		let mut idx = 0;
		loop {
			let c= self.next_byte;

			if c == 0 {
				self.next_char = '\0';
				self.at_eof = true;
				return Ok(result);
			}

			// Common case: Not utf-8.
			if idx == 0 && c <= 127 {
				self.next_char = c as char;

				// Consume the byte.
				self.advance_byte()?;
				return Ok(result);
			}

			// In this case, we actually overshot the buf. Store that
			// byte for next time, so don't advance.
			if idx != 0 && c <= 127 {
				break;
			}

			// Otherwise, keep filling up the buf, and consume that byte
			// from the input.
			full_buf[idx] = c;
			idx += 1;

			self.advance_byte()?;

			// The maximum number of bytes that can be in a single codepoint,
			// encoded in UTF-8, is 6 bytes.
			if idx >= 6 {
				break;
			}
		}

		let full_buf = &full_buf[0..idx];

		// Here, we must convert the full_buf into utf8.
		let Ok(str) = std::str::from_utf8(&full_buf) else {
			self.error(db, format!("Invalid UTF-8 byte sequence: {:?}", full_buf));
			self.next_char = '?';
			return Ok(result);
		};

		//assert!(str.len() == 1, "Byte sequence was not one character: {:?}", full_buf);
		self.next_char = str.chars().next().unwrap();

		return Ok(result);
	}

	fn peek(&mut self) -> char {
		return self.next_char;
	}

	/// Advances past all the whitespace, THEN advances 1 character.
	fn advance_past_whitespace(&mut self, db: &mut Db) -> std::io::Result<char> {
		while is_whitespace(self.peek()) {
			self.advance(db)?;
		}

		// Reset the start and buffer
		self.start = self.current;
		self.buffer.clear();

		return self.advance(db);
	}

	fn advance_if(&mut self, at: char, db: &mut Db) -> std::io::Result<bool> {
		if self.peek() == at {
			self.advance(db)?;
			return Ok(true);
		}
		return Ok(false);
	}

	fn tok_eq(&mut self, non_equal: Tok, with_equal: Tok, db: &mut Db) -> std::io::Result<Tok> {
		Ok(match self.advance_if('=', db)? {
			true => with_equal,
			false => non_equal
		})
	}

	fn error(&mut self, db: &mut Db, message: String) {
		self.had_error = true;
		db.report_error(Error::simple(
			format!("Parse error: {}", message),
			self.get_current_location()
		));
	}

	fn string(&mut self, db: &mut Db) -> std::io::Result<Token> {
		loop {
			let next = self.advance(db)?;

			if next == '\\' {
				// Unconditionally advance, don't check quote
				self.advance(db)?;
			}
			else if next == '\"' {
				break;
			}

			// TODO: Implement string escapes, etc..
			if self.at_eof {
				self.error(db, "Unterminated string".into());
				break;
			}
		}
		return self.mk_token_res(db, Tok::StringSimple);
	}

	fn ident(&mut self, db: &mut Db) -> std::io::Result<Token> {
		// The dummy next char at eof will terminate this automatically.
		while is_ident(self.peek()) { self.advance(db)?; }

		let mut token = self.mk_token(db, Tok::Identifier);

		// Replace token with keyword if it matches one
		if let Some(key_ty) = db.lookup_key(token.lexeme) {
			token.typ = key_ty;
		}

		return Ok(token);
	}

	fn number(&mut self, db: &mut Db) -> std::io::Result<Token> {
		while is_num(self.peek()) { self.advance(db)?; }

		// No more dot. Numbers are now integers.
		// let ty = if self.peek() == '.' {
		// 	// Eat the dot
		// 	self.advance(db)?;

		// 	while is_num(self.peek()) { self.advance(db)?; }

		// 	Tok::DecimalNumber
		// } else { Tok::WholeNumber };
		return self.mk_token_res(db, Tok::WholeNumber);
	}

	fn line_comment(&mut self, db: &mut Db) -> std::io::Result<Option<Token>> {
		// Here we also handle special kinds of comments.
		enum CommentKind {
			None,
			Doc,
			TestLine,
			TestErr,
		}

		let mut kind = CommentKind::None;

		if db.test_mode {
			if self.advance_if('!', db)? {
				kind = CommentKind::TestLine;

				// Start the buffer at the beginning of the line.
				self.buffer.clear();
			}
			if self.advance_if('?', db)? {
				kind = CommentKind::TestErr;
				self.buffer.clear();
			}
		}

		if self.advance_if('/', db)? {
			kind = CommentKind::Doc;
			self.buffer.clear();
			// TODO: Skip prefixed whitespace?
		}

		while !self.at_eof {
			if self.advance(db)? == '\n' {
				break;
			}
		}

		match kind {
			CommentKind::None => { },

			// For test lines, we add them to the expected output in the Db.
			CommentKind::TestLine => {
				let line = self.buffer.trim();
				db.test_lines.push(line.to_string());
			}

			CommentKind::TestErr => {
				let line = self.buffer.trim();
				db.test_errors.push(line.to_string());
			}

			CommentKind::Doc => {
				return Ok(Some(self.mk_token(db, Tok::DocComment)))
			}
		}

		Ok(None)
	}

	fn mk_eof(&mut self, db: &mut Db) -> std::io::Result<Token> {
		// When at the EOF, create a lexeme that looks like <EOF> so that
		// we can nicely output it.
		//
		// This also helps with a problem where before we were creating a
		// lexeme of \0, making it very difficult to test against it.
		self.buffer.clear();
		self.buffer.push_str("<EOF>");
		return self.mk_token_res(db, Tok::Eof);
	}

	pub fn next_token(&mut self, db: &mut Db) -> std::io::Result<Token> {
		if self.at_eof {
			return self.mk_eof(db);
		}

		let c = self.advance_past_whitespace(db)?;

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
			'.' => {
				if self.advance_if('.', db)? {
					if self.advance_if('=', db)? {
						Tok::DotDotEqual
					}
					else {
						Tok::DotDot
					}
				}
				else {
					Tok::Dot
				}
			}

			';' => Tok::Semicolon,
			':' => Tok::Colon,
			'?' => Tok::Question,
			'%' => Tok::Percent,
			'&' => Tok::Ampersand,
			'|' => Tok::VerticalBar,

			'-' => {
				if self.advance_if('=', db)? {
					Tok::MinusEqual
				}
				else if self.advance_if('>', db)? {
					Tok::LeftArrow
				}
				else {
					Tok::Minus
				}
			},
			'+' => self.tok_eq(Tok::Plus, Tok::PlusEqual, db)?,
			'/' => {
				if self.advance_if('/', db)? {
					let c = self.line_comment(db)?;

					if let Some(c) = c {
						// Doc comments
						return Ok(c);
					}
					// TODO: Speed this up in the case of multiline comments...
					// we really don't want to recurse here...
					return self.next_token(db);
				}

				self.tok_eq(Tok::Slash, Tok::SlashEqual, db)?
			},
			'*' => self.tok_eq(Tok::Star, Tok::StarEqual, db)?,

			'!' => self.tok_eq(Tok::Bang, Tok::BangEqual, db)?,
			'=' => {
				if self.advance_if('=', db)? {
					Tok::EqualEqual
				}
				else if self.advance_if('.', db)? {
					if self.advance_if('.', db)? {
						if self.advance_if('=', db)? {
							Tok::EqualDotDotEqual
						}
						else {
							Tok::EqualDotDot
						}
					}
					else {
						// TODO: Probably we want to handle some of this stuff
						// in the parser, not the lexer.
						self.error(db, "Invalid sequence '=.'".into());
						return self.mk_token_res(db, Tok::Equal);
					}
				}
				else {
					Tok::Equal
				}
			}
			'>' => self.tok_eq(Tok::Greater, Tok::GreaterEqual, db)?,
			'<' => self.tok_eq(Tok::Less, Tok::LessEqual, db)?,

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

				// Note that if we're in the self.at_eof == true case, we do
				// want to return an eof, because we really are there. (Here
				// we should be matching on the \0 that we generate above.)
				return self.mk_eof(db);
			}
		};

		return self.mk_token_res(db, ty);
	}
}