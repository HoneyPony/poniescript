include!(concat!(env!("OUT_DIR"), "/expr.gen.rs"));

use std::mem::MaybeUninit;

use crate::{db::*, lexer::Token};
use crate::source::SourceLocation;
use crate::lexer::Tok;

/// Information for a variable.
pub struct Var {
	pub name: Token,
	pub typ: TypId,
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
