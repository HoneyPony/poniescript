use crate::db::*;
use crate::module::Module;
use crate::module::ExprId;
use crate::module::StmtId;
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
}

struct TypeCheckErr;
type Result<T> = std::result::Result<T, TypeCheckErr>;

macro_rules! type_error {
    ($expr:expr, $db:ident, $location:expr, $($arg:tt)*) => {
		match $expr {
			Ok(ty) => ty,
			Err(_) => {
				$db.err_locate($location);
				eprintln!($($arg)*);

				return Err(TypeCheckErr)
			}
		}
    };
}

impl TypeChecker {
	fn new() -> Self {
		TypeChecker {
			had_error: false,

			global_scope: false,
		}
	}

	//fn unify_bi(&mut self, db: &mut Db, ty_a: TypId, ty_b: TypId)

	// Tries to get the expression on the right to have the same type as the one
	// on the left. If the rightward expression does not match, then it is considered
	// an error with the rightward one.
	//fn unify_right(&mut self, db: &mut Db, ty_left: TypId, expr: &mut Expr) -> TypId {
	//	ty_left
	//}

	fn unify_left(&mut self, db: &mut Db, lhs: TypId, rhs: TypId) -> Result<TypId> {
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

			// More branches to come with parameterized types...

			_ => return Err(TypeCheckErr)
		};

		Ok(unified)
	}

	fn unify_bi(&mut self, db: &mut Db, lhs: TypId, rhs: TypId) -> Result<TypId> {
		if lhs == rhs {
			return Ok(lhs);
		}

		// The idea here is that, we try both ways to see if the type can get
		// "stronger", and so if one of them changes, we go with the one
		// that changed.
		//
		// If our unify_left method is sound, it should only be possible for
		// the type values to move in one direction.
		if let Ok(candidate) = self.unify_left(db, lhs, rhs) {
			// TODO: Are these if checks redundant..?
			if candidate != rhs {
				return Ok(candidate);
			}
		}

		// If that didn't do anything, try unifying the other way.
		if let Ok(candidate) = self.unify_left(db, rhs, lhs) {
			if candidate != lhs {
				return Ok(candidate);
			}
		}

		// If neither unification worked, then the types are not compatible
		// in either direction.
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

	fn do_assign(&mut self, module: &mut Module, db: &mut Db, at: &SourceLocation, var: VarId, expr: ExprId) -> Result<TypId> {
		let value = self.do_type(module, db, expr)?;

		let unified = type_error!(
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

	fn do_type(&mut self, module: &mut Module, db: &mut Db, expr: ExprId) -> Result<TypId> {
		let expr = module.get_mut(expr);
		
		Ok(match expr {
			Expr::Binary(binary) => {
				let left = self.do_type(module, db, binary.left)?;
				let right = self.do_type(module, db, binary.right)?;

				let unified = type_error!(
					self.unify_bi(db, left, right),
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
		})
	}

	fn declare(&mut self, module: &mut Module, db: &mut Db, declare: &mut Declare) {
		self.do_assign(module, db, &declare.location, declare.identity, declare.value);
	}

	fn typecheck(&mut self, db: &mut Db, module: &mut Module) {
		for global in &mut module.globals {
			self.declare(module, db, global);
		}
	}
}

pub fn typecheck(db: &mut Db, modules: &mut Vec<Module>) -> bool {
	let mut had_error = false;

	for module in modules {
		let mut checker = TypeChecker::new();
		checker.typecheck(db, module);

		had_error = had_error || checker.had_error;
	}

	had_error
}