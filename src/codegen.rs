use crate::db::*;
use crate::lexer::Tok;
use crate::module::Module;
use crate::typ::Type;

use crate::expr::*;

use std::fmt::Write as _;
use std::io::Write as _;

struct Codegen<'a> {
	functions: Vec<String>,

	/// The code generator is responsible for propagating concrete numerical
	/// types leftward into subexpressions. To do so, we keep a stack of the
	/// known concrete types, and use the latest one when needed.
	context_types: Vec<TypId>,

	val_idx: usize,

	db: &'a mut Db
}

/// Some of the output buffers used for code generation. Separate from
/// Codegen so that we can pass them to self.methods() without borrow checker
/// errors.
struct CodegenOutputs {
	/// Buffer containing the declarations for all global variables.
	global_define: String,
			
	/// Buffer containing the initialization code for all global variables.
	global_init: String,
}

impl CodegenOutputs {
	pub fn new() -> Self {
		CodegenOutputs {
			global_define: String::new(),
			global_init: String::new(),
		}
	}
}

/// The Val is the way that we make generating code much easier.
/// Instead of trying to generate good C code, we simply generate C code
/// that uses a lot of temporary variables. Each one of these is given
/// a unique name.
/// 
/// This lets us write the codegen as though we are generating code
/// for a pure stack machine.
/// 
/// What's espcially nice about this is that, if we want to generate some
/// more compact expressions, we can simply add a new case to Val -- and as
/// long as we code it correctly, we will more or less automatically create
/// a syntax tree in C, with very little effort on top of what it would take
/// to do a pure stack-based style.
enum Val {
	Tmp(usize),
	DirectLit {
		ctype: &'static str,
		lit: &'static str
	},
}

/// The idea with this macro is that, according to the Rust documentation,
/// fmt::Write is supposed to be infallible in terms of pure formatting, i.e.
/// the error would only occur due to the underlying stream.
/// 
/// As such, there is very little reason to try to handle those errors. Instead,
/// we should basically unwrap() every single one. This macro helps make that
/// kind of idea easier to write.
macro_rules! inf_write {
	($into:expr, $($arg:tt)*) => {
		match write!($into, $($arg)*) {
			Ok(_) => {},
			Err(_) => {
				panic!("codegen: 'infallible' write to buffer failed");
			}
		}
	}
}

macro_rules! inf_writeln {
	($into:expr, $($arg:tt)*) => {
		match writeln!($into, $($arg)*) {
			Ok(_) => {},
			Err(_) => {
				panic!("codegen: 'infallible' write to buffer failed");
			}
		}
	}
}


impl std::fmt::Display for Val {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Val::Tmp(idx) => write!(f, "tmp{}", idx),
			Val::DirectLit {ctype, lit } => write!(f, "(({ctype}){lit})")	,
		}
		
	}
}

impl<'a> Codegen<'a> {
	fn new(db: &'a mut Db) -> Self {
		return Codegen {
			functions: Vec::new(),

			context_types: Vec::new(),

			val_idx: 0,

			db
		}
	}

	fn new_val(&mut self) -> Val {
		let val = Val::Tmp(self.val_idx);
		self.val_idx += 1;
		val
	}

	fn get_expr_ctype(&mut self, ty: TypId) -> &'static str {
		let ty = match self.db.get(ty) {
			Type::UnassignedDecimal | Type::UnassignedNumeric => {
				*self.context_types.last().unwrap()
			},

			_ => ty
		};

		self.db.get_ctype(ty)
	}

	fn push(&mut self, ty: TypId) {
		match self.db.get(ty) {
			Type::UnassignedDecimal | Type::UnassignedNumeric => return,
			_ => {}
		}
		self.context_types.push(ty);
	}

	fn pop(&mut self, ty: TypId) {
		match self.db.get(ty) {
			Type::UnassignedDecimal | Type::UnassignedNumeric => return,
			_ => {}
		}
		self.context_types.pop();
	}

	fn binary(&mut self, binary: &Binary, into: &mut String) -> Val {
		self.push(binary.typ);

		let left = self.expr(&binary.left, into);
		let right = self.expr(&binary.right, into);

		self.pop(binary.typ);

		let op = match binary.op {
			Tok::Star => '*',
			Tok::Plus => '+',
			Tok::Minus => '-',
			Tok::Slash => '/',
			_ => unreachable!()
		};

		let val = self.new_val();
		let ctype = self.get_expr_ctype(binary.typ);
		// TODO: Indentation system
		inf_writeln!(into, "const {ctype} {val} = {left} {op} {right};");

		val
	}

	fn expr(&mut self, expr: &Expr, into: &mut String) -> Val {
		match expr {
			Expr::Binary(binary) => self.binary(binary, into),
			Expr::Variable(_) => todo!(),
			Expr::Assign(_) => todo!(),
			Expr::Literal(lit) => {
				Val::DirectLit {
					ctype: self.get_expr_ctype(lit.typ),
					lit: self.db.get(lit.contents.lexeme),
				}
			},
		}
	}

	fn assign(&mut self, var: VarId, expr: &Expr, into: &mut String) {
		let ctx = self.db.get_var_type(var);
		self.push(ctx);
		let value = self.expr(expr, into);
		self.pop(ctx);
		inf_writeln!(into, "{} = {value};", self.db.get_cname(var));
	}

	fn codegen_to_buffers(&mut self, module: &Module, out: &mut CodegenOutputs) {
		for global in &module.globals {
			inf_writeln!(out.global_define, "{} {};",
				self.db.get_var_ctype(global.identity), self.db.get_cname(global.identity));

			self.assign(global.identity, &global.value, &mut out.global_init)
		}
	}

	fn codegen(&mut self, modules: &Vec<Module>) {
		let mut outputs = CodegenOutputs::new();

		for module in modules {
			self.codegen_to_buffers(module, &mut outputs);
		}

		println!("{}", outputs.global_define);
		println!("void poni_init() {{");
		println!("{}", outputs.global_init);
		println!("}}");
	}
}

pub fn codegen(db: &mut Db, modules: &Vec<Module>) {
	let mut codegen = Codegen::new(db);

	codegen.codegen(modules);
}