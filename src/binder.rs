use crate::db::*;
use crate::error::Error;
use crate::expr::*;
use crate::lexer::Token;
use crate::module::Module;
use crate::source::SourceLocation;

struct NameChecker {
	buffer: String,
	own_length: usize,
}

impl NameChecker {
	pub fn scoped(previous: &NameChecker, scope: &str) -> Self {
		// We add a dot after the scope, as that's what the names will
		// look like.
		//
		// The previous scope will also have a dot (or be the empty global
		// scope), so we directly push it against our own.
		let buffer = format!("{}{}.", previous.buffer, scope);
		let own_length = buffer.len();

		return NameChecker { buffer, own_length };
	}

	pub fn global() -> Self {
		return NameChecker { buffer: String::new(), own_length: 0 };
	}

	pub fn check(&mut self, db: &Db, name: StrId) -> ScopeEntry {
		// Push the name fragment on to the buffer for checking.
		self.buffer.push_str(db.get(name));
		let result = db.lookup_full_name(&self.buffer);
		self.buffer.truncate(self.own_length);

		result
	}
}

struct Binder<'db> {
	db: &'db mut Db,

	had_error: bool,

	checkers: Vec<NameChecker>,
}

impl<'db> Binder<'db> {
	pub fn new(db: &'db mut Db) -> Self {
		return Binder {
			db,

			had_error: false,

			checkers: Vec::new(),
		}
	}

	fn resolve_unbound(&mut self, ident: StrId, location: SourceLocation) -> Option<Expr> {
		for checker in self.checkers.iter_mut().rev() {
			match checker.check(self.db, ident) {
				ScopeEntry::Var(var) => return Some(Expr::mk_variable(location, var)),
				ScopeEntry::Fun(_) => todo!("resolve unbound fun values to 'that function as a value'"),
				ScopeEntry::None => continue,
			}
		}

		self.db.report_error(Error::simple(
			format!("Unknown identifier '{}'", self.db.get(ident)),
			&location
		));
		
		self.had_error = true;
		None
	}

	fn resolve_unbound_call(&mut self, unbound: &mut UnboundCall) -> Option<Expr> {
		for checker in self.checkers.iter_mut().rev() {
			match checker.check(self.db, unbound.identifier.lexeme) {
				ScopeEntry::Var(_) => todo!("resolve unbound var into a call"),
				ScopeEntry::Fun(fun) =>
					return Some(Expr::mk_funcall(unbound.location.clone(), fun, 
					// TODO: Figure out a better way to get the args out of the UnboundCall
					// then this, as it likely leads to an additional allocation..?
						std::mem::take(&mut unbound.args))),
				ScopeEntry::None => continue,
			}
		}

		None
	}

	fn resolve_expr(&mut self, expr: &mut Expr) -> Option<Expr> {
		match expr {
			// For most expression types, we simply visit each inner expression
			// and then return.

			Expr::Binary(binary) => {
				self.visit_expr(&mut binary.left);
				self.visit_expr(&mut binary.right);
				return None;
			},

			Expr::If(if_) => {
				self.visit_expr(&mut if_.condition);
				self.visit_expr(&mut if_.then_branch);
				if_.else_branch.as_mut().map(|e| self.visit_expr(e));
				None
			}
			
			Expr::Assign(assign) => {
				self.visit_expr(&mut assign.value);
				return None;
			},

			Expr::Block(block) => {
				for stmt in &mut block.stmts {
					self.visit_stmt(stmt);
				}
				return None;
			},
			
			Expr::Print(Print { exprs, .. }) | Expr::Str(Str { exprs, .. }) => {
				for expr in exprs {
					self.visit_expr(expr);
				}
				return None;
			}

			// Nothing to resolve.
			Expr::Variable(_) | Expr::NumLiteral(_) | Expr::StrLiteral(_) | Expr::BoolLiteral(_) => {
				return None;
			}
			
			Expr::Unbound(ident) => {
				// TODO: Do we want to avoid the clone here..?
				self.resolve_unbound(ident.identifier.lexeme, ident.location.clone())
			},

			Expr::FunCall(call) => {
				// Must visit all the arguments of the call
				for arg in &mut call.args {
					self.visit_expr(arg);
				}
				None
			},

			Expr::UnboundCall(unbound) => {
				// Must visit all the arguments of the call, so that they can
				// be bound.
				for arg in &mut unbound.args {
					self.visit_expr(arg);
				}
				self.resolve_unbound_call(unbound)
			}
		}
	}

	fn visit_expr(&mut self, expr: &mut Expr) {
		if let Some(resolved) = self.resolve_expr(expr) {
			// Replace the unbound identifier with the resolved expression.
			*expr = resolved;
		}
	}

	fn visit_stmt(&mut self, stmt: &mut Stmt) {
		match stmt {
			Stmt::Declare(declare) => {
				self.visit_expr(&mut declare.value);
			},
			Stmt::Expression(expr) => {
				self.visit_expr(&mut expr.expression);
			},
			Stmt::FunDeclare(fun_declare) => {
				self.visit_function(fun_declare);
			},
			Stmt::Return(ret) => {
				if let Some(expr) = &mut ret.expression {
					self.visit_expr(expr);
				}
			},
		}
	}

	fn visit_function(&mut self, function: &mut FunDeclare) {
		// TODO: Push my name.
		self.visit_expr(&mut function.value);
	}

	pub fn visit_module(&mut self, module: &mut Module) {
		for global in &mut module.globals {
			self.visit_expr(&mut global.value);
		}

		// Don't add the global() checker until after the globals have been
		// visited. This prevents cyclic references in the globals.
		self.checkers.push(NameChecker::global());

		for fun in &mut module.functions {
			self.visit_function(fun);
		}
	}
}

pub fn bind(db: &mut Db, modules: &mut Vec<Module>) -> bool {
	let mut had_error = false;

	for module in modules {
		// TODO: Run one binder per thread.
		let mut binder = Binder::new(db);
		binder.visit_module(module);

		if binder.had_error { had_error = true; }
	}

	had_error
}