use std::sync::Arc;

use crate::db::*;
use crate::error::Error;
use crate::expr::*;
use crate::module::Module;
use crate::source::SourceLocation;
use crate::typ::Type;

use crate::arena::IndexCell;

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
	in_class: bool,

	checkers: Vec<NameChecker>,
}

impl<'db> Binder<'db> {
	pub fn new(db: &'db mut Db) -> Self {
		return Binder {
			db,

			had_error: false,
			in_class: false,

			checkers: Vec::new(),
		}
	}

	fn resolve_class_name(&mut self, ident: StrId, location: &SourceLocation) -> Option<ClassId> {
		for checker in self.checkers.iter_mut().rev() {
			match checker.check(self.db, ident) {
				ScopeEntry::Var(_) => break, // TODO: Figure out an ergonomic way to do this.
				ScopeEntry::Fun(_) => break,
				ScopeEntry::Class(class) => {
					return Some(class)
				}
				ScopeEntry::None => continue,
			}
		}

		self.db.report_error(Error::simple(
			format!("Unknown class name '{}'", self.db.get(ident)),
			location.clone()
		));
		
		self.had_error = true;
		None
	}

	/// If we're currently inside a class, gets a new SelfVal; otherwise, returns
	/// None. Useful for resolving AST types that can optionally operate on an
	/// object.
	fn get_selfval(&mut self, ast: &AstProxy, location: SourceLocation) -> Option<ExprId> {
		if self.in_class {
			Some(Expr::push_selfval(ast, location, self.db.types.unassigned))
		}
		else { None }
	}

	fn resolve_unbound(&mut self, ast: &AstProxy, ident: StrId, location: SourceLocation) -> Option<Expr> {
		for checker in self.checkers.iter_mut().rev() {
			match checker.check(self.db, ident) {
				ScopeEntry::Var(var) => return Some(Expr::mk_variable(location, var)),
				ScopeEntry::Fun(fun) => return Some(Expr::mk_funcapture(location.clone(), location.clone(), fun, self.db.types.fun_sig_unassigned, 
					self.get_selfval(ast, location))),
				ScopeEntry::Class(_) => {
					todo!("what to do when we resolve an Unbound into a Class");
				}
				ScopeEntry::None => continue,
			}
		}

		self.db.report_error(Error::simple(
			format!("Unknown identifier '{}'", self.db.get(ident)),
			location.clone()
		));
		
		self.had_error = true;
		None
	}

	fn resolve_unbound_assign(&mut self, ident: StrId, location: SourceLocation, ident_location: SourceLocation, expr: ExprId) -> Option<Expr> {
		for checker in self.checkers.iter_mut().rev() {
			match checker.check(self.db, ident) {
				ScopeEntry::Var(var) => return Some(Expr::mk_assign(location, ident_location, var, expr)),
				ScopeEntry::Fun(_) => {
					self.db.report_error(Error::simple(
						format!("Cannot assign to a function."),
						location.clone()
					));

					self.had_error = true;
					return None;
				},
				ScopeEntry::Class(_) => {
					self.db.report_error(Error::simple(
						format!("Cannot assign to a class."),
						location.clone()
					));

					self.had_error = true;
					return None;
				}
				ScopeEntry::None => continue,
			}
		}

		self.db.report_error(Error::simple(
			format!("Unknown identifier '{}'", self.db.get(ident)),
			location.clone()
		));

		self.had_error = true;
		None
	}

	fn resolve_unbound_funcapture(&mut self, ast: &AstProxy, unbound: &mut UnboundFunCapture) -> Option<Expr> {
		for checker in self.checkers.iter_mut().rev() {
			match checker.check(self.db, unbound.identifier.lexeme) {
				ScopeEntry::Var(v) => {
					return Some(Expr::mk_variable(unbound.location.clone(), v));
				}
				ScopeEntry::Fun(fun) => {
					return Some(Expr::mk_funcapture(unbound.location.clone(), unbound.identifier.location.clone(), fun, self.db.types.unassigned, 
						self.get_selfval(ast, unbound.location.clone())))
				}
				ScopeEntry::Class(_) => {
					self.db.report_error(Error::simple(
						format!("Cannot call a class."),
						unbound.location.clone()
					));

					self.had_error = true;
					return None;
				}
				ScopeEntry::None => continue,
			}
		}

		None
	}

	fn resolve_expr(&mut self, ast: &AstProxy, expr: &mut Expr) -> Option<Expr> {
		// eprintln!("visit {:?}", expr);
		match expr {
			// For most expression types, we simply visit each inner expression
			// and then return.

			Expr::Binary(binary) => {
				self.visit_expr(ast, binary.left);
				self.visit_expr(ast, binary.right);
				return None;
			},

			Expr::Unary(unary) => {
				self.visit_expr(ast, unary.inner);
				return None;
			}

			Expr::Lerp(lerp) => {
				// TODO: Is there some way to make this less tedious?
				self.visit_expr(ast, lerp.from);
				self.visit_expr(ast, lerp.to);
				self.visit_expr(ast, lerp.amount);
				return None;
			}

			Expr::Comparison(compare) => {
				self.visit_expr(ast, compare.left);
				self.visit_expr(ast, compare.right);
				return None;
			},

			Expr::Logical(logical) => {
				self.visit_expr(ast, logical.left);
				self.visit_expr(ast, logical.right);
				return None;
			}

			Expr::If(if_) => {
				self.visit_expr(ast, if_.condition);
				self.visit_expr(ast, if_.then_branch);
				//if_.else_branch.as_mut().map(|e| self.visit_expr(ast, e));
				if let Some(else_b) = if_.else_branch {
					self.visit_expr(ast, else_b);
				}
				None
			}

			Expr::Loop(loop_) => {
				self.visit_expr(ast, loop_.inner);
				None
			}

			Expr::WhileLoop(while_) => {
				self.visit_expr(ast, while_.condition);
				self.visit_expr(ast, while_.inner);
				None
			}

			Expr::ForLoop(for_) => {
				self.visit_expr(ast, for_.inner);
				self.visit_expr(ast, for_.iterator);

				// Anywhere where the parser might generate a Type::UnboundIdent,
				// we need to try resolving that identifier.
				self.visit_var_type(for_.identity);
				None
			}

			Expr::Break(break_) => {
				if let Some(value) = break_.value { self.visit_expr(ast, value); }
				None
			}
			
			Expr::Assign(assign) => {
				self.visit_expr(ast, assign.value);
				return None;
			},

			Expr::Block(block) => {
				for stmt in &block.stmts {
					self.visit_stmt(ast, *stmt);
				}
				return None;
			},
			
			Expr::Print(Print { exprs, .. }) | Expr::Str(Str { exprs, .. }) => {
				for expr in exprs {
					self.visit_expr(ast, *expr);
				}
				return None;
			}

			// Nothing to resolve.
			Expr::Variable(_) | Expr::NumLiteral(_) | Expr::StrLiteral(_) | Expr::BoolLiteral(_) => {
				return None;
			}
			
			Expr::Unbound(ident) => {
				// TODO: Do we want to avoid the clone here..?
				self.resolve_unbound(ast, ident.identifier.lexeme, ident.location.clone())
			},

			Expr::UnboundAssign(assign) => {
				// Important: Must visit the value node too
				self.visit_expr(ast, assign.value);
				// TODO: Do we want to avoid the clone here?
				self.resolve_unbound_assign(assign.identifier.lexeme, assign.location.clone(), assign.identifier.location.clone(), assign.value)
			},

			Expr::FunCall(call) => {
				// Must visit all the arguments of the call
				for arg in &call.args {
					self.visit_expr(ast, *arg);
				}
				None
			},

			Expr::ValCall(call) => {
				self.visit_expr(ast, call.value);
				for arg in &call.args {
					self.visit_expr(ast, *arg);
				}
				None
			},

			// Nothing to visit.
			Expr::FunCapture(_) => None,

			Expr::UnboundFunCapture(unbound) => {
				// If the UnboundFunCapture is on an object, we need to visit
				// that object, and we also can't even try to resolve it yet,
				// as it's bound to an object, not to a scope.
				if let Some(object) = unbound.object {
					self.visit_expr(ast, object);
					None
				}
				else {
					// If it isn't on an object, then it must be capturing
					// a function in the lexical scope, so we have to resolve
					// it lexically.
					self.resolve_unbound_funcapture(ast, unbound)
				}
			},

			Expr::FunDeclare(fun_declare) => {
				self.visit_function(ast, fun_declare);
				return None;
			},

			Expr::New(new) => {
				new.class = self.resolve_class_name(new.identifier.lexeme, &new.location)?;
				// Set the type here, so we don't have to mess with it again.
				new.typ = self.db.put_type(Type::Class(new.class));

				for init in &mut new.initializers {
					self.visit_expr(ast, init.value);
					
					if let Some(id) = self.db.lookup_property(new.typ, init.ident.lexeme) {
						init.var = id;
					} else {
						self.db.report_error(Error::simple(
							format!("Class '{}' has no such property '{}'",
							self.db.repr_class(new.class),
							self.db.get(init.ident.lexeme)),
							init.location.clone()
						));
						
						self.had_error = true;
					};
				}

				return None;
			}
			
			Expr::Get(get) => {
				self.visit_expr(ast, get.lhs);
				
				return None;
			}
			
			Expr::Set(set) => {
				self.visit_expr(ast, set.rhs);
				self.visit_expr(ast, set.lhs);
				return None;
			}

			Expr::SelfVal(_) => {
				return None;
			}
			
			Expr::Undefined(_) => {
				return None;
			}

			Expr::ArrayLit(lit) => {
				for val in &lit.values {
					self.visit_expr(ast, *val);
				}
				return None;
			}

			Expr::Index(index) => {
				self.visit_expr(ast, index.value);
				self.visit_expr(ast, index.index);
				return None;
			}

			Expr::SetIndex(set) => {
				self.visit_expr(ast, set.value);
				self.visit_expr(ast, set.index);
				self.visit_expr(ast, set.rhs);
				return None;
			}

			Expr::MakeTuple(make_tuple) => {
				for expr in &make_tuple.values {
					self.visit_expr(ast, *expr);
				}
				return None;
			}

			Expr::MakeRange(make_range) => {
				self.visit_expr(ast, make_range.left);
				self.visit_expr(ast, make_range.right);
				return None;
			}

			Expr::MakeSumType(_sum) => {
				// Currently nothing to do. This will change...
				return None;
			}

			Expr::OptionElse(optelse) => {
				self.visit_expr(ast, optelse.value);
				self.visit_expr(ast, optelse.otherwise);
				return None;
			}

			Expr::Promote(_) => panic!("ICE: Tried to bind Expr::Promote"),
		}
	}

	fn visit_expr(&mut self, ast: &AstProxy, expr: ExprId) {
		let mut binding = ast.exprs.get_mut(expr);
		if let Some(resolved) = self.resolve_expr(ast, binding.as_mut()) {
			drop(binding);
			// Replace the unbound identifier with the resolved expression.
			*ast.exprs.get_mut(expr) = resolved;
		}
	}

	fn visit_class(&mut self, ast: &AstProxy, class_declare: &mut ClassDeclare) {
		let enclosing_in_class = self.in_class;
		self.in_class = true;

		let class = self.db.get(class_declare.identity);
		let name = self.db.get(class.name);
		let new_scope = NameChecker::scoped(self.checkers.last().expect("class"), name);
		self.checkers.push(new_scope);

		for fun in &mut class_declare.funs {
			self.visit_function(ast, fun);
		}

		for var in &mut class_declare.vars {
			self.visit_expr(ast, var.value);

			// Bind variable types
			self.visit_var_type(var.identity);
		}

		self.checkers.pop();
		self.in_class = enclosing_in_class;
	}

	fn resolve_type(&mut self, name: StrId, location: &SourceLocation) -> Option<Type> {
		for checker in self.checkers.iter_mut().rev() {
			match checker.check(self.db, name) {
				ScopeEntry::Var(_) => break, // TODO: Figure out an ergonomic way to do this.
				ScopeEntry::Fun(_) => break,
				ScopeEntry::Class(class) => {
					return Some(Type::Class(class))
				}
				ScopeEntry::None => continue,
			}
		}

		self.db.report_error(Error::simple(
			format!("Unknown named type '{}'", self.db.get(name)),
			location.clone()
		));
		
		self.had_error = true;
		None
	}

	fn visit_type(&mut self, typ: TypId, location: &SourceLocation) -> TypId {
		let ty = self.db.get(typ).clone();

		match ty {
			Type::UnboundIdent(str_id) => {
				let ty = self.resolve_type(str_id, location);

				// If we successfully resolved the type, return that; otherwise,
				// we already reported the error, so just hang on to the unknown
				// type.
				if let Some(ty) = ty {
					return self.db.put_type(ty);
				}
				return typ;
			},
			Type::Fun(sig) => {
				// TODO: Consider adding a flag here that will keep us from re-visiting
				// the same funs over and over. In particular, any fully resolved TypId
				// does not have to be visited again. But, any partially unresolved one
				// does, as the same name may refer to different things in different scopes.
				let mut resolved_params = self.db.get(sig).parameters.clone();
				for param in &mut resolved_params {
					// TODO: Somehow get more location information?
					*param = self.visit_type(*param, location);
				}

				let return_type = self.visit_type(self.db.get(sig).return_type, location);
				
				let sig = self.db.put_sig(&Sig {
					parameters: resolved_params,
					return_type
				});
				return self.db.put_type(Type::Fun(sig));
			},
			Type::FunRaw(sig) => {
				// TODO: Consider adding a flag here that will keep us from re-visiting
				// the same funs over and over. In particular, any fully resolved TypId
				// does not have to be visited again. But, any partially unresolved one
				// does, as the same name may refer to different things in different scopes.
				let mut resolved_params = self.db.get(sig).parameters.clone();
				for param in &mut resolved_params {
					// TODO: Somehow get more location information?
					*param = self.visit_type(*param, location);
				}

				let return_type = self.visit_type(self.db.get(sig).return_type, location);
				
				let sig = self.db.put_sig(&Sig {
					parameters: resolved_params,
					return_type
				});
				return self.db.put_type(Type::FunRaw(sig));
			}
			Type::ArrayOf(inner_typ) => {
				let inner = self.visit_type(inner_typ, location);
				if inner != inner_typ {
					return self.db.put_type(Type::ArrayOf(inner));
				}
				return typ;
			}
			Type::Tuple(inner) => {
				// TODO: Any way to optimize this?
				let mut resolved = Vec::new();
				for typ in inner.iter() {
					resolved.push(self.visit_type(*typ, location));
				}
				return self.db.put_type(Type::Tuple(Arc::from(resolved)));
			}
			Type::RangeOf(left, right, inner_typ) => {
				let inner = self.visit_type(inner_typ, location);
				if inner != inner_typ {
					return self.db.put_type(Type::RangeOf(left, right, inner));
				}
				// No change.
				return typ;
			}
			Type::Option(inner_typ) => {
				let inner = self.visit_type(inner_typ, location);
				if inner != inner_typ {
					return self.db.put_type(Type::Option(inner));
				}
				return typ;
			}
			_ => { return typ; }
		}
	}
	
	/// Performs the visit_type logic on a particular VarId using that var's
	/// source location information. Performs logic common to Stmt::Declare and
	/// FunDeclare.
	fn visit_var_type(&mut self, var: VarId) {
		// TODO: Avoid this clone.
		let var_type = self.visit_type(self.db.get_var_type(var), &self.db.get(var).location.clone());
		self.db.get_mut(var).typ = var_type;
	}

	fn visit_stmt(&mut self, ast: &AstProxy, stmt: StmtId) {
		match ast.stmts.get_mut(stmt).as_mut() {
			Stmt::Declare(declare) => {
				self.visit_expr(ast, declare.value);

				// Anywhere where the parser might generate a Type::UnboundIdent,
				// we need to try resolving that identifier.
				self.visit_var_type(declare.identity);
			},
			Stmt::Expression(expr) => {
				self.visit_expr(ast, expr.expression);
			},
			Stmt::Return(ret) => {
				if let Some(expr) = &mut ret.expression {
					self.visit_expr(ast, *expr);
				}
			},
			Stmt::ClassDeclare(class_declare) => {
				self.visit_class(ast, class_declare);
			}
		}
	}

	fn visit_function(&mut self, ast: &AstProxy, function: &mut FunDeclare) {
		// TODO: Push my name.
		self.visit_expr(ast, function.value);

		let param_count = self.db.get(function.identity).parameters.len();
		for param in 0..param_count {
			let var = self.db.get(function.identity).parameters[param];
			self.visit_var_type(var);
		}

		// Visit the function return value type in case it is UnboundIdent.
		let ret_type = self.visit_type(self.db.get(function.identity).return_type, &function.location);
		self.db.get_mut(function.identity).return_type = ret_type;
	}

	pub fn visit_module(&mut self, ast: &AstProxy, module: &mut Module) {
		// Now that we have the topological sort pass, we want to have the
		// globals scope available even for the global variables.
		self.checkers.push(NameChecker::global());

		for global in &mut module.globals {
			self.visit_expr(ast, global.value);
		}

		for fun in &mut module.functions {
			self.visit_function(ast, fun);
		}

		for class in &mut module.classes {
			self.visit_class(ast, class);
		}
	}
}

pub fn bind(db: &mut Db, ast: &mut Ast) -> bool {
	let mut had_error = false;

	let proxy = ast.get_proxy();

	for module in proxy.sources.iter() {
		// TODO: Run one binder per thread.
		let mut binder = Binder::new(db);
		let mut source = proxy.sources.get_mut(module);
		binder.visit_module(&proxy, &mut source.module);

		if binder.had_error { had_error = true; }
	}

	proxy.commit();

	had_error
}