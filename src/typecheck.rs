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
					$location
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

	// Takes an expression, and promotes its type as if it were just assigned
	// to an unassigned value.
	//
	// Should be called on any expression that is otherwise not used, which
	// includes:
	// - arguments to print()
	// - statement expressions whose value is not used
	fn promote_from_unassigned(&mut self, expr: &mut Expr) -> TypId {
		let ty = expr.typ(self.db);
		let promoted = self.promote_ty_from_unassigned(ty);

		if promoted != ty {
			expr.promote(promoted, &self.db);
		}

		promoted
	}

	fn promote_ty_from_unassigned(&mut self, ty: TypId) -> TypId {
		if ty == self.db.types.assume_float {
			return self.db.types.float;
		}

		if ty == self.db.types.assume_int {
			return self.db.types.int;
		}

		// Add other promotions as needed. Note that this should correspond
		// in part to the match() in compute_assignable.

		ty
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

			// An unassigned clashing with an Assume resolves the Assume to its
			// assumed value.
			(Type::Unassigned, Type::AssumeInt) => return Ok(self.db.types.int),
			(Type::Unassigned, Type::AssumeFloat) => return Ok(self.db.types.float),

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

			// If the 'to' is unassigned, then anything is assignable to it.
			(Type::Unassigned, _) => return Ok(from),

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

		let ty_left = self.db.get(left);
		let ty_right = self.db.get(right);

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

			_ => return Err(TypeComputeErr)
		}
	}

	fn check_assign(&mut self, at: &SourceLocation, var: VarId, expr: &mut Expr, assign_ty: bool) -> Result<TypId> {
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

		if computed == self.db.types.void {
			type_error!(self, at,
			"Variable '{}' is type 'void' which is not a valid type for a variable.",
			self.db.repr_var(var));
		}

		// Only assign the type if we're in a declaration.
		if assign_ty {
			self.db.get_mut(var).typ = computed;
		}
		expr.promote(computed, self.db);

		Ok(computed)
	}


	// TODO: We could, inside this function, just directly call
	// promote_from_unassigned on any expr that has value_used = false -- we
	// should consider if that would make sense.
	fn check_expr(&mut self, expr: &mut Expr, value_used: bool) -> Result<TypId> {
		Ok(match expr {
			Expr::Binary(binary) => {
				let left = self.check_expr(&mut binary.left, true)?;
				let right = self.check_expr(&mut binary.right, true)?;

				let computed = maybe_type_error!(
					self,
					self.compute_intersect(true, left, right),

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
			Expr::Comparison(compare) => {
				let left = self.check_expr(&mut compare.left, true)?;
				let right = self.check_expr(&mut compare.right, true)?;

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

				compare.left.promote(computed, self.db);
				compare.right.promote(computed, self.db);

				// Comparisons always return bool.
				self.db.types.bool
			},
			Expr::Logical(logical) => {
				// We are expecting a real kind of value from the sub-expressions
				// (namely a bool, or promotable to bool), so we must say that the
				// value is used.
				let left = self.check_expr(&mut logical.left, true)?;
				let right = self.check_expr(&mut logical.right, true)?;

				let left_check = self.compute_assignable(self.db.types.bool, left);
				let right_check = self.compute_assignable(self.db.types.bool, right);

				let left_check = maybe_type_error!(self, left_check,
					logical.left.location(),
					"Invalid conditional expression in LHS to logical operator: Expression has type '{}'",
					self.db.repr_type(left));

				let right_check = maybe_type_error!(self, right_check,
					logical.right.location(),
					"Invalid conditional expression in RHS to logical operator: Expression has type '{}'",
					self.db.repr_type(right));

				logical.left.promote(left_check, self.db);
				logical.right.promote(right_check, self.db);

				// Logical operators always return bool.
				self.db.types.bool
			}
			Expr::If(if_) => {
				let condition_ty = self.check_expr(&mut if_.condition, true)?;
				let cond_computed =
					self.compute_assignable(self.db.types.bool, condition_ty);

				// TODO: Report the error at condition.location(), we need an
				// autogenerated method that does this.
				let cond_computed = maybe_type_error!(self, cond_computed,
					if_.condition.location(),
					"Invalid conditional expression: Expression has type '{}'",
					self.db.repr_type(condition_ty));

				if_.condition.promote(cond_computed, &self.db);

				let then_ty = self.check_expr(&mut if_.then_branch, value_used)?;

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

				let else_ty = self.check_expr(else_branch, value_used)?;

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
						&if_.location.begin()
					);
					let error = error.add_note(format!("then branch has type '{}'", self.db.repr_type(then_ty)),
						Some(if_.then_branch.val_location()));
					let error = error.add_note(format!("else branch has type '{}'", self.db.repr_type(else_ty)),
						Some(else_branch.val_location()));

					error
				});

				if_.then_branch.promote(computed, &self.db);
				else_branch.promote(computed, &self.db);

				if_.typ = computed;

				computed
			},
			Expr::Variable(var) => self.db.get(var.identity).typ,
			Expr::Assign(assign) => {
				self.check_assign(&assign.location, assign.identity, &mut assign.value, false)?
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
					self.check_stmt(stmt, false)?;
				}

				// If the value isn't used, we can simply type-check the
				// last statement then bail with Void.
				if !value_used {
					println!("block value not used @ {}", block.location.offset);
					block.stmts.last_mut().map(|stmt| self.check_stmt(stmt, false));
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
				let Some(val) = self.check_stmt(stmt, true)? else {
					type_error!(self, &block.location,
						"Return value of block is used, but its last statement has no value.");
				};

				// Return the computed TypId.
				println!("block type = {}", self.db.repr_type(val));
				block.typ = val;

				stmt.promote(val, self.db);

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
					self.check_expr(expr, true)?;
					self.promote_from_unassigned(expr);
				}

				self.check_expr(&mut print.exprs[0], true)?;
				let computed = self.promote_from_unassigned(expr);

				// TODO: We could store this type directly on the print() if we
				// wanted to -- that's what other ast nodes do...
				computed 
			},
			Expr::Str(str) => {
				// Str is very similar to print(), except it always return StrBuf instead
				// of its first argument.
				for expr in &mut str.exprs {
					self.check_expr(expr, true)?;
					self.promote_from_unassigned(expr);
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
						call.args.len(),
						fun_arity);
				}

				for i in 0..fun_arity {
					// Check each argument against the corresponding parameter.
					let arg = self.check_expr(&mut call.args[i], true)?;

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

					call.args[i].promote(computed, self.db);
				}

				self.db.get_fun_ret_type(call.identity)
			},

			Expr::ValCall(call) => {
				let value = self.check_expr(&mut call.value, true)?;
			
				// Now, we need to make sure that the value is Assignable to
				// a function type.
				let computed = self.compute_assignable(self.db.types.fun_sig_unassigned, value);

				let computed = maybe_type_error!(self, computed, &call.location,
					"Cannot call a value of type '{}'",
					self.db.repr_type(value));

				// TODO: Also support FunRaw calling..?
				call.value.promote(computed, &self.db);

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
					let arg = self.check_expr(&mut call.args[i], true)?;

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

					call.args[i].promote(computed, self.db);
				}

				self.db.get(call.sig).return_type
			},

			Expr::FunCapture(capt) => {
				// TODO: Ensure all functions have sigs.
				let sig = self.db.get(capt.identity).sig;

				if sig == self.db.sig_unassigned {
					panic!("FunCapture captured a function with unassigned sig. This will not work.");
				}

				// Make sure we use this sig.
				self.db.use_sig(sig);

				// TODO: Also support FunRaw captures.
				capt.typ = self.db.put_type(Type::Fun(sig));
				capt.typ
			},

			Expr::FunDeclare(declare) => {
				self.fix_fun_declare(declare);
				self.check_fun_declare(declare)?;

				let sig = self.db.get(declare.identity).sig;

				// This is basically the same idea as FunCapture.
				if sig == self.db.sig_unassigned {
					panic!("FunDeclare declared a function with unassigned sig. This will not work.");
				}

				// If we're capturing the value from the function, make sure
				// the sig is used.
				if value_used {
					self.db.use_sig(sig);
				}

				// TODO: Also support FunRaw -- in this case, I suppose the
				// function would itself know if it is FunRaw..?
				declare.typ = self.db.put_type(Type::Fun(sig));
				declare.typ
			},

			Expr::New(new) => {
				if new.typ == self.db.types.unassigned {
					panic!("New expression has unassigned type from Binder");
				}

				new.typ
			}

			Expr::Get(get) => {
				// Get the type of the dotted expression. This lets us look up
				// the property on that type.
				let lhs = self.check_expr(&mut get.lhs, true)?;
				let property = self.db.lookup_property(lhs, get.identifier.lexeme);

				let Some(property) = property else {
					type_error!(self,
						&get.location,
						"Object of type '{}' has no such property '{}'",
						self.db.repr_type(lhs),
						self.db.get(get.identifier.lexeme));
				};

				// We must actually store the looked-up property.
				get.var = property;

				self.db.get_var_type(property)
			}

			Expr::Set(set) => {
				// Get the type of the dotted expression. This lets us look up
				// the property on that type.
				let lhs = self.check_expr(&mut set.lhs, value_used)?;
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

				let rhs = self.check_expr(&mut set.rhs, true)?;

				let computed =
					self.compute_assignable(self.db.get_var_type(property), rhs);

				
				println!("set expr: property = {}, property type = {}, rhs type = {}",
					self.db.repr_var(property),
					self.db.repr_var_type(property),
					self.db.repr_type(rhs));

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
				set.rhs.promote(computed, self.db);

				self.db.get_var_type(property)
			}

			Expr::Unbound(unbound) => {
				// In theory we will resolve all idents beforehand? But this might
				// be different if we have function overloading.
				panic!("Internal compiler error: Tried to typecheck an unbound identifier expression '{}' at {}",
					self.db.get(unbound.identifier.lexeme),
					unbound.location.offset);
			},
			Expr::UnboundCall(_) => {
				panic!("compiler-err:tried-to-typecheck-an-unbound-call-expression");
			}
			Expr::UnboundAssign(_) => panic!("Internal compiler error: Tried to typecheck an UnboundAssign"),
			Expr::Undefined(_) => panic!("Internal compiler error: Tried to typecheck an Undefined"),
		})
	}

	fn check_class(&mut self, class_declare: &mut ClassDeclare) -> Result<()> {
		for declare in &mut class_declare.vars {
			self.check_declare(declare)?;
		}

		for fun in &mut class_declare.funs {
			self.check_fun_declare(fun)?;
		}

		Ok(())
	}

	fn check_stmt(&mut self, stmt: &mut Stmt, value_used: bool) -> Result<Option<TypId>> {
		match stmt {
			Stmt::Declare(declare) => {
				let typ = self.check_declare(declare)?;

				if typ == self.db.types.bottom {
					return Ok(Some(typ));
				}

				Ok(None)
			},
			Stmt::ClassDeclare(class_declare) => {
				self.check_class(class_declare)?;
				Ok(None)
			},
			Stmt::Expression(expr) => {
				let mut typ = self.check_expr(&mut expr.expression, value_used)?;
				if !value_used {
					// Non-value-used exprs should be promoted from unassigned.
					// If their value is used, the value-user will be responsible
					// for calling promote() with the proper type.
					typ = self.promote_from_unassigned(&mut expr.expression);
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

				let typ = self.check_expr(inner, true)?;
				let computed = self.compute_assignable( 
					return_type,
					typ);

				let computed = maybe_type_error!(self, 
					computed,
					&ret.location,
					"Trying to return {} in function returning {}",

					self.db.repr_type(typ),
					self.db.repr_type(return_type));

				inner.promote(computed, self.db);

				Ok(Some(self.db.types.bottom))
			}
		}
	}

	fn check_declare(&mut self, declare: &mut Declare) -> Result<TypId> {
		self.check_assign(&declare.location, declare.identity, &mut declare.value, true)
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
			let computed = self.compute_assignable(
				self.db.get_fun_return_typid(fun.identity),
				inner);

			let computed = maybe_type_error!(self, 
				computed,
				&fun.location,
				"Value of function body is {} but function returns {}",
				self.db.repr_type(inner),
				self.db.repr_type(self.db.get_fun_return_typid(fun.identity)));
		
			fun.value.promote(computed, self.db);
		}

		Ok(())
	}

	/// Takes a FunDeclare and ensures that its Sig matches its actual
	/// value.
	/// TODO: Make sure binder binds type names.... and so forth...
	/// 
	/// I suppose the signature could be generated in the parser, and then
	/// the unbound type names in that signature would be fixed by binder?
	fn fix_fun_declare(&mut self, fun_declare: &mut FunDeclare) {
		let mut sig = Sig { parameters: vec![], return_type: self.db.types.unassigned };

		for param in &self.db.get(fun_declare.identity).parameters {
			// We're essentially assuming that the type of param is good so far...
			// which is probably not true...
			sig.parameters.push(self.db.get_var_type(*param));
		}
		sig.return_type = self.db.get(fun_declare.identity).return_type;

		let sig = self.db.put_sig(&sig);
		self.db.get_mut(fun_declare.identity).sig = sig;
	}

	fn check_module(&mut self, module: &mut Module) {
		// HACK: Visit classes first so that type inference for properites works.
		// We really should get this working so that type inferences can directly
		// drive class type inference (i.e. type inference for the class members)
		// when needed.
		for class in &mut module.classes {
			self.check_class(class);
		}

		// For now, in order to get FunCaptures working correctly, we make a first
		// pass which "fix"es functions, which must be done for all functions
		// (e.g. call_captured_rev.poni). We might come up with a more sophisticated
		// system later...
		for fun in &mut module.functions {
			self.fix_fun_declare(fun);
		}

		for fun in &mut module.functions {
			// Ignore errors at this point as there's no need to unwind the stack.
			let _ = self.check_fun_declare(fun);
		}

		for global in &mut module.globals {
			self.check_declare(global);
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