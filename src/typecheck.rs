use crate::db::*;
use crate::module::Module;
use crate::source::SourceLocation;
use crate::typ::Type;

use crate::expr::*;

// Notes on type inference:
// I sort of want the type inference rules to be simple, simply because that
// way they're less magical, and even simple type inference rules go a long way.
//
// That said, I think the most obvious case where simple type inference might
// not be sufficient is with collection types.
//
// For example,
// 
//     var list = new List();
//     for i in range(0, 5) {
//         list.push(i);
//     }
//
// It would be very nice for the push() to be able to infer the type of the 
// list.
//
// This is especially annoying with overloaded methods being a possibility.
// For example:
//
//     var x = 3;
//     some_method(x); // has overloads for int, long, float, double
//     some_other_method(x); // has overloads for only int
//
// In this case we could infer that x is int. But, to do so, we have to do it
// with a more "constraint" based style where we might have additional passes..?
//
// That is, because method selection itself requires knowing the types, but
// inferring the types from a method requires knowing which method was selected...
//
// Same idea, sort of, with variable resolution.
// (actually, variables should probably be resolved before type checking).
//
// I think maybe the best solution would be 3 passes.
// 1. Bind names to everything BUT overloaded functions.
// 2. Type inference in a "straightforward" style. Unbound functions are skipped.
//    Exception: If all the info is known for the function selection, the unbound
//               function is bound. (for things like var x = max(3.0, y))
// 3. Bind the overloaded functions.
//
// In any case, some objects can only be inferred when they are declared, namely
// global variables and member variables. These objects can ONLY be inferred
// from their initial declaration. (At least for now).
//
// Funnily enough, the type checker might just automatically support inferring
// those objects anyways... but we should error out nonetheless.

// Stores any state needed while type checking.
struct TypeChecker {
	had_error: bool,

	global_scope: bool,

	return_types: Vec<TypId>,
}

struct TypeCheckErr;
type Result<T> = std::result::Result<T, TypeCheckErr>;

macro_rules! maybe_type_error {
    ($self:ident, $expr:expr, $db:ident, $location:expr, $($arg:tt)*) => {
		match $expr {
			Ok(ty) => ty,
			Err(_) => {
				$self.had_error = true;
				$db.err_locate($location);
				eprintln!($($arg)*);

				return Err(TypeCheckErr)
			}
		}
    };
}

macro_rules! type_error {
    ($self:ident, $db:ident, $location:expr, $($arg:tt)*) => {
		{
			$self.had_error = true;
			$db.err_locate($location);
			eprintln!($($arg)*);

			return Err(TypeCheckErr)
		}
    };
}

impl TypeChecker {
	fn new() -> Self {
		TypeChecker {
			had_error: false,

			global_scope: false,

			return_types: Vec::new(),
		}
	}

	//fn unify_bi(&mut self, db: &mut Db, ty_a: TypId, ty_b: TypId)

	// Tries to get the expression on the right to have the same type as the one
	// on the left. If the rightward expression does not match, then it is considered
	// an error with the rightward one.
	//fn unify_right(&mut self, db: &mut Db, ty_left: TypId, expr: &mut Expr) -> TypId {
	//	ty_left
	//}

	fn unify_lhs_superset_rhs(&mut self, db: &mut Db, lhs: TypId, rhs: TypId) -> Result<TypId> {
		// If equal: Nothing else to be learned.
		if lhs == rhs {
			return Ok(lhs);
		}

		let left = db.get(lhs);
		let right = db.get(rhs);

		let unified = match (left, right) {
			(Type::UnassignedDecimal, Type::UnassignedNumeric) => {
				// Numerics become further constrained by Decimal.
				lhs
			}

			// Ints dominate numerics.
			(Type::Int, Type::UnassignedNumeric) => {
				lhs
			}

			// Floats dominate whole number and decimals.
			(Type::Float, Type::UnassignedNumeric | Type::UnassignedDecimal) => {
				lhs
			}

			(_, Type::Unassigned) => lhs,

			// The bottom type is a subtype of everything.
			(_, Type::Bottom) => lhs,

			// More branches to come with parameterized types...

			_ => return Err(TypeCheckErr)
		};

		Ok(unified)
	}

	// Computes the "intersection" of the two types if possible.
	// Note that, e.g., Type::Bottom intersect Anything = Type::Bottom
	fn unify_intersect(&mut self, db: &mut Db, lhs: TypId, rhs: TypId) -> Result<TypId> {
		if lhs == rhs {
			return Ok(lhs);
		}

		let left = db.get(lhs);
		let right = db.get(rhs);

		// The original idea was to try to use a single-directional type to
		// infer these. But, that doesn't quite work.
		//
		// In particular, consider, e.g. int x = <bottom> -- this is a valid
		// assignment to x.
		//
		// But then consider 3 + <bottom> -- this should actually have type
		// <bottom> in our system. But the single-directional type rule would
		// assign 'int' to this, perhaps.
		//
		// So the "intersection" rule must be unique somehow.

		match (left, right) {
			(Type::Bottom, _) => return Ok(lhs),
			(_, Type::Bottom) => return Ok(rhs),

			(Type::Int, Type::UnassignedNumeric) => return Ok(lhs),
			(Type::UnassignedNumeric, Type::Int) => return Ok(rhs),

			(Type::Float, Type::UnassignedNumeric | Type::UnassignedDecimal) => return Ok(lhs),
			(Type::UnassignedNumeric | Type::UnassignedDecimal, Type::Float) => return Ok(rhs),
		
			(Type::UnassignedDecimal, Type::UnassignedNumeric) => return Ok(lhs),
			(Type::UnassignedNumeric, Type::UnassignedDecimal) => return Ok(lhs),
			_ => { }
		}

		return Err(TypeCheckErr)
	}

	fn unify_assign(&mut self, db: &mut Db, var: VarId, value: TypId) -> Result<TypId> {
		let var_ty = db.get(var).typ;

		// If they're already equal, then there is no more info we can get here.
		// Note that the way we've designed TypIds means that equal TypId corresponds
		// to equal Type.
		if var_ty == value {
			return Ok(value);
		}

		let left = db.get(var_ty);
		let right = db.get(value);

		let unified = match (left, right, self.global_scope) {
			(Type::Unassigned, Type::UnassignedNumeric, true) => {
				// In global scope, if we have an un-inferred var, then the
				// unassigned numeric must become a concrete type.
				// For now, we make the dodgy decision that Whole -> Int
				// and Decimal -> Float.
				// One other option would be to make global vars require
				// a type clause.
				db.put_type(Type::Int)
			},

			(Type::Unassigned, Type::UnassignedDecimal, true) => {
				// dodgy global var
				db.put_type(Type::Float)
			},

			(Type::UnassignedNumeric, Type::UnassignedDecimal, _) => {
				// Numerics become further constrained by Decimal.
				value
			}

			// Ints dominate numerics.
			(Type::Int, Type::UnassignedNumeric, _) => {
				var_ty
			}

			// Floats dominate whole number and decimals.
			(Type::Float, Type::UnassignedNumeric | Type::UnassignedDecimal, _) => {
				var_ty
			}

			(Type::Unassigned, _, _) => value,

			// More branches to come with parameterized types...

			_ => return Err(TypeCheckErr)
		};

		db.get_mut(var).typ = unified;

		Ok(unified)
	}

	fn do_assign(&mut self, db: &mut Db, at: &SourceLocation, var: VarId, expr: &mut Expr) -> Result<TypId> {
		let value = self.do_type(db, expr, true)?;

		let unified = maybe_type_error!(
			self,
			self.unify_assign(db, var, value),

			db,
			at,
			"Invalid assignment to '{}': need {}, but value is {}",
			db.repr_var(var),
			db.repr_var_type(var),
			db.repr_type(value)
		);

		Ok(unified)
	}

	fn do_type(&mut self, db: &mut Db, expr: &mut Expr, value_used: bool) -> Result<TypId> {
		Ok(match expr {
			Expr::Binary(binary) => {
				let left = self.do_type(db, &mut binary.left, value_used)?;
				let right = self.do_type(db, &mut binary.right, value_used)?;

				let unified = maybe_type_error!(
					self,
					self.unify_intersect(db, left, right),
					db,
					&binary.location,
					"Invalid operands to binary operator: LHS is {}, RHS is {}",
					db.repr_type(left),
					db.repr_type(right)
				);

				binary.typ = unified;
				
				unified
			},
			Expr::Variable(var) => db.get(var.identity).typ,
			Expr::Assign(assign) => {
				self.do_assign(db, &assign.location, assign.identity, &mut assign.value)?
			},
			Expr::Literal(lit) => {
				lit.typ
			},
			Expr::Block(block) => {
				// We must type-check every statement inside the block.
				// However, the last statement is checked specially.
				let all_but_last = match block.stmts.len() {
					0 => 0,
					n => n - 1,
				};
				for stmt in &mut block.stmts[0..all_but_last] {
					self.stmt(db, stmt, false)?;
				}

				// If the value isn't used, we can simply type-check the
				// last statement then bail with Void.
				if !value_used {
					block.stmts.last_mut().map(|stmt| self.stmt(db, stmt, false));
					return Ok(db.put_type(Type::Bottom));
				}

				// Otherwise, we need to compute a type for the value.
				// If the block has no statements, that's an error.
				let Some(stmt) = block.stmts.last_mut() else {
					type_error!(self, db, &block.location,
						"Return value of block is used, but the block is empty.");
				};

				// If the block has a statement, defer to self.stmt(). But we
				// need to get a TypId at the end.
				let Some(val) = self.stmt(db, stmt, true)? else {
					type_error!(self, db, &block.location,
						"Return value of block is used, but its last statement has no value.");
				};

				// Return the computed TypId.
				block.typ = val;
				val
			}
		})
	}

	fn stmt(&mut self, db: &mut Db, stmt: &mut Stmt, value_used: bool) -> Result<Option<TypId>> {
		match stmt {
			Stmt::Declare(_) => todo!(),
			Stmt::Expression(expr) => {
				Ok(Some(self.do_type(db, &mut expr.expression, value_used)?))
			},
			Stmt::FunDeclare(_) => todo!(),
			Stmt::Return(ret) => {
				// The return statement is interesting in that it entirely
				// ignores value_used. Because 'return' always returns
				// the bottom type, its value may always be used if needed.
				//
				// That said, it DOES need to always get a value from its inner
				// expr.

				// TODO: CHeck return types
				// Also, there's no need for a separate stack of these...
				// We can do the good old trick where you push/pop as part of
				// the function
				let return_type = *self.return_types.last().unwrap();

				let inner = match &mut ret.expression {
					Some(expr) => expr,
					None => {
						if return_type != db.put_type(Type::Void) {
							type_error!(self, 
								db, &ret.location,
								"Trying to return value in function returning void");
						}

						return Ok(Some(db.put_type(Type::Bottom)));
					},
				};

				let typ = self.do_type(db, inner, true)?;

				// TODO check return_types
				let valid = self.unify_lhs_superset_rhs(db, 
					return_type,
					typ);

				maybe_type_error!(self, 
					valid,
					db, &ret.location,
					"Trying to return {} in function returning {}",
					db.repr_type(typ),
					db.repr_type(return_type));

				Ok(Some(db.put_type(Type::Bottom)))
			}
		}
	}

	fn declare(&mut self, db: &mut Db, declare: &mut Declare) {
		self.do_assign(db, &declare.location, declare.identity, &mut declare.value);
	}

	fn fun_declare(&mut self, db: &mut Db, fun: &mut FunDeclare) -> Result<()> {
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
		let value_used = !db.does_fun_return_void(fun.identity);
		let return_type = db.get_fun_return_typid(fun.identity);

		self.return_types.push(return_type);

		let inner = self.do_type(db, &mut fun.value, value_used)?;

		self.return_types.pop();

		// If we're using the value of the expression, it must match the return
		// type.
		if value_used {
			let valid = self.unify_lhs_superset_rhs(db,
				db.get_fun_return_typid(fun.identity),
				inner);
			maybe_type_error!(self, 
				valid,
				db, &fun.location,
				"Value of function body is {} but function returns {}",
				db.repr_type(inner),
				db.repr_type(db.get_fun_return_typid(fun.identity)));
		}

		Ok(())
	}

	fn module(&mut self, db: &mut Db, module: &mut Module) {
		for global in &mut module.globals {
			self.declare(db, global);
		}

		for fun in &mut module.functions {
			self.fun_declare(db, fun);
		}
	}

	fn typecheck(&mut self, db: &mut Db, modules: &mut Vec<Module>) {
		self.global_scope = true;
		for module in modules {
			self.module(db, module);
		}
	}
}

pub fn typecheck(db: &mut Db, modules: &mut Vec<Module>) -> bool {
	let mut checker = TypeChecker::new();

	checker.typecheck(db, modules);

	checker.had_error
}