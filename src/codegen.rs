use crate::db::*;
use crate::lexer::Tok;
use crate::module::Module;
use crate::source::SourceLocation;
use crate::typ::Type;

use crate::expr::*;

use std::fmt::Write as _;
use std::io::Write as _;

struct Codegen {
	functions: Vec<String>,

	/// The code generator is responsible for propagating concrete numerical
	/// types leftward into subexpressions. To do so, we keep a stack of the
	/// known concrete types, and use the latest one when needed.
	context_types: Vec<TypId>,

	val_idx: usize,
}

/// The Val is the way that we make generating code much easier.
/// Instead of trying to generate good C code, we simply generate C code
/// that uses a lot of temporary variables. Each one of these is given
/// a unique name.
/// 
/// This lets us write the codegen as though we are generating code
/// for a pure stack machine.
struct Val {
	name: usize,
}

impl std::fmt::Display for Val {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "tmp{}", self.name)
	}
}

impl Codegen {
	fn new() -> Self {
		return Codegen {
			functions: Vec::new(),

			context_types: Vec::new(),

			val_idx: 0,
		}
	}

	fn new_val(&mut self) -> Val {
		let val = Val { name: self.val_idx };
		self.val_idx += 1;
		val
	}

	fn get_expr_ctype(&mut self, db: &mut Db, ty: TypId) -> String {
		let ty = match db.get(ty) {
			Type::UnassignedDecimal | Type::UnassignedNumeric => {
				*self.context_types.last().unwrap()
			},

			_ => ty
		};

		db.get_ctype(ty)
	}

	fn push(&mut self, db: &mut Db, ty: TypId) {
		match db.get(ty) {
			Type::UnassignedDecimal | Type::UnassignedNumeric => return,
			_ => {}
		}
		self.context_types.push(ty);
	}

	fn pop(&mut self, db: &mut Db, ty: TypId) {
		match db.get(ty) {
			Type::UnassignedDecimal | Type::UnassignedNumeric => return,
			_ => {}
		}
		self.context_types.pop();
	}

	fn binary(&mut self, db: &mut Db, binary: &Binary, into: &mut String) -> Val {
		self.push(db, binary.typ);

		let left = self.expr(db, &binary.left, into);
		let right = self.expr(db, &binary.right, into);

		self.pop(db, binary.typ);

		let op = match binary.op {
			Tok::Star => "*",
			Tok::Plus => "+",
			Tok::Minus => "-",
			Tok::Slash => "/",
			_ => unreachable!()
		};

		let val = self.new_val();
		let ctype = self.get_expr_ctype(db, binary.typ);
		// TODO: Indentation system
		writeln!(into, "const {ctype} {val} = {left} {op} {right};");

		val
	}

	fn expr(&mut self, db: &mut Db, expr: &Expr, into: &mut String) -> Val {
		match expr {
			Expr::Binary(binary) => self.binary(db, binary, into),
			Expr::Variable(_) => todo!(),
			Expr::Assign(_) => todo!(),
			Expr::Literal(lit) => {
				let val = self.new_val();
				let ctype = self.get_expr_ctype(db, lit.typ);
				let literal = db.get(lit.contents.lexeme);

				writeln!(into, "const {ctype} {val} = {literal};");

				val
			},
		}
	}

	fn assign(&mut self, db: &mut Db, var: VarId, expr: &Expr, into: &mut String) {
		let ctx = db.get_var_type(var);
		self.push(db, ctx);
		let value = self.expr(db, expr, into);
		self.pop(db, ctx);
		writeln!(into, "{} = {value};", db.get_cname(var));
	}

	fn codegen_to_buffers(&mut self, db: &mut Db, module: &Module, global_define: &mut String, global_init: &mut String) {
		for global in &module.globals {
			writeln!(global_define, "{} {};",
				db.get_var_ctype(global.identity), db.get_cname(global.identity));

			self.assign(db, global.identity, &global.value, global_init)
		}
	}

	fn codegen(&mut self, db: &mut Db, modules: &Vec<Module>) {
		/// Buffer containing the declarations for all global variables.
		let mut global_define = String::new();
		
		/// Buffer containing the initialization code for all global variables.
		let mut global_init = String::new();

		// TODO: Move those to a separate struct..?

		for module in modules {
			self.codegen_to_buffers(db, module, &mut global_define, &mut global_init);
		}

		println!("{}", global_define);
		println!("void poni_init() {{");
		println!("{}", global_init);
		println!("}}");
	}
}

pub fn codegen(db: &mut Db, modules: &Vec<Module>) {
	let mut codegen = Codegen::new();

	codegen.codegen(db, modules);
}