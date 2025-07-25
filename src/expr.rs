include!(concat!(env!("OUT_DIR"), "/expr.gen.rs"));

use rustc_hash::FxHashMap;

use crate::arena::ArenaKey;
use crate::typ::Type;
use crate::{db::*, lexer::Token};
use crate::source::SourceLocation;
use crate::lexer::Tok;

use crate::arena::IndexCell;

pub struct NewInitElem {
	pub var: VarId,
	pub ident: Token,
	pub value: ExprId,
	pub location: SourceLocation
}

impl Stmt {
	pub fn val_location(&self, _ast: &impl AstAbstract) -> SourceLocation {
		self.location().clone()
	}
}

/// Necessary in order to use std::mem::take on Expr's for some parts of the
/// compiler.
impl std::default::Default for Expr {
	fn default() -> Self {
		return Expr::mk_undefined(SourceLocation {
			// SAFETY: We only use this in order to std::mem::take something
			// that will no longer be accessed.
			// 
			// (this is sketch... maybe we can come up with something better?)
			source: unsafe { SourceId::from_index(0) },
			offset: 0,
			length: 0
		})
	}
}

impl Expr {
	

	// TODO:
	// Right now the only reason this takes &mut Db rather than &Db is so that
	// we can use put_type for Expr::New.
	// We could change this by either storing a general TypId in Expr::New
	// (either alongside the class or instead of), but should we...?
	pub fn typ(&self, ast: &impl AstAbstract, db: &Db) -> TypId {
		match self {
			Expr::ArrayLit(lit) => {
				lit.arr_typ
			}
			Expr::Binary(binary) => {
				binary.typ
			},
			Expr::If(if_) => {
				// Note that ifs are similar to binary expressions.
				if_.typ
			}
			Expr::Variable(var) => {
				db.get_var_type(var.identity)
			},
			Expr::FunCall(call) => {
				db.get_fun_ret_type(call.identity)
			},
			Expr::ValCall(call) => {
				db.get(call.sig).return_type
			},
			Expr::FunCapture(capt) => {
				// db.put_type(Type::Fun(db.get(capt.identity).sig))
				// Maybe store the type on the FunCapture..?
				capt.typ
			},
			Expr::FunDeclare(declare) => {
				// Maybe do it like FunCapture..?
				declare.typ
			}
			Expr::Assign(assign) => {
				db.get_var_type(assign.identity)
			},
			Expr::NumLiteral(lit) => {
				lit.typ
			},
			Expr::StrLiteral(_) => {
				db.types.str_const
			},
			Expr::BoolLiteral(_) => {
				db.types.bool
			},
			Expr::Comparison(_) => {
				db.types.bool
			},
			Expr::Logical(_) => db.types.bool,
			Expr::Block(block) => {
				block.typ
			},
			Expr::Unbound(_) => panic!("calling Expr::typ() on Unbound"),
			Expr::UnboundFunCapture(_) => panic!("calling Expr::typ() on UnboundFunCapture"),
			Expr::UnboundAssign(_) => panic!("calling Expr::typ() on UnboundAssign"),
			Expr::Print(print) => {
				print.exprs[0].typ(ast, db)
			},
			Expr::Str(_) => db.types.str_buf,
			Expr::New(new) => new.typ,
			Expr::Get(get) => db.get_var_type(get.var),
			Expr::Set(set) => db.get_var_type(set.var),
			Expr::Undefined(_) => panic!("calling Expr::typ() on Undefined"),
			Expr::SelfVal(selfval) => selfval.typ,
			Expr::Index(index) => index.typ,
			Expr::SetIndex(set) => set.typ,
		}
	}

	pub fn promote(&mut self, typ: TypId, ast: &Ast, db: &Db) -> bool {
		// Cannot promote to Bottom.
		if typ == db.types.bottom {
			return false;
		}

		match self {
			Expr::Binary(binary) => {
				if db.is_not_concrete(binary.typ) && db.is_concrete(typ) {
					// Recursively promote to any concrete type.
					//
					// NOTE that this is still O(n) in the number of AST nodes,
					// because once a type is promoted to concrete once, it cannot
					// have to do it again.
					binary.left.promote(typ, ast, db);
					binary.right.promote(typ, ast, db);
				}
				binary.typ = typ;
				true
			},
			Expr::Index(index) => {
				//index.index.promote(db.types.int, db);
				index.typ = typ;
				true
			}
			Expr::SetIndex(set) => {
				set.typ = typ;
				true
			}
			Expr::ArrayLit(lit) => {
				// Only promote if the incoming type is actually an Array of
				// something.
				let incoming_elem_typ = match db.get(typ) {
					Type::ArrayOf(elem) => *elem,
					_ => return false
				};

				if db.is_not_concrete(lit.elem_typ) && db.is_concrete(incoming_elem_typ) {
					// Recursively promote to any concrete element type
					//
					// NOTE that this is still O(n) in the number of AST nodes,
					// because once a type is promoted to concrete once, it cannot
					// have to do it again.
					for expr in &mut lit.values {
						expr.promote(incoming_elem_typ, ast, db);
					}
				}
				// The array type is the direct incoming typ.
				lit.arr_typ = typ;
				lit.elem_typ = incoming_elem_typ;
				true
			}
			Expr::Comparison(_) => {
				// Types do not propagate down into the comparison. It will
				// promote its own operands, though.
				false
			},
			Expr::Logical(_) => {
				// Does not promote.
				false
			},
			Expr::If(if_) => {
				if db.is_not_concrete(if_.typ) && db.is_concrete(typ) {
					// Same idea as binary.
					if_.then_branch.promote(typ, ast, db);
					if_.else_branch.as_mut().map(|b| b.promote(typ, ast, db));
				}
				if_.typ = typ;
				true
			}
			// TODO: Subclasses...?
			Expr::Variable(_) => false,
			Expr::SelfVal(_) => false,
			Expr::Assign(_) => false,
			Expr::FunCall(_) => false,
			Expr::ValCall(_) => false,
			Expr::FunCapture(_) => false,
			Expr::FunDeclare(_) => false,
			Expr::NumLiteral(lit) => {
				lit.typ = typ;
				true
			},
			Expr::StrLiteral(_) => false,
			Expr::BoolLiteral(_) => false,
			Expr::Block(block) => {
				block.typ = typ;
				if let Some(last) = block.stmts.last_mut() {
					return last.promote(typ, ast, db);
				}
				true
			},
			Expr::Unbound(_) => false,
			Expr::UnboundFunCapture(_) => false,
			Expr::UnboundAssign(_) => false,
			Expr::Print(print) => {
				print.exprs[0].promote(typ, ast, db)
			},
			Expr::Str(_) => false,
			Expr::New(_) => {
				// TODO: Promote to superclasses of this class.
				false
			}
			Expr::Get(_) => {
				// TODO: Promote to superclasses..?
				false
			},
			Expr::Set(_) => { false }
			Expr::Undefined(_) => panic!("calling Expr::promote() on Undefined")
		}
	}
}

impl Stmt {
	pub fn promote(&mut self, typ: TypId, ast: &Ast, db: &Db) -> bool {
		match self {
			Stmt::Declare(_) => return false,
			Stmt::Expression(expr) => expr.expression.promote(typ, ast, db),

			// The value of a Return is always Bottom, and so it cannot be
			// affected by promote().
			Stmt::Return(_) => return false,
			Stmt::ClassDeclare(_) => return false,
		}
	}
}

/// Information for a variable.
pub struct Var {
	pub name: Token,
	pub typ: TypId,

	/// If this variable is a member of a class, this stores the class id.
	pub class: Option<ClassId>,
	/// For class members, stores whether this variable was initialized.
	/// (TODO: Is there a way to not have this field on non-class variables?)
	pub init: bool,

	/// The initializer for this variable.
	pub initializer: Option<ExprId>,
	/// The "location" for the variable.
	pub location: SourceLocation,
}

pub struct Fun {
	pub name: Option<Token>,
	pub sig: SigId,

	/// Parameters are the values when the function is defined, arguments
	/// are the values passed by the caller.
	pub parameters: Vec<VarId>,
	pub return_type: TypId,

	pub class: Option<ClassId>,

	pub expression: ExprId,
}

/// Represents a function signature. Includes the types of all parameters
/// and of the return value.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Sig {
	pub parameters: Vec<TypId>,
	pub return_type: TypId,
}

pub struct Class {
	pub name: Token,
	pub vars: Vec<VarId>,
	pub funs: Vec<FunId>,

	pub var_map: FxHashMap<StrId, VarId>,
	pub fun_map: FxHashMap<StrId, FunId>,
}
