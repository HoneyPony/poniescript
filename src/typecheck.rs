use std::any::Any;

use crate::db::*;
use crate::module::Module;
use crate::source::SourceLocation;
use crate::typ::Type;

use crate::expr::*;
use crate::error::Error;

use crate::arena::IndexCell;

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

impl<'db> TypeChecker<'db> {
	fn new(db: &'db mut Db) -> Self {
		TypeChecker {
			db,

			had_error: false,

			global_scope: false,

			return_types: Vec::new(),

			current_class: None
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
				for ty in &inner {
					inner_promoted.push(self.promote_ty_from_unassigned(*ty));
				}
				self.db.put_type(Type::Tuple(inner_promoted))
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

				return Ok(self.db.put_type(Type::Tuple(new_from)))
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

	// Computes the common "intersection" type of the two types. This can result
	// in promotions, e.g. from int to float (even though float is technically
	// not an intersection of float and int), while it can also result in 
	// "restrictions" (e.g. AssumeInt -> Float).
	//
	// Finally, one thing to note is the intersection of Bottom with anything
	// is itself.
	fn compute_intersect(&mut self, bottom_eats: bool, left: TypId, right: TypId) -> std::result::Result<TypId, TypeComputeErr> {
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

			// TODO: Is this correct?
			// It seems necessary for array_nested_empty_lhs, array_nested_empty_rhs
			(Type::Unassigned, _) => return Ok(right),
			(_, Type::Unassigned) => return Ok(left),

			(Type::ArrayOf(lhs), Type::ArrayOf(rhs)) => {
				// For arrays, the intersection is the intersection of their inner
				// types.
				// Note that right now, because we always return left or right
				// for the other cases for this function, we don't have to synthesize
				// a new type here. If we ever change that, we will.
				let inner = self.compute_intersect(bottom_eats, lhs, rhs)?;
				if inner == lhs { return Ok(lhs); }
				return Ok(rhs);
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

				return Ok(self.db.put_type(Type::Tuple(inner)))
			}

			_ => return Err(TypeComputeErr)
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

	
	/// DO NOT CALL THIS FUNCTION UNLESS YOU ARE do_promote_expr OR promote_from_unassigned.
	/// 
	/// This function will promote either to a Bottom type (i.e. promote_from_unassigned)
	/// or from no type, on the child nodes. It is mainly use to implement pushing
	/// Bottom types down the tree properly in promote_from_unassigned.
	/// 
	/// Note that it does contain the main "meat" of the do_promote_expr function.
	fn really_do_promote_expr(&mut self, ast: &AstProxy, expr_id: &mut ExprId, promote_to: TypId) {
		// First, we visit the child expr with promote_expr.
		self.promote_expr(ast, *expr_id, promote_to);

		// If the child node's type does NOT equal the promoted type, we synthesize
		// a runtime promotion.
		if ast.get_expr(*expr_id).typ(ast, &self.db) != promote_to {
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
			Stmt::Declare(declare) => {
				// Should have already promoted.
			},
			Stmt::Expression(expression) => {
				self.do_promote_expr(ast, &mut expression.expression, promote_to);
			},
			Stmt::Return(_) => {
				// Should have already promoted.
			},
			Stmt::ClassDeclare(class_declare) => {
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

				self.do_promote_expr(ast, &mut binary.left, binary.typ);
				self.do_promote_expr(ast, &mut binary.right, binary.typ);
			},
			Expr::Comparison(_) => {
				// We can't promote to any type, so we should have already
				// promoted our children.
			},
			Expr::Variable(_) => { /* Can't promote. */ },
			Expr::Logical(_) => { /* Can't promote. */ },
			Expr::FunCall(_) => { /* Can't promote. */ },
			Expr::FunDeclare(fun_declare) => {},
			Expr::ValCall(val_call) => {},
			Expr::FunCapture(fun_capture) => {},
			Expr::Assign(assign) => {},
			Expr::UnboundAssign(unbound_assign) => panic!("ICE: promote_expr UnboundAssign"),
			Expr::NumLiteral(num_literal) => {
				// Promote to the incoming type.
				num_literal.typ = promote_to;
			},
			Expr::StrLiteral(str_literal) => {
				// For now: Don't promote, promote in codegen stage.
				// OPT: Promote here, let the codegen make better use of information?
			},
			Expr::BoolLiteral(bool_literal) => {},
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
			Expr::Unbound(_) => panic!("ICE: promote_expr(Unbound)"),
			Expr::UnboundFunCapture(_) => panic!("ICE: promote_expr(UnboundFunCapture)"),
			Expr::Print(_) => {},
			Expr::Str(_) => { /* TODO: Possibly promote here, to improve stuff in backend? */ },
			Expr::New(_) => {},
			Expr::Get(get) => {},
			Expr::Set(set) => {},
			Expr::SelfVal(self_val) => {},
			Expr::ArrayLit(array_lit) => {
				let incoming_elem_typ = match self.db.get(promote_to) {
					Type::ArrayOf(elem) => *elem,
					_ => panic!("ICE: promote_expr(ArrayLit) to non-array type {}", self.db.repr_type(promote_to))
				};

				// The ArrayLit should be kind of like a big binary expression.
				// If we already have a concrete type, e.g. because we are an
				// array of float variables, then we can't promote to e.g. an
				// array of int, and we should actually eventually get an 
				// error.
				if self.db.is_not_concrete(array_lit.elem_typ) {
					array_lit.elem_typ = incoming_elem_typ;
					array_lit.arr_typ = promote_to;
				}

				// Promote all child nodes to our final elem type.
				for expr in &mut array_lit.values {
					self.do_promote_expr(ast, expr, array_lit.elem_typ);
				}
			},
			Expr::Index(index) => {},
			Expr::SetIndex(set_index) => {},
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
			Expr::Promote(_) => {
				// If we hit this, it means we're re-writing an earlier promote
				// with a different one.
				//
				// I'm actually not sure if that is a valid thing to do.
				//
				// Let's try panicing and see what happens.
				panic!("ICE: promote_expr(Promote)")
			},
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
			self.db.get_mut(var).typ = computed;
		}
		self.do_promote_expr(ast, expr_id, computed);

		Ok(computed)
	}


	// TODO: We could, inside this function, just directly call
	// promote_from_unassigned on any expr that has value_used = false -- we
	// should consider if that would make sense.
	fn check_expr(&mut self, ast: &AstProxy, expr_id: ExprId, value_used: bool) -> Result<TypId> {
		let mut binding = ast.exprs.get_mut(expr_id);
		let expr = binding.as_mut();
		Ok(match expr {
			Expr::Binary(binary) => {
				let left = self.check_expr(ast, binary.left, true)?;
				let right = self.check_expr(ast, binary.right, true)?;

				let computed = maybe_type_error!(
					self,
					self.compute_intersect(true, left, right),

					&binary.location,
					"Invalid operands to binary operator: LHS is {}, RHS is {}",
					self.db.repr_type(left),
					self.db.repr_type(right)
				);

				// PROMOTION: occurs in promote_expr

				binary.typ = computed;
				
				computed
			},
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
			Expr::Index(index) => {
				let arr_ty = self.check_expr(ast, index.value, true)?;

				let elem_ty = match self.db.get(arr_ty).clone() {
					Type::ArrayOf(elem) => elem,
					_ => type_error!(self, &index.location, "Can only index an array.")
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
					_ => type_error!(self, &set.location, "Can only index an array.")
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
					"Invalid assignment to array: need {}, but value is {}",
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

			Expr::ValCall(call) => {
				let value = self.check_expr(ast, call.value, true)?;
			
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

					self.do_promote_expr(ast, &mut call.args[i], computed);
				}

				// TODO: Should ValCall's use_sig their sig?

				self.db.get(call.sig).return_type
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
					panic!("ICE: New expression has unassigned type from Binder");
				}

				for init in &mut new.initializers {
					self.check_assign(ast, &init.location, init.var, &mut init.value, false)?;
				}

				new.typ
			}

			Expr::Get(get) => {
				// Get the type of the dotted expression. This lets us look up
				// the property on that type.
				let lhs = self.check_expr(ast, get.lhs, true)?;
				if let Some(property) = self.db.lookup_property(lhs, get.identifier.lexeme) {
					// We must actually store the looked-up property.
					get.var = property;

					return Ok(self.db.get_var_type(property));
				}

				// Check for possible function capture. If so, then we turn this
				// Get into a FunCapture.
				if let Some(fun) = self.db.lookup_member_fn(lhs, get.identifier.lexeme) {
					let as_funcapture = FunCapture {
						location: get.location.clone(),
						identity: fun,
						typ: self.db.types.unassigned,
						object: Some(get.lhs),
					};

					*expr = Expr::FunCapture(as_funcapture);
					drop(binding);
					return self.check_expr(ast, expr_id, value_used);
				}

				type_error!(self,
					&get.location,
					"Object of type '{}' has no such property '{}'",
					self.db.repr_type(lhs),
					self.db.get(get.identifier.lexeme));
			}

			Expr::Set(set) => {
				// Get the type of the dotted expression. This lets us look up
				// the property on that type.
				let lhs = self.check_expr(ast, set.lhs, value_used)?;
				let property = self.db.lookup_property(lhs, set.identifier.lexeme);

				let Some(property) = property else {
					type_error!(self,
						&set.location,
						"Object of type '{}' has no such property '{}'",
						self.db.repr_type(lhs),
						self.db.get(set.identifier.lexeme));
				};

				// We must actually store the looked-up property.
				set.var = property;

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
				// In theory we will resolve all idents beforehand? But this might
				// be different if we have function overloading.
				panic!("ICE: Tried to typecheck an unbound identifier expression '{}' at {}",
					self.db.get(unbound.identifier.lexeme),
					unbound.location.offset);
			},
			Expr::UnboundFunCapture(capt) => {
				// Here we will have to replace this UnboundFunCapture with
				// a FunCapture, for the current state of the project.

				let obj_ty = {
					let Some(object) = &mut capt.object else {
						type_error!(self, &capt.location, "Can't resolve function call.");
					};
					self.check_expr(ast, *object, true)?
				};

				if let Some(fun) = self.db.lookup_member_fn(obj_ty, capt.identifier.lexeme) {
					let as_funcapture = FunCapture {
						location: capt.location.clone(),
						identity: fun,
						typ: self.db.types.unassigned,
						object: capt.object
					};
					*expr = Expr::FunCapture(as_funcapture);
					drop(binding);
					return self.check_expr(ast, expr_id, value_used);
				}

				if let Some(property) = self.db.lookup_property(obj_ty, capt.identifier.lexeme) {
					let as_get = Get {
						location: capt.location.clone(),
						identifier: capt.identifier.clone(),
						lhs: capt.object.unwrap(), // Safety: We already checked this above
						var: property
					};
					*expr = Expr::Get(as_get);
					drop(binding);
					return self.check_expr(ast, expr_id, value_used);
				}

				type_error!(self,
					&capt.location,
					"Object of type {} has no such function or property {}.",
					self.db.repr_type(obj_ty),
					self.db.get(capt.identifier.lexeme));
			}
			Expr::UnboundAssign(_) => panic!("ICE: Tried to typecheck an UnboundAssign"),
			Expr::Undefined(_) => panic!("ICE: Tried to typecheck an Undefined"),

			Expr::MakeTuple(tuple) => {
				let mut inner = Vec::new();
				for expr in &tuple.values {
					inner.push(self.check_expr(ast, *expr, value_used)?);
				}

				let typ = self.db.put_type(Type::Tuple(inner));
				tuple.typ = typ;

				typ
			},

			// Promote should not be generated until we get to the TypeCheck stage.
			Expr::Promote(_) => panic!("ICE: Tried to typecheck Promote"),
		})
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
			},
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

				Ok(Some(self.db.types.bottom))
			}
		}
	}

	fn check_declare(&mut self, ast: &AstProxy, declare: &mut Declare) -> Result<TypId> {
		self.check_assign(ast, &declare.location, declare.identity, &mut declare.value, true)
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

	fn check_modules(&mut self, ast: &AstProxy, modules: &mut Vec<Module>) {
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
				panic!("ICE: Tried to typecheck global without initializer");
			};
			let _ = self.check_assign(ast, &self.db.get(*global).location.clone(), *global, &mut initializer, true);
			// Be sure to re-set the initializer
			self.db.get_mut(*global).initializer = Some(initializer);
		}
		self.db.globals = globals;

		for module in modules {
			self.check_module(ast, module);
		}
	}
}

pub fn typecheck(db: &mut Db, ast: &mut Ast, modules: &mut Vec<Module>) -> bool {
	let mut checker = TypeChecker::new(db);

	let proxy = ast.get_proxy();
	checker.check_modules(&proxy, modules);
	proxy.commit();

	checker.had_error
}