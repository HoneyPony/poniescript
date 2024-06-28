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

	return_types: Vec<TypId>,

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

	fun_declare: String,

	fun_define: String,
}

impl CodegenOutputs {
	pub fn new() -> Self {
		CodegenOutputs {
			global_define: String::new(),
			global_init: String::new(),

			fun_declare: String::new(),
			fun_define: String::new(),
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

	/// Equivalent to Type::Bottom, sort of.
	Bottom,

	/// Used in cases where there's no value.
	None,
}

impl Val {
	pub fn is_bottom(&self) -> bool {
		match self {
			Val::Bottom => true,
			_ => false,
		}
	}
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
			Val::Bottom => write!(f, "<pony:compiler-err:bottom-val>"),
			Val::None => Ok(()),
		}
		
	}
}

impl<'a> Codegen<'a> {
	fn new(db: &'a mut Db) -> Self {
		return Codegen {
			functions: Vec::new(),

			context_types: Vec::new(),

			return_types: Vec::new(),

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
		let ty = self.db.get_context_type(ty);
		self.context_types.push(ty);
	}

	fn pop(&mut self, ty: TypId) {
		match self.db.get(ty) {
			Type::UnassignedDecimal | Type::UnassignedNumeric => return,
			_ => {}
		}
		let ty = self.db.get_context_type(ty);
		self.context_types.pop();
	}

	fn binary(&mut self, binary: &Binary, into: &mut String) -> Val {
		self.push(binary.typ);

		let left = self.expr(&binary.left, into);
		if left.is_bottom() { return Val::Bottom; }

		let right = self.expr(&binary.right, into);
		if right.is_bottom() { return Val::Bottom; }

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
			Expr::Block(block) => {
				let val = if self.db.type_generates_value(block.typ) {
					let val = self.new_val();

					inf_writeln!(into, "{} {};",
						self.get_expr_ctype(block.typ),
						val);

					val
				} else { Val::Bottom };

				let all_but_last = match block.stmts.len() {
					0 => 0,
					n => n - 1,
				};
				for stmt in &block.stmts[0..all_but_last] {
					self.stmt(stmt, into);
				}

				match (block.stmts.last(), val) {
					// If the block has no val, then generate a statement
					// and return Val::None.
					(last, Val::Bottom) => {
						last.map(|last| self.stmt(last, into));
						Val::Bottom
					},

					// If the block has a val, then last MUST exist
					// (otherwise the type checker is broken)
					// so return its value.
					(last, val) => {
						let last = self.stmt(last.unwrap(), into);
						let last = last.unwrap();
						if !val.is_bottom() {
							inf_writeln!(into, "{val} = {last};");
						}

						val
					}
				}
			}
		}
	}

	fn stmt(&mut self, stmt: &Stmt, into: &mut String) -> Option<Val> {
		match stmt {
			Stmt::Declare(_) => todo!(),
			Stmt::Expression(expression) => {
				// The value of the expression is unused inside a statement.
				// Note that this automatically results in some kinds of
				// dead-code elimination, such as 30; turning into nothing.
				//
				// And, because the expression itself generates any code,
				// this function simply has to delegate to it.
				Some(self.expr(&expression.expression, into))
			},
			Stmt::FunDeclare(_) => todo!(),
			Stmt::Return(ret) => {
				match &ret.expression {
					Some(value) => {
						self.push(*self.return_types.last().unwrap());
						let val = self.expr(value, into);
						self.pop(*self.return_types.last().unwrap());
						// If the inner value is also a bottom type,
						// then we can't really generate a return here.
						if !val.is_bottom() {
							inf_writeln!(into, "return {val};");
						}
					},
					None => {
						inf_writeln!(into, "return;");
					}
				}

				Some(Val::Bottom)
			}
		}
	}

	fn assign(&mut self, var: VarId, expr: &Expr, into: &mut String) {
		let ctx = self.db.get_var_type(var);
		self.push(ctx);
		let value = self.expr(expr, into);
		self.pop(ctx);
		inf_writeln!(into, "{} = {value};", self.db.get_cname(var));
	}

	// Does not generate the code for a function declaration (e.g. assigning
	// it to a local).
	fn function(&mut self, fun: FunId, body: &Expr) {
		let mut own_buffer = String::new();

		inf_writeln!(own_buffer, "{} {}({}) {{",
			self.db.get_fun_ret_ctype(fun),
			self.db.get_fun_cname(fun),
			self.db.get_fun_cparams(fun));

		// Same idea as in codegen()
		let ctx_type = self.db.get_context_type(
			self.db.get_fun_return_typid(fun)
		);
		self.push(ctx_type);
		self.return_types.push(ctx_type);

		let val = self.expr(body, &mut own_buffer);
		match val {
			// If the block has no value, that's fine...
			// TODO: Consider getting rid of Val::None
			Val::None | Val::Bottom => { },

			// But if it does have a value, then we write it as a default
			// return value.
			val => {
				inf_writeln!(own_buffer, "return {val};");
			}
		}

		// Pop type value
		self.return_types.pop();
		self.pop(ctx_type);

		inf_writeln!(own_buffer, "}}");

		self.functions.push(own_buffer);
	}

	fn codegen_to_buffers(&mut self, module: &Module, out: &mut CodegenOutputs) {
		for global in &module.globals {
			inf_writeln!(out.global_define, "{} {};",
				self.db.get_var_ctype(global.identity), self.db.get_cname(global.identity));

			self.assign(global.identity, &global.value, &mut out.global_init)
		}

		for fun in &module.functions {
			inf_writeln!(out.fun_declare, "{} {}({});",
				self.db.get_fun_ret_ctype(fun.identity),
				self.db.get_fun_cname(fun.identity),
				self.db.get_fun_cparams(fun.identity));
			
			self.function(fun.identity, &fun.value);
		}
	}

	fn codegen(&mut self, modules: &Vec<Module>) {
		let mut outputs = CodegenOutputs::new();

		// Strange but important: Any unassigned numeric type needs SOME kind
		// of assigned type, such as a statement expression 1 + 2;
		//
		// This could, to some degree, be handled by some dead code elimination.
		// But for now, we can just say that any undefined types are by default
		// floats.
		let float = self.db.put_type(Type::Float);
		self.push(float);

		for module in modules {
			self.codegen_to_buffers(module, &mut outputs);
		}

		println!("// --- global variables ---\n{}", outputs.global_define);
		println!("// --- function declarations ---\n{}", outputs.fun_declare);
		println!("// --- function definitions ---");
		for fun in &self.functions {
			println!("{}", fun);
		}
		println!("void poni_init() {{");
		println!("{}", outputs.global_init);
		println!("}}");
	}
}

pub fn codegen(db: &mut Db, modules: &Vec<Module>) {
	let mut codegen = Codegen::new(db);

	codegen.codegen(modules);
}