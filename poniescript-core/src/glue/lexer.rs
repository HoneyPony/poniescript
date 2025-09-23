use std::io::BufReader;
use std::io::Read;

use rustc_hash::FxHashMap;

use crate::db::*;
use crate::error::Error;
use crate::source::*;

// Kinds of things we can parse:
// Annotation: PS_FUN ( "param", )
//
// Types: ps_int ps_float ps_vec2 void
//        struct x
//        struct x*

// Function declaration: fun_name ( Type param_name , Type param_name ) { ignore ignore ignore }
//
// Struct declaration:
// struct name {
//    Type object;
//
//    PS_VAR() Type var;
// };
//
// Global var declaration:
// PS_VAR() Type name;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum GlueTok {
	LeftParen, RightParen,
	LeftBrace, RightBrace,
	LeftSquare, RightSquare,
	Comma,
	
	Star,

	Semicolon,

	Identifier, String,

	AnnotateVar,
    AnnotateFun,
	AnnotateClass,
    AnnotateMember,
	AnnotateAbi,

    Struct, Void, 

    Unknown,

	Eof
}

#[derive(Clone)]
pub struct GlueToken {
	pub typ: GlueTok,
	pub lexeme: StrId,
	pub location: SourceLocation,
}

pub fn build_key_lookup_map(db: &mut Db) -> FxHashMap<StrId, GlueTok> {
	let mut map = FxHashMap::default();

	let mut add = |key, value: GlueTok| {
		let key = db.put_str(key);
		map.insert(key, value);
	};

    add("PS_FUN"   , GlueTok::AnnotateFun);
    add("PS_VAR"   , GlueTok::AnnotateVar);
	add("PS_CLASS" , GlueTok::AnnotateClass);
    add("PS_MEMBER", GlueTok::AnnotateMember);
	add("PS_ABI"   , GlueTok::AnnotateAbi);
	add("struct"   , GlueTok::Struct);

	return map;
}

pub struct Lexer {
	input: BufReader<Box<dyn std::io::Read>>,
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
	pub fn new(input: Box<dyn std::io::Read>, source_id: SourceId) -> Self {
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

	fn mk_token(&self, db: &mut Db, ty: GlueTok) -> GlueToken {
		let location = self.get_current_location();

		return GlueToken {
			typ: ty,
			lexeme: db.put_str(&self.buffer),
			location
		}
	}

	fn mk_token_res(&self, db: &mut Db, ty: GlueTok) -> std::io::Result<GlueToken> {
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

	fn tok_eq(&mut self, non_equal: GlueTok, with_equal: GlueTok, db: &mut Db) -> std::io::Result<GlueTok> {
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

	fn string(&mut self, db: &mut Db) -> std::io::Result<GlueToken> {
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
		return self.mk_token_res(db, GlueTok::String);
	}

	fn ident(&mut self, db: &mut Db) -> std::io::Result<GlueToken> {
		// The dummy next char at eof will terminate this automatically.
		while is_ident(self.peek()) { self.advance(db)?; }

		let mut token = self.mk_token(db, GlueTok::Identifier);

		// Replace token with keyword if it matches one
		if let Some(key_ty) = db.lookup_glue_key(token.lexeme) {
			token.typ = key_ty;
		}

		return Ok(token);
	}

	fn mk_eof(&mut self, db: &mut Db) -> std::io::Result<GlueToken> {
		// When at the EOF, create a lexeme that looks like <EOF> so that
		// we can nicely output it.
		//
		// This also helps with a problem where before we were creating a
		// lexeme of \0, making it very difficult to test against it.
		self.buffer.clear();
		self.buffer.push_str("<EOF>");
		return self.mk_token_res(db, GlueTok::Eof);
	}

	pub fn next_token(&mut self, db: &mut Db) -> std::io::Result<GlueToken> {
		if self.at_eof {
			return self.mk_eof(db);
		}

		let c = self.advance_past_whitespace(db)?;

		// In terms of code structure, we check the identifier and numerical
		// case first, so that we can have a big match at the end.

		let ty = match c {
			'(' => GlueTok::LeftParen,
			')' => GlueTok::RightParen,
			'{' => GlueTok::LeftBrace,
			'}' => GlueTok::RightBrace,
			'[' => GlueTok::LeftSquare,
			']' => GlueTok::RightSquare,
			',' => GlueTok::Comma,
			';' => GlueTok::Semicolon,

			'*' => GlueTok::Star,

            '/' => {
                if self.advance_if('/', db)? {
                    while !self.at_eof {
                        if self.advance(db)? == '\n' {
                            break;
                        }
                    }
                    return self.next_token(db);
                }
                if self.advance_if('*', db)? {
                    while !self.at_eof {
                        if self.advance(db)? == '*' {
                            if self.advance(db)? == '/' {
                                break;
                            }
                        }
                    }
                    return self.next_token(db);
                }

                // Otherwise, unknown token.
                GlueTok::Unknown
            }


			'"' => {
				return self.string(db);
			},

			'a'..='z' | 'A'..='Z' | '_' => {
				return self.ident(db);
			},

			_ => {
                if !self.at_eof {
                    // Just skip past anything we don't know about.
                    return self.mk_token_res(db, GlueTok::Unknown);
                }

				// Note that if we're in the self.at_eof == true case, we do
				// want to return an eof, because we really are there. (Here
				// we should be matching on the \0 that we generate above.)
				return self.mk_eof(db);
			}
		};

		return self.mk_token_res(db, ty);
	}
}