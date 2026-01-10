use std::io;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use poni_arena::ArenaBorrowMut;
use rustc_hash::FxHashSet;
use crate::db::*;

use crate::lexer::*;

use crate::expr::*;
use crate::error::Error;
use crate::source::Source;
use crate::source::SourceLocation;
use crate::typ::RangeEnd;
use crate::typ::Type;

use poni_arena::IndexCell;

struct Scope {
	map: FxHashMap<StrId, ScopeEntry>,
}

impl Scope {
	pub fn new() -> Self {
		return Scope {
			map: FxHashMap::default(),
		}
	}
}

struct RangeTypes {
	key_ii: StrId,
	key_ie: StrId,
	key_ei: StrId,
	key_ee: StrId,
	key_iu: StrId,
	key_eu: StrId,
	key_ui: StrId,
	key_ue: StrId,
	key_uu: StrId,
}

impl RangeTypes {
	pub fn build(db: &mut Db) -> Self {
		Self {
			key_ii: db.put_str("Closed"),
			key_ie: db.put_str("ClosedOpen"),
			key_ei: db.put_str("OpenClosed"),
			key_ee: db.put_str("Open"),
			key_iu: db.put_str("ClosedInf"),
			key_eu: db.put_str("OpenInf"),
			key_ui: db.put_str("InfClosed"),
			key_ue: db.put_str("InfOpen"),
			key_uu: db.put_str("Every"), // ?
		}
	}

	pub fn is_any(&self, key: StrId) -> bool {
		key == self.key_ii ||
		key == self.key_ie ||
		key == self.key_ei ||
		key == self.key_ee ||
		key == self.key_iu ||
		key == self.key_eu ||
		key == self.key_ui ||
		key == self.key_ue ||
		key == self.key_uu
	}

	pub fn into_type(&self, key: StrId, inner_typ: TypId) -> Type {
		if key == self.key_ii { return Type::RangeOf(RangeEnd::Inclusive, RangeEnd::Inclusive, inner_typ); }
		if key == self.key_ie { return Type::RangeOf(RangeEnd::Inclusive, RangeEnd::Exclusive, inner_typ); }
		if key == self.key_ei { return Type::RangeOf(RangeEnd::Exclusive, RangeEnd::Inclusive, inner_typ); }
		if key == self.key_ee { return Type::RangeOf(RangeEnd::Exclusive, RangeEnd::Exclusive, inner_typ); }
		if key == self.key_iu { return Type::RangeOf(RangeEnd::Inclusive, RangeEnd::Unbounded, inner_typ); }
		if key == self.key_eu { return Type::RangeOf(RangeEnd::Exclusive, RangeEnd::Unbounded, inner_typ); }
		if key == self.key_ui { return Type::RangeOf(RangeEnd::Unbounded, RangeEnd::Inclusive, inner_typ); }
		if key == self.key_ue { return Type::RangeOf(RangeEnd::Unbounded, RangeEnd::Exclusive, inner_typ); }
		if key == self.key_uu { return Type::RangeOf(RangeEnd::Unbounded, RangeEnd::Unbounded, inner_typ); }
		panic!("ICE: Called RangeTypes::into_type() when the key was not a valid Range type.")
	}
}

pub struct Parser<'b> {
	lexer: Lexer,
	db: &'b mut Db,
	ast: &'b mut Ast,

	source_id: SourceId,

	current: Token,
	last_location: SourceLocation,

	range_types: RangeTypes,

	scopes: Vec<Scope>,
	scope_name: String,
	global_scope: Scope,

	/// Tracks whether we are currently in a member initializer. If so, we
	/// forbid the 'self' keyword as a straightforward way to keep things
	/// more correct.
	in_member_initializer: bool,

	cur_doc_comment: Vec<Token>,
	prev_doc_comment: Vec<Token>,

	pub had_error: bool,
}

pub enum ParseErr {
	SyntaxErr,
	IoErr(std::io::Error)
}

pub type Result<T> = std::result::Result<T, ParseErr>;

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
	pub fn new(input: Box<dyn std::io::Read>, source_id: SourceId, db: &'b mut Db, ast: &'b mut Ast) -> std::io::Result<Self> {
		// TODO: Technically we only need one of this, even with multiple parsers...
		let range_types = RangeTypes::build(db);
		let mut lexer = Lexer::new(input, source_id);

		// TODO: Move File initialization to Lexer

		// Make a dummy current token, due to DocComment handling.
		let current = Token::synth_tok(db, db.str_x, Tok::Eof);
		
		let mut parser = Parser {
			lexer,

			db,
			ast,

			source_id,

			range_types,

			scopes: Vec::new(),
			// TODO: Push and pop things from this name
			scope_name: String::new(),
			global_scope: Scope::new(),

			cur_doc_comment: Vec::new(),
			prev_doc_comment: Vec::new(),

			in_member_initializer: false,

			current,
			last_location: SourceLocation {
				source: source_id,
				offset: 0,
				length: 0,
			},

			had_error: false,
		};

		// Prime the parser with the first token in the file (and collect
		// doc comments, etc).
		parser.advance().map_err(|e| {
			match e {
				// Simply advance()ing shouldn't result in a Syntax error.
				// We would rather return an IoErr here.
				//
				// TODO: Maybe make advance() return std::io::Result, and then
				// implement From<> for ParseErr?
				ParseErr::SyntaxErr => unreachable!(),
				ParseErr::IoErr(error) => error,
			}
		})?;

		Ok(parser)
	}

	fn should_report_errors(&self) -> bool {
		// TODO: panic mode, etc

		// Don't report parse errors if the lexer has an error
		!self.lexer.had_error
	}

	fn push_scope(&mut self) {
		self.scopes.push(Scope::new());
	}

	fn pop_scope(&mut self) {
		self.scopes.pop();
	}

	fn start(&mut self) -> SourceLocation {
		self.current.location.clone()
	}

	fn end(&self, mut location: SourceLocation) -> SourceLocation {
		location.length = (self.last_location.offset - location.offset) + self.last_location.length;
		location
	}

	fn location_of(&self, entry: &ScopeEntry) -> SourceLocation {
		match entry {
			ScopeEntry::Var(var) => self.db.get(*var).location.clone(),
			ScopeEntry::Fun(fun) => {
				// We really should just store a SourceLocation on the function.
				self.db.get(*fun).location.clone()
			}
			ScopeEntry::Class(class) => {
				self.db.get(*class).location.clone()
			}
			ScopeEntry::None => todo!("location_of() for ScopeEntry::None"),
		}
	}

	fn scope_put_entry(&mut self, name: StrId, entry: ScopeEntry) {
		log::trace!("scope: put entry with name {}", self.db.get(name));
		match self.scopes.last_mut() {
			Some(last) => {
				// For local scopes, it is OK to redefine the name with a new
				// value -- that's just shadowing.
				last.map.insert(name, entry);
			},
			None => {
				self.global_scope.map.insert(name, entry);

				// TODO: Can this concatenation be made more efficient..?
				// Maybe the DB could have a buffer for this purpose...
				let full_name = format!("{}{}", self.scope_name, self.db.get(name));
				let old =  self.db.add_full_name(&full_name, entry);

				// For global scopes, redefining a name is not allowed.
				if let Some(old) = old {
					// NOTE: If we eventually support global function overloading,
					// then that WILL have to be allowed.
					let mut error = begin_error!(self, "Redefinition of global name '{}'", self.db.get(name));
					note!(error, Some(self.location_of(&old)), "Previous definition was here");
					semantic_error_with!(self, error);
				}
			}
		}
	}

	// TODO: We will also have to add_full_name for functions,
	// fields, etc... not sure where though yet.

	fn scope_lookup(&mut self, name: StrId) -> ScopeEntry {
		if self.scopes.is_empty() {
			return *self.global_scope.map.get(&name).unwrap_or(&ScopeEntry::None);
		}

		// If we do have a scope, then we must NOT look in the global scope
		// (as it's possible that a global name will later be shadowed
		// by a class/node field). Instead, only resolve local variables right
		// now.

		for scope in self.scopes.iter().rev() {
			match scope.map.get(&name) {
				Some(entry) => return *entry,
				_ => { }
			}
		}

		// Default to no value if we can't find one.
		ScopeEntry::None
	}

	fn peek_typ(&self) -> Tok {
		return self.current.typ;
	}

	fn peek_lexeme(&self) -> StrId {
		return self.current.lexeme;
	}

	fn advance(&mut self) -> Result<Token> {
		self.last_location = self.current.location.clone();
		
		let next = loop {
			let next = self.lexer
				.next_token(self.db)
				.map_err(|err| ParseErr::IoErr(err))?;

			if matches!(next.typ, Tok::DocComment) {
				self.cur_doc_comment.push(next);
			}
			else {
				self.prev_doc_comment = std::mem::take(&mut self.cur_doc_comment);
				break next;
			}
		};

		Ok(std::mem::replace(&mut self.current, next))
	}

	fn is_at_end(&self) -> bool {
		return self.current.typ == Tok::Eof;
	}

	fn match_(&mut self, ty: Tok) -> Result<Option<Token>> {
		if self.peek_typ() == ty {
			return Ok(Some(self.advance()?));
		}

		return Ok(None)
	}

	fn at(&mut self, ty: Tok) -> bool {
		return self.peek_typ() == ty;
	}

	fn number(&mut self) -> Result<ExprId> {
		let mut number = self.advance()?; // Eat numerical token... TODO expected! with multiple
		let mut typ = Type::AssumeInt;
		// types..?
		//let number = expected!(self, Tok::Number, "number literal")?;

		// If we're at a single dot, followed by a number, we want to consume
		// the dot and the number.
		if self.match_(Tok::Dot)?.is_some() {
			// If there's no number, that's also fine, e.g. 1. => 1.0.
			// Find a tail if there is one.
			let concat = match self.match_(Tok::WholeNumber)? {
				Some(tail) => format!("{}.{}",
					self.db.get(number.lexeme), self.db.get(tail.lexeme)),
				None => format!("{}.0", self.db.get(number.lexeme))
			};
			// TODO: Maybe we should try to also change the location to include
			// the whole span? This is fine for now...?
			number.lexeme = self.db.put_str(&concat);
			typ = Type::AssumeFloat;
		}

		return Expr::put_numliteral_ok(self.ast, number.location.clone(),
			number,
			self.db.put_type(typ));
	}

	// Expects to be at the first param, after the LeftParen.
	fn expr_call_finish(&mut self, location: SourceLocation, ident: Token, object: Option<ExprId>) -> Result<ExprId> {
		let mut args = Vec::new();

		while !self.at(Tok::RightParen) && !self.is_at_end() {
			args.push(self.expression()?);

			// TODO: Make sure we require a Comma after every param but the
			// last.
			self.match_(Tok::Comma)?;
		}

		expected!(self, Tok::RightParen, "')' after argument list")?;

		// Note: This is handled by expr_prefix() now.
		//if self.match_(Tok::LeftParen)?.is_some() {
		//	todo!("calling the return value of a call");
		//}

		// For any binding that we're doing in the parser, we CANNOT be using
		// our local scope lookups for things bound to objects. Our current
		// scope is totally unrelated to that scope.
		let lookup = match object {
			Some(_) => ScopeEntry::None,
			None => self.scope_lookup(ident.lexeme)
		};

		match lookup {
			ScopeEntry::Var(v) => {
				let inner = Expr::put_variable(self.ast, location.clone(), v);
				return Expr::put_valcall_ok(self.ast, self.end(location), inner, args, self.db.sig_unassigned)
			},

			// It may seem in poor taste to have a specific Expr for function
			// calls all throughout the syntax tree. But, the hope is that this
			// makes it easier to generate reasonable code in the common cases.
			// I guess we don't have FunCalls that are on an object right now?
			// Unsure...
			ScopeEntry::Fun(fun) => {
				log::trace!("new bound fun: {} object.is_some(): {}",
					self.db.get(ident.lexeme), object.is_some());
				Expr::put_funcall_ok(self.ast, self.end(location), ident.location, fun, args, None)
			}
			ScopeEntry::Class(_) => {
				semantic_error_with!(self, Error::simple("Can't call a class.".to_string(), self.current.location.clone()));

				// Just return an UnboundCall, as we have a semantic error rather than parse error.
				let call = Expr::put_unboundfuncapture(self.ast, self.end(location.clone()), ident, object);
				Expr::put_valcall_ok(self.ast, self.end(location), call, args, self.db.sig_unassigned)
			}

			ScopeEntry::None => {
				// Special cased functions can be looked up here.
				if ident.lexeme == self.db.str_lerp {
					if args.len() != 3 {
						semantic_error_with!(self, Error::simple("lerp() expects 3 arguments".to_string(), self.current.location.clone()));
						// In this case, just fall through to the unboundfuncapture.
					}
					else {
						return Expr::put_lerp_ok(self.ast, location,
							args[0], args[1], args[2], self.db.types.unassigned);
					}
					
				}

				log::trace!("new unbound fun capture: {} object.is_some(): {}",
					self.db.get(ident.lexeme), object.is_some());

				let call = Expr::put_unboundfuncapture(self.ast, self.end(location.clone()), ident, object);
				Expr::put_valcall_ok(self.ast, location, call, args, self.db.sig_unassigned)
			}
		}
	}

	fn expr_ident(&mut self) -> Result<ExprId> {
		let location = self.start();
		let ident = expected!(self, Tok::Identifier, "identifier")?;

		if self.match_(Tok::LeftParen)?.is_some() {
			// We're not dotted, so we have no object.
			return self.expr_call_finish(location, ident, None);
		}

		// In the future, if we see a dot or a (), we might generate a getter/setter/call.
		// For now, we just generate either a Variable or some unbound name.
		let expr = match self.scope_lookup(ident.lexeme) {
			ScopeEntry::Var(identity) => Expr::mk_variable(ident.location.clone(), identity),
			ScopeEntry::Fun(identity) =>
				Expr::mk_funcapture(ident.location.clone(), ident.location.clone(), identity, self.db.types.unassigned, None),
			ScopeEntry::Class(_) => {
				todo!("what to do when a class is referenced directly. Perhaps a ClassCapture?");
			}
			ScopeEntry::None => Expr::mk_unbound(ident.location.clone(), ident),
		};

		if self.match_(Tok::Equal)?.is_some() {
			let rhs = self.expression()?;

			// Assignment
			match expr {
				Expr::Variable(variable) => 
					return Expr::put_assign_ok(self.ast, self.end(location), variable.location, variable.identity, rhs),
				Expr::FunCapture(_) => {
					let error = Error::simple(format!("Cannot assign to a function"), self.end(location));
					semantic_error_with!(self, error);

					// semantic error, but the parse tree is still basically fine.
					return Ok(self.ast.exprs.push(expr));
				}
				Expr::Unbound(unbound) => {
					return Expr::put_unboundassign_ok(self.ast, self.end(location), unbound.identifier, rhs)
				}
				Expr::Get(_) => {
					panic!("ICE: Tried to assign to Get. This should have generated a Set.");
				}
				_ => unreachable!()
			}
		}

		Ok(self.ast.exprs.push(expr))
	}

	fn expr_print_or_str(&mut self, is_print: bool) -> Result<ExprId> {
		let kind = if is_print { "print" } else { "str" };

		let location = self.start();
		let key_print = expected!(self,
			if is_print { Tok::Print } else { Tok::Str },
			"'{kind}'")?;

		expected_after!(self, Tok::LeftParen, key_print, "'('")?;

		let mut exprs = Vec::new();
		while !self.at(Tok::RightParen) && !self.is_at_end() {
			let expr = self.expression()?;
			exprs.push(expr);
			self.match_(Tok::Comma)?;
		}

		expected!(self, Tok::RightParen, "')' after '{kind}' arguments")?;

		if exprs.is_empty() {
			parse_error!(self, "Expected at least one argument to '{kind}'");
		}

		if is_print {
			Expr::put_print_ok(self.ast, self.end(location), exprs, self.db.types.unassigned)
		}
		else {
			Expr::put_str_ok(self.ast, self.end(location), exprs)
		}
	}

	fn expr_if(&mut self) -> Result<ExprId> {
		let location = self.start();

		expected!(self, Tok::If, "'if'")?;

		let condition = self.expression()?;

		// Like functions, for now we will expect an LBrace and then parse
		// a block. But, we could also add some sort of non-Block ifs later.

		if !self.at(Tok::LeftBrace) {
			got!(self, "'{{' after if condition");
		}

		let then_branch = self.block()?;

		// Now we are at the point where there might be an else.
		let else_branch = if self.match_(Tok::Else)?.is_some() {
			// If there's an immediate 'if', then parse another if/else, and
			// make that our else branch.
			if self.at(Tok::If) {
				Some(self.expr_if()?)
			}
			else {
				if !self.at(Tok::LeftBrace) {
					// Now we have the same "left brace or something" conundrum.
					got!(self, "'{{' or 'if' after 'else'");
				}
				// Else branch is a block.
				Some(self.block()?)
			}
		} else { None };

		Expr::put_if_ok(self.ast, self.end(location),
			condition, 
			then_branch,
			else_branch,
			self.db.types.unassigned
		)
	}

	fn expr_loop(&mut self) -> Result<ExprId> {
		let location = self.start();

		expected!(self, Tok::Loop, "'loop'")?;

		// Like functions, for now we will expect an LBrace and then parse
		// a block. But, we could also add some sort of non-Block ifs later.

		if !self.at(Tok::LeftBrace) {
			got!(self, "'{{' after loop keyword");
		}

		let inner = self.block()?;

		// Loops are infinite (i.e. Never) until proven otherwise...
		Expr::put_loop_ok(self.ast, self.end(location), inner, self.db.types.bottom, Vec::new())
	}

	fn expr_while(&mut self) -> Result<ExprId> {
		let location = self.start();

		expected!(self, Tok::While, "'while'")?;

		let condition = self.expression()?;

		if !self.at(Tok::LeftBrace) {
			got!(self, "'{{' after while condition");
		}
		let inner = self.block()?;

		return Expr::put_whileloop_ok(self.ast, self.end(location), condition, inner,
			self.db.types.unassigned, Vec::new())
	}

	fn expr_for(&mut self) -> Result<ExprId> {
		let location = self.start();
		let key_for = expected!(self, Tok::For, "'for'")?;

		let name = expected_after!(self, Tok::Identifier, key_for,
			"variable name")?;

		let mut typ = self.db.types.unassigned;
		let mut has_explicit_type = false;

		if self.match_(Tok::Colon)?.is_some() {
			typ = self.typ()?;
			has_explicit_type = true;
		}

		// TODO: This should be after the typ if we see a type declaration...
		expected_after!(self, Tok::In, name, "'in' in for loop")?;

		let iterable = self.expression()?;

		if !self.at(Tok::LeftBrace) {
			got!(self, "'{{' after for loop iterable");
		}

		// Note that the var is added to the scope AFTER it is created, so it
		// by nature can't refer to itself.
		//
		// For for loops, the variable must be in scope for the block, but not
		// for the iterable.
		let name_str = name.lexeme;
		let name_loc = name.location.clone();

		// Push a scope for the for loop. This prevents the variable for the initializer
		// leaking into the surrounding scope.
		self.push_scope();

		// When we create variables, don't set the class yet, as we don't
		// know what it is -- we wire it back in once we're done parsing a 
		// class.
		//
		// TODO: For classes, support variables that don't have an initializer?
		let identity = self.db.new_var(name.lexeme, typ, None, None, None, name.location, 
			// Currently, the for loop variable can't have a doc comment?
			// This could be changed.
			None);
		self.scope_put_entry(name_str, ScopeEntry::Var(identity));

		let inner = self.block()?;
		self.pop_scope();

		// eprintln!("-- trace parser: {}:[{}] var '{}'", name.location.offset, name.location.length, self.db.get(name.lexeme));
		
		return Expr::put_forloop_ok(self.ast, self.end(location), name_loc, identity, iterable, has_explicit_type,
			inner);
	}

	fn expr_prefix_callable(&mut self) -> Result<ExprId> {
		match self.peek_typ() {
			Tok::LeftBrace => self.block(),

			Tok::LeftParen => {
				let begin = self.start();
				// Eat left paren
				self.advance()?;
				// Inner expression
				let inner = self.expression()?;

				// Tuple
				if self.at(Tok::Comma) {
					let mut inner = vec![inner];

					self.advance()?;

					while !self.at(Tok::RightParen) && !self.is_at_end() {
						inner.push(self.expression()?);
						self.eat_comma(Tok::RightParen)?;
					}

					expected!(self, Tok::RightParen, "')' after tuple items")?;

					return Expr::put_maketuple_ok(self.ast, self.end(begin), inner, self.db.types.unassigned);
				}

				// Expect right paren after expression
				expected!(self, Tok::RightParen, "')' after parenthesized expression")?;
				Ok(inner)
			}

			Tok::Identifier => self.expr_ident(),

			Tok::If => self.expr_if(),
			Tok::Loop => self.expr_loop(),
			Tok::While => self.expr_while(),
			Tok::For => self.expr_for(),

			Tok::StringSimple => {
				let lit = self.advance()?;
				let id = self.db.put_str_const_simple(self.db.get(lit.lexeme));
				// TODO: Make sure the contents of the string literal are
				// what we expect...
				Expr::put_strliteral_ok(self.ast, lit.location, id)
			}

			Tok::Fun => {
				let fun = self.fun_declaration(false)?;
				return Ok(self.ast.exprs.push(Expr::FunDeclare(fun)));
			}

			_ => unreachable!()
		}
	}

	fn eat_comma(&mut self, terminator: Tok) -> Result<()> {
		if self.at(terminator) { return Ok(()); }
		if self.is_at_end() { return Ok(()); }
		if self.match_(Tok::Comma)?.is_some() { return Ok(()); }

		got!(self, "','");
	}

	/// Parses a 'new' expression, e.g. new Example {}
	fn new_(&mut self) -> Result<ExprId> {
		let location = self.start();

		let key_new = expected!(self, Tok::New, "'new'")?;
		let name = expected_after!(self, Tok::Identifier, key_new, "class name after 'new'")?;

		let mut initializers = Vec::new();

		expected!(self, Tok::LeftBrace, "'{{' in 'new' expression")?;

		while !self.at(Tok::RightBrace) && !self.is_at_end() {
			let location = self.start();
			let ident = expected!(self, Tok::Identifier, "identifier inside 'new' block")?;
			expected_after!(self, Tok::Colon, name, "':' after member name")?;

			let value = self.expression()?;
			initializers.push(NewInitElem { var: self.db.var_unassigned, ident, value, location: self.end(location) });

			self.eat_comma(Tok::RightBrace)?;
		}
		// TODO: Parse inner arguments, etc.
		expected!(self, Tok::RightBrace, "'}}' in 'new' expression")?;

		Expr::put_new_ok(self.ast, self.end(location), name, 
			self.db.class_unassigned,
			self.db.types.unassigned,
			initializers)
	}

	fn array_literal(&mut self) -> Result<ExprId> {
		let location = self.start();
		let _lbracket = self.advance()?;

		let mut values = Vec::new();
		while !self.at(Tok::RightSquare) && !self.is_at_end() {
			let value = self.expression()?;
			values.push(value);

			self.eat_comma(Tok::RightSquare)?;
		}

		expected!(self, Tok::RightSquare, "']' at end of array literal")?;

		Expr::put_arraylit_ok(self.ast, self.end(location), values, self.db.types.unassigned, self.db.types.unassigned)
	}

	fn expr_prefix(&mut self) -> Result<ExprId> {
		match self.peek_typ() {
			Tok::LeftBrace | Tok::LeftParen | Tok::Identifier | Tok::If | Tok::Loop | Tok::While | Tok::For | Tok::Fun | Tok::StringSimple => {
				let location = self.start();
				let mut inner = self.expr_prefix_callable()?;

				while self.at(Tok::LeftParen) || self.at(Tok::LeftSquare) || self.at(Tok::Dot) {
					while self.match_(Tok::LeftParen)?.is_some() {
						// Parse args
						let mut args = Vec::new();

						while !self.at(Tok::RightParen) && !self.is_at_end() {
							args.push(self.expression()?);

							// TODO: Make sure we require a Comma after every param but the
							// last.
							self.match_(Tok::Comma)?;
						}

						expected!(self, Tok::RightParen, "')' after argument list")?;
						inner = Expr::put_valcall(self.ast, self.end(location.clone()), inner, args, self.db.sig_unassigned);
					}
					while self.match_(Tok::LeftSquare)?.is_some() {
						// TODO: Can the index take multiple args?
						let index = self.expression()?;
						expected!(self, Tok::RightSquare, "']' after index expression")?;

						if self.match_(Tok::Equal)?.is_some() {
							let rhs = self.expression()?;
							// TODO: Should this be moved to expr_ident as well...????????

							// Return out of the loop--once we see an equals, we can't keep
							// consuming more () [].
							return Expr::put_setindex_ok(self.ast, self.end(location),
								inner,
								index,
								self.db.types.unassigned,
								rhs);
						}

						inner = Expr::put_index(self.ast, self.end(location.clone()), inner, index, self.db.types.unassigned);
					}
					while self.match_(Tok::Dot)?.is_some() {
						// For get expressions, we can have '.0' and so forth
						// for tuples.
						if !self.at(Tok::Identifier) && !self.at(Tok::WholeNumber) {
							got!(self, "Expected identifier after '.'");
						}
						let identifier = self.advance()?; //expected!(self, Tok::Identifier, "identifier after '.'")?;

						// TODO: Do we want to move this logic into expr_ident to go
						// with the other ones?
						if self.match_(Tok::Equal)?.is_some() {
							let value = self.expression()?;
							return Expr::put_set_ok(self.ast, self.end(location), identifier, inner, self.db.var_unassigned, value);
						}
						// Function calls are mutually exclusive with assignment.
						//
						// An assignment would be like:
						// object.thing() = 5;  or object.thing() = new Thing {};
						// But this doesn't make sense, because in either case we're
						// basically creating a new temporary that isn't really an lvalue.
						//
						// So function calls are distinct from assignments.
						// 
						// Same logic as above with arrays--we return early
						// if we end up making an assignment.
						else if self.match_(Tok::LeftParen)?.is_some() {
							// We have to finish the call right now because
							// it is a call on this particular idenitifer, not
							// really a call on the previous property.
							//
							// (Although, we could make that work too).
							inner = self.expr_call_finish(location.clone(), identifier, Some(inner))?;
						}
						else {
							inner = Expr::put_get(self.ast, self.end(location.clone()), identifier, inner, self.db.var_unassigned);
						}
					}
				}

				return Ok(inner);
			}

			Tok::DecimalNumber | Tok::WholeNumber => {
				self.number()
			},

			Tok::Print => self.expr_print_or_str(true),
			Tok::Str => self.expr_print_or_str(false),

			Tok::Return => {
				let location = self.start();
				self.advance()?;
				// If there's an immediate Semicolon, Comma, RParen, RBrace, or RBracket,
				// it's an empty return.
				//
				// (Are there any other valid terminating tokens for expressions?
				// e.g. should 'return and 5' be validly parsed as an and between
				// a  'return' and a '5'? Seems pretty wrong. But, there might be
				// other legitimate tokens I missed.)
				match self.peek_typ() {
					Tok::Semicolon | Tok::Comma | Tok::RightParen | Tok::RightBrace | Tok::RightSquare => {
						// The equivalent of self.match_ is we advance now. However,
						// we actually don't want to consume the token.
						return Expr::put_return_ok(self.ast, self.end(location), None);
					},
					_ => {}
				}

				let inner = self.expression()?;
				expected!(self, Tok::Semicolon, "';' after return value")?;
				Expr::put_return_ok(self.ast, self.end(location), Some(inner))
			}

			Tok::Break => {
				let location = self.start();
				self.advance()?;
				// If there's an immediate Semicolon, it's an empty break.
				if self.match_(Tok::Semicolon)?.is_some() {
					return Expr::put_break_ok(self.ast, self.end(location), None);
				}

				let inner = self.expression()?;
				expected!(self, Tok::Semicolon, "';' after break value")?;
				Expr::put_break_ok(self.ast, self.end(location), Some(inner))
			}

			Tok::Continue => {
				let location = self.start();
				self.advance()?;
				Expr::put_continue_ok(self.ast, self.end(location))
			}

			Tok::True => {
				let location = self.advance()?.location;
				Expr::put_boolliteral_ok(self.ast, location, true)
			}
			Tok::False => {
				let location = self.advance()?.location;
				Expr::put_boolliteral_ok(self.ast, location, false)
			}

			Tok::New => {
				self.new_()
			}

			Tok::KeySelf => {
				if self.in_member_initializer {
					// TODO: Come up with good terminology for how this works.
					semantic_error_with!(self,
						Error::simple("Cannot use 'self' in direct member initializer.".into(),
						self.current.location.clone())
					);
				}
				let location = self.advance()?.location;
				Expr::put_selfval_ok(self.ast, location, self.db.types.unassigned)
			}

			Tok::LeftSquare => {
				self.array_literal()
			}

			Tok::Nil => {
				let tok = expected!(self, Tok::Nil, "'nil'")?;
				Expr::put_makesumtype_ok(self.ast, tok.location, self.db.put_type(Type::Option(self.db.types.unassigned)))
			}

			Tok::Plus | Tok::Minus => {
				let location = self.start();
				let op = self.advance()?;

				let inner = self.expression()?;
				Expr::put_unary_ok(self.ast, self.end(location), op.typ, inner, self.db.types.unassigned)
			}

			_ => {
				got!(self, "Expected expression")
			}
		}
	}

	fn peek_precedence(&self) -> (u32, u32) {
		// Note: This matches up with expr_infix().
		// If (a, b) a < b this operator is left-associative, else right-associative.
		match self.peek_typ() {
			// `else` has very low precedence.
			Tok::Else => (1, 2),
			// Or has lower precedence than And.
			Tok::Or => (3, 4),
			Tok::And => (5, 6),
			Tok::Less | Tok::LessEqual | Tok::Greater | 
				Tok::GreaterEqual | Tok::EqualEqual | Tok::BangEqual => (7, 8),

			// The range-making operators should come below math so that you
			// can do e.g. 1 + 2..3 * 4
			Tok::DotDot | Tok::EqualDotDotEqual |
				Tok::DotDotEqual | Tok::EqualDotDot => (9, 10),

			Tok::Plus | Tok::Minus => (11, 12),
			Tok::Star | Tok::Slash => (13, 14),

			Tok::Dot => (15, 16),

			// Any other tokens should not be parsed as infix.
			_ => (0, 0)
		}
	}

	fn expr_infix(&mut self, lhs: ExprId) -> Result<ExprId> {
		// TODO: Pass location downwards so that we get the correct value for
		// infix operators.
		let location = self.start();

		// We want to bind rightward to any expressions that left-associate
		// towards us, so we use the right-hand precedence.
		let cur_prec = self.peek_precedence().1;

		match self.peek_typ() {
			// Option else-expression
			Tok::Else => {
				// TODO: Maybe we do want this 'op' for error reporting
				let _op = self.advance()?;
				let rhs = self.expr_precedence(cur_prec)?;
				return Expr::put_optionelse_ok(self.ast, self.end(location), lhs, rhs, self.db.types.unassigned);
			}

			// Binary expressions
			Tok::Plus | Tok::Minus | Tok::Star | Tok::Slash => {
				let op = self.advance()?;
				let rhs = self.expr_precedence(cur_prec)?;
				return Expr::put_binary_ok(self.ast, self.end(location), op.typ, lhs, rhs, self.db.types.unassigned);
			},

			typ @ (Tok::DotDot | Tok::DotDotEqual | Tok::EqualDotDot | Tok::EqualDotDotEqual) => {
				let _op = self.advance()?;
				let rhs = self.expr_precedence(cur_prec)?;

				let left_end = match typ {
					Tok::DotDot => RangeEnd::Inclusive,
					Tok::DotDotEqual => RangeEnd::Inclusive,
					// Oops.
					Tok::EqualDotDot => RangeEnd::Inclusive,
					Tok::EqualDotDotEqual => RangeEnd::Inclusive,
					_ => unreachable!()
				};

				let right_end = match typ {
					Tok::DotDot => RangeEnd::Exclusive,
					Tok::DotDotEqual => RangeEnd::Inclusive,
					Tok::EqualDotDot => RangeEnd::Exclusive,
					Tok::EqualDotDotEqual => RangeEnd::Inclusive,
					_ => unreachable!()
				};

				// Note that for the unbounded ranges, we will need
				// both AST support and parser support. The AST will need
				// to have e.g. Optional exprs, and then the parser will need
				// to be able to find terminating expressions on the RHS
				// of a .., e.g. ..), ..], ..}, ..;, and so forth.

				return Expr::put_makerange_ok(self.ast, self.end(location),
					 lhs, rhs, left_end, right_end, self.db.types.unassigned);
			}

			Tok::Less | Tok::LessEqual | Tok::Greater | 
				Tok::GreaterEqual | Tok::EqualEqual | Tok::BangEqual => {
				let op = self.advance()?;
				let rhs = self.expr_precedence(cur_prec)?;
				return Expr::put_comparison_ok(self.ast, self.end(location), op.typ, lhs, rhs, self.db.types.unassigned);
			}

			Tok::And | Tok::Or => {
				let op = self.advance()?;
				let rhs = self.expr_precedence(cur_prec)?;
				return Expr::put_logical_ok(self.ast, self.end(location), op.typ, lhs, rhs);
			}

			// TODO: Deduplicate this with the expr_prefix stuff..?
			Tok::Dot => {
				let _op = self.advance()?;
				// TODO: Check number tokens for being simple, e.g. not something
				// like 0xff or 1234i32 (if we have postfixes at some point)
				if !self.at(Tok::Identifier) && !self.at(Tok::WholeNumber) {
					got!(self, "Expected identifier after '.'");
				}
				let identifier = self.advance()?; //expected!(self, Tok::Identifier, "identifier after '.'")?;

				// TODO: Do we want to move this logic into expr_ident to go
				// with the other ones?
				if self.match_(Tok::Equal)?.is_some() {
					let value = self.expression()?;
					return Expr::put_set_ok(self.ast, self.end(location), identifier, lhs, self.db.var_unassigned, value);
				}
				// Function calls are mutually exclusive with assignment.
				//
				// An assignment would be like:
				// object.thing() = 5;  or object.thing() = new Thing {};
				// But this doesn't make sense, because in either case we're
				// basically creating a new temporary that isn't really an lvalue.
				//
				// So function calls are distinct from assignments.
				// 
				// Same logic as above with arrays--we return early
				// if we end up making an assignment.
				else if self.match_(Tok::LeftParen)?.is_some() {
					// We have to finish the call right now because
					// it is a call on this particular idenitifer, not
					// really a call on the previous property.
					//
					// (Although, we could make that work too).
					return self.expr_call_finish(location.clone(), identifier, Some(lhs));
				}
				else {
					return Expr::put_get_ok(self.ast, self.end(location.clone()), identifier, lhs, self.db.var_unassigned);
				}
			}

			// We should never call expr_infix() with an invalid operator,
			// because we have to go through the peek_precedence() table to
			// get here.
			_ => unreachable!()
		}
	}

	fn expr_precedence(&mut self, precedence: u32) -> Result<ExprId> {
		let mut expr = self.expr_prefix()?;

		// Our precedence is coming from the right of the previous expr, so we compare to the left-hand
		// side precdence.
		while precedence < self.peek_precedence().0 {
			expr = self.expr_infix(expr)?;
		}

		Ok(expr)
	}

	fn expression(&mut self) -> Result<ExprId> {
		self.expr_precedence(0)
	}

	fn typ(&mut self) -> Result<TypId> {
		// TODO: Consider making ?Ty the option type, not  Ty?. This does make
		// parsing it slightly more natural because it is just, see ?, then
		// call typ() again, and we don't need this typ_prefix function.

		let typ = self.typ_prefix()?;
		if self.match_(Tok::Question)?.is_some() {
			return Ok(self.db.put_type(Type::Option(typ)));
		}
		Ok(typ)
	}

	fn typ_prefix(&mut self) -> Result<TypId> {
		let tok = self.advance()?;
		Ok(match tok.typ {
			Tok::Identifier => {
				// TODO: Maybe another lookup table similar to keywords..?
				if tok.lexeme == self.db.put_str("int") {
					return Ok(self.db.types.int)
				}
				if tok.lexeme == self.db.put_str("float") {
					return Ok(self.db.types.float)
				}
				if tok.lexeme == self.db.put_str("bool") {
					return Ok(self.db.types.bool)
				}

				// TODO: SHould these also be resolved at binding?
				if tok.lexeme == self.db.put_str("vec2") {
					return Ok(self.db.types.vec2);
				}
				if tok.lexeme == self.db.put_str("vec3") {
					return Ok(self.db.types.vec3);
				}
				if tok.lexeme == self.db.put_str("vec4") {
					return Ok(self.db.types.vec4);
				}
				if tok.lexeme == self.db.put_str("vec2i") {
					return Ok(self.db.types.vec2i);
				}
				if tok.lexeme == self.db.put_str("vec3i") {
					return Ok(self.db.types.vec3i);
				}
				if tok.lexeme == self.db.put_str("vec4i") {
					return Ok(self.db.types.vec4i);
				}

				// TODO: These names should probably be resolved at the binding
				// pass, otherwise we cannot shadow them...
				if tok.lexeme == self.db.put_str("StrBuf") {
					return Ok(self.db.types.str_buf)
				}
				if tok.lexeme == self.db.put_str("Str") {
					return Ok(self.db.types.str)
				}
				if tok.lexeme == self.db.put_str("StrConst") {
					return Ok(self.db.types.str_const)
				}
				
				if tok.lexeme == self.db.put_str("Array") {
					expected!(self, Tok::LeftSquare, "'[' after 'Array'")?;

					let inner = self.typ()?;

					expected!(self, Tok::RightSquare, "']' after inner type")?;

					return Ok(self.db.put_type(Type::ArrayOf(inner)));
				}

				if tok.lexeme == self.db.put_str("DynArray") {
					expected!(self, Tok::LeftSquare, "'[' after 'DynArray'")?;

					let inner = self.typ()?;

					expected!(self, Tok::RightSquare, "']' after inner type")?;

					let arr_ty = self.db.put_type(Type::ArrayOf(inner));
					return Ok(self.db.put_type(Type::DynArrayOf(inner, arr_ty)));
				}

				if self.range_types.is_any(tok.lexeme) {
					expected!(self, Tok::LeftSquare, "'[' after '{}'", self.db.get(tok.lexeme))?;

					let inner = self.typ()?;

					expected!(self, Tok::RightSquare, "']' after inner type")?;

					return Ok(self.db.put_type(self.range_types.into_type(tok.lexeme, inner)));
				}

				self.db.put_type(Type::UnboundIdent(tok.lexeme))
			},

			Tok::Fun => {
				// Sig. Note that we still need to support actually re-visiting
				// sigs to resolve the names...
				//
				// Also some syntax: fun() is equivalent to fun() -> void
				let mut sig = Sig { parameters: vec![], return_type: self.db.types.void };
				
				// fun* means a "raw" function.
				let raw = self.match_(Tok::Star)?.is_some();

				expected!(self, Tok::LeftParen, "'(' after 'fun' in type name")?;

				while !self.at(Tok::RightParen) && !self.is_at_end() {
					let next_ty = self.typ()?;

					sig.parameters.push(next_ty);

					// TODO: Expect comma or rightparen
					self.match_(Tok::Comma)?;
				}

				expected!(self, Tok::RightParen, "')' after parameter list for fun type")?;

				if self.match_(Tok::LeftArrow)?.is_some() {
					sig.return_type = self.typ()?;
				}

				// TODO: put_sig REALLY should not take a reference, as
				// we have not used that once.
				let sig = self.db.put_sig(&sig);
				
				if raw { self.db.put_type(Type::FunRaw(sig)) } else { self.db.put_type(Type::Fun(sig)) }
			}

			Tok::LeftParen => {
				let mut inner = Vec::new();

				while !self.at(Tok::RightParen) && !self.is_at_end() {
					let next_ty = self.typ()?;

					inner.push(next_ty);

					self.eat_comma(Tok::RightParen)?;
				}

				// Unlike for MakeTuple, we don't care about whether a tuple
				// has a (,) because it's unambiguous.
				expected!(self, Tok::RightParen, "')' after tuple type name")?;

				self.db.put_type(Type::Tuple(Arc::from(inner)))
			}

			// More type syntax to come...

			_ => {
				parse_error!(self, "Expected type, got {}", self.db.get(tok.lexeme));
				return Err(ParseErr::SyntaxErr)
			}
		})
	}

	fn var_declaration(&mut self, require_initializer: bool) -> Result<Declare> {
		let doc_comment = self.get_doc_comment();
		let location = self.start();
		let key_var = expected!(self, Tok::Var, "'var''")?;

		let name = expected_after!(self, Tok::Identifier, key_var,
			"variable name")?;

		let mut typ = self.db.types.unassigned;
		let mut has_explicit_type = false;

		if self.match_(Tok::Colon)?.is_some() {
			typ = self.typ()?;
			has_explicit_type = true;
		}

		// We are always *allowed* to have an initializer, but it is *required*
		// for local variables.
		let initializer = if require_initializer || self.at(Tok::Equal) {
			expected_after!(self, Tok::Equal, name, "'=' in declaration")?;

			Some(self.expression()?)
		}
		else {
			None
		};

		// We cannot have a variable without an initializer and without an
		// explicit type.
		//
		// This could technically be relaxed with global type inference or
		// whatever, but I think that would be bad design for my purposes.
		if initializer.is_none() && !has_explicit_type {
			semantic_error_with!(self, Error::simple(
				"Variable without an initializer must have an explicit type annotation".into(),
				self.current.location.clone()
			));
		}

		expected!(self, Tok::Semicolon, "';' after variable declaration")?;

		// eprintln!("-- trace parser: {}:[{}] var '{}'", name.location.offset, name.location.length, self.db.get(name.lexeme));
		
		let name_str = name.lexeme;
		let name_loc = name.location.clone();
		// When we create variables, don't set the class yet, as we don't
		// know what it is -- we wire it back in once we're done parsing a 
		// class.
		//
		// TODO: For classes, support variables that don't have an initializer?
		let identity = self.db.new_var(name.lexeme,
			typ, None, None,
			initializer, name.location,
			doc_comment);

		// Note that the var is added to the scope AFTER it is created, so it
		// by nature can't refer to itself.
		self.scope_put_entry(name_str, ScopeEntry::Var(identity));

		return Stmt::new_declare_ok(self.end(location), name_loc, identity, initializer, has_explicit_type);
	}

	fn block(&mut self) -> Result<ExprId> {
		let location = self.start();
		expected!(self, Tok::LeftBrace, "'{{' at beginning of block")?;

		let mut stmts = Vec::new();

		self.push_scope();

		let mut in_err_mode = false;

		while !self.at(Tok::RightBrace) && !self.is_at_end() {
			let err = self.stmt();
			match err {
				Ok(stmt) => {
					stmts.push(stmt);
					// Reset in_err_mode whenever we successfully parse something.
					in_err_mode = false;
				}
				Err(ParseErr::IoErr(err)) => return Err(ParseErr::IoErr(err)),
				Err(ParseErr::SyntaxErr) => {
					// Inside a block is essentially a synchronization point.
					// Keep going until we see a '}'.

					// Note that we have to start advance()ing ourselves if
					// we are e.g. somewhere that is simply not a valid statement
					// starter. But, in case the next token *is* the start of
					// a valid statement, such as in:
					//
					// var x = 
					// var y = 30;
					//
					// We do want to parse it correctly. So don't start advancing
					// until we've already set in_err_mode to true.

					// TODO: For this to work properly, we *also* need to disable
					// reporting further errors while we're in_err_mode. This
					// is because we'll just get duplicate errors like this.

					if in_err_mode { self.advance()?; }
					in_err_mode = true;
				}
			}
		}

		self.pop_scope();

		expected!(self, Tok::RightBrace, "'}}' at end of block")?;

		// Note: This needs to start out as Bottom in the case that
		// it ends up actually being Bottom, in an expression.
		//
		// If it isn't in an expression, it doesn't matter that it's bottom..
		// while if it IS in an expression, it will either be bottom, or it will
		// be something else.
		//
		// Essentially, if we assign Void, then it will stay void even if the
		// last statement is return; because bottom can be assigned to void.
		//
		// We may want to consider simply deleting the Void type.
		Expr::put_block_ok(self.ast, self.end(location), stmts, self.db.types.unassigned)
	}

	fn stmt(&mut self) -> Result<StmtId> {
		let location = self.start();
		match self.peek_typ() {
			Tok::Var => {
				// Var declarations in general require initializers
				let inner = self.var_declaration(true)?;
				Ok(self.ast.stmts.push(Stmt::Declare(inner)))
			},
			_ => {
				let inner = self.expression()?;

				let mut expect_semicolon = match self.ast.exprs.get(inner).as_ref() {
					// If the inner expression is a block or a similar "block-like"
					// thing, then we don't need a semicolon.
					Expr::Block(_) | Expr::If(_) | Expr::Loop(_) | Expr::WhileLoop(_) | Expr::ForLoop(_) => false,
					Expr::FunDeclare(declare) => {
						// Named function declarations don't need a semicolon.
						// Lambda ones are more expression-like, so they do..?
						!self.db.get(declare.identity).name.is_some()
					},
					_ => true,
				};

				// If we're the last statement in a { } block, then we don't need
				// a semicolon.
				if self.at(Tok::RightBrace) { expect_semicolon = false; }
				
				if expect_semicolon {
					expected!(self, Tok::Semicolon, "';' after statement expression")?;
				}
				else {
					// Still consume a semicolon if needed.
					self.match_(Tok::Semicolon)?;
				}
				Stmt::put_expression_ok(self.ast, self.end(location), inner)
			}
		}
	}

	fn parameter(&mut self) -> Result<VarId> {
		let name = expected!(self, Tok::Identifier, "parameter name")?;
		expected!(self, Tok::Colon, "':' after parameter name")?;
		let typ = self.typ()?;

		let name_str = name.lexeme;

		let identity = self.db.new_var(name.lexeme, typ, None, None, None,
			name.location,
			// Currenlty, doc comments are not supported for parameters.
			None);
		self.scope_put_entry(name_str, ScopeEntry::Var(identity));

		Ok(identity)
	}

	fn fun_declaration(&mut self, require_name: bool) -> Result<FunDeclare> {
		let doc_comment = self.get_doc_comment();
		let location = self.start();
		let _key_fun = expected!(self, Tok::Fun, "'fun'")?;

		let mut name = None;

		let pushed_name =
		if let Some(name_) = self.match_(Tok::Identifier)? {
			let pushed_name = self.push_name(&name_);
			name = Some(name_);
			pushed_name
		}
		else {
			if require_name {
				let error = Error::simple(
					format!("Expected function name after 'fun'"),
					self.current.location.clone()
				);
				semantic_error_with!(self, error);
			}

			self.push_name_anon()
		};

		expected!(self, Tok::LeftParen, "'(' to begin function parameter list")?;

		self.push_scope();

		let mut parameters = vec![];

		while !self.at(Tok::RightParen) && !self.is_at_end() {
			parameters.push(self.parameter()?);

			// NOTE: Right now, this means you can have a trailing comma
			// in a parameter list. That might be fine though -- trailing commas
			// are useful in a lot of places -- maybe we should try it?
			self.match_(Tok::Comma)?;
		}

		expected!(self, Tok::RightParen, "')' after function parameter list")?;

		let fun_location = self.end(location.clone());

		let mut return_type = self.db.types.void;

		if self.match_(Tok::LeftArrow)?.is_some() {
			// Parse return type
			return_type = self.typ()?;
		}

		// For now, the function body MUST be a block. But, we can change it
		// to be a single expression, likely we other syntax, later.

		if !self.at(Tok::LeftBrace) {
			got!(self, "Expected '{{' after function parameter list");
		}
		let value = self.block()?;

		self.pop_scope();

		let name_str = name.as_ref().map(|t| t.lexeme);

		// TODO: Avoid this clone? Also this is entirely for the LSP at the moment.
		let parameters_for_set = parameters.clone();

		// TODO: Maybe make this also take a non-ref for speed?
		let identity = self.db.push(Fun {
			name: name_str,
			parameters,
			return_type,
			sig: self.db.sig_unassigned,
			class: None, // Class is not assigned for now, the class parser will assign it later.
			expression: Some(value),
			location: fun_location, // We always have a location even if we don't have a name

			// TODO: Consider not bothering with doc comments in the compiler,
			// only in the documenter, as they probably add some overhead.
			doc_comment,
		});

		for param in parameters_for_set {
			self.db.get_mut(param).fun = Some(identity);
		}

		// We must pop our pushed_name before we put the function name in the scope.
		self.pop_name(pushed_name);

		// Put the identity in to the current scope. For lexical scoped function
		// names, they can't be used until they're defined...
		// TODO: Do we want to be able to have mutually recursive functions local
		// to a function...?
		if let Some(name_str) = name_str {
			self.scope_put_entry(name_str, ScopeEntry::Fun(identity));

			// TODO: Function names that are nested should be <something>.<something>,
			// so this will work even for methods and other nestedly-named functions.
			// (same for vars)
			if name_str == self.db.put_str("init") {
				if self.db.fun_init.is_some() {
					parse_error!(self, "Function 'init' redefined");
				}
				self.db.fun_init = Some(identity);
			}
		}

		Expr::new_fundeclare_ok(self.end(location), identity, value, self.db.types.unassigned, )
	}

	fn push_name(&mut self, name: &Token) -> usize {
		self.scope_name.push_str(self.db.get(name.lexeme));
		self.scope_name.push('.');
		
		self.db.get(name.lexeme).len() + 1
	}

	fn push_name_anon(&mut self) -> usize {
		// TODO: Right now this won't work because it'll put all anonymous
		// names in effectively the same scope. Instead, we need to figure
		// out a way to essentially forbid anything from looking into an
		// anonymous scope at all. (Although, I suppose this already
		// works for that..?)
		let str = format!("<anon>.");
		self.scope_name.push_str(&str);
		str.len()
	}

	fn pop_name(&mut self, size: usize) {
		self.scope_name.truncate(self.scope_name.len() - size);
	}

	fn get_doc_comment(&mut self) -> Option<Vec<Token>> {
		let doc_comment = std::mem::take(&mut self.prev_doc_comment);
		if !doc_comment.is_empty() {
			return Some(doc_comment);
		}
		return None;
	}

	fn class_declaration(&mut self) -> Result<ClassDeclare> {
		let doc_comment = self.get_doc_comment();
		let location = self.start();
		let key_class = expected!(self, Tok::Class, "'class'")?;

		let name = expected_after!(self, Tok::Identifier, key_class,
			"class name")?;

		let pushed_name = self.push_name(&name);

		expected!(self, Tok::LeftBrace, "'{{' at beginning of class")?;

		let mut declare_funs = Vec::<FunDeclare>::new();
		let mut declare_vars = Vec::<Declare>::new();

		let mut funs = Vec::<FunId>::new();
		let mut vars = Vec::<VarId>::new();

		let mut var_map = FxHashMap::default();
		let mut fun_map = FxHashMap::default();

		let mut mandatory_vars = FxHashSet::default();

		loop {
			match self.peek_typ() {
				Tok::Var => {
					// Disallow 'self' in member initializers.
					let enclosing_in_member = self.in_member_initializer;
					self.in_member_initializer = true;

					// Var declarations in classes do NOT require initializers.
					//
					// If a var doesn't have an initializer, it must be provided
					// when the class is constructed.
					let declare = self.var_declaration(false)?;
					vars.push(declare.identity);
					if declare.value.is_none() {
						// Add any variable without an initializer to the mandatory
						// var map.
						mandatory_vars.insert(declare.identity);
					}
					var_map.insert(self.db.get(declare.identity).name, declare.identity);
					declare_vars.push(declare);

					self.in_member_initializer = enclosing_in_member;
				},
				Tok::Fun => {
					let fun = self.fun_declaration(true)?;
					funs.push(fun.identity);
					// We require name so this must have a name.
					fun_map.insert(*self.db.get(fun.identity).name.as_ref().unwrap(), fun.identity);
					declare_funs.push(fun);
				},
				Tok::Class => {
					todo!("nested class support")
				}
				Tok::RightBrace => {
					break;
				},
				_ => {
					// Error in class.
					got!(self, "Expected 'var', 'const', 'class', or 'fun'");
				}
			}
		}

		expected!(self, Tok::RightBrace, "'}}' at end of class")?;

		let name_str = name.lexeme;

		log::trace!("mandatory var count: {}", mandatory_vars.len());

		let identity = self.db.push(Class {
			name: name_str,
			vars,
			funs,
			var_map,
			fun_map,
			mandatory_vars,
			location: name.location,
			doc_comment,
		});

		for var in &declare_vars {
			self.db.get_mut(var.identity).class = Some(identity);
		}

		for fun in &declare_funs {
			self.db.get_mut(fun.identity).class = Some(identity);
		}

		self.pop_name(pushed_name);

		self.scope_put_entry(name_str, ScopeEntry::Class(identity));

		Stmt::new_classdeclare_ok(self.end(location), identity, declare_funs, declare_vars)
	}

	fn get_source(&self) -> ArenaBorrowMut<'_, Source, SourceId> {
		self.ast.sources.get_mut(self.source_id)
	}

	fn parse_top_level(&mut self) -> Result<()> {
		match self.peek_typ() {
			Tok::Eof => { },

			Tok::Var => {
				// Global variables require initializers.
				let global = self.var_declaration(true)?;
				self.db.globals.push(global.identity);
				self.get_source().module.globals.push(global);
			},

			Tok::Fun => {
				// At the top level, unless preceded by a var .. = , a function
				// must have a name.
				let fun = self.fun_declaration(true)?;
				self.get_source().module.functions.push(fun);
			}

			Tok::Class => {
				let class = self.class_declaration()?;
				self.get_source().module.classes.push(class);
			}

			_ => {
				// Skip the erroneous token, as nothing else will drive
				// parsing forward.
				//
				// TODO: Does this need an in_err_mode too?
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