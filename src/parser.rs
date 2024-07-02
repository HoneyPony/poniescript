use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::io;

use rustc_hash::FxHashMap;

use crate::db::*;

use crate::lexer::*;
use crate::module::Module;

use crate::expr::*;
use crate::source::SourceLocation;
use crate::typ::Type;

#[derive(Clone, Copy)]
enum ScopeEntry {
	Var(VarId),
	Fun(FunId),

	None
}

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

	scopes: Vec<Scope>,
	global_scope: Scope,

	pub had_error: bool,
}

pub enum ParseErr {
	SyntaxErr,
	IoErr(std::io::Error)
}

pub type Result<T> = std::result::Result<T, ParseErr>;

macro_rules! parse_error {
	($parser:ident, $($arg:tt)*) => {
		// For now, just eprintln()... TODO Implement error handling system
		$parser.had_error = true;
		eprintln!($($arg)*); 
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
			global_scope: Scope::new(),

			current,

			had_error: false,
		};

		Ok(parser)
	}

	fn scope_put_var(&mut self, name: StrId, var: VarId) {
		match self.scopes.last_mut() {
			Some(last) => {
				last.map.insert(name, ScopeEntry::Var(var));
			},
			None => {
				self.global_scope.map.insert(name, ScopeEntry::Var(var));
			}
		}
	}

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

	fn save_location(&self) -> SourceLocation {
		return self.current.location.clone();
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
			Tok::DecimalNumber => Type::UnassignedDecimal,
			Tok::WholeNumber => Type::UnassignedNumeric,
			_ => unreachable!()
		};

		return Expr::mk_numliteral_ok(number.location.clone(),
			number,
			self.db.put_type(typ));
	}

	fn expr_ident(&mut self) -> Result<Expr> {
		let ident = expected!(self, Tok::Identifier, "identifier")?;

		// In the future, if we see a dot or a (), we might generate a getter/setter/call.
		// For now, we just generate either a Variable or some unbound name.
		let mut expr = match self.scope_lookup(ident.lexeme) {
			ScopeEntry::Var(identity) => Expr::mk_variable(ident.location, identity),
			ScopeEntry::Fun(_) => todo!(),
			ScopeEntry::None => Expr::mk_unbound(ident.location.clone(), ident),
		};

		if let Some(equal) = self.match_(Tok::Equal)? {
			let rhs = self.expression()?;

			// Assignment
			return match expr {
				Expr::Variable(variable) => 
					Expr::mk_assign_ok(variable.location, variable.identity, rhs),
				Expr::Unbound(_) => todo!(),
				_ => unreachable!()
			}
		}

		Ok(expr)
	}

	fn expr_print(&mut self) -> Result<Expr> {
		let key_print = expected!(self, Tok::Print, "'print'")?;

		expected_after!(self, Tok::LeftParen, key_print, "'('")?;

		let mut exprs = Vec::new();
		while !self.at(Tok::RightParen) && !self.is_at_end() {
			let expr = self.expression()?;
			exprs.push(expr);
			self.match_(Tok::Comma)?;
		}

		expected!(self, Tok::RightParen, "')' after print arguments")?;

		if exprs.is_empty() {
			parse_error!(self, "Expected at least one argument to 'print'");
		}

		Expr::mk_print_ok(key_print.location, exprs, self.db.put_type(Type::Unassigned))
	}

	fn expr_prefix(&mut self) -> Result<Expr> {
		match self.peek_typ() {
			Tok::DecimalNumber | Tok::WholeNumber => {
				self.number()
			},

			Tok::LeftBrace => self.block(),

			Tok::Identifier => self.expr_ident(),

			Tok::Print => self.expr_print(),

			Tok::StringSimple => {
				let lit = self.advance()?;
				let id = self.db.put_str_const_simple(self.db.get(lit.lexeme));
				// TODO: Make sure the contents of the string literal are
				// what we expect...
				Expr::mk_strliteral_ok(lit.location.clone(), id)
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
			Tok::Plus | Tok::Minus => (1, 2),
			Tok::Star | Tok::Slash => (3, 4),

			// Any other tokens should not be parsed as infix.
			_ => (0, 0)
		}
	}

	fn expr_infix(&mut self, lhs: Expr) -> Result<Expr> {
		// We want to bind rightward to any expressions that left-associate
		// towards us, so we use the right-hand precedence.
		let cur_prec = self.peek_precedence().1;

		match self.peek_typ() {
			// Binary expressions
			Tok::Plus | Tok::Minus | Tok::Star | Tok::Slash => {
				let op = self.advance()?;
				let rhs = self.expr_precedence(cur_prec)?;
				return Expr::mk_binary_ok(op.location, op.typ, lhs, rhs, self.db.put_type(Type::Unassigned));
			},

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
					return Ok(self.db.put_type(Type::Int))
				}
				if tok.lexeme == self.db.put_str("float") {
					return Ok(self.db.put_type(Type::Float))
				}

				self.db.put_type(Type::UnboundIdent(tok.lexeme))
			},

			// More type syntax to come...

			_ => {
				parse_error!(self, "Expected type, got {}", self.db.get(tok.lexeme));
				return Err(ParseErr::SyntaxErr)
			}
		})
	}

	fn var_declaration(&mut self) -> Result<Declare> {
		let key_var = expected!(self, Tok::Var, "'var''")?;

		let name = expected_after!(self, Tok::Identifier, key_var,
			"variable name")?;

		let mut typ = self.db.put_type(Type::Unassigned);

		if let Some(colon) = self.match_(Tok::Colon)? {
			typ = self.typ()?;
		}

		// TODO: This should be after the typ if we see a type declaration...
		let equal = expected_after!(self, Tok::Equal, name, "'=' in declaration")?;

		let initializer = self.expression()?;

		expected!(self, Tok::Semicolon, "';' after initializer expression")?;
		
		let name_str = name.lexeme;
		let identity = self.db.new_var(name, typ);

		// Note that the var is added to the scope AFTER it is created, so it
		// by nature can't refer to itself.
		self.scope_put_var(name_str, identity);

		return Stmt::new_declare_ok(equal.location, identity, initializer);
	}

	fn block(&mut self) -> Result<Expr> {
		let lbrace = expected!(self, Tok::LeftBrace, "'{{' at beginning of block")?;

		let mut stmts = Vec::new();

		while !self.at(Tok::RightBrace) && !self.is_at_end() {
			stmts.push(self.stmt()?);
		}

		expected!(self, Tok::RightBrace, "'}}' at end of block");

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
		Expr::mk_block_ok(lbrace.location, stmts, self.db.put_type(Type::Bottom))
	}

	fn stmt(&mut self) -> Result<Stmt> {
		match self.peek_typ() {
			Tok::Return => {
				let key_return = self.advance()?;
				// If there's an immediate Semicolon, it's an empty return.
				if self.match_(Tok::Semicolon)?.is_some() {
					return Stmt::mk_return_ok(key_return.location, None);
				}

				let inner = self.expression()?;
				let semicolon = expected!(self, Tok::Semicolon, "';' after return value")?;
				Stmt::mk_return_ok(key_return.location, Some(inner))
			},
			Tok::Var => {
				Ok(Stmt::Declare(self.var_declaration()?))
			},
			_ => {
				let loc = self.save_location();
				let inner = self.expression()?;
				let semicolon = expected!(self, Tok::Semicolon, "';' after statement expression")?;
				Stmt::mk_expression_ok(loc, inner)
			}
		}
	}

	fn named_fun_declaration(&mut self) -> Result<FunDeclare> {
		let key_fun = expected!(self, Tok::Fun, "'fun'")?;

		let name = expected_after!(self, Tok::Identifier, key_fun,
			"function name")?;

		expected!(self, Tok::LeftParen, "'(' after function name")?;

		let parameters = vec![];

		while !self.at(Tok::RightParen) && !self.is_at_end() {

		}

		expected!(self, Tok::RightParen, "')' after function parameter list")?;

		let mut return_type = self.db.put_type(Type::Void);

		if let Some(arrow) = self.match_(Tok::LeftArrow)? {
			// Parse return type
			return_type = self.typ()?;
		}

		// For now, the function body MUST be a block. But, we can change it
		// to be a single expression, likely we other syntax, later.

		if !self.at(Tok::LeftBrace) {
			got!(self, "Expected '{{' after function parameter list");
		}
		let value = self.block()?;

		let name_str = name.lexeme;

		let identity = self.db.new_id(Fun {
			name,
			parameters,
			return_type
		});

		// TODO: Function names that are nested should be <something>.<something>,
		// so this will work even for methods and other nestedly-named functions.
		// (same for vars)
		if name_str == self.db.put_str("init") {
			if self.db.fun_init.is_some() {
				parse_error!(self, "Function 'init' redefined");
			}
			self.db.fun_init = Some(identity);
		}

		Stmt::new_fundeclare_ok(key_fun.location, identity, value)
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
				let fun = self.named_fun_declaration()?;
				self.module.functions.push(fun);
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