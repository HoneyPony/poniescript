use std::sync::Arc;

use crate::db::*;
use crate::error::Error;
use crate::expr::*;
use crate::lexer::Tok;
use crate::module::Module;
use crate::source::SourceLocation;
use crate::typ::Type;

use poni_arena::IndexCell;

struct NameChecker {
	buffer: String,
	own_length: usize,

	// In the future, this will need to be some sort of chain, to let us
	// resolve calls/references to parent class functions/variables (in the Java sense).
	//
	// For now, this simply represents whether this scope should involve a
	// new SelfVal bound to any function calls/variables, or not.
	self_val: bool,

	// Whether this ScopeChecker is the "stopping point." For example, if we
	// are a class that has not parent class, we are the upper bound (because
	// accesses to our parent class would be wrong.)
	is_upper_bound: bool,
}

impl NameChecker {
	pub fn scoped(previous: &NameChecker, scope: &str, self_val: bool, is_upper_bound: bool) -> Self {
		// We add a dot after the scope, as that's what the names will
		// look like.
		//
		// The previous scope will also have a dot (or be the empty global
		// scope), so we directly push it against our own.
		let buffer = format!("{}{}.", previous.buffer, scope);
		let own_length = buffer.len();

		return NameChecker { buffer, own_length, self_val, is_upper_bound };
	}

	pub fn global() -> Self {
		return NameChecker {
			buffer: String::new(),
			own_length: 0,
			self_val: false,
			// For now, even though the global is technically an upper bound,
			// we'll say it isn't so we can have better error messages.
			is_upper_bound: false
		};
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

	/// Gets a new SelfVal if appropriate.
	fn get_selfval(&mut self, ast: &AstProxy, location: SourceLocation, selfval: bool) -> Option<ExprId> {
		if selfval {
			// Shrink the location to just its beginning, so that it doesn't hog
			// the tokens when we're using our visit trait.
			Some(Expr::push_selfval(ast, location.begin(), self.db.types.unassigned))
		}
		else { None }
	}

	fn resolve_unbound_inner(&mut self, ast: &AstProxy, ident: StrId, location: SourceLocation) -> (Option<Expr>, bool) {
		let mut hit_upper_bound = false;

		for checker in self.checkers.iter_mut().rev() {
			match checker.check(self.db, ident) {
				ScopeEntry::Var(var) => {
					let expr = Some(Expr::mk_variable(location, var));

					return (expr, hit_upper_bound);
				}
				ScopeEntry::Fun(fun) => {
					log::trace!("resolved unbound '{}' to fun in scope '{}'; self_val: {}",
						self.db.get(ident),
						checker.buffer,
						checker.self_val);

					let self_val = checker.self_val;
					let self_val = self.get_selfval(ast, location.clone(), self_val);
					let expr = Some(Expr::mk_funcapture(location.clone(), location, fun, self.db.types.fun_sig_unassigned, 
						self_val));

					return (expr, hit_upper_bound);
				}
				ScopeEntry::Class(_) => {
					todo!("what to do when we resolve an Unbound into a Class");
				}
				ScopeEntry::None => {
					if checker.is_upper_bound { hit_upper_bound = true; }
					continue;
				}
			}
		}

		return (None, hit_upper_bound)
	}

	fn resolve_unbound(&mut self, ast: &AstProxy, ident: StrId, location: SourceLocation) -> Option<Expr> {
		let (expr, hit_upper_bound) = self.resolve_unbound_inner(ast, ident, location.clone());

		match (expr, hit_upper_bound) {
			(Some(expr), false) => { return Some(expr); },
			(Some(_), true) => {
				self.db.report_error(Error::simple(
					format!("Identifier '{}' is not available in this scope.", self.db.get(ident)),
					location.clone()
				).add_note(format!("Variables from outer classes are only available to @inner classes."), None));
				
				self.had_error = true;
			}
			(None, _) => {
				self.db.report_error(Error::simple(
					format!("Unknown identifier '{}'", self.db.get(ident)),
					location.clone()
				));
				
				self.had_error = true;
			}
		}

		None
	}

	fn resolve_unbound_assign(&mut self, ident: StrId, location: SourceLocation, ident_location: SourceLocation, expr: ExprId, op: Tok) -> Option<Expr> {
		for checker in self.checkers.iter_mut().rev() {
			match checker.check(self.db, ident) {
				ScopeEntry::Var(var) => return Some(Expr::mk_assign(location, ident_location, var, expr, op)),
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
					// The Binder is not equipped to handle ANY name resolution
					// on a bound object. That MUST wait until type checking.
					//
					// As such, panic if we ever attempt this. This is even wrong
					// in an LSP context; it is literally a bug if we get here,
					// not just an improperly handled case.
					//
					// This also means that we *always* call self.get_selfval(),
					// as the object is always already None. We used to only
					// call self.get_selfval() if the object was None, but
					// this extra logic is unnecessary, because the object should
					// ALWAYS be None.
					if unbound.object.is_some() {
						panic!("ICE: Binder tried to resolve_unbound_funcapture on a bound expression. This should never happen.");
					}

					log::trace!("resolved unbound funcapture '{}' to fun in scope '{}'; self_val: {}",
						self.db.get(unbound.identifier.lexeme),
						checker.buffer,
						checker.self_val);

					let self_val = checker.self_val;
					let self_val = self.get_selfval(ast, unbound.location.clone(), self_val);

					return Some(Expr::mk_funcapture(unbound.location.clone(), unbound.identifier.location.clone(), fun, self.db.types.unassigned, 
						self_val))
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

			Expr::Continue(_) => {
				None
			}

			Expr::Return(ret) => {
				if let Some(expr) = &mut ret.expression {
					self.visit_expr(ast, *expr);
				}
				// TODO: I think we probably want to eliminate the return if
				// the inner type is also Never? But not super necessary...
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
				self.resolve_unbound_assign(assign.identifier.lexeme,
					assign.location.clone(),
					assign.identifier.location.clone(),
					assign.value,
					assign.op)
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
				// Resolve non-parented new chains here. For .new, we can
				// only ever refer to child classes of the given object, so
				// bind it in the typechecker.
				if new.parent.is_none() {
					// SAFETY: We always parse at least one identifier.
					let (first, rest) = new.identifiers.split_first().unwrap();

					// TODO: We actually want to store the entire chain of classes, for the LSP.
					let mut class_id = self.resolve_class_name(first.lexeme, &new.location)?;
					
					log::trace!("bind: got class {} for Expr::New", self.db.get(self.db.get(class_id).name));
					for tok in rest {
						let class = self.db.get(class_id);
						let next = class.class_map.get(&tok.lexeme);

						log::trace!("bind: resolving inner class: {} -> is_some? {}", self.db.get(tok.lexeme), next.is_some());

						let next = match next {
							Some(next) => next,
							None => {
								self.db.report_error(Error::simple(
									format!("Class '{}' has no such inner class '{}'",
									self.db.repr_class(class_id),
									self.db.get(tok.lexeme)),
									tok.location.clone()
								));
								
								self.had_error = true;

								class_id = self.db.class_unassigned;
								break;
							}
						};

						class_id = *next;
					}

					new.class = class_id;
					// Set the type here, so we don't have to mess with it again.
					new.typ = self.db.put_type(Type::Class(new.class));
				}

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
			Expr::BuiltinCall(_) => panic!("ICE: Tried to bind Expr::BuiltinCall"),
			Expr::BuiltinCapture(_) => panic!("ICE: Tried to bind Expr::BuiltinCapture"),
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

	/// Awkwardly, this logic doesn't actually work for in-PonieScript classes,
	/// only for the C imported ones. We should probably revisit the Binder in
	/// more detail eventually.
	fn visit_class_id_only_call_this_on_imported_classes(&mut self, ast: &AstProxy, id: ClassId) {
		let enclosing_in_class = self.in_class;
		self.in_class = true;

		let name = self.db.get(self.db.get(id).name);
		let new_scope = NameChecker::scoped(
			self.checkers.last().expect("class"),
			name,
			true,
			self.db.get(id).parent.is_none()
		);
		self.checkers.push(new_scope);

		for i in 0..self.db.get(id).funs.len() {
			self.visit_function_id(ast, self.db.get(id).funs[i]);
		}

		for i in 0..self.db.get(id).vars.len() {
			let var = self.db.get(id).vars[i];

			if let Some(value) = self.db.get(var).initializer {
				self.visit_expr(ast, value);
			}

			// Bind variable types
			self.visit_var_type(var);
		}

		self.checkers.pop();
		self.in_class = enclosing_in_class;
	}

	fn visit_class(&mut self, ast: &AstProxy, class_declare: &mut ClassDeclare) {
		let enclosing_in_class = self.in_class;
		self.in_class = true;

		let class = self.db.get(class_declare.identity);
		let name = self.db.get(class.name);
		let new_scope = NameChecker::scoped(
			self.checkers.last().expect("class"),
			name,
			true,
			self.db.get(class_declare.identity).parent.is_none()
		);
		self.checkers.push(new_scope);

		for fun in &mut class_declare.funs {
			self.visit_function(ast, fun);
		}

		for var in &mut class_declare.vars {
			if let Some(value) = var.value {
				self.visit_expr(ast, value);
			}

			// Bind variable types
			self.visit_var_type(var.identity);
		}

		for class in &mut class_declare.classes {
			self.visit_class(ast, class);
		}

		self.checkers.pop();
		self.in_class = enclosing_in_class;
	}

	fn resolve_type_members(&mut self, mut class: ClassId, rest: &[StrId], location: &SourceLocation) -> Option<Type> {
		for member in rest {
			let class_ = self.db.get(class);

			let Some(next) = class_.class_map.get(member) else {
				self.db.report_error(Error::simple(
					format!("Class '{}' has no such inner class '{}'",
						self.db.get(class_.name),
						self.db.get(*member)),
						location.clone()
				));
				
				self.had_error = true;
				return None;
			};

			class = *next;
		}

		Some(Type::Class(class))
	}

	fn resolve_type(&mut self, name: &Vec<StrId>, location: &SourceLocation) -> Option<Type> {
		// Safety: Types should always have at least one identifier.
		let (first, rest) = name.split_first().unwrap();

		for checker in self.checkers.iter_mut().rev() {
			match checker.check(self.db, *first) {
				ScopeEntry::Var(_) => break, // TODO: Figure out an ergonomic way to do this.
				ScopeEntry::Fun(_) => break,
				ScopeEntry::Class(class) => {
					// Ok, we identified the class type. Now look up any inner
					// members.
					return self.resolve_type_members(class, rest, location);
				}
				ScopeEntry::None => continue,
			}
		}

		self.db.report_error(Error::simple(
			format!("Unknown named type '{}'", self.db.get(*first)),
			location.clone()
		));
		
		self.had_error = true;
		None
	}

	fn visit_type(&mut self, typ: TypId, location: &SourceLocation) -> TypId {
		let ty = self.db.get(typ).clone();

		match ty {
			Type::UnboundIdent(str_id) => {
				let ty = self.resolve_type(&str_id, location);

				// If we successfully resolved the type, return that; otherwise,
				// we already reported the error, so just hang on to the unknown
				// type.
				if let Some(ty) = ty {
					return self.db.put_type(ty);
				}
				return typ;
			},
			Type::UnboundCStructPtr(str_id) => {
				if let Some(ty) = self.db.lookup_c_struct(str_id) {
					return self.db.put_type(Type::Class(ty));
				}
				// Report an error.
				self.db.report_error(Error::simple(
			format!("Unresolved C struct '{}'", self.db.get(str_id)),
					location.clone()
				));
				return typ;
			}
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
			Type::DynArrayOf(elem_typ, _) => {
				let inner = self.visit_type(elem_typ, location);
				if inner != elem_typ {
					let arr_typ = self.db.put_type(Type::ArrayOf(inner));
					return self.db.put_type(Type::DynArrayOf(inner, arr_typ));
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
				if let Some(value) = declare.value {
					self.visit_expr(ast, value);
				}

				// Anywhere where the parser might generate a Type::UnboundIdent,
				// we need to try resolving that identifier.
				self.visit_var_type(declare.identity);
			},
			Stmt::Expression(expr) => {
				self.visit_expr(ast, expr.expression);
			},
			Stmt::ClassDeclare(class_declare) => {
				self.visit_class(ast, class_declare);
			}
		}
	}

	fn visit_function_id(&mut self, _ast: &AstProxy, id: FunId) {
		let param_count = self.db.get(id).parameters.len();
		for param in 0..param_count {
			let var = self.db.get(id).parameters[param];
			self.visit_var_type(var);
		}

		// Visit the function return value type in case it is UnboundIdent.
		let location = self.db.get(id).location.clone();
		let ret_type = self.visit_type(self.db.get(id).return_type, &location);
		self.db.get_mut(id).return_type = ret_type;
	}

	fn visit_function(&mut self, ast: &AstProxy, function: &mut FunDeclare) {
		// TODO: Push my name.
		self.visit_expr(ast, function.value);

		self.visit_function_id(ast, function.identity);
	}

	pub fn visit_module(&mut self, ast: &AstProxy, module: &mut Module) {
		// Now that we have the topological sort pass, we want to have the
		// globals scope available even for the global variables.
		self.checkers.push(NameChecker::global());

		for global in &mut module.globals {
			// NOTE: We could panic here, but I guess we won't (?)
			if let Some(value) = global.value {
				self.visit_expr(ast, value);
			}
			self.visit_var_type(global.identity);
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

	// We also need to visit every class and fun that was imported.
	for i in 0..db.imported_funs.len() {
		let fun = db.imported_funs[i];
		let mut binder = Binder::new(db);
		binder.checkers.push(NameChecker::global());
		binder.visit_function_id(&proxy, fun);
	}

	for i in 0..db.imported_classes.len() {
		let class = db.imported_classes[i];
		let mut binder = Binder::new(db);
		binder.checkers.push(NameChecker::global());
		binder.visit_class_id_only_call_this_on_imported_classes(&proxy, class);
	}

	proxy.commit();

	had_error
}