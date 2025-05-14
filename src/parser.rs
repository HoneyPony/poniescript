use std::fs::File;
use std::io;
use std::process::id;

use rustc_hash::FxHashMap;

use crate::db::*;

use crate::lexer::*;
use crate::module::Module;

use crate::expr::*;
use crate::error::Error;
use crate::source::SourceLocation;
use crate::typ::Type;

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

pub struct Parser<'a, 'b> {
	lexer: Lexer,
	module: &'a mut Module,
	db: &'b mut Db,

	current: Token,
	last_location: SourceLocation,

	scopes: Vec<Scope>,
	scope_name: String,
	global_scope: Scope,

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
			&$parser.current.location
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
				&$parser.current.location
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
//    expected!\([^\)]+\)[^?]
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

			scopes: Vec::new(),
			// TODO: Push and pop things from this name
			scope_name: String::new(),
			global_scope: Scope::new(),

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

	fn location_of(&self, entry: &ScopeEntry) -> &SourceLocation {
		match entry {
			ScopeEntry::Var(var) => &self.db.get(*var).name.location,
			ScopeEntry::Fun(fun) => {
				if let Some(name) = &self.db.get(*fun).name {
					&name.location
				}
				else {
					todo!("how do we location_of() for functions without names..?")
				}
			}
			ScopeEntry::Class(class) => {
				&self.db.get(*class).name.location
			}
			ScopeEntry::None => todo!(),
		}
	}

	fn scope_put_entry(&mut self, name: StrId, entry: ScopeEntry) {
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
		let next = self.lexer
			.next_token(self.db)
			.map_err(|err| ParseErr::IoErr(err))?;
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

	fn number(&mut self) -> Result<Expr> {
		let number = self.advance()?; // Eat numerical token... TODO expected! with multiple
		// types..?
		//let number = expected!(self, Tok::Number, "number literal")?;

		let typ = match number.typ {
			Tok::DecimalNumber => Type::AssumeFloat,
			Tok::WholeNumber => Type::AssumeInt,
			_ => unreachable!()
		};

		return Expr::mk_numliteral_ok(number.location.clone(),
			number,
			self.db.put_type(typ));
	}

	// Expects to be at the first param, after the LeftParen.
	fn expr_call_finish(&mut self, location: SourceLocation, ident: Token, object: Option<Expr>) -> Result<Expr> {
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

		match self.scope_lookup(ident.lexeme) {
			ScopeEntry::Var(v) => {
				let inner = Expr::mk_variable(location.clone(), v);
				return Expr::mk_valcall_ok(self.end(location), inner, args, self.db.sig_unassigned)
			},

			// It may seem in poor taste to have a specific Expr for function
			// calls all throughout the syntax tree. But, the hope is that this
			// makes it easier to generate reasonable code in the common cases.
			ScopeEntry::Fun(fun) => Expr::mk_funcall_ok(location, fun, args),

			ScopeEntry::Class(class) => {
				semantic_error_with!(self, Error::simple("Can't call a class.".to_string(), &self.current.location));

				// Just return an UnboundCall, as we have a semantic error rather than parse error.
				let call = Expr::mk_unboundfuncapture(self.end(location.clone()), ident, object);
				Expr::mk_valcall_ok(self.end(location), call, args, self.db.sig_unassigned)
			}

			ScopeEntry::None => {
				let call = Expr::mk_unboundfuncapture(self.end(location.clone()), ident, object);
				Expr::mk_valcall_ok(location, call, args, self.db.sig_unassigned)
			}
		}
	}

	fn expr_ident(&mut self) -> Result<Expr> {
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
				Expr::mk_funcapture(ident.location.clone(), identity, self.db.types.unassigned, None),
			ScopeEntry::Class(_) => {
				todo!("What should happen when you reference a class without anything else? I guess a ClassCapture?");
			}
			ScopeEntry::None => Expr::mk_unbound(ident.location.clone(), ident),
		};

		if self.match_(Tok::Equal)?.is_some() {
			let rhs = self.expression()?;

			// Assignment
			match expr {
				Expr::Variable(variable) => 
					return Expr::mk_assign_ok(self.end(location), variable.identity, rhs),
				Expr::FunCapture(_) => {
					let error = Error::simple(format!("Cannot assign to a function"), &self.end(location));
					semantic_error_with!(self, error);

					// semantic error, but the parse tree is still basically fine.
					return Ok(expr);
				}
				Expr::Unbound(unbound) => {
					return Expr::mk_unboundassign_ok(self.end(location), unbound.identifier, rhs)
				}
				Expr::Get(_) => {
					panic!("omg!");
				}
				_ => unreachable!()
			}
		}

		Ok(expr)
	}

	fn expr_print_or_str(&mut self, is_print: bool) -> Result<Expr> {
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
			Expr::mk_print_ok(self.end(location), exprs, self.db.types.unassigned)
		}
		else {
			Expr::mk_str_ok(self.end(location), exprs)
		}
	}

	fn expr_if(&mut self) -> Result<Expr> {
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

		Expr::mk_if_ok(self.end(location),
			condition, 
			then_branch,
			else_branch,

			// We have to start at bottom, which is a little bit weird.
			self.db.types.bottom
		)
	}

	fn expr_prefix_callable(&mut self) -> Result<Expr> {
		match self.peek_typ() {
			Tok::LeftBrace => self.block(),

			Tok::Identifier => self.expr_ident(),

			Tok::If => self.expr_if(),

			Tok::Fun => {
				let fun = self.fun_declaration(false)?;
				return Ok(Expr::FunDeclare(fun));
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
	fn new_(&mut self) -> Result<Expr> {
		let location = self.start();

		let key_new = expected!(self, Tok::New, "'new'")?;
		let name = expected_after!(self, Tok::Identifier, key_new, "class name after 'new'")?;

		let mut initializers = Vec::new();

		expected!(self, Tok::LeftBrace, "'{{' in 'new' expression");

		while !self.at(Tok::RightBrace) && !self.is_at_end() {
			let location = self.start();
			let ident = expected!(self, Tok::Identifier, "identifier inside 'new' block")?;
			expected_after!(self, Tok::Colon, name, "':' after member name");

			let value = self.expression()?;
			initializers.push(NewInitElem { var: self.db.var_unassigned, ident, value, location: self.end(location) });

			self.eat_comma(Tok::RightBrace)?;
		}
		// TODO: Parse inner arguments, etc.
		expected!(self, Tok::RightBrace, "'}}' in 'new' expression");

		Expr::mk_new_ok(self.end(location), name, 
			self.db.class_unassigned,
			self.db.types.unassigned,
			initializers)
	}

	fn array_literal(&mut self) -> Result<Expr> {
		let location = self.start();
		let lbracket = self.advance()?;

		let mut values = Vec::new();
		while !self.at(Tok::RightSquare) && !self.is_at_end() {
			let value = self.expression()?;
			values.push(value);

			self.eat_comma(Tok::RightSquare)?;
		}

		expected!(self, Tok::RightSquare, "']' at end of array literal");

		Expr::mk_arraylit_ok(self.end(location), values, self.db.types.unassigned, self.db.types.unassigned)
	}

	fn expr_prefix(&mut self) -> Result<Expr> {
		match self.peek_typ() {
			Tok::LeftBrace | Tok::Identifier | Tok::If | Tok::Fun => {
				let location = self.start();
				let mut inner = self.expr_prefix_callable()?;
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
					inner = Expr::mk_valcall(self.end(location.clone()), inner, args, self.db.sig_unassigned);
				}

				return Ok(inner);
			}

			Tok::DecimalNumber | Tok::WholeNumber => {
				self.number()
			},

			Tok::Print => self.expr_print_or_str(true),
			Tok::Str => self.expr_print_or_str(false),

			Tok::True => Expr::mk_boolliteral_ok(self.advance()?.location, true),
			Tok::False => Expr::mk_boolliteral_ok(self.advance()?.location, false),

			Tok::StringSimple => {
				let lit = self.advance()?;
				let id = self.db.put_str_const_simple(self.db.get(lit.lexeme));
				// TODO: Make sure the contents of the string literal are
				// what we expect...
				Expr::mk_strliteral_ok(lit.location, id)
			}

			Tok::New => {
				self.new_()
			}

			Tok::LeftSquare => {
				self.array_literal()
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
			// Or has lower precedence than And.
			Tok::Or => (1, 2),
			Tok::And => (3, 4),
			Tok::Less | Tok::LessEqual | Tok::Greater | Tok::GreaterEqual => (5, 6),

			Tok::Plus | Tok::Minus => (7, 8),
			Tok::Star | Tok::Slash => (9, 10),

			Tok::Dot => (11, 12),

			// Any other tokens should not be parsed as infix.
			_ => (0, 0)
		}
	}

	fn expr_infix(&mut self, lhs: Expr) -> Result<Expr> {
		// TODO: Pass location downwards so that we get the correct value for
		// infix operators.
		let location = self.start();

		// We want to bind rightward to any expressions that left-associate
		// towards us, so we use the right-hand precedence.
		let cur_prec = self.peek_precedence().1;

		match self.peek_typ() {
			// Binary expressions
			Tok::Plus | Tok::Minus | Tok::Star | Tok::Slash => {
				let op = self.advance()?;
				let rhs = self.expr_precedence(cur_prec)?;
				return Expr::mk_binary_ok(self.end(location), op.typ, lhs, rhs, self.db.types.unassigned);
			},

			Tok::Less | Tok::LessEqual | Tok::Greater | Tok::GreaterEqual => {
				let op = self.advance()?;
				let rhs = self.expr_precedence(cur_prec)?;
				return Expr::mk_comparison_ok(self.end(location), op.typ, lhs, rhs, self.db.types.unassigned);
			}

			Tok::And | Tok::Or => {
				let op = self.advance()?;
				let rhs = self.expr_precedence(cur_prec)?;
				return Expr::mk_logical_ok(self.end(location), op.typ, lhs, rhs);
			}

			Tok::Dot => {
				let op = self.advance()?;
				let identifier = expected_after!(self, Tok::Identifier, op, "property name")?;

				// TODO: Do we want to move this logic into expr_ident to go
				// with the other ones?
				if self.match_(Tok::Equal)?.is_some() {
					let value = self.expression()?;
					return Expr::mk_set_ok(self.end(location), identifier, lhs, self.db.var_unassigned, value);
				}
				// Function calls are mutually exclusive with assignment.
				//
				// An assignment would be like:
				// object.thing() = 5;  or object.thing() = new Thing {};
				// But this doesn't make sense, because in either case we're
				// basically creating a new temporary that isn't really an lvalue.
				//
				// So function calls are distinct from assignments.
				else if self.match_(Tok::LeftParen)?.is_some() {
					return self.expr_call_finish(location, identifier, Some(lhs));
				}
				return Expr::mk_get_ok(self.end(location), identifier, lhs, self.db.var_unassigned);
			}

			// We should never call expr_infix() with an invalid operator,
			// because we have to go through the peek_precedence() table to
			// get here.
			_ => unreachable!()
		}
	}

	fn expr_precedence(&mut self, precedence: u32) -> Result<Expr> {
		let mut expr = self.expr_prefix()?;

		// Our precedence is coming from the right of the previous expr, so we compare to the left-hand
		// side precdence.
		while precedence < self.peek_precedence().0 {
			expr = self.expr_infix(expr)?;
		}

		Ok(expr)
	}

	fn expression(&mut self) -> Result<Expr> {
		self.expr_precedence(0)
	}

	fn typ(&mut self) -> Result<TypId> {
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

				expected!(self, Tok::LeftParen, "'(' after 'fun' in type name");

				while !self.at(Tok::RightParen) && !self.is_at_end() {
					let next_ty = self.typ()?;

					sig.parameters.push(next_ty);

					// TODO: Expect comma or rightparen
					self.match_(Tok::Comma)?;
				}

				expected!(self, Tok::RightParen, "')' after parameter list for fun type");

				if self.match_(Tok::LeftArrow)?.is_some() {
					sig.return_type = self.typ()?;
				}

				// TODO: put_sig REALLY should not take a reference, as
				// we have not used that once.
				let sig = self.db.put_sig(&sig);
				
				if raw { self.db.put_type(Type::FunRaw(sig)) } else { self.db.put_type(Type::Fun(sig)) }
			}

			// More type syntax to come...

			_ => {
				parse_error!(self, "Expected type, got {}", self.db.get(tok.lexeme));
				return Err(ParseErr::SyntaxErr)
			}
		})
	}

	fn var_declaration(&mut self) -> Result<Declare> {
		let location = self.start();
		let key_var = expected!(self, Tok::Var, "'var''")?;

		let name = expected_after!(self, Tok::Identifier, key_var,
			"variable name")?;

		let mut typ = self.db.types.unassigned;

		if self.match_(Tok::Colon)?.is_some() {
			typ = self.typ()?;
		}

		// TODO: This should be after the typ if we see a type declaration...
		expected_after!(self, Tok::Equal, name, "'=' in declaration")?;

		let initializer = self.expression()?;

		expected!(self, Tok::Semicolon, "';' after initializer expression")?;
		
		let name_str = name.lexeme;
		// When we create variables, don't set the class yet, as we don't
		// know what it is -- we wire it back in once we're done parsing a 
		// class.
		//
		// TODO: For classes, support variables that don't have an initializer?
		let identity = self.db.new_var(name, typ, None, true);

		// Note that the var is added to the scope AFTER it is created, so it
		// by nature can't refer to itself.
		self.scope_put_entry(name_str, ScopeEntry::Var(identity));

		return Stmt::new_declare_ok(self.end(location), identity, initializer);
	}

	fn block(&mut self) -> Result<Expr> {
		let location = self.start();
		expected!(self, Tok::LeftBrace, "'{{' at beginning of block")?;

		let mut stmts = Vec::new();

		self.push_scope();

		while !self.at(Tok::RightBrace) && !self.is_at_end() {
			stmts.push(self.stmt()?);
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
		Expr::mk_block_ok(self.end(location), stmts, self.db.types.unassigned)
	}

	fn stmt(&mut self) -> Result<Stmt> {
		let location = self.start();
		match self.peek_typ() {
			Tok::Return => {
				self.advance()?;
				// If there's an immediate Semicolon, it's an empty return.
				if self.match_(Tok::Semicolon)?.is_some() {
					return Stmt::mk_return_ok(self.end(location), None);
				}

				let inner = self.expression()?;
				expected!(self, Tok::Semicolon, "';' after return value")?;
				Stmt::mk_return_ok(self.end(location), Some(inner))
			},
			Tok::Var => {
				Ok(Stmt::Declare(self.var_declaration()?))
			},
			_ => {
				let inner = self.expression()?;

				let mut expect_semicolon = match &inner {
					// If the inner expression is a block or a similar "block-like"
					// thing, then we don't need a semicolon.
					Expr::Block(_) | Expr::If(_) => false,
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
				Stmt::mk_expression_ok(self.end(location), inner)
			}
		}
	}

	fn parameter(&mut self) -> Result<VarId> {
		let name = expected!(self, Tok::Identifier, "parameter name")?;
		expected!(self, Tok::Colon, "':' after parameter name")?;
		let typ = self.typ()?;

		let name_str = name.lexeme;

		let identity = self.db.new_var(name, typ, None, false);
		self.scope_put_entry(name_str, ScopeEntry::Var(identity));

		Ok(identity)
	}

	fn fun_declaration(&mut self, require_name: bool) -> Result<FunDeclare> {
		let location = self.start();
		let key_fun = expected!(self, Tok::Fun, "'fun'")?;

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
					&self.current.location
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

		// TODO: Maybe make this also take a non-ref for speed?
		let identity = self.db.new_id(Fun {
			name,
			parameters,
			return_type,
			sig: self.db.sig_unassigned,
			class: None, // Class is not assigned for now, the class parser will assign it later.
		});

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

	fn class_declaration(&mut self) -> Result<ClassDeclare> {
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

		loop {
			match self.peek_typ() {
				Tok::Var => {
					let declare = self.var_declaration()?;
					vars.push(declare.identity);
					var_map.insert(self.db.get(declare.identity).name.lexeme, declare.identity);
					declare_vars.push(declare);
				},
				Tok::Fun => {
					let fun = self.fun_declaration(true)?;
					funs.push(fun.identity);
					// We require name so this must have a name.
					fun_map.insert(self.db.get(fun.identity).name.as_ref().unwrap().lexeme, fun.identity);
					declare_funs.push(fun);
				},
				Tok::Class => {
					todo!("nested classes")
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

		let identity = self.db.new_id(Class {
			name,
			vars,
			funs,
			var_map,
			fun_map
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

	fn parse_top_level(&mut self) -> Result<()> {
		match self.peek_typ() {
			Tok::Eof => { },

			Tok::Var => {
				let global = self.var_declaration()?;
				self.module.globals.push(global);
			},

			Tok::Fun => {
				// At the top level, unless preceded by a var .. = , a function
				// must have a name.
				let fun = self.fun_declaration(true)?;
				self.module.functions.push(fun);
			}

			Tok::Class => {
				let class = self.class_declaration()?;
				self.module.classes.push(class);
			}

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