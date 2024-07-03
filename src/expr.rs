include!(concat!(env!("OUT_DIR"), "/expr.gen.rs"));

use std::mem::MaybeUninit;

use crate::{db::*, lexer::Token};
use crate::source::SourceLocation;
use crate::lexer::Tok;

impl Expr {
	pub fn typ(&self, db: &Db) -> TypId {
		match self {
			Expr::Binary(binary) => {
				binary.typ
			},
			Expr::Variable(var) => {
				db.get_var_type(var.identity)
			},
			Expr::Assign(assign) => {
				db.get_var_type(assign.identity)
			},
			Expr::NumLiteral(lit) => {
				lit.typ
			},
			Expr::StrLiteral(_) => {
				db.types.str_const
			},
			Expr::Block(block) => {
				block.typ
			},
			Expr::Unbound(_) => todo!(),
			Expr::Print(print) => {
				print.exprs[0].typ(db)
			},
			Expr::Str(_) => db.types.str_buf,
		}
	}

	pub fn promote(&mut self, typ: TypId, db: &Db) -> bool {
		match self {
			Expr::Binary(binary) => {
				if db.is_not_concrete(binary.typ) && db.is_concrete(typ) {
					// Recursively promote to any concrete type.
					//
					// NOTE that this is still O(n) in the number of AST nodes,
					// because once a type is promoted to concrete once, it cannot
					// have to do it again.
					binary.left.promote(typ, db);
					binary.right.promote(typ, db);
				}
				binary.typ = typ;
				true
			}
			Expr::Variable(_) => false,
			Expr::Assign(_) => false,
			Expr::NumLiteral(lit) => {
				lit.typ = typ;
				true
			},
			Expr::StrLiteral(_) => false,
			Expr::Block(block) => {
				block.typ = typ;
				if let Some(last) = block.stmts.last_mut() {
					return last.promote(typ, db);
				}
				true
			},
			Expr::Unbound(_) => false,
			Expr::Print(print) => {
				print.exprs[0].promote(typ, db)
			},
			Expr::Str(_) => false,
		}
	}
}

impl Stmt {
	pub fn promote(&mut self, typ: TypId, db: &Db) -> bool {
		match self {
			Stmt::Declare(_) => return false,
			Stmt::Expression(expr) => expr.expression.promote(typ, db),
			Stmt::FunDeclare(_) => return false,

			// The value of a Return is always Bottom, and so it cannot be
			// affected by promote().
			Stmt::Return(_) => return false,
		}
	}
}

/// Information for a variable.
pub struct Var {
	pub name: Token,
	pub typ: TypId,
}

pub struct Fun {
	pub name: Token,

	/// Parameters are the values when the function is defined, arguments
	/// are the values passed by the caller.
	pub parameters: Vec<VarId>,
	pub return_type: TypId,
}

struct BoxAlloc {
	exprs: &'static mut [MaybeUninit<Expr>],
	stmts: &'static mut [MaybeUninit<Stmt>],

	next_expr: usize,
	next_stmt: usize,
}

static mut alloc: Option<BoxAlloc> = None;

fn get_alloc() -> &'static mut BoxAlloc {
	unsafe {
		if let Some(a) = &mut alloc {
			return a;
		}

		alloc = Some(BoxAlloc {
			exprs: alloc_block(),
			stmts: alloc_block(),

			next_expr: 0,
			next_stmt: 0,
		});

		return get_alloc();
	}
}

fn alloc_block<T>() -> &'static mut [MaybeUninit<T>] {
	unsafe {
		// Copied straight from MaybeUninit::uninit_array
		let a = MaybeUninit::<[MaybeUninit<T>; 512]>::uninit().assume_init();
		return Vec::from(a).leak();
	}
}

pub fn alloc_expr(expr: Expr) -> &'static mut Expr {
	let a = get_alloc();

	if a.next_expr >= a.exprs.len() {
		// TODO: Figure out if there is a way to do this with less syntax...
		a.exprs = alloc_block();
		a.next_expr = 0;
	}

	let idx = a.next_expr;
	a.next_expr += 1;

	let res = &mut a.exprs[idx];
	*res = MaybeUninit::new(expr);

	unsafe { return res.assume_init_mut(); }
}

pub fn alloc_stmt(stmt: Stmt) -> &'static mut Stmt {
	let a = get_alloc();

	if a.next_stmt >= a.stmts.len() {
		// TODO: Figure out if there is a way to do this with less syntax...
		a.stmts = alloc_block();
		a.next_stmt = 0;
	}

	let idx = a.next_stmt;
	a.next_stmt += 1;

	let res = &mut a.stmts[idx];
	*res = MaybeUninit::new(stmt);

	unsafe { return res.assume_init_mut(); }
}
