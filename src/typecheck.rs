use crate::db::*;
use crate::module::Module;
use crate::source::SourceLocation;
use crate::typ::Type;

use crate::expr::*;
use crate::error::Error;

// Current plan for type inference:
// variable declarations may infer a type for the variable:
//     var x = new Player(); // x is now a Player
//
// besides this, there is no type inference (at the current time).

// Stores any state needed while type checking.
struct TypeChecker<'db> {
	db: &'db mut Db,

	had_error: bool,

	global_scope: bool,

	return_types: Vec<TypId>,
}

struct TypeCheckErr;
type Result<T> = std::result::Result<T, TypeCheckErr>;

macro_rules! maybe_type_error {
    ($self:ident, $expr:expr, $location:expr, $($arg:tt)*) => {
		match $expr {
			Ok(ty) => ty,
			Err(_) => {
				$self.had_error = true;
				$self.db.report_error(Error::simple(
					format!($($arg)*),
					$location
				));

				return Err(TypeCheckErr)
			}
		}
    };
}

macro_rules! type_error {
    ($self:ident, $location:expr, $($arg:tt)*) => {
		{
			$self.had_error = true;
			$self.db.report_error(Error::simple(
				format!($($arg)*),
				$location
			));

			return Err(TypeCheckErr)
		}
    };
}

impl<'db> TypeChecker<'db> {
	fn new(db: &'db mut Db) -> Self {
		TypeChecker {
			db,

			had_error: false,

			global_scope: false,

			return_types: Vec::new(),
		}
	}

	// Returns what the new "from" type would be.
	fn compute_assignable(&mut self, to: TypId, from: TypId) -> Result<TypId> {
		if to == from { return Ok(to); }

		let ty_to = self.db.get(to);
		let ty_from = self.db.get(from);

		match (ty_to, ty_from) {
			(_, Type::Bottom) => return Ok(from),

			(Type::Int, Type::AssumeInt) => return Ok(to),

			// Ints and all Assume types promote to float.
			(Type::Float, Type::AssumeInt | Type::AssumeFloat | Type::Int) => return Ok(to),

			// An unassigned clashing with an Assume resolves the Assume to its
			// assumed value.
			(Type::Unassigned, Type::AssumeInt) => return Ok(self.db.types.int),
			(Type::Unassigned, Type::AssumeFloat) => return Ok(self.db.types.float),

			// If the 'to' is unassigned, then anything is assignable to it.
			(Type::Unassigned, _) => return Ok(from),

			// Everything else is an error.
			_ => return Err(TypeCheckErr)
		}
	}

	// Computes the common "intersection" type of the two types. This can result
	// in promotions, e.g. from int to float (even though float is technically
	// not an intersection of float and int), while it can also result in 
	// "restrictions" (e.g. AssumeInt -> Float).
	//
	// Finally, one thing to note is the intersection of Bottom with anything
	// is itself.
	fn compute_intersect(&mut self, left: TypId, right: TypId) -> Result<TypId> {
		if left == right { return Ok(left); }

		let ty_left = self.db.get(left);
		let ty_right = self.db.get(right);

		match (ty_left, ty_right) {
			(Type::Bottom, _) => return Ok(left),
			(_, Type::Bottom) => return Ok(right),
		
			(Type::Float, Type::AssumeInt | Type::AssumeFloat | Type::Int) => return Ok(left),
			(Type::AssumeInt | Type::AssumeFloat | Type::Int, Type::Float) => return Ok(right),

			(Type::Int, Type::AssumeInt) => return Ok(left),
			(Type::AssumeInt, Type::Int) => return Ok(right),

			(Type::AssumeFloat, Type::AssumeInt) => return Ok(left),
			(Type::AssumeInt, Type::AssumeFloat) => return Ok(right),

			_ => return Err(TypeCheckErr)
		}
	}

	fn check_assign(&mut self, at: &SourceLocation, var: VarId, expr: &mut Expr) -> Result<TypId> {
		let value = self.check_expr(expr, true)?;
		let computed =
			self.compute_assignable(self.db.get_var_type(var), value);

		let computed = maybe_type_error!(
			self,
			computed,

			at,
			"Invalid assignment to '{}': need {}, but value is {}",
			self.db.repr_var(var),
			self.db.repr_var_type(var),
			self.db.repr_type(value)
		);

		self.db.get_mut(var).typ = computed;
		expr.promote(computed, self.db);

		Ok(computed)
	}

	fn check_expr(&mut self, expr: &mut Expr, value_used: bool) -> Result<TypId> {
		Ok(match expr {
			Expr::Binary(binary) => {
				let left = self.check_expr(&mut binary.left, value_used)?;
				let right = self.check_expr(&mut binary.right, value_used)?;

				let computed = maybe_type_error!(
					self,
					self.compute_intersect(left, right),

					&binary.location,
					"Invalid operands to binary operator: LHS is {}, RHS is {}",
					self.db.repr_type(left),
					self.db.repr_type(right)
				);

				binary.typ = computed;
				binary.left.promote(computed, self.db);
				binary.right.promote(computed, self.db);
				
				computed
			},
			Expr::Variable(var) => self.db.get(var.identity).typ,
			Expr::Assign(assign) => {
				self.check_assign(&assign.location, assign.identity, &mut assign.value)?
			},
			Expr::NumLiteral(lit) => {
				lit.typ
			},
			Expr::StrLiteral(_) => {
				self.db.types.str_const
			}
			Expr::Block(block) => {
				// We must type-check every statement inside the block.
				// However, the last statement is checked specially.
				let all_but_last = match block.stmts.len() {
					0 => 0,
					n => n - 1,
				};
				for stmt in &mut block.stmts[0..all_but_last] {
					self.check_stmt(stmt, false)?;
				}

				// If the value isn't used, we can simply type-check the
				// last statement then bail with Void.
				if !value_used {
					block.stmts.last_mut().map(|stmt| self.check_stmt(stmt, false));
					return Ok(self.db.types.bottom);
				}

				// Otherwise, we need to compute a type for the value.
				// If the block has no statements, that's an error.
				let Some(stmt) = block.stmts.last_mut() else {
					type_error!(self, &block.location,
						"Return value of block is used, but the block is empty.");
				};

				// If the block has a statement, defer to self.stmt(). But we
				// need to get a TypId at the end.
				let Some(val) = self.check_stmt(stmt, true)? else {
					type_error!(self, &block.location,
						"Return value of block is used, but its last statement has no value.");
				};

				// Return the computed TypId.
				block.typ = val;
				val
			},
			Expr::Print(print) => {
				// At least for now, all possible types are allowed inside the
				// print. So, simply type check each one. Then, the print is
				// supposed to return its first argument.

				for expr in &mut print.exprs[1..] {
					let inner = self.check_expr(expr, false)?;

					// The idea here is that each argument to the print is essentially
					// an assignment to an Unassigned variable. As such, the arguments
					// should automatically promote to Int or Float if they're AssumeInt
					// or AssumeFloat.
					//
					// We could special-case this logic, as it might speed up type-checking
					// print statements slightly.
					let computed = self.compute_assignable(self.db.types.unassigned, inner)
						.unwrap_or_else(|_| panic!("compute_assignable should always succeed with LHS of unassigned"));

					expr.promote(computed, self.db);
				}

				let inner = self.check_expr(&mut print.exprs[0], true)?;
				let computed = self.compute_assignable(self.db.types.unassigned, inner)
						.unwrap_or_else(|_| panic!("compute_assignable should always succeed with LHS of unassigned"));

				expr.promote(computed, self.db);

				computed 
			}
			Expr::Unbound(_) => {
				// In theory we will resolve all idents beforehand? But this might
				// be different if we have function overloading.
				panic!("compiler-err:tried-to-typecheck-an-unbound-identifier-expression");
			}
		})
	}

	fn check_stmt(&mut self, stmt: &mut Stmt, value_used: bool) -> Result<Option<TypId>> {
		match stmt {
			Stmt::Declare(declare) => {
				self.check_declare(declare);
				Ok(None)
			},
			Stmt::Expression(expr) => {
				Ok(Some(self.check_expr(&mut expr.expression, value_used)?))
			},
			Stmt::FunDeclare(_) => todo!(),
			Stmt::Return(ret) => {
				// The return statement is interesting in that it entirely
				// ignores value_used. Because 'return' always returns
				// the bottom type, its value may always be used if needed.
				//
				// That said, it DOES need to always get a value from its inner
				// expr.

				// TODO: there's no need for a separate stack of these...
				// We can do the good old trick where you push/pop as part of
				// the function
				let Some(&return_type) = self.return_types.last() else {
					type_error!(self, &ret.location,
						"Trying to return outside of a function.");
				};

				let inner = match &mut ret.expression {
					Some(expr) => expr,
					None => {
						if return_type != self.db.types.void {
							type_error!(self, &ret.location,
								"Trying to return value in function returning void");
						}

						return Ok(Some(self.db.types.bottom));
					},
				};

				let typ = self.check_expr(inner, true)?;
				let valid = self.compute_assignable( 
					return_type,
					typ);

				maybe_type_error!(self, 
					valid,
					&ret.location,
					"Trying to return {} in function returning {}",

					self.db.repr_type(typ),
					self.db.repr_type(return_type));

				Ok(Some(self.db.types.bottom))
			}
		}
	}

	fn check_declare(&mut self, declare: &mut Declare) {
		let _ = self.check_assign(&declare.location, declare.identity, &mut declare.value);
	}

	fn check_fun_declare(&mut self, fun: &mut FunDeclare) -> Result<()> {
		// The idea with whether we need the value to be used is somewhat tricky.
		// Basically, in the simplest case, if we DO need a return value, then
		// either we need:
		//     fun example() { value; }
		// or we need:
		//     fun example() { return value; }
		//
		// In the second case, it might seem like 'return value;' doesn't evaluate 
		// to any type, which is true, but it does evaluate to the "Has No Value"
		// type, which can be "assigned" to any other type. So, it is perfectly fine
		// for return value; to be the last case, and in that case, we do still
		// "use" the value, but we just use the empty value of it.
		//
		// So, the only thing that affects whether we need a value is the return type.
		// If it's void, we need no value; otherwise, we need a value.
		let value_used = !self.db.does_fun_return_void(fun.identity);
		let return_type = self.db.get_fun_return_typid(fun.identity);

		self.return_types.push(return_type);

		let inner = self.check_expr(&mut fun.value, value_used)?;

		self.return_types.pop();

		// If we're using the value of the expression, it must match the return
		// type.
		if value_used {
			let valid = self.compute_assignable(
				self.db.get_fun_return_typid(fun.identity),
				inner);
			maybe_type_error!(self, 
				valid,
				&fun.location,
				"Value of function body is {} but function returns {}",
				self.db.repr_type(inner),
				self.db.repr_type(self.db.get_fun_return_typid(fun.identity)));
		}

		Ok(())
	}

	fn check_module(&mut self, module: &mut Module) {
		for global in &mut module.globals {
			self.check_declare(global);
		}

		for fun in &mut module.functions {
			self.check_fun_declare(fun);
		}
	}

	fn check_modules(&mut self, modules: &mut Vec<Module>) {
		self.global_scope = true;
		for module in modules {
			self.check_module(module);
		}
	}
}

pub fn typecheck(db: &mut Db, modules: &mut Vec<Module>) -> bool {
	let mut checker = TypeChecker::new(db);

	checker.check_modules(modules);

	checker.had_error
}