
use std::sync::Arc;

use crate::db::*;
use crate::lexer::{Tok, Token};
use crate::module::Module;
use crate::source::SourceLocation;
use crate::typ::{RangeEnd, Type};

use crate::expr::*;
use crate::error::Error;

use poni_arena::IndexCell;

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

	/// Which class we're currently in. Used to give types to SelfVal.
	current_class: Option<TypId>,

	/// A vector of the 'break' statements inside the current loop.
	break_exprs: Vec<ExprId>,
}

struct TypeCheckErr;
type Result<T> = std::result::Result<T, TypeCheckErr>;

// In order to make sure we correctly use the value of each compute() function,
// use a separate error type from TypeCheckErr. Essentially, we have to transform
// Result<.., TypeComputeErr> to Result<.., TypeCheckErr>, which is easiest through
// the maybe_type_error! and similar macros.
struct TypeComputeErr;

macro_rules! maybe_type_error {
    ($self:ident, $expr:expr, $location:expr, $($arg:tt)*) => {
		match $expr {
			Ok(ty) => ty,
			Err(_) => {
				$self.had_error = true;
				$self.db.report_error(Error::simple(
					format!($($arg)*),
					$location.clone()
				));

				return Err(TypeCheckErr)
			}
		}
    };
}

macro_rules! maybe_type_error_with {
	($self:ident, $expr:expr, $error:block) => {
		match $expr {
			Ok(ty) => ty,
			Err(_) => {
				$self.had_error = true;
				$self.db.report_error($error);

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
				$location.clone()
			));

			return Err(TypeCheckErr)
		}
    };
}

const PANIC_ON_BAD_NODE: bool = false;

/// Maps +=, -= etc to their corresponding +, -, etc
fn map_assign_op(op: Tok) -> Tok {
	match op {
		Tok::PlusEqual    => Tok::Plus,
		Tok::MinusEqual   => Tok::Minus,
		Tok::StarEqual    => Tok::Star,
		Tok::SlashEqual   => Tok::Slash,
		Tok::PercentEqual => Tok::Percent,
		_ => unreachable!("ICE: Bad assign operator")
	}
}

impl<'db> TypeChecker<'db> {
	fn new(db: &'db mut Db) -> Self {
		TypeChecker {
			db,

			had_error: false,

			global_scope: false,

			return_types: Vec::new(),

			current_class: None,

			break_exprs: Vec::new(),
		}
	}

	// Takes an expression, and promotes its type as if it were just assigned
	// to an unassigned value.
	//
	// Should be called on any expression that is otherwise not used, which
	// includes:
	// - arguments to print()
	// - statement expressions whose value is not used
	fn promote_from_unassigned(&mut self, ast: &AstProxy, expr: &mut ExprId) -> TypId {
		let ty = expr.typ(ast, self.db);
		let promoted = self.promote_ty_from_unassigned(ty);

		if promoted == self.db.types.bottom {
			// If we are promoting to the Bottom type, that means the expression
			// value does not actually get created. What should we do in this
			// case? Is it safe to skip the promotion?

			// At this point, we really do want to promote to Bottom. But,
			// it should then recursively call do_promote, which will instead
			// promote_from_unassigned for inner expressions.
			self.really_do_promote_expr(ast, expr, promoted);
			return promoted;
		}

		// Here we must now unconditionally promote. This is because we are
		// expecting do_promote to be called on every expression exactly once,
		// and this is done through promote_from_unassigned sometimes.
		self.do_promote_expr(ast, expr, promoted);

		promoted
	}

	// In order to properly promote to the Bottom type, we need to separaretly
	// promote as if the value was unused.
	//
	// This is for cases like { 1 } * { return 5 }; where the left-hand side
	// will be promoted to Bottom, but that doesn't do anything inside expr.promote.
	//
	// Instead, we have to manually promote that case.
	//
	// TODO: Is this correct? More testing is needed. It would also be nice to

	fn promote_ty_from_unassigned(&mut self, ty: TypId) -> TypId {
		if ty == self.db.types.assume_float {
			return self.db.types.float;
		}

		if ty == self.db.types.assume_int {
			return self.db.types.int;
		}

		// Add other promotions as needed. Note that this should correspond
		// in part to the match() in compute_assignable.
		match self.db.get(ty) {
			Type::ArrayOf(inner) => {
				let inner_promoted = self.promote_ty_from_unassigned(*inner);
				self.db.put_type(Type::ArrayOf(inner_promoted))
			},
			Type::Tuple(inner) => {
				// TODO: Allocate needed size from start
				let mut inner_promoted = Vec::new();

				// Big TODO: Fix this nonsense. Grr.
				let inner = inner.clone();
				for ty in inner.iter() {
					inner_promoted.push(self.promote_ty_from_unassigned(*ty));
				}
				self.db.put_type(Type::Tuple(Arc::from(inner_promoted)))
			}
			Type::Option(inner) => {
				let inner_promoted = self.promote_ty_from_unassigned(*inner);
				self.db.put_type(Type::Option(inner_promoted))
			}
			Type::RangeOf(l, r, inner) => {
				// Bindings for borrow checker
				let l = *l; let r = *r;
				let inner_promoted = self.promote_ty_from_unassigned(*inner);
				self.db.put_type(Type::RangeOf(l, r, inner_promoted))
			}
			_ => ty
		}
	}

	// Returns what the new "from" type would be.
	fn compute_assignable(&mut self, to: TypId, from: TypId) -> std::result::Result<TypId, TypeComputeErr> {
		if to == from { return Ok(to); }

		let ty_to = self.db.get(to);
		let ty_from = self.db.get(from);

		match (ty_to, ty_from) {
			(_, Type::Bottom) => return Ok(from),

			(Type::Int, Type::AssumeInt) => return Ok(to),

			// Ints and all Assume types promote to float.
			(Type::Float, Type::AssumeInt | Type::AssumeFloat | Type::Int) => return Ok(to),

			// StrConst promotes to Str.
			(Type::Str, Type::StrConst) => return Ok(to),

			// StrConst and Str both promote to StrBuf.
			(Type::StrBuf, Type::StrConst | Type::Str) => return Ok(to),

			(Type::ArrayOf(lhs), Type::ArrayOf(rhs)) => {
				let elem_typ = self.compute_assignable(*lhs, *rhs)?;
				return Ok(self.db.put_type(Type::ArrayOf(elem_typ)));
			}

			// Array promotes to DynArray.
			(Type::DynArrayOf(lhs, _), Type::ArrayOf(rhs)) => {
				let elem_typ = self.compute_assignable(*lhs, *rhs)?;
				let arr_typ = self.db.put_type(Type::ArrayOf(elem_typ));
				return Ok(self.db.put_type(Type::DynArrayOf(elem_typ, arr_typ)));
			}

			(Type::DynArrayOf(lhs, _), Type::DynArrayOf(rhs, _)) => {
				let elem_typ = self.compute_assignable(*lhs, *rhs)?;
				let arr_typ = self.db.put_type(Type::ArrayOf(elem_typ));
				return Ok(self.db.put_type(Type::DynArrayOf(elem_typ, arr_typ)));
			}

			(Type::Tuple(lhs), Type::Tuple(rhs)) => {
				// TODO: Let us assign a bigger tuple to a smaller tuple..?
				// maybe not.
				if lhs.len() != rhs.len() {
					return Err(TypeComputeErr);
				}

				let mut new_from = Vec::new();

				// Big TODO: Fix this nonsense. Grr.
				let lhs = lhs.clone();
				let rhs = rhs.clone();

				for (l, r) in lhs.iter().zip(rhs.iter()) {
					new_from.push(self.compute_assignable(*l, *r)?);
				}

				return Ok(self.db.put_type(Type::Tuple(Arc::from(new_from))))
			}

			(Type::RangeOf(la, ra, left), Type::RangeOf(lb, rb, right)) => {
				// Both ends of the range must be the same. We could eventually
				// let e.g. int..=int be assignable to int..int, as there is
				// a natural interpretation.
				if *la != *lb { return Err(TypeComputeErr); }
				if *ra != *rb { return Err(TypeComputeErr); }

				let la = *la; let ra = *ra;

				let inner = self.compute_assignable(*left, *right)?;
				return Ok(self.db.put_type(Type::RangeOf(la, ra, inner)));
			}

			(Type::Option(lhs), Type::Option(rhs)) => {
				let inner = self.compute_assignable(*lhs, *rhs)?;
				return Ok(self.db.put_type(Type::Option(inner)))
			}

			(Type::Option(lhs), _) => {
				// If the from type is equal to lhs, we can promote. This is
				// the "implicit some" rule.
				if *lhs == from {
					return Ok(to);
				}
				// In theory, it is assignable if lhs = some from is valid, 
				// which means e.g. var x: Animal? = new Horse{} should work,
				// so we have to call through compute_assignable.
				//
				// This will make the codegen logic more annoying, once we
				// actually implement it for real.
				let inner = self.compute_assignable(*lhs, from)?;
				return Ok(self.db.put_type(Type::Option(inner)));
			}

			(Type::Fun(sig), Type::Fun(sig2)) => {
				if *sig == self.db.sig_unassigned {
					return Ok(from);
				}

				if *sig == *sig2 {
					return Ok(to);
				}

				// TODO: Compute if each sig arg is assignable, e.g.
				// class Dog extends Animal, fun(Animal) may be assigned to fun(Dog)
				// (contravariance)

				return Err(TypeComputeErr);
			}

			// Both unassigned is an error.
			(Type::Unassigned, Type::Unassigned) => return Err(TypeComputeErr),

			// Anything being assigned to Unassigned is just the same thing
			// as a promote_ty_from_unassigned.
			(Type::Unassigned, _) => {
				// Promote from unassigned.
				Ok(self.promote_ty_from_unassigned(from))
			},

			// Just the RHS unassigned is fine.
			(_, Type::Unassigned) => return Ok(to),
			
			// Everything else is an error.
			_ => return Err(TypeComputeErr)
		}
	}

	// Wrapper that does logging.
	fn compute_intersect(&mut self, bottom_eats: bool, left: TypId, right: TypId) -> std::result::Result<TypId, TypeComputeErr> {
		let result = self.compute_intersect_nolog(bottom_eats, left, right);
		// Only log if left != right, because those are the interesting cases.
		if left != right {
			match result {
				Ok(result) => log::trace!("compute_intersect {} + {} -> {}",
					self.db.repr_type(left),
					self.db.repr_type(right),
					self.db.repr_type(result)),
				Err(_) => log::trace!("compute_intersect {} + {} -> ERR",
					self.db.repr_type(left),
					self.db.repr_type(right)),
			}
		}

		result
	}

	// Computes the common "intersection" type of the two types. This can result
	// in promotions, e.g. from int to float (even though float is technically
	// not an intersection of float and int), while it can also result in 
	// "restrictions" (e.g. AssumeInt -> Float).
	//
	// Finally, one thing to note is the intersection of Bottom with anything
	// is itself.
	fn compute_intersect_nolog(&mut self, bottom_eats: bool, left: TypId, right: TypId) -> std::result::Result<TypId, TypeComputeErr> {
		if left == right { return Ok(left); }

		let ty_left = self.db.get(left).clone();
		let ty_right = self.db.get(right).clone();

		match (ty_left, ty_right) {
			// For something like 5 + return; we do want the bottom to take over
			// the expression.
			//
			// But for something like x = if blah { 5 } else { return }; we 
			// actually want the type to be integer. So, Bottom sort of needs
			// its own rule here...
			(Type::Bottom, _) => if bottom_eats { return Ok(left) } else { return Ok(right) },
			(_, Type::Bottom) => if bottom_eats { return Ok(right) } else { return Ok(left) },
		
			(Type::Float, Type::AssumeInt | Type::AssumeFloat | Type::Int) => return Ok(left),
			(Type::AssumeInt | Type::AssumeFloat | Type::Int, Type::Float) => return Ok(right),

			(Type::Int, Type::AssumeInt) => return Ok(left),
			(Type::AssumeInt, Type::Int) => return Ok(right),

			(Type::AssumeFloat, Type::AssumeInt) => return Ok(left),
			(Type::AssumeInt, Type::AssumeFloat) => return Ok(right),

			// In order to match up with the assignment semantics, and for e.g.
			// misc/array_of_strbuf, we need the intersection of various string
			// types to be the "most generic" available one.
			(Type::StrBuf, Type::Str | Type::StrConst) => return Ok(left),
			(Type::Str | Type::StrConst, Type::StrBuf) => return Ok(right),
			(Type::Str, Type::StrConst) => return Ok(left),
			(Type::StrConst, Type::Str) => return Ok(right),

			// TODO: Is this correct?
			// It seems necessary for array_nested_empty_lhs, array_nested_empty_rhs
			(Type::Unassigned, _) => return Ok(right),
			(_, Type::Unassigned) => return Ok(left),

			(Type::ArrayOf(lhs), Type::ArrayOf(rhs)) => {
				// For arrays, the intersection is the intersection of their inner
				// types.
				let inner = self.compute_intersect(bottom_eats, lhs, rhs)?;
				// TODO: We used to have an optimization that involved returning
				// an existing TypId if we have one. It might be nice to keep
				// doing that...
				return Ok(self.db.put_type(Type::ArrayOf(inner)));
			}

			(Type::DynArrayOf(lhs, _), Type::DynArrayOf(rhs, _)) => {
				let elem_typ = self.compute_assignable(lhs, rhs)?;
				let arr_typ = self.db.put_type(Type::ArrayOf(elem_typ));
				return Ok(self.db.put_type(Type::DynArrayOf(elem_typ, arr_typ)));
			}

			(Type::Option(lhs), Type::Option(rhs)) => {
				// Same idea as Array.
				let inner = self.compute_intersect(bottom_eats, lhs, rhs)?;
				return Ok(self.db.put_type(Type::Option(inner)));
			}
			// The following two rules are for implict-Some, e.g. in an array
			// of [nil, new Horse {}, nil]
			(Type::Option(lhs), _) => {
				let inner = self.compute_intersect(bottom_eats, lhs, right)?;
				return Ok(self.db.put_type(Type::Option(inner)));
			}
			(_, Type::Option(rhs)) => {
				let inner = self.compute_intersect(bottom_eats, left, rhs)?;
				return Ok(self.db.put_type(Type::Option(inner)));
			}

			(Type::Tuple(lhs), Type::Tuple(rhs)) => {
				// TODO: Let us assign a bigger tuple to a smaller tuple..?
				// maybe not.
				if lhs.len() != rhs.len() {
					return Err(TypeComputeErr);
				}

				let mut inner = Vec::new();

				// Big TODO: Fix this nonsense. Grr.
				let lhs = lhs.clone();
				let rhs = rhs.clone();

				for (l, r) in lhs.iter().zip(rhs.iter()) {
					inner.push(self.compute_intersect(bottom_eats, *l, *r)?);
				}

				return Ok(self.db.put_type(Type::Tuple(Arc::from(inner))))
			}

			(Type::RangeOf(la, ra, left), Type::RangeOf(lb, rb, right)) => {
				// Both ends of the range must be the same. We could eventually
				// let e.g. int..=int be assignable to int..int, as there is
				// a natural interpretation.
				if la != lb { return Err(TypeComputeErr); }
				if ra != rb { return Err(TypeComputeErr); }

				let inner = self.compute_intersect(bottom_eats, left, right)?;
				return Ok(self.db.put_type(Type::RangeOf(la, ra, inner)));
			}

			_ => return Err(TypeComputeErr)
		}
	}

	/// Computes the intersection between two types such as (int, float) against float,
	/// or also (((int, int), float), (AssumeFloat)) against float.
	/// 
	/// Note that bottom_eats should be irrelevant, but that's OK.
	fn compute_scalar_tuple_intersect(&mut self, bottom_eats: bool, scalar: TypId, vector: TypId) -> std::result::Result<TypId, TypeComputeErr> {
		if scalar == vector { panic!("ICE: compute_scalar_tuple_intersect where the types match") }

		let ty_vector = self.db.get(vector).clone();

		match ty_vector {
			Type::Tuple(inner) => {
				let mut new_inner = Vec::new();
				for i in inner.iter() {
					if self.is_scalar(*i) {
						// For scalar types, we compute the normal intersect between
						// the overall scalar and the tuple member.
						new_inner.push(self.compute_intersect(bottom_eats, scalar, *i)?);
					}
					else {
						// Otherwise, we recursively compute the scalar-vector intersect.
						new_inner.push(self.compute_scalar_tuple_intersect(bottom_eats, scalar, *i)?);
					}
				}

				Ok(self.db.put_type(Type::Tuple(Arc::from(new_inner))))
			},
			_ => {
				panic!("ICE: compute_scalar_tuple_intersect where the vector isn't a vector")
			}
		}
	}

	/// Promotion in the new system works as follows.
	/// 
	/// We ONLY need to promote when a value is actually assigned to something.
	/// If a value is not assigned, it is not actually used, and so it does
	/// not need to be promoted. That said, these values are still assigned to
	/// an "unassigned" type so that they get a valid type.
	/// 
	/// We walk down the tree of this value and recursively try to promote to
	/// the target type. If a particular node can't be promoted to the target type,
	/// then it must be promoted at run-time, so we synthesize an Expr::Promote
	/// node.
	fn do_promote_expr(&mut self, ast: &AstProxy, expr_id: &mut ExprId, promote_to: TypId) {
		if promote_to == self.db.types.bottom {
			// Special case: If promoting to bottom, promote to unassigned instead
			// This will then push further promotions to really_
			self.promote_from_unassigned(ast, expr_id);
			return;
		}

		// Otherwise, just jump straight into really_
		self.really_do_promote_expr(ast, expr_id, promote_to);
	}

	/// Computes whether a given promotion is unsynthesizable.
	/// 
	/// This applies to something like assign an Array of a particular type
	/// to an Array of an incompatible one, or an Array[int] to a DynArray[int].
	/// 
	/// In these cases, we cannot actually synthesize the promotion, because it  
	/// would implicitly copy, which is not what we want.
	/// 
	/// This might not be the best way to implement this -- maybe we should
	/// instead change how we determine which expressions are promotable.
	/// But this should work for now.
	fn promote_is_unsynthesizable(&self, assign_to: TypId, assign_from: TypId) -> bool {
		// These are always valid.
		if assign_to == assign_from { return false; }

		let to = self.db.get(assign_to);
		let from = self.db.get(assign_from);
		
		match (to, from) {
			(Type::ArrayOf(_), Type::ArrayOf(_)) => {
				// Not allowed.
				//
				// Note that in the future, if we have something like
				// ReadonlyArray[Animal], an array of Horse would in
				// theory be valid to assign to this.
				return true;
			}
			(Type::DynArrayOf(..), Type::DynArrayOf(..)) => {
				// Not allowed.
				return true;
			}
			(Type::DynArrayOf(..), Type::ArrayOf(_)) => {
				// Not allowed.
				return true;
			}
			(Type::Tuple(to), Type::Tuple(from)) => {
				if to.len() != from.len() { return false; }

				for i in 0..to.len() {
					if self.promote_is_unsynthesizable(to[i], from[i]) {
						return true;
					}
				}

				return false;
			}
			_ => {
				// Everything else is allowed, I guess.
				return false;
			}
		}
	}
	
	/// DO NOT CALL THIS FUNCTION UNLESS YOU ARE do_promote_expr OR promote_from_unassigned.
	/// 
	/// This function will promote either to a Bottom type (i.e. promote_from_unassigned)
	/// or from no type, on the child nodes. It is mainly use to implement pushing
	/// Bottom types down the tree properly in promote_from_unassigned.
	/// 
	/// Note that it does contain the main "meat" of the do_promote_expr function.
	fn really_do_promote_expr(&mut self, ast: &AstProxy, expr_id: &mut ExprId, promote_to: TypId) {
		// No need to promote if we're already the right type. (?)
		//
		// NOTE: Uncommenting this code results in a bunch of failing tests.
		// That's weird. I think though it's probably because we aren't pushing
		// the promotions down the tree (we must call promote_expr on every
		// node exactly once). I guess the solution is to let Expr::Promote
		// be promoted.
		// if ast.get_expr(*expr_id).typ(ast, &self.db) == promote_to {
		// 	return;
		// }

		// First, we visit the child expr with promote_expr.
		self.promote_expr(ast, *expr_id, promote_to);

		// If the child node's type does NOT equal the promoted type, we synthesize
		// a runtime promotion.
		if ast.get_expr(*expr_id).typ(ast, &self.db) != promote_to {
			let promote_from = ast.get_expr(*expr_id).typ(ast, &self.db);

			log::trace!("synthesizing Promote: {:?}: {} -> {}",
				ast.get_expr(*expr_id).as_ref(),
				self.db.repr_type(promote_from),
				self.db.repr_type(promote_to));

			if self.promote_is_unsynthesizable(promote_to, promote_from) {
				self.had_error = true;
				let msg = format!("Invalid promotion from {} to {}.",
					self.db.repr_type(promote_from), self.db.repr_type(promote_to));
				self.db.report_error(Error::simple(msg, expr_id.location(ast)));
			}

			let id = ast.exprs.push(Expr::Promote(Promote {
				location: ast.get_expr(*expr_id).location().clone(),
				inner: *expr_id,
				promote_to
			}));

			*expr_id = id;
		}
	}

	fn do_promote_stmt(&mut self, ast: &AstProxy, stmt_id: StmtId, promote_to: TypId) {
		let mut binding = ast.stmts.get_mut(stmt_id);
		let stmt = binding.as_mut();

		match stmt {
			Stmt::Declare(_) => {
				// Should have already promoted.
			},
			Stmt::Expression(expression) => {
				self.do_promote_expr(ast, &mut expression.expression, promote_to);
			},
			Stmt::ClassDeclare(_) => {
				// Should have already promoted.
			},
		}
	}
	
	fn promote_expr(&mut self, ast: &AstProxy, expr_id: ExprId, promote_to: TypId) {
		let mut binding = ast.exprs.get_mut(expr_id);
		let expr = binding.as_mut();

		match expr {
			Expr::Binary(binary) => {
				// Promote children to own type if we already have a concrete type,
				// otherwise to the incoming type (in which case that becomes our
				// type).
				if self.db.is_not_concrete(binary.typ) {
					binary.typ = promote_to;
				}

				let left_ty = binary.left.typ(ast, self.db);
				let right_ty = binary.right.typ(ast, self.db);

				// For scalar-vec ops, we promote the vec to match our own type,
				// and promote the scalar from unassigned.
				if self.is_vec(left_ty) && self.is_scalar(right_ty) {
					self.do_promote_expr(ast, &mut binary.left, binary.typ);
					self.promote_from_unassigned(ast, &mut binary.right);
				} 
				else if self.is_scalar(left_ty) && self.is_vec(right_ty) {
					self.promote_from_unassigned(ast, &mut binary.left);
					self.do_promote_expr(ast, &mut binary.right, binary.typ);
				}
				else {
					self.do_promote_expr(ast, &mut binary.left, binary.typ);
					self.do_promote_expr(ast, &mut binary.right, binary.typ);
				}
			},
			Expr::MakeRange(range) => {
				if self.db.is_not_concrete(range.typ) {
					// Ensure that we are actually getting a range type
					assert!(matches!(self.db.get(promote_to), Type::RangeOf(..)));
					range.typ = promote_to;
				}

				let inner = *match self.db.get(range.typ) {
					Type::RangeOf(.., typ) => typ,
					_ => panic!("ICE: MakeRange promotion has non-range type"),
				};

				self.do_promote_expr(ast, &mut range.left, inner);
				self.do_promote_expr(ast, &mut range.right, inner);
			}
			Expr::Unary(unary) => {
				if self.db.is_not_concrete(unary.typ) {
					unary.typ = promote_to;
				}

				self.do_promote_expr(ast, &mut unary.inner, unary.typ);
			}
			Expr::OptionElse(opt_else) => {
				log::trace!("promote_expr: OptionElse: promote_to = {}", self.db.repr_type(promote_to));

				if self.db.is_not_concrete(opt_else.typ) {
					opt_else.typ = promote_to;
				}

				// Must promote the left branch to the Option[T]. This is the
				// Option[T] of our overall type, so this can't fail.

				let option_ty = self.db.put_type(Type::Option(opt_else.typ));

				self.do_promote_expr(ast, &mut opt_else.value, option_ty);
				// Promote otherwise to our overall type, as it must be the
				// type we are trying to else into.
				self.do_promote_expr(ast, &mut opt_else.otherwise, opt_else.typ);
			}
			Expr::Lerp(lerp) => {
				if self.db.is_not_concrete(lerp.typ) {
					lerp.typ = promote_to;
				}

				self.do_promote_expr(ast, &mut lerp.from, lerp.typ);
				self.do_promote_expr(ast, &mut lerp.to, lerp.typ);
				// amount already rpomoted
			}
			Expr::Comparison(_) => {
				// We can't promote to any type, so we should have already
				// promoted our children.
			},
			Expr::Variable(_) => { /* Can't promote. */ },
			Expr::Logical(_) => { /* Can't promote. */ },
			Expr::FunCall(_) => { /* Can't promote. */ },
			Expr::BuiltinCall(_) => { /* Can't promote. */ }
			Expr::BuiltinCapture(_) => {
				// TODO: This should return an error, as it means we have
				// a BuiltinCapture that wasn't eaten by a ValCall. For now,
				// stuff will just explode later.
			}
			Expr::FunDeclare(_) => {},
			Expr::ValCall(_) => {},
			Expr::FunCapture(_) => {},
			Expr::Assign(_) => {},
			Expr::UnboundAssign(_) => { if PANIC_ON_BAD_NODE { panic!("ICE: promote_expr UnboundAssign") } },
			Expr::NumLiteral(num_literal) => {
				// Promote to the incoming type.
				num_literal.typ = promote_to;
			},
			Expr::StrLiteral(_) => {
				// For now: Don't promote, promote in codegen stage.
				// OPT: Promote here, let the codegen make better use of information?
			},
			Expr::BoolLiteral(_) => {},
			Expr::Block(block) => {
				// Promote the last statement.
				if let Some(last) = block.stmts.last() {
					self.do_promote_stmt(ast, *last, promote_to);
					// Our type now reflects that statement's type (so, we could
					// promote if it could).
					block.typ = ast.get_stmt(*last).typ(ast, &self.db);
				}
			},
			Expr::If(if_) => {
				if self.db.is_not_concrete(if_.typ) {
					if_.typ = promote_to;
				}

				// Promote if branches here.
				self.do_promote_expr(ast, &mut if_.then_branch, if_.typ);
				if let Some(else_) = if_.else_branch.as_mut() {
					self.do_promote_expr(ast, else_, if_.typ);
				}
			},
			// Loop is basically like an if with any number of branches.
			Expr::Loop(loop_) => {
				if self.db.is_not_concrete(loop_.typ) {
					loop_.typ = promote_to;
				}

				for break_ in &loop_.breaks {
					let mut break_ = ast.get_expr_mut(*break_);
					let Expr::Break(break_) = break_.as_mut() else { unreachable!(); };

					// Promote every 'branch' of the loop to the now-concrete
					// type of the loop.
					if let Some(inner) = break_.value.as_mut() {
						self.do_promote_expr(ast, inner, promote_to);
					}
				}
			}
			Expr::WhileLoop(_) => {
				// For now, there is nothing to promote.
			}
			Expr::ForLoop(_) => {
				panic!("ICE: Tried to promote ForLoop: Should have been lowered before promotion")
			}
			Expr::Break(_) => {
				// The inner expression of the break is promoted by the Loop,
				// not by the Break. See above.
			}
			Expr::Continue(_) => {
				// No value to promote.
			}
			Expr::Return(_) => {
				// Should have already promoted.
			}
			Expr::Unbound(_) => if PANIC_ON_BAD_NODE { panic!("ICE: promote_expr(Unbound)") },
			Expr::UnboundFunCapture(_) => if PANIC_ON_BAD_NODE { panic!("ICE: promote_expr(UnboundFunCapture)") },
			Expr::Print(_) => {},
			Expr::Str(_) => { /* TODO: Possibly promote here, to improve stuff in backend? */ },
			Expr::New(_) => {},
			Expr::Get(_) => {},
			Expr::Set(_) => {},
			Expr::SelfVal(_) => {},
			Expr::ArrayLit(array_lit) => {
				fn unwrap_array_type(checker: &mut TypeChecker, id: TypId) -> TypId {
					match checker.db.get(id) {
						Type::ArrayOf(elem) => *elem,
						Type::DynArrayOf(elem, _) => *elem,
						Type::Option(id) => {
							// It's unfortunate but we kind of have to explicitly
							// unwrap the option type here. I wonder if there is a way
							// to do this more nicely, so that we don't have to explicitly
							// check every single composition of relevant types...
							unwrap_array_type(checker, *id)
						}
						_ => panic!("ICE: promote_expr(ArrayLit) to non-array type {}", checker.db.repr_type(id))
					}
				}

				let incoming_elem_typ = unwrap_array_type(self, promote_to);

				// The ArrayLit should be kind of like a big binary expression.
				// If we already have a concrete type, e.g. because we are an
				// array of float variables, then we can't promote to e.g. an
				// array of int, and we should actually eventually get an 
				// error.
				if self.db.is_not_concrete(array_lit.elem_typ) {
					array_lit.elem_typ = incoming_elem_typ;
					array_lit.arr_typ = promote_to;
				}

				if matches!(self.db.get(array_lit.arr_typ), Type::ArrayOf(_)) &&
					matches!(self.db.get(promote_to), Type::DynArrayOf(..)) {
					// Promote to incoming DynArray.
					array_lit.elem_typ = incoming_elem_typ;
					array_lit.arr_typ = promote_to;
				}

				// Promote all child nodes to our final elem type.
				for expr in &mut array_lit.values {
					self.do_promote_expr(ast, expr, array_lit.elem_typ);
				}
			},
			Expr::Index(_) => {},
			Expr::SetIndex(_) => {},
			Expr::MakeTuple(make_tuple) => {
				// This is also kind of like a big binary expression.
				if self.db.is_not_concrete(make_tuple.typ) {
					make_tuple.typ = promote_to;
				}

				let elem_typs = match self.db.get(make_tuple.typ) {
					Type::Tuple(vec) => vec.clone(), // TODO: Don't clone
					_ => panic!("ICE: promote_expr(MakeTuple) to non-tuple type"),
				};

				if elem_typs.len() != make_tuple.values.len() {
					panic!("ICE: promote_expr(MakeTuple) to wrong tuple size");
				}

				// Promote child nodes to fit into the final type.
				for (expr, typ) in make_tuple.values.iter_mut().zip(elem_typs.iter()) {
					self.do_promote_expr(ast, expr, *typ);
				}
			},
			Expr::Promote(promote) => {
				// If we hit this, it means we're re-writing an earlier promote
				// with a different one.
				//
				// I'm actually not sure if that is a valid thing to do.
				//
				// For now, we will just let the promote_to change. In the
				// future, we need to probably re-check that the promotion is
				// valid.
				log::trace!("promote Expr::Promote from {} to {} (inner is {})",
					self.db.repr_type(promote.promote_to), self.db.repr_type(promote_to),
					self.db.repr_type(promote.inner.typ(ast, self.db)));

				promote.promote_to = promote_to;
			},
			Expr::MakeSumType(sum) => {
				// Pretend that in the future, Type::Option will be used for
				// all sum types. I think that will have to be the way that this
				// evolves, essentially.
				let _incoming_inner_typ = match self.db.get(promote_to) {
					Type::Option(inner) => *inner,

					// Also, because promotion is sometimes where the type is
					// assigned at all, this will have to be a regualr user-facing
					// error.
					_ => panic!("ICE: promote_expr(MakeSumType) to non-option (sum) type {}", self.db.repr_type(promote_to))
				};
				
				if self.db.is_not_concrete(sum.typ) {
					sum.typ = promote_to;
				}
			}
			Expr::Undefined(_) => panic!("ICE: promote_expr(Undefined)"),
		}
	}

	fn check_assign(&mut self, ast: &AstProxy, at: &SourceLocation, var: VarId, expr_id: &mut ExprId, assign_ty: bool) -> Result<TypId> {
		let value = self.check_expr(ast, *expr_id, true)?;

		if self.db.get_var_type(var) == self.db.types.unassigned && value == self.db.types.unassigned {
			type_error!(self, at, "Invalid assignment: Type annotations needed.");
		}

		let computed =
			self.compute_assignable(self.db.get_var_type(var), value);

		// TODO: This will have to NOT be done in certain new{} expressions.
		if self.db.get(var).readonly {
			let readonly_error = Error::simple(
				format!("Invalid assignment to '{}': cannot be written to.",
					self.db.repr_var(var)),
				at.clone()
			);
			self.had_error = true;
			self.db.report_error(readonly_error);
		}

		let computed = maybe_type_error!(
			self,
			computed,

			at,
			"Invalid assignment to '{}': need {}, but value is {}",
			self.db.repr_var(var),
			self.db.repr_var_type(var),
			self.db.repr_type(value)
		);

		// Only assign the type if we're in a declaration.
		if assign_ty && self.db.get_var_type(var) == self.db.types.unassigned {
			if computed == self.db.types.void {
				type_error!(self, at,
				"Variable '{}' is type 'void' which is not a valid type for a variable.",
				self.db.repr_var(var));
			}

			if computed == self.db.types.bottom {
				type_error!(self, at, "Variable '{}' is type 'bottom' which is not a valid type for a variable.",
					self.db.repr_var(var));
			}

			// If the type is not concrete, it's also not valid, e.g.
			//     var x = nil;
			if self.db.is_not_concrete(computed) {
				type_error!(self, at, "Variable '{}' is type '{}' which is invalid. The variable may require a type annotation.",
					self.db.repr_var(var), self.db.repr_type(computed));
			}

			self.db.get_mut(var).typ = computed;
		}
		log::trace!("check_assign: computed {}", self.db.repr_type(computed));
		self.do_promote_expr(ast, expr_id, computed);

		Ok(computed)
	}

	fn is_numeric_or_vec(&self, typ: TypId) -> bool {
		match self.db.get(typ) {
			// TODO:
			// We allow bottom here, but I'm not sure that's actually necessary.
			// In particular, it is useful to be able to do e.g. 1 + if(cond) {
			// thing } else { return; } but in that case the type is int, not
			// bottom. I'm not sure.
			Type::Bottom => true,

			Type::Int | Type::Float => true,
			Type::AssumeInt | Type::AssumeFloat => true,
			Type::Tuple(inner) => {
				let sad = inner.clone();
				for typ in sad.iter() {
					if !self.is_numeric_or_vec(*typ) { return false; }
				}
				true
			},
			_ => false
		}
	}

	fn is_vec(&self, typ: TypId) -> bool {
		match self.db.get(typ) {
			// Unlike is_numeric_or_vec, we definitely don't want Bottom
			// to be is_vec or is_scalar, because we use these functions to
			// decide whether to do certain operations.
			Type::Bottom => false,

			Type::Tuple(inner) => {
				let sad = inner.clone();
				for typ in sad.iter() {
					if !self.is_numeric_or_vec(*typ) { return false; }
				}
				true
			},
			_ => false
		}
	}

	fn is_scalar(&self, typ: TypId) -> bool {
		match self.db.get(typ) {
			Type::Bottom => false,

			Type::Int | Type::Float => true,
			Type::AssumeInt | Type::AssumeFloat => true,

			_ => false,
		}
	}

	// TODO: We could, inside this function, just directly call
	// promote_from_unassigned on any expr that has value_used = false -- we
	// should consider if that would make sense.
	fn check_expr(&mut self, ast: &AstProxy, expr_id: ExprId, value_used: bool) -> Result<TypId> {
		let mut binding = ast.exprs.get_mut(expr_id);
		let expr = binding.as_mut();
		log::trace!("check_expr: {:?}", expr);
		let result = Ok(match expr {
			Expr::Binary(binary) => {
				let left = self.check_expr(ast, binary.left, true)?;
				let right = self.check_expr(ast, binary.right, true)?;

				if self.is_vec(left) && self.is_scalar(right) {
					let computed = maybe_type_error!(
						self,
						self.compute_scalar_tuple_intersect(true, right, left),

						&binary.location,
						"Invalid operands to binary operator: LHS is {}, RHS is {}",
						self.db.repr_type(left),
						self.db.repr_type(right)
					);

					// Vec op Scalar -- the result type is the vec type. Promotion
					// occurs later...
					binary.typ = computed;
				}
				else if self.is_scalar(left) && self.is_vec(right) {
					let computed = maybe_type_error!(
						self,
						self.compute_scalar_tuple_intersect(true, left, right),

						&binary.location,
						"Invalid operands to binary operator: LHS is {}, RHS is {}",
						self.db.repr_type(left),
						self.db.repr_type(right)
					);

					binary.typ = computed;
				}
				else {
					let computed = maybe_type_error!(
						self,
						self.compute_intersect(true, left, right),

						&binary.location,
						"Invalid operands to binary operator: LHS is {}, RHS is {}",
						self.db.repr_type(left),
						self.db.repr_type(right)
					);

					if !self.is_numeric_or_vec(computed) {
						type_error!(self,
							binary.location,
							"Invalid operands to binary operator: Type is not numerical"
						);
					}

					// PROMOTION: occurs in promote_expr

					binary.typ = computed;
				}
					
				binary.typ
			},

			Expr::MakeRange(range) => {
				let left = self.check_expr(ast, range.left, true)?;
				let right = self.check_expr(ast, range.right, true)?;

				let computed = maybe_type_error!(
					self,
					self.compute_intersect(true, left, right),

					&range.location,
					"Invalid operands for range: LHS is {}, RHS is {}",
					self.db.repr_type(left),
					self.db.repr_type(right)
				);

				// PROMOTION: occurs in promote_expr

				range.typ = self.db.put_type(Type::RangeOf(range.left_end,
					range.right_end, computed));

				range.typ
			}

			Expr::Unary(unary) => {
				let inner = self.check_expr(ast, unary.inner, value_used)?;
				
				if !self.is_numeric_or_vec(inner) {
					type_error!(self,
						unary.location,
						"Invalid operand to unary operator: Type is not numerical"
					);
				}

				// PROMOTION: occurs in promote_expr

				unary.typ = inner;
				unary.typ
			}
			Expr::Lerp(lerp) => {
				let left = self.check_expr(ast, lerp.from, true)?;
				let right = self.check_expr(ast, lerp.to, true)?;

				let computed = maybe_type_error!(
					self,
					self.compute_intersect(true, left, right),

					&lerp.location,
					"Invalid operands to lerp: 'from' is {}, 'to' is {}",
					self.db.repr_type(left),
					self.db.repr_type(right)
				);

				let amount = self.check_expr(ast, lerp.amount, true)?;
				if amount != self.db.types.bool {
					// If amount is not a bool, promote to float. (We could also
					// potentially promote to int in some cases...)
					self.do_promote_expr(ast, &mut lerp.amount, self.db.types.float);
				}

				lerp.typ = computed;

				computed
			}
			Expr::ArrayLit(lit) => {
				let mut final_ty: TypId = self.db.types.unassigned;

				if let Some((first, rest)) = lit.values.split_first_mut() {
					final_ty = self.check_expr(ast, *first, true)?;
					for value in rest {
						let value_ty = self.check_expr(ast, *value, true)?;
						
						final_ty = maybe_type_error!(
							self,
							self.compute_intersect(true, final_ty, value_ty),
							
							&lit.location,
							"Invalid type of array elements: {} vs {}",
							self.db.repr_type(final_ty),
							self.db.repr_type(value_ty)
						);
					}
				}

				// PROMOTION: occurs in promote_expr

				lit.elem_typ = final_ty;
				if lit.elem_typ != self.db.types.unassigned {
					lit.arr_typ = self.db.put_type(Type::ArrayOf(lit.elem_typ));
				}

				lit.arr_typ
			}
			Expr::Comparison(compare) => {
				let left = self.check_expr(ast, compare.left, true)?;
				let right = self.check_expr(ast, compare.right, true)?;

				let computed = maybe_type_error!(
					self,
					self.compute_intersect(true, left, right),

					&compare.location,
					"Invalid operands to comparison: LHS is {}, RHS is {}",
					self.db.repr_type(left),
					self.db.repr_type(right)
				);

				// The comparison, besides promoting its operands as needed,
				// also "consumes" them like a function, so they should be
				// promoted from AssumeInt, etc.
				let computed = self.promote_ty_from_unassigned(computed);	
				
				// Keep track of the type that we're "doing the comparison as."
				compare.compare_as = computed;

				self.do_promote_expr(ast, &mut compare.left, computed);
				self.do_promote_expr(ast, &mut compare.right, computed);

				// Comparisons always return bool.
				self.db.types.bool
			},
			Expr::Logical(logical) => {
				// We are expecting a real kind of value from the sub-expressions
				// (namely a bool, or promotable to bool), so we must say that the
				// value is used.
				let left = self.check_expr(ast, logical.left, true)?;
				let right = self.check_expr(ast, logical.right, true)?;

				let left_check = self.compute_assignable(self.db.types.bool, left);
				let right_check = self.compute_assignable(self.db.types.bool, right);

				let left_check = maybe_type_error!(self, left_check,
					logical.left.location(ast),
					"Invalid conditional expression in LHS to logical operator: Expression has type '{}'",
					self.db.repr_type(left));

				let right_check = maybe_type_error!(self, right_check,
					logical.right.location(ast),
					"Invalid conditional expression in RHS to logical operator: Expression has type '{}'",
					self.db.repr_type(right));

				self.do_promote_expr(ast, &mut logical.left, left_check);
				self.do_promote_expr(ast, &mut logical.right, right_check);

				// Logical operators always return bool.
				self.db.types.bool
			}
			Expr::If(if_) => {
				let condition_ty = self.check_expr(ast, if_.condition, true)?;
				let cond_computed =
					self.compute_assignable(self.db.types.bool, condition_ty);

				// TODO: Report the error at condition.location(), we need an
				// autogenerated method that does this.
				let cond_computed = maybe_type_error!(self, cond_computed,
					if_.condition.location(ast),
					"Invalid conditional expression: Expression has type '{}'",
					self.db.repr_type(condition_ty));

				self.do_promote_expr(ast, &mut if_.condition, cond_computed);

				let then_ty = self.check_expr(ast, if_.then_branch, value_used)?;

				let Some(else_branch) = if_.else_branch.as_mut() else {
					// If there's no else branch, then things are a bit weird:
					// we can't return a value, which could be called 'void'.
					// We are allowed to print() or str() voids, but maybe that's
					// ok...
					if_.typ = self.db.types.void;
					return Ok(self.db.types.void);
				};

				// If we have both then and else branches, then we need them to have
				// the same type. This is very similar to a binary operator, and
				// essentially just requires two types where one can be promoted
				// to the other.

				let else_ty = self.check_expr(ast, *else_branch, value_used)?;

				// If the value is not used, though, we can just return type.void and
				// call it a day. This is so if branches can have differing types in
				// some cases.
				if !value_used {
					// In this case, the type of the if itself must also be void.
					if_.typ = self.db.types.void;
					return Ok(self.db.types.void);
				}

				// Okay, we do need the types to be compatible, so compute an 
				// intersection. Note that Bottom should not take over, because
				// in the case that one branch is Bottom, the other branch simply
				// is "unconditional" in terms of typing.
				let computed = self.compute_intersect(false, then_ty, else_ty);

				let computed = maybe_type_error_with!(self, computed, {
					let error = Error::simple(
						format!("Branches of 'if' expression are incompatible: then has type '{}' but else has type '{}'",
							self.db.repr_type(then_ty), self.db.repr_type(else_ty)),
						if_.location.begin()
					);
					let error = error.add_note(format!("then branch has type '{}'", self.db.repr_type(then_ty)),
						Some(if_.then_branch.val_location(ast)));
					let error = error.add_note(format!("else branch has type '{}'", self.db.repr_type(else_ty)),
						Some(else_branch.val_location(ast)));

					error
				});

				// PROMOTION: then-branch and else-branch promotion occurs in promote_expr

				if_.typ = computed;

				computed
			},
			Expr::WhileLoop(while_) => {
				// At the time, we do not support usage of the value inside a 
				// while loop.
				if value_used {
					type_error!(self, while_.location, "Value of while loop is used. This is currently not supported.");
				}

				let condition_ty = self.check_expr(ast, while_.condition, true)?;
				let cond_computed =
					self.compute_assignable(self.db.types.bool, condition_ty);

				// TODO: Report the error at condition.location(), we need an
				// autogenerated method that does this.
				let cond_computed = maybe_type_error!(self, cond_computed,
					while_.condition.location(ast),
					"Invalid conditional expression: Expression has type '{}'",
					self.db.repr_type(condition_ty));

				self.do_promote_expr(ast, &mut while_.condition, cond_computed);

				let enclosing_breaks = std::mem::take(&mut self.break_exprs);
				self.break_exprs = Vec::new();
				
				// For while loops, we do eventually want to use the inner value.
				// But for now, let's keep things simple, and just treat the type
				// as void.
				self.check_expr(ast, while_.inner, false)?;

				let breaks = std::mem::replace(&mut self.break_exprs, enclosing_breaks);
				for break_ in &breaks {
					let break_ = ast.get_expr(*break_);
					let Expr::Break(break_) = break_.as_ref() else { unreachable!(); };

					// In the future, we will need to check break types like with
					// Loop; for now, just ensure they don't have a value.
					if break_.value.is_some() {
						type_error!(self, break_.location, "Break with value in while loop is currently not supported.");
					}
				}
				while_.breaks = breaks;

				// For now, use void type.
				while_.typ = self.db.types.void;

				while_.typ
			}
			Expr::Loop(loop_) => {
				// The loop type is going to be like an if/else, except that
				// any number of 'break's can have distinct types.

				// TODO: We probably need to store the break exprs in the loop
				// so that we can promote them later.
				let enclosing_breaks = std::mem::take(&mut self.break_exprs);
				self.break_exprs = Vec::new();
				
				// For the inner expression, the value is never used, for an
				// infinite loop. The only way to produce a value is with
				// a break expression.
				//
				// This also means there is no need to promote the inner value,
				// because it is not used.
				self.check_expr(ast, loop_.inner, false)?;

				// Okay, now we have a list of break exprs. Ensure that they
				// all have the same type.

				let mut typ = self.db.types.unassigned;
				let mut has_value = false;
				let breaks = std::mem::replace(&mut self.break_exprs, enclosing_breaks);
				
				if let Some((first, rest)) = breaks.split_first() {
					// Subtle but important:
					// If there were any breaks at all, then the loop no longer
					// has type Bottom, but instead type Void. This is because
					// it does return!
					//
					// Update the type immediately; if we find that it *also*
					// produces a value, then this will be overwritten again.
					loop_.typ = self.db.types.void;

					let first = ast.get_expr(*first);
					let Expr::Break(first) = first.as_ref() else { unreachable!(); };

					if let Some(inner) = first.value {
						typ = inner.typ(ast, self.db);
						has_value = true;
					}
					
					for expr in rest {
						let expr = ast.get_expr(*expr);
						let Expr::Break(expr) = expr.as_ref() else { unreachable!(); };

						let mut inner_has_value = false;

						if let Some(inner) = expr.value {
							inner_has_value = true;
							let intersect = self.compute_intersect(false, typ, inner.typ(ast, self.db));

							typ = maybe_type_error!(self, intersect,
								expr.location,
								"Breaks in loop have incompatible types");
						}

						if inner_has_value != has_value {
							type_error!(self, expr.location,
								"Incompatible breaks in loop: either all breaks must provide a value, or no breaks.");
						}
					} 
				}

				if has_value {
					loop_.typ = typ;
				}

				// For promotions.... I think we have to store the vec of 
				// break exprs, and then promote them all sort of like we 
				// were an if-else.
				loop_.breaks = breaks;

				// Promotion occurs in promote_expr

				loop_.typ
			}
			Expr::Break(break_) => {
				if let Some(inner) = break_.value {
					// So....
					// Technically, the value used of this break is really just
					// the value_used of the enclosing loop. So, something like:
					//
					// loop {
					//     if a { break 1; } else { break "hello"; }
					// }
					//
					// is sooooort of valid. I think, however, we should just
					// always consider the value used, because if you're going
					// to break with a value, you should make the values match
					// up, IMO.
					self.check_expr(ast, inner,  true)?;
				}

				self.break_exprs.push(expr_id);

				// The Break itself is always Never.
				self.db.types.bottom
			}
			Expr::Continue(_) => {
				self.db.types.bottom
			}
			Expr::Return(ret) => {
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

						return Ok(self.db.types.bottom);
					},
				};

				let typ = self.check_expr(ast, *inner, true)?;
				let computed = self.compute_assignable( 
					return_type,
					typ);

				let computed = maybe_type_error!(self, 
					computed,
					&ret.location,
					"Trying to return {} in function returning {}",

					self.db.repr_type(typ),
					self.db.repr_type(return_type));

				self.do_promote_expr(ast, inner, computed);

				self.db.types.bottom
			}
			Expr::OptionElse(opt_else) => {
				let value_ty = self.check_expr(ast, opt_else.value, value_used)?;
				let otherwise_ty = self.check_expr(ast, opt_else.otherwise, value_used)?;

				log::trace!("check_expr: OptionElse: value_ty = {}, otherwise_ty = {}",
					self.db.repr_type(value_ty), self.db.repr_type(otherwise_ty));

				let value_unwrapped = match self.db.get(value_ty) {
					Type::Option(inner) => *inner,
					_ => {
						type_error!(self, opt_else.location,
							"'else' operator can only be applied to Option types.");
					}
				};

				// PROMOTION: value and otherwise promotion occurs in promote_expr

				// For now, we use compute_intersect with bottom_eats as false.
				//
				// There is a problem:
				//     { return } else { value }
				// should have type Bottom, but it won't in this case.
				//
				// Although, Bottom cannot even have `else` called on it, so,
				// idk.
				let computed = self.compute_intersect(false, value_unwrapped, otherwise_ty);

				let computed = maybe_type_error!(self, computed, 
					opt_else.location,
					// TODO: Note that LHS should equal RHS?
					"Invalid 'else' expression: LHS has type {}, but RHS has type {}",
					self.db.repr_type(value_ty),
					self.db.repr_type(otherwise_ty),
				);

				opt_else.typ = computed;

				log::trace!("check_expr: OptionElse: computed = {}", self.db.repr_type(computed));

				computed
			}
			Expr::Index(index) => {
				let arr_ty = self.check_expr(ast, index.value, true)?;

				let elem_ty = match self.db.get(arr_ty).clone() {
					Type::ArrayOf(elem) => elem,
					Type::DynArrayOf(elem, _) => elem,
					// We don't have a good string indexing strategy yet. For now,
					// treat strings as essentially arrays of integers.
					Type::Str | Type::StrBuf | Type::StrConst => self.db.types.int,
					_ => type_error!(self, &index.location, "Can only index a string or array.")
				};

				let index_ty = self.check_expr(ast, index.index, true)?;
				let index_computed = self.compute_assignable(self.db.types.int, index_ty);
				
				let index_computed = maybe_type_error!(self,
					index_computed,
					index.index.location(ast),
					"Invalid index expression: Expression has type {}",
					self.db.repr_type(index_ty)
				);

				self.do_promote_expr(ast, &mut index.index, index_computed);

				index.typ = elem_ty;

				elem_ty
			}
			Expr::SetIndex(set) => {
				// Get the type of the dotted expression. This lets us look up
				// the property on that type.
				let arr_ty = self.check_expr(ast, set.value, value_used)?;

				let elem_ty = match self.db.get(arr_ty).clone() {
					Type::ArrayOf(elem) => elem,
					Type::DynArrayOf(elem, _) => elem,
					// We don't have a good string indexing strategy yet. For now,
					// treat strings as essentially arrays of integers.
					Type::Str | Type::StrBuf | Type::StrConst => self.db.types.int,
					_ => type_error!(self, &set.location, "Can only index a string or array.")
				};

				// We can't check the variable just like an Assign, as that
				// will overwrite the type (the type is given by the array type).
				// But, we do need to check that the RHS is assignable here.
				let rhs = self.check_expr(ast, set.rhs, true)?;

				let computed =
					self.compute_assignable(elem_ty, rhs);

				let computed = maybe_type_error!(
					self,
					computed,

					&set.location,
					"Invalid indexed assignment: need {}, but value is {}",
					self.db.repr_type(elem_ty),
					self.db.repr_type(rhs)
				);

				// Promote the RHS based on the computed type.
				self.do_promote_expr(ast, &mut set.rhs, computed);


				// Handle the index just like in Index.
				let index_ty = self.check_expr(ast, set.index, true)?;
				let index_computed = self.compute_assignable(self.db.types.int, index_ty);
				
				let index_computed = maybe_type_error!(self,
					index_computed,
					set.index.location(ast),
					"Invalid index expression: Expression has type {}",
					self.db.repr_type(index_ty)
				);

				self.do_promote_expr(ast, &mut set.index, index_computed);

				set.typ = elem_ty;

				elem_ty
			}
			Expr::Variable(var) => self.db.get(var.identity).typ,
			Expr::Assign(assign) => {
				// I'm not sure EXACTLY how I want to do assigns, but I think
				// it's straightforward enough to do it like this:
				//
				// Desugar the assign ahead of time, THEN check it. This ensures
				// that assigns behave EXACTLY like whatever binary operators
				// we've implemented.
				if assign.op != Tok::Equal {
					// Read from the variable
					let null_location = assign.location.begin();
					let read = Expr::push_variable(ast, null_location,
						assign.identity);
					// Perform a binary op, with RHS the assign's current value
					let binop = Expr::push_binary(ast, assign.location.clone(),
						map_assign_op(assign.op), read, assign.value, self.db.types.unassigned);
					// That is now what we're assigning.
					assign.value = binop;

					// The assign is now a regular assign. (As of writing this
					// comment, nothing else in the code reads this though.)
					assign.op = Tok::Equal;
				}
				self.check_assign(ast, &assign.location, assign.identity, &mut assign.value, false)?
			},
			Expr::NumLiteral(lit) => {
				lit.typ
			},
			Expr::StrLiteral(_) => {
				self.db.types.str_const
			},
			Expr::BoolLiteral(_) => {
				self.db.types.bool
			}
			Expr::Block(block) => {
				// We must type-check every statement inside the block.
				// However, the last statement is checked specially.
				let all_but_last = match block.stmts.len() {
					0 => 0,
					n => n - 1,
				};
				for stmt in &mut block.stmts[0..all_but_last] {
					self.check_stmt(ast, *stmt, false)?;
				}

				// If the value isn't used, we can simply type-check the
				// last statement then bail with Void.
				if !value_used {
					block.stmts.last_mut().map(|stmt| self.check_stmt(ast, *stmt, false));
					// We also need to assign our own type to void in this case
					// -- our type is not yet assigned.
					block.typ = self.db.types.void;
					return Ok(self.db.types.void);
				}

				// Otherwise, we need to compute a type for the value.
				// If the block has no statements, that's an error.
				let Some(stmt) = block.stmts.last_mut() else {
					type_error!(self, &block.location,
						"Return value of block is used, but the block is empty.");
				};

				// If the block has a statement, defer to self.stmt(). But we
				// need to get a TypId at the end.
				let Some(val) = self.check_stmt(ast, *stmt, true)? else {
					type_error!(self, &block.location,
						"Return value of block is used, but its last statement has no value.");
				};

				// Return the computed TypId.
				block.typ = val;

				// PROMOTION: Occurs in promote_expr
				//self.promote_stmt(ast, *stmt, val);

				val
			},
			Expr::Print(print) => {
				if print.exprs.len() <= 0 {
					// For print, default to void type as it is usually used
					// as a statement.
					return Ok(self.db.types.void);
				}

				// At least for now, all possible types are allowed inside the
				// print. So, simply type check each one. Then, the print is
				// supposed to return its first argument.
				for expr in &mut print.exprs[1..] {
					// The idea here is that each argument to the print is essentially
					// an assignment to an Unassigned variable. As such, the arguments
					// should automatically promote to Int or Float if they're AssumeInt
					// or AssumeFloat.
					//
					// This logic is the same as unused statement expressions and the like,
					// so it gets its own helper function.
					self.check_expr(ast, *expr, true)?;
					self.promote_from_unassigned(ast, expr);
				}

				self.check_expr(ast, print.exprs[0], true)?;
				let computed = self.promote_from_unassigned(ast, &mut print.exprs[0]);

				// TODO: We could store this type directly on the print() if we
				// wanted to -- that's what other ast nodes do...
				computed 
			},
			Expr::Str(str) => {
				// Str is very similar to print(), except it always return StrBuf instead
				// of its first argument.
				for expr in &mut str.exprs {
					self.check_expr(ast, *expr, true)?;
					self.promote_from_unassigned(ast, expr);
				}

				self.db.types.str_buf
			},
			Expr::FunCall(call) => {
				// Function calls bear a resemblance both to print/str, and also
				// to variable assignment. This is because we are essentially
				// "assigning" each of its parameters (which are VarIds!) to
				// the arguments.
				//
				// Of course, we don't mutate the parameter type -- so it's different
				// in that way, but otherwise very similar.

				let fun_arity = self.db.get(call.identity).parameters.len();

				if call.args.len() != fun_arity {
					// TODO: Add a Note about the function definition.
					type_error!(self,
						&call.location,
						"Incorrect arguments to function '{}'. Function expects {} arguments but {} were given",
						self.db.get_fun_name(call.identity),
						fun_arity,
						call.args.len());
				}

				for i in 0..fun_arity {
					// Check each argument against the corresponding parameter.
					let arg = self.check_expr(ast, call.args[i], true)?;

					let param = self.db.get(call.identity).parameters[i];

					// Note that we do NOT mutate the var type in any way.
					let computed = self.compute_assignable(
						self.db.get_var_type(param), arg);

					let computed = maybe_type_error!(self, computed,
						&call.location,
						"Incorrect argument to function '{}': Parameter '{}' expects '{}', but was given '{}'",
						self.db.get_fun_name(call.identity),
						self.db.repr_var(param),
						self.db.repr_var_type(param),
						self.db.repr_type(arg)
					);

					self.do_promote_expr(ast, &mut call.args[i], computed);
				}

				self.db.get_fun_ret_type(call.identity)
			},

			Expr::BuiltinCall(call) => {
				let (ret_type, parameters) = call.ptr.get_types(self.db, call.object.typ(ast, self.db));
				let fun_arity = parameters.len();

				if call.args.len() != fun_arity {
					// TODO: Add a Note about the function definition.
					// Also TODO: We need to store the name of the builtin somewhere...
					type_error!(self,
						&call.location,
						"Incorrect arguments to builtin function");
				}

				for i in 0..fun_arity {
					// Check each argument against the corresponding parameter.
					let arg = self.check_expr(ast, call.args[i], true)?;

					let param = parameters[i];

					// Note that we do NOT mutate the var type in any way.
					let computed = self.compute_assignable(
						param, arg);

					let computed = maybe_type_error!(self, computed,
						&call.location,
						"Incorrect argument to builtin: Parameter '{}' expects '{}', but was given '{}'",
						i,
						self.db.repr_type(param),
						self.db.repr_type(arg)
					);

					self.do_promote_expr(ast, &mut call.args[i], computed);
				}

				call.typ = ret_type;
				ret_type
			}

			Expr::BuiltinCapture(_) => {
				// Right now, capturing methods is not supported; but, we
				// need to be able to type check this node because it is temporarily
				// synthesized. So, just return an unassigned type.
				self.db.types.unassigned
			}

			Expr::ValCall(call) => {
				// TODO: Is it safe to check_expr this inner value once, if
				// we're going to replace it? I believe the answer is *yes*
				// if it is a FunCapture or BuiltinCapture, which are the cases
				// that matter.
				let value = self.check_expr(ast, call.value, true)?;

				{
					// Optimization + semantics: if we are a ValCall of a FunCapture, replace
					// us with a FunCall.
					//
					// This is important for BuiltinMethods because they, in general,
					// cannot be captured. (TODO: Error message for that?)
					let mut inner_bind = ast.exprs.get_mut(call.value);
					if let Expr::FunCapture(capt) = inner_bind.as_mut() {
						// TODO: FunCall on an Object. Until then, we still have to use
						// ValCall(FunCapture).
						let as_funcall = FunCall {
							location: call.location.clone(),
							fn_name: capt.fn_name.clone(),
							identity: capt.identity,
							args: std::mem::take(&mut call.args),
							object: capt.object,
							arg_boundaries: std::mem::take(&mut call.arg_boundaries),
						};

						*expr = Expr::FunCall(as_funcall);
						// Because this is happening first, we have to re-check
						// the expr.
						drop(inner_bind);
						drop(binding);
						return self.check_expr(ast, expr_id, value_used);
					}

					if let Expr::BuiltinCapture(capt) = inner_bind.as_mut() {
						log::trace!("ValCall>BuiltinCapture => BuiltinCall");
						let as_builtincall = BuiltinCall {
							location: call.location.clone(),
							fn_name: capt.fn_name.clone(),
							// TODO: Can I just pass the function pointers themselves?
							// Arc seems unnecessary.
							ptr: Arc::clone(&capt.ptr),
							args: std::mem::take(&mut call.args),
							object: capt.object,
							typ: self.db.types.unassigned,
						};
						*expr = Expr::BuiltinCall(as_builtincall);
						drop(inner_bind);
						drop(binding);
						return self.check_expr(ast, expr_id, value_used);
					}
				}
			
				// Now, we need to make sure that the value is Assignable to
				// a function type.
				let computed = self.compute_assignable(self.db.types.fun_sig_unassigned, value);

				let computed = maybe_type_error!(self, computed, &call.location,
					"Cannot call a value of type '{}'",
					self.db.repr_type(value));

				// TODO: Also support FunRaw calling..?
				// TODO: This is probably slightly wrong, or maybe not. Maybe FunCapture
				// will have to promote itself...?
				self.do_promote_expr(ast, &mut call.value, computed);

				let correct_sig = match self.db.get(computed) {
					Type::Fun(sig) => *sig,
					Type::FunRaw(sig) => *sig,
					Type::Bottom => {
						// If our type is bottom, bail.
						return Ok(self.db.types.bottom);
					},
					_ => unreachable!()
				};
				call.sig = correct_sig;

				let fun_arity = self.db.get(call.sig).parameters.len();

				if call.args.len() != fun_arity {
					type_error!(self,
						&call.location,
						"Incorrect arguments to call. A value of type '{}' expects {} arguments but {} were given",
						self.db.repr_type(value),
						call.args.len(),
						fun_arity);
				}

				// Check each argument against the corresponding parameter.
				for i in 0..fun_arity {
					let arg = self.check_expr(ast, call.args[i], true)?;

					let param = self.db.get(call.sig).parameters[i];

					let computed = self.compute_assignable(
						param, arg);

					let computed = maybe_type_error!(self, computed,
						&call.location,
						"Incorrect argument to call: The {} parameter expects '{}', but was given '{}'",
						self.db.repr_nth_idx(i),
						self.db.repr_type(param),
						self.db.repr_type(arg)
					);

					log::trace!("check_expr: ValCall: {} parameter: {}", i, self.db.repr_type(computed));

					self.do_promote_expr(ast, &mut call.args[i], computed);
				}

				// TODO: Should ValCall's use_sig their sig?

				let ret_type = self.db.get(call.sig).return_type;

				ret_type
			},

			Expr::FunCapture(capt) => {
				// TODO: Ensure all functions have sigs.
				let sig = self.db.get(capt.identity).sig;

				if sig == self.db.sig_unassigned {
					panic!("ICE: Tried to typecheck FunCapture for a function with unassigned sig");
				}

				// Make sure we use this sig.
				self.db.use_sig(sig);

				// TODO: Also support FunRaw captures.
				capt.typ = self.db.put_type(Type::Fun(sig));
				capt.typ
			},

			Expr::FunDeclare(declare) => {
				self.check_fun_declare(ast, declare)?;

				let sig = self.db.get(declare.identity).sig;

				// This is basically the same idea as FunCapture.
				if sig == self.db.sig_unassigned {
					panic!("ICE: Tried to typecheck FunDeclare for a function with unassigned sig");
				}

				// If we're capturing the value from the function, make sure
				// the sig is used.
				
				// Note: We must, at least for now, unconditionally use the sig
				// here because in codegen.rs we unconditionally generated a value
				// containing the function object (which requires the sig).
				self.db.use_sig(sig);

				// TODO: Also support FunRaw -- in this case, I suppose the
				// function would itself know if it is FunRaw..?
				declare.typ = self.db.put_type(Type::Fun(sig));
				declare.typ
			},

			Expr::New(new) => {
				if new.typ == self.db.types.unassigned {
					if PANIC_ON_BAD_NODE {
						panic!("ICE: New expression has unassigned type from Binder");
					}
					else {
						// No way to check the initializers.
						return Ok(new.typ)
					}
				}

				let Type::Class(class_id) = self.db.get(new.typ) else {
					if PANIC_ON_BAD_NODE {
						panic!("ICE: New expression with non-class type");
					}
					return Ok(new.typ);
				};

				// We create a copy of the mandatory vars set so that we can
				// "check" them off as we go through the initializers.
				//
				// This might be slightly less performant than some other
				// strategies but I believe it should be OK.
				let mut checklist = self.db.get(*class_id).mandatory_vars.clone();

				for init in &mut new.initializers {
					self.check_assign(ast, &init.location, init.var, &mut init.value, false)?;
					checklist.remove(&init.var);
				}

				log::trace!("new expression checklist len: {}", checklist.len());
				if !checklist.is_empty() {
					let mut iter = checklist.iter();
					let mut error = Error::simple(
						format!("'new' expression is missing initializer for mandatory variable '{}'",
							// We know the checklist is nonempty, so we can
							// definitely extract one var.
							self.db.repr_var(*iter.next().unwrap())),
						new.location.clone(),
					);

					// Now attach the rest of the uninitialized vars as notes.
					for var in iter {
						error = error.add_note(format!("also missing '{}'", self.db.repr_var(*var)), None);
					}

					self.db.report_error(error);
					// A little awkward that we have to remember to put this.
					self.had_error = true;
					
					// I don't actually think there's any reason to return Err here,
					// as this error can't cause additional type errors.
				}

				new.typ
			}

			Expr::Get(get) => {
				// Get the type of the dotted expression. This lets us look up
				// the property on that type.
				let mut lhs = self.check_expr(ast, get.lhs, true)?;

				let mut var_chain = Vec::new();

				// First, build a Vec of vars for all but the last item in the
				// chain.
				for propname in &get.chain[0..get.chain.len() - 1] {
					if let Some(property) = self.db.lookup_property(lhs, propname.lexeme) {
						var_chain.push(property);
						// The LHS type advances as we walk the chain.
						lhs = self.db.get_var_type(property);
					}
					else {
						type_error!(self,
							&propname.location,
							"Object of type '{}' has no such property '{}'",
							self.db.repr_type(lhs),
							self.db.get(propname.lexeme));
					}
				}

				// Safety: We always have a non-empty chain.
				//
				// Clone this token just to make life easy. 
				let last = get.chain.last().unwrap().clone();

				// The last property is special. It might just be another var
				// in the var chain, OR it might be a FunCapture.
				if let Some(property) = self.db.lookup_property(lhs, last.lexeme) {
					// We must actually store the looked-up property.
					var_chain.push(property);
					get.vars = var_chain;

					return Ok(self.db.get_var_type(property));
				}

				// Check for possible function capture. If so, then we turn this
				// Get into a FunCapture.
				if let Some(fun) = self.db.lookup_member_fn(lhs, last.lexeme) {
					let mut funcapt_lhs = get.lhs;
					if !var_chain.is_empty() {
						// Remove the last element in the chain
						let mut chain = std::mem::take(&mut get.chain);
						chain.truncate(get.chain.len() - 1);

						funcapt_lhs = Expr::push_get(ast, get.location.clone(),
							chain, get.lhs, var_chain);
					}

					let as_funcapture = FunCapture {
						location: get.location.clone(),
						fn_name: last.location.clone(),
						identity: fun,
						typ: self.db.types.unassigned,
						object: Some(funcapt_lhs),
					};

					*expr = Expr::FunCapture(as_funcapture);
					drop(binding);
					return self.check_expr(ast, expr_id, value_used);
				}

				// Failed to look up the last property.
				type_error!(self,
					&last.location,
					"Object of type '{}' has no such property '{}'",
					self.db.repr_type(lhs),
					self.db.get(last.lexeme))
			}

			Expr::Set(set) => {
				// Get the type of the dotted expression. This lets us look up
				// the property on that type.
				// Get the type of the dotted expression. This lets us look up
				// the property on that type.
				let mut lhs = self.check_expr(ast, set.lhs, true)?;

				let mut var_chain = Vec::new();

				// First, build a Vec of vars for all but the last item in the
				// chain.
				for propname in &set.chain[0..set.chain.len() - 1] {
					if let Some(property) = self.db.lookup_property(lhs, propname.lexeme) {
						var_chain.push(property);
						// The LHS type advances as we walk the chain.
						lhs = self.db.get_var_type(property);
					}
					else {
						type_error!(self,
							&propname.location,
							"Object of type '{}' has no such property '{}'",
							self.db.repr_type(lhs),
							self.db.get(propname.lexeme));
					}
				}

				// Safety: We always have a non-empty chain. 
				let last = set.chain.last().unwrap();

				let property = self.db.lookup_property(lhs, last.lexeme);
				let Some(property) = property else {
					type_error!(self,
						&set.location,
						"Object of type '{}' has no such property '{}'",
						self.db.repr_type(lhs),
						self.db.get(last.lexeme));
				};

				// We only check the  last property for readonly, for now.
				//
				// I suppose we will also need to check any value types along
				// the way for readonly.  Huh.
				if self.db.get(property).readonly {
					let readonly_error = Error::simple(
						format!("Invalid assignment to property '{}', which cannot be written to.",
							self.db.repr_var(property)),
						set.location.clone()
					);
					self.had_error = true;
					self.db.report_error(readonly_error);
				}

				// We must actually store the looked-up property.
				var_chain.push(property); //  Push the last property
				set.vars = var_chain;

				if set.op != Tok::Equal {
					// very important TODO: We actually need to store the
					// value in a temporary variable, that we read from as
					// the LHS of both the set and the get. This is so that
					// something like call_fun().x += 5 does not cause
					// a double evaluation of call_fun().

					// Read from the variable
					let null_location = set.location.begin();
					let read = Expr::push_get(ast, null_location,
						set.chain.clone(), set.lhs, set.vars.clone());
					// Perform a binary op, with RHS the assign's current value
					let binop = Expr::push_binary(ast, set.location.clone(),
						map_assign_op(set.op), read, set.rhs, self.db.types.unassigned);
					// That is now what we're assigning.
					set.rhs = binop;

					// The assign is now a regular assign. (As of writing this
					// comment, nothing else in the code reads this though.)
					set.op = Tok::Equal;
				}

				// We can't check the variable just like an Assign, as that
				// will overwrite the type (the type is given ONLY by the class
				// definition itself). But, we do need to check that the RHS
				// is assignable to this variable.
				let rhs = self.check_expr(ast, set.rhs, true)?;

				let computed =
					self.compute_assignable(self.db.get_var_type(property), rhs);

				// TODO: Should we actually use the "computed" value here for
				// anything?
				let computed = maybe_type_error!(
					self,
					computed,

					&set.location,
					"Invalid assignment to property '{}': need {}, but value is {}",
					self.db.repr_var(property),
					self.db.repr_var_type(property),
					self.db.repr_type(rhs)
				);

				// Promote the RHS based on the computed type.
				self.do_promote_expr(ast, &mut set.rhs, computed);

				self.db.get_var_type(property)
			}

			Expr::SelfVal(selfval) => {
				let Some(typ) = self.current_class else {
					type_error!(
						self,
						&selfval.location,
						"Trying to use 'self' outside of a class."
					);
				};

				selfval.typ = typ;
				typ
			}

			Expr::Unbound(unbound) => {
				if PANIC_ON_BAD_NODE {
					// In theory we will resolve all idents beforehand? But this might
					// be different if we have function overloading.
					panic!("ICE: Tried to typecheck an unbound identifier expression '{}' at {}",
						self.db.get(unbound.identifier.lexeme),
						unbound.location.offset);
				}
				else {
					// Use unknown type (?)
					self.db.types.unassigned
				}
			},
			Expr::UnboundFunCapture(capt) => {
				// Here we will have to replace this UnboundFunCapture with
				// a FunCapture, for the current state of the project.

				let obj_ty = {
					let Some(object) = &mut capt.object else {
						type_error!(self, &capt.location, "Can't resolve function call.");
					};
					self.check_expr(ast, *object, true)?;
					
					// Strange but kind of true: We want to immediately promote
					// the object. We need a concrete type to try to resolve
					// member functions.
					self.promote_from_unassigned(ast, object)
				};

				if let Some(fun) = self.db.lookup_member_fn(obj_ty, capt.identifier.lexeme) {
					log::trace!("resolved unbound fun '{}' to member of '{}'",
						self.db.get(capt.identifier.lexeme), self.db.repr_type(obj_ty));
					let as_funcapture = FunCapture {
						location: capt.location.clone(),
						fn_name: capt.identifier.location.clone(),
						identity: fun,
						typ: self.db.types.unassigned,
						object: capt.object
					};
					*expr = Expr::FunCapture(as_funcapture);
					drop(binding);
					return self.check_expr(ast, expr_id, value_used);
				}

				if let Some(property) = self.db.lookup_property(obj_ty, capt.identifier.lexeme) {
					log::trace!("resolved unbound fun '{}' to Get{{}} in '{}'",
						self.db.get(capt.identifier.lexeme), self.db.repr_type(obj_ty));
					let as_get = Get {
						location: capt.location.clone(),
						chain: vec![capt.identifier.clone()],
						lhs: capt.object.unwrap(), // Safety: We already checked this above
						vars: vec![property]
					};
					*expr = Expr::Get(as_get);
					drop(binding);
					return self.check_expr(ast, expr_id, value_used);
				}

				if let Some(builtin) = self.db.lookup_builtin_method(obj_ty, capt.identifier.lexeme) {
					log::trace!("identified builtin: {}::{}", self.db.repr_type(obj_ty), self.db.get(capt.identifier.lexeme));
					// For builtin methods, we currently only support immediately calling them.
					//
					// I'm not actually sure how to quite do this...? It essentially needs to
					// be that we replace the *valcall* node, but this node isn't a ValCall.
					let as_builtincapt = BuiltinCapture {
						location: capt.location.clone(),
						fn_name: capt.identifier.location.clone(),
						ptr: Arc::clone(&builtin),
						object: capt.object.unwrap(), // A little ugly. We should store the object above.
					};
					*expr = Expr::BuiltinCapture(as_builtincapt);
					drop(binding);
					return self.check_expr(ast, expr_id, value_used);
				}

				type_error!(self,
					&capt.location,
					"Object of type {} has no such function or property {}.",
					self.db.repr_type(obj_ty),
					self.db.get(capt.identifier.lexeme));
			}
			Expr::UnboundAssign(assign) => {
				if PANIC_ON_BAD_NODE {
					panic!("ICE: Tried to typecheck an UnboundAssign")
				}
				else {
					// This is a best-effort attempt at typechecking to help
					// the LSP out.
					self.check_expr(ast, assign.value, value_used)?;
					self.promote_from_unassigned(ast, &mut assign.value)
				}
			}
			Expr::Undefined(_) => {
				if PANIC_ON_BAD_NODE {
					panic!("ICE: Tried to typecheck an Undefined")
				}
				else {
					self.db.types.unassigned
				}
			}

			Expr::MakeTuple(tuple) => {
				let mut inner = Vec::new();
				for expr in &tuple.values {
					inner.push(self.check_expr(ast, *expr, value_used)?);
				}

				let typ = self.db.put_type(Type::Tuple(Arc::from(inner)));
				tuple.typ = typ;

				typ
			},

			Expr::MakeSumType(sum) => {
				// Nothing to do yet.
				sum.typ
			}

			Expr::ForLoop(for_) => {
				// First, we check the iterable. This tells us how to desugar it.
				self.check_expr(ast, for_.iterator, true)?;
				// We have to promote from unassigned, as this is where this value
				// is used.
				let iterable = self.promote_from_unassigned(ast, &mut for_.iterator);

				let iter_ty = self.db.get(iterable);
				match iter_ty {
					Type::RangeOf(a, b, typ) if *typ == self.db.types.int => {
						let _a = *a; let b = *b;
						// Use a 0-width location for all the synthesized nodes,
						// so that we don't take up space.
						let inner_loc = for_.location.begin();
						// Desugar the for loop into the following:
						// var <var> = <start>
						// while <var> < <end> {
						//    inner
						//    var = var + 1;
						// }
						let initializer = Expr::push_get(ast, inner_loc.clone(),
							vec![Token::synthesize_ident_from(self.db, "left")],
							for_.iterator, vec![self.db.get_range_left(iterable)]);
						// NOTE: For now, in order to make for loops work with
						// 'continue', we will write our loops in a weird way.
						// (go from start - 1 to end; move increment to beginning
						// of loop)
						// What we should do instead is probably synthesize a label
						// at the *end* of the loop, that we jump to in the continue
						// statement.
						let one = Expr::push_numliteral(ast, inner_loc.clone(),
							Token::synth_tok_from(self.db, "1", Tok::WholeNumber),
							self.db.types.int);
						let sub = Expr::push_binary(ast, inner_loc.clone(),
							Tok::Minus, initializer, one, self.db.types.int);
						// Make the declare have its own location...?
						let declare = Stmt::push_declare(ast, for_.ident.clone(),
							for_.ident.clone(), for_.identity, Some(sub), for_.has_explicit_type);
						
						let read = Expr::push_variable(ast, inner_loc.clone(),
							for_.identity);
						let one = Expr::push_numliteral(ast, inner_loc.clone(),
							Token::synth_tok_from(self.db, "1", Tok::WholeNumber),
							self.db.types.int);
						let add = Expr::push_binary(ast, inner_loc.clone(),
							Tok::Plus, read, one, self.db.types.int);
						let assign = Expr::push_assign(ast, inner_loc.clone(),
							self.db.srcloc_dummy(), for_.identity, add, Tok::Equal);

						// Grab location from the inner
						let inner_stmt_loc = for_.inner.location(ast);
						let inner_stmt = Stmt::push_expression(ast, inner_stmt_loc.clone(),
							for_.inner);
						let assign_stmt = Stmt::push_expression(ast, inner_loc.clone(),
							assign);
							
						// AWKWARD/TODO: Once we care about the value of the while block,
						// this is not going to be it...?
						let inner_block = Expr::push_block(ast,  inner_stmt_loc,
							// Due to our 'continue' jank, the assign has to come
							// before the inner.
							vec![assign_stmt, inner_stmt], self.db.types.void);

						// Rhs of the comparison.
						let rhs = Expr::push_get(ast, inner_loc.clone(),
							vec![Token::synthesize_ident_from(self.db, "right")],
							for_.iterator, vec![self.db.get_range_right(iterable)]);
						// More 'continue' JANK: synthesize a -1 for the RHS
						// of the loop as well.
						let one = Expr::push_numliteral(ast, inner_loc.clone(),
							Token::synth_tok_from(self.db, "1", Tok::WholeNumber),
							self.db.types.int);
						let rhs = Expr::push_binary(ast, inner_loc.clone(),
							Tok::Minus, rhs, one, self.db.types.int);
						// TODO: Can we re-used the read above? For now, synthesize
						// two nodes.
						let read = Expr::push_variable(ast, inner_loc.clone(),
							for_.identity);

						// Switch comparison based on the range type.
						let compare_type = match b {
							RangeEnd::Inclusive => Tok::LessEqual,
							RangeEnd::Exclusive => Tok::Less,
							RangeEnd::Unbounded => todo!(),
						};
						let comparison = Expr::push_comparison(ast, inner_loc.clone(),
							compare_type, read, rhs, self.db.types.int);

						// These muse encompas the entire for loop in terms of location.
						let while_loop = Expr::push_whileloop(ast, for_.location.clone(),
							comparison, inner_block, self.db.types.void, Vec::new());
						
						let while_stmt = Stmt::push_expression(ast, for_.location.clone(),
							while_loop);
						
						let block = Block {
							location: for_.location.clone(),
							stmts: vec![declare, while_stmt],
							typ: self.db.types.void,
						};

						// Now, drop the binding, modify ourselves to be the
						// new block, and re-check it.
						drop(binding);
						let mut binding = ast.get_expr_mut(expr_id);
						*binding = Expr::Block(block);
						drop(binding);

						return self.check_expr(ast, expr_id, value_used);
					},
					_ => {
						type_error!(self,
							&for_.location,
							"Don't know how to iterate over object of type {}",
							self.db.repr_type(iterable));
					}
				}
			}

			// Promote should not be generated until we get to the TypeCheck stage.
			Expr::Promote(_) => panic!("ICE: Tried to typecheck Promote"),
		});

		log::trace!("check_expr: {:?} -> {}", expr, self.db.repr_type(expr.typ(ast, self.db)));
		result
	}

	fn check_class(&mut self, ast: &AstProxy, class_declare: &mut ClassDeclare) -> Result<()> {
		let enclosing_class = self.current_class;
		self.current_class = Some(self.db.put_type(Type::Class(class_declare.identity)));

		// Iterate class variables in the order found in the DB.
		let vars = std::mem::take(&mut self.db.get_mut(class_declare.identity).vars);
		for var in &vars {
			if let Some(initializer) = self.db.get(*var).initializer {
				let mut init = initializer;
				self.check_assign(ast, &self.db.get(*var).location.clone(), *var, &mut init, true)?;
				// Be sure to manually copy the expr back
				self.db.get_mut(*var).initializer = Some(init);
			}
		}
		self.db.get_mut(class_declare.identity).vars = vars;

		for fun in &mut class_declare.funs {
			self.check_fun_declare(ast, fun)?;
		}

		for class in &mut class_declare.classes {
			self.check_class(ast, class)?;
		}

		self.current_class = enclosing_class;

		Ok(())
	}

	fn check_stmt(&mut self, ast: &AstProxy, stmt_id: StmtId, value_used: bool) -> Result<Option<TypId>> {
		match ast.stmts.get_mut(stmt_id).as_mut() {
			Stmt::Declare(declare) => {
				let typ = self.check_declare(ast, declare)?;

				if typ == self.db.types.bottom {
					return Ok(Some(typ));
				}

				Ok(None)
			},
			Stmt::ClassDeclare(class_declare) => {
				self.check_class(ast, class_declare)?;
				Ok(None)
			},
			Stmt::Expression(expr) => {
				let mut typ = self.check_expr(ast, expr.expression, value_used)?;
				if !value_used {
					// Non-value-used exprs should be promoted from unassigned.
					// If their value is used, the value-user will be responsible
					// for calling promote() with the proper type.
					typ = self.promote_from_unassigned(ast, &mut expr.expression);
				}
				Ok(Some(typ))
			}
		}
	}

	fn check_declare(&mut self, ast: &AstProxy, declare: &mut Declare) -> Result<TypId> {
		if let Some(value) = declare.value.as_mut() {
			self.check_assign(ast, &declare.location, declare.identity, value, true)
		}
		else {
			// In this case, we shiould (?) have had an explicit type from the
			// parser, so the variable is good. There is also no RHS to typecheck.
			// So, just return that value.
			Ok(self.db.get_var_type(declare.identity))
		}
	}

	fn check_fun_declare(&mut self, ast: &AstProxy, fun: &mut FunDeclare) -> Result<()> {
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

		log::trace!("check_fun_declare: {}", self.db.get_fun_name(fun.identity));

		self.return_types.push(return_type);

		let inner = self.check_expr(ast, fun.value, value_used)?;

		self.return_types.pop();

		// If we're using the value of the expression, it must match the return
		// type.
		if value_used {
			let computed = self.compute_assignable(
				self.db.get_fun_return_typid(fun.identity),
				inner);

			let computed = maybe_type_error!(self, 
				computed,
				&fun.location,
				"Value of function body is {} but function returns {}",
				self.db.repr_type(inner),
				self.db.repr_type(self.db.get_fun_return_typid(fun.identity)));
		
			self.do_promote_expr(ast, &mut fun.value, computed);
		}
		else {
			// In this case, the block should have already promoted its own
			// last value. TODO: Is this jank???
			// self.promote_from_unassigned(ast, &mut fun.value);
		}

		Ok(())
	}

	/// Takes a FunDeclare and ensures that its Sig matches its actual
	/// value.
	/// TODO: Make sure binder binds type names.... and so forth...
	/// 
	/// I suppose the signature could be generated in the parser, and then
	/// the unbound type names in that signature would be fixed by binder?
	fn fix_fun_declare(&mut self, fun: FunId) {
		let mut sig = Sig { parameters: vec![], return_type: self.db.types.unassigned };

		for param in &self.db.get(fun).parameters {
			// We're essentially assuming that the type of param is good so far...
			// which is probably not true...
			sig.parameters.push(self.db.get_var_type(*param));
		}
		sig.return_type = self.db.get(fun).return_type;

		let sig = self.db.put_sig(&sig);
		self.db.get_mut(fun).sig = sig;
	}

	fn check_module(&mut self, ast: &AstProxy, module: &mut Module) {
		// HACK: Visit classes first so that type inference for properites works.
		// We really should get this working so that type inferences can directly
		// drive class type inference (i.e. type inference for the class members)
		// when needed.
		for class in &mut module.classes {
			// Ignore errors at this point as there's no need to unwind the stack.
			let _ = self.check_class(ast, class);
		}

		for fun in &mut module.functions {
			// Ignore errors at this point as there's no need to unwind the stack.
			let _ = self.check_fun_declare(ast, fun);
		}

		//for global in &mut module.globals {
		//	self.check_declare(ast, global);
		//}

		
	}

	fn check_modules(&mut self, ast: &AstProxy) {
		self.global_scope = true;

		// Before anything else, fix all function signatures.
		//
		// For now, in order to get FunCaptures working correctly, we make a first
		// pass which "fix"es functions, which must be done for all functions
		// (e.g. call_captured_rev.poni). We might come up with a more sophisticated
		// system later...
		for fun in self.db.iter_fun() {
			self.fix_fun_declare(fun);
		}

		// Check globals based on the ordering in db.
		let globals = std::mem::take(&mut self.db.globals);
		for global in &globals {
			let Some(mut initializer) = self.db.get(*global).initializer else {
				if PANIC_ON_BAD_NODE {
					panic!("ICE: Tried to typecheck global without initializer");
				}
				else {
					// Skip globals without initializers. In theory we might
					// let the language server have global variables without
					// initializers.
					continue;
				}
			};
			let _ = self.check_assign(ast, &self.db.get(*global).location.clone(), *global, &mut initializer, true);
			// Be sure to re-set the initializer
			self.db.get_mut(*global).initializer = Some(initializer);
		}
		self.db.globals = globals;

		for source in ast.sources.iter() {
            let mut source = ast.sources.get_mut(source);
            let module = &mut source.module;
			self.check_module(ast, module);
		}
	}
}

pub fn typecheck(db: &mut Db, ast: &mut Ast) -> bool {
	let mut checker = TypeChecker::new(db);

	let proxy = ast.get_proxy();
	checker.check_modules(&proxy);
	proxy.commit();

	checker.had_error
}