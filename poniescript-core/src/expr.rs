include!(concat!(env!("OUT_DIR"), "/expr.gen.rs"));

use rustc_hash::{FxHashMap, FxHashSet};

use poni_arena::ArenaKey;
use crate::{db::*, lexer::Token};
use crate::typ::RangeEnd;
use crate::source::SourceLocation;
use crate::lexer::Tok;

use poni_arena::IndexCell;

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

impl Stmt {
	pub fn typ(&self, ast: &impl AstAbstract, db: &Db) -> TypId {
		match self {
			Stmt::Declare(declare) => {
				db.get(declare.identity).typ
			},
			Stmt::Expression(expression) => {
				ast.get_expr(expression.expression).typ(ast, db)
			},
			Stmt::ClassDeclare(_) => {
				// TODO: Different typing for ClassDeclare?
				db.types.void
			},
		}
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
			Expr::Unary(unary) => unary.typ,
			Expr::If(if_) => {
				// Note that ifs are similar to binary expressions.
				if_.typ
			}
			Expr::Loop(loop_) => loop_.typ,
			Expr::WhileLoop(while_) => while_.typ,
			// A for loop can't have a type as it is replaced with a while loop.
			Expr::ForLoop(_) => db.types.unassigned,
			Expr::Break(_) => db.types.bottom,
			Expr::Continue(_) => db.types.bottom,
			Expr::Return(_) => db.types.bottom,
			Expr::Variable(var) => {
				db.get_var_type(var.identity)
			},
			Expr::FunCall(call) => {
				db.get_fun_ret_type(call.identity)
			},
			Expr::BuiltinCall(call) => {
				// Because this type is dynamic, we might as well just cache it.
				call.typ
			}
			Expr::BuiltinCapture(_) => {
				// For now, these are always errors, so unassigned.
				db.types.unassigned
			}
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
			Expr::Unbound(_) => db.types.unassigned, // panic!("calling Expr::typ() on Unbound"),
			Expr::UnboundFunCapture(_) => db.types.unassigned, //panic!("calling Expr::typ() on UnboundFunCapture"),
			Expr::UnboundAssign(_) => db.types.unassigned, //panic!("calling Expr::typ() on UnboundAssign"),
			Expr::Print(print) => {
				// Note: This must be kept up-to-date with the typechecker
				if let Some(first) = print.exprs.get(0) { first.typ(ast, db) }
				else { db.types.void }
			},
			Expr::Str(_) => db.types.str_buf,
			Expr::New(new) => new.typ,
			Expr::Get(get) => db.get_var_type(get.vars.last().copied().unwrap_or(db.var_unassigned)),
			Expr::Set(set) => db.get_var_type(set.vars.last().copied().unwrap_or(db.var_unassigned)),
			Expr::Undefined(_) => db.types.unassigned, //panic!("ICE: Called Expr::typ() on Undefined"),
			Expr::SelfVal(selfval) => selfval.typ,
			Expr::Index(index) => index.typ,
			Expr::SetIndex(set) => set.typ,
			Expr::MakeTuple(make_tuple) => make_tuple.typ,
			Expr::MakeRange(make_range) => make_range.typ,
			Expr::Promote(promote) => promote.promote_to,
			Expr::Lerp(lerp) => lerp.typ,
			Expr::MakeSumType(sum) => sum.typ,
			Expr::OptionElse(optelse) => optelse.typ,
		}
	}
}

/// Information for a variable.
pub struct Var {
	pub name: StrId,
	pub typ: TypId,

	/// Stores whether this variable is readonly.
	pub readonly: bool,

	/// If this variable is a member of a class, this stores the class id.
	pub class: Option<ClassId>,
	/// If this variable is a function parameter, this stores the function id.
	pub fun: Option<FunId>,

	/// The initializer for this variable.
	pub initializer: Option<ExprId>,
	/// The "location" for the variable.
	pub location: SourceLocation,

	/// Doc comment for this variable.
	pub doc_comment: Option<Vec<Token>>,
}

pub struct Fun {
	pub name: Option<StrId>,
	pub sig: SigId,

	/// Parameters are the values when the function is defined, arguments
	/// are the values passed by the caller.
	pub parameters: Vec<VarId>,
	pub return_type: TypId,

	pub class: Option<ClassId>,

	/// Should be Some() if this is a function we are compiling, or None if
	/// this is an imported function from a C module.
	pub expression: Option<ExprId>,

	/// Location pointing to where the function is declared/defined.
	pub location: SourceLocation,

	/// Doc comment for this function.
	pub doc_comment: Option<Vec<Token>>,
}

/// Represents a function signature. Includes the types of all parameters
/// and of the return value.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Sig {
	pub parameters: Vec<TypId>,
	pub return_type: TypId,
}

pub enum ImportKind {
	Not,
	/// For now, things imported from C headers should *not* be declared by us.
	/// 
	/// This may change.
	CHeader,
}

pub struct Class {
	pub name: StrId,
	pub vars: Vec<VarId>,
	pub funs: Vec<FunId>,

	/// Variables that new{} expressions are mandated to initialize.
	/// 
	/// We store these in a set so that we can easily "check them off" in the
	/// type checker.
	pub mandatory_vars: FxHashSet<VarId>,

	pub import_kind: ImportKind,

	pub var_map: FxHashMap<StrId, VarId>,
	pub fun_map: FxHashMap<StrId, FunId>,

	/// Location pointing to where the class is declared/defined.
	pub location: SourceLocation,

	/// Doc comment for this class.
	pub doc_comment: Option<Vec<Token>>,
}
