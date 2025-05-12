use crate::db::*;
use crate::lexer::Tok;
use crate::module::Module;
use crate::typ::Type;

use crate::expr::*;

use std::fmt::Write as _;

struct Codegen<'a> {
	functions: Vec<String>,
	structs: Vec<String>,

	return_types: Vec<TypId>,

	/// Helps us resolve variables to the correct thing.
	/// TODO: Do we want to instead synthesize AST nodes for variables that
	/// are inside classes..?
	inside_class: Vec<ClassId>,

	val_idx: usize,

	indent_level: usize,

	fun_init_buffer: String,

	db: &'a Db
}

/// Some of the output buffers used for code generation. Separate from
/// Codegen so that we can pass them to self.methods() without borrow checker
/// errors.
struct CodegenOutputs {
	/// Buffer containing the declarations for all global variables.
	global_define: String,
			
	/// Buffer containing the initialization code for all global variables.
	global_init: String,

	string_const_define: String,
	string_const_init: String,

	/// Declares structs used for classes, etc.
	struct_declare: String,
	struct_define: String,

	fun_declare: String,
}

impl CodegenOutputs {
	pub fn new() -> Self {
		CodegenOutputs {
			global_define: String::new(),
			global_init: String::new(),

			string_const_define: String::new(),
			string_const_init: String::new(),

			fun_declare: String::new(),

			struct_declare: String::new(),
			struct_define: String::new(),
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
	DirectVar {
		name: &'static str,
		// The number of class accesses we have to walk to get to the variable.
		//
		// Assumption: Each function has a local variable "this" which lets
		// us get to the current class members. Then, "this" also lets us
		// get to the superclass.
		depth: usize,
	},
	DirectSelf,
	StringLit {
		id: StrConstId,
	},
	BoolLit {
		val: bool,
	},

	/// Equivalent to Type::Bottom, sort of.
	Bottom,

	/// Equivalent to Type::Void. The kind of val produced in:
	/// fun test() { }
	/// print(test())
	/// 
	/// As test() returns void.
	Void,
}

impl Val {
	pub fn typed(self, typ: TypId) -> TypedVal {
		return TypedVal {
			val: self,
			typ,
		}
	}
}

struct TypedVal {
	val: Val,
	typ: TypId,
}

impl TypedVal {
	pub fn is_bottom(&self) -> bool {
		// NOTE:
		// Apparently we are able to form TypedVal that have a typ of Bottom
		// that do not have a Val of Bottom. Spooky...
		self.val.is_bottom()
	}

	pub fn needs_storage(&self) -> bool {
		self.val.needs_storage()
	}
}

enum PromotedVal {
	Simple(Val),
	Promoted(Val, &'static str),
	Bottom,
	Void,
}

impl PromotedVal {
	pub fn is_bottom(&self) -> bool {
		match self {
			PromotedVal::Bottom => true,
			PromotedVal::Simple(v) | PromotedVal::Promoted(v, _) => v.is_bottom(),
			PromotedVal::Void => false,
		}
	}

	pub fn is_void(&self) -> bool {
		match self {
			PromotedVal::Bottom => false,
			PromotedVal::Simple(v) | PromotedVal::Promoted(v, _) => v.is_void(),
			PromotedVal::Void => true,
		}
	}
}

impl Val {
	pub fn is_bottom(&self) -> bool {
		match self {
			Val::Bottom => true,
			_ => false,
		}
	}

	pub fn is_void(&self) -> bool {
		match self {
			Val::Void => true,
			_ => false,
		}
	}

	pub fn needs_storage(&self) -> bool {
		match self {
			Val::Bottom | Val::Void => false,
			_ => true,
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
#[macro_export]
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

#[macro_export]
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
			Val::DirectVar { name, depth } => {
				if *depth > 0 {
					// TODO: The problem with this system is it doesn't seem
					// like it can meaningfully support static variables in a
					// clean way. We probably do want to change into synthesizing
					// AST nodes of some sort.
					write!(f, "this->")?;
					let mut depth_loop = depth - 1;
					while depth_loop > 0 {
						panic!("todo: add nested class support, etc");
						depth_loop -= 1;
					}
				}
				write!(f, "{name}")
			}
			Val::DirectSelf => {
				write!(f, "this")
			}

			// String literals are always stored in variables with a consistent naming scheme.
			Val::StringLit { id } => write!(f, "ps_str_const{}", id.to_usize()),
			Val::BoolLit { val } => match val {
				true => write!(f, "((ps_bool)1)"),
				false => write!(f, "((ps_bool)0)"),
			}
			Val::Bottom => write!(f, "<pony:compiler-err:bottom-val>"),

			// Void values have no representation.
			Val::Void => Ok(())
		}
		
	}
}

impl std::fmt::Display for PromotedVal {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			PromotedVal::Simple(inner) => write!(f, "{}", inner),
			PromotedVal::Promoted(inner, promo_fn) => write!(f, "{promo_fn}({inner})"),
			PromotedVal::Bottom => write!(f, "<pony:compiler-err:bottom-val>"),
			PromotedVal::Void => Ok(()),
		}
	}
}

struct Indenter {
	level: usize,
}

impl std::fmt::Display for Indenter {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		for _ in 0..self.level {
			write!(f, "\t")?;
		}
		Ok(())
	}
}

macro_rules! define_val {
	($self:ident, $into:ident, $val:expr, $($arg:tt)*) => {
		if $val.needs_storage() {
			inf_write!($into, "{}{} {}",
				$self.indent(),
				$self.db.get_ctype($val.typ),
				$val.val
			);
			inf_write!($into, $($arg)*);
		}
	}
}

macro_rules! set_val {
	($self:ident, $into:ident, $val:expr, $($arg:tt)*) => {
		if $val.needs_storage() {
			inf_write!($into, "{}{}", $self.indent(), $val.val);
			inf_write!($into, $($arg)*);
		}
	}
}

impl<'a> Codegen<'a> {
	fn new(db: &'a Db) -> Self {
		return Codegen {
			functions: Vec::new(),
			structs: Vec::new(),

			return_types: Vec::new(),

			val_idx: 0,

			indent_level: 0,

			db,

			fun_init_buffer: String::new(),

			inside_class: Vec::new(),
		}
	}

	fn indent(&self) -> Indenter { return Indenter { level: self.indent_level } }

	fn new_val(&mut self) -> Val {
		let val = Val::Tmp(self.val_idx);
		self.val_idx += 1;
		val
	}

	// TODO: MOve all uses of new_val() to this function
	fn new_val_typed(&mut self, typ: TypId) -> TypedVal {
		if typ == self.db.types.void { return Val::Void.typed(typ); }
		if typ == self.db.types.bottom { return Val::Bottom.typed(typ); }

		self.new_val().typed(typ)
	}

	fn promote(&self, val: TypedVal, to: TypId) -> PromotedVal {
		if val.typ == to {
			return PromotedVal::Simple(val.val);
		}

		if val.is_bottom() || val.typ == self.db.types.bottom {
			return PromotedVal::Bottom;
		}

		// TODO: Does Any type automatically promote to Bottom?
		// It's not clear if this is correct, but it is seemingly necessary
		// for test cases such as variable/assign_to_bottom_binop_var
		// and set/set_bottom_etc.
		//
		// The justification seems to be that if we're trying to promote
		// something to bottom, it's because we already have a bottom somewhere
		// in the expression.
		if to == self.db.types.bottom {
			return PromotedVal::Bottom;
		}

		if val.typ == self.db.types.int && to == self.db.types.float {
			return PromotedVal::Promoted(val.val, "ps_promote_int_to_float")
		}

		if val.typ == self.db.types.str_const &&
		        to == self.db.types.str_buf
		{
			return PromotedVal::Promoted(val.val, "ps_promote_str_to_buf")	
		}

		if val.typ == self.db.types.str &&
		        to == self.db.types.str_buf
		{
			return PromotedVal::Promoted(val.val, "ps_promote_str_to_buf")	
		}

		if val.typ == self.db.types.str_const &&
		        to == self.db.types.str
		{
			return PromotedVal::Promoted(val.val, "ps_promote_str_const_to_str")	
		}

		// Note: We will only actually try to promote() if the type check stage
		// at some point creates code where we need a promotion. So we should
		// generally get this panic if something is either missing in the typechecker,
		// or if we're missing a promotion corresponding to a case in compute_assignable.
		panic!("compiler err: unknown promotion {} -> {}", self.db.repr_type(val.typ), self.db.repr_type(to));
	}

	fn compile_binary(&mut self, binary: &Binary, into: &mut String) -> TypedVal {
		let left = self.expr(&binary.left, into);
		if left.is_bottom() { println!("binary: left is bottom"); return left; /* Val::Bottom */ }
		// TODO: Maybe we should have each function return a (Val, TypId) tuple,
		// so that we can save time here..?
		let left = self.promote(left, binary.typ);

		let right = self.expr(&binary.right, into);
		if right.is_bottom() { println!("binary: right is bottom"); return right; /* Val::Bottom */ }
		let right = self.promote(right, binary.typ);

		let op = match binary.op {
			Tok::Star => '*',
			Tok::Plus => '+',
			Tok::Minus => '-',
			Tok::Slash => '/',
			_ => unreachable!()
		};

		let val = self.new_val();
		let ctype = self.db.get_ctype(binary.typ);
		let indent = self.indent();
		// TODO: Indentation system
		inf_writeln!(into, "{indent}const {ctype} {val} = {left} {op} {right};");

		val.typed(binary.typ)
	}

	fn compile_comparison(&mut self, compare: &Comparison, into: &mut String) -> TypedVal {
		let left = self.expr(&compare.left, into);
		if left.is_bottom() { return left; }

		let right = self.expr(&compare.right, into);
		if right.is_bottom() { return right; }

		let op = match compare.op {
			Tok::Less => "<",
			Tok::LessEqual => "<=",
			Tok::Greater => ">",
			Tok::GreaterEqual => ">=",
			_ => unreachable!(),
		};

		let val = self.new_val();

		let left = self.promote(left, compare.compare_as);
		let right = self.promote(right, compare.compare_as);
		let indent = self.indent();

		inf_writeln!(into, "{indent}const ps_bool {val} = (ps_bool)({left} {op} {right});");

		val.typed(self.db.types.bool)
	}

	fn compile_partial_print(&mut self, val: &TypedVal, into: &mut String) {
		let typid = val.typ;

		// No promotion is possible inside a print, so simply unwrap the val
		// for printing. We will return the result later.
		let val = &val.val;

		let typ = self.db.get(typid);

		let indent = self.indent();

		match typ {
			Type::Int => inf_writeln!(into, "{indent}ps_print_int({val});"),
			Type::Float => inf_writeln!(into, "{indent}ps_print_float({val});"),
			Type::Bool => inf_writeln!(into, "{indent}ps_print_bool({val});"),
			Type::Void => inf_writeln!(into, "{indent}/* ps_print_void */"),
			Type::StrConst | Type::Str => inf_writeln!(into, "{indent}ps_print_str({val});"),
			Type::StrBuf => inf_writeln!(into, "{indent}ps_print_str({val}->buffer);"),
			Type::Bottom => { },
			Type::Unassigned => inf_writeln!(into, "{indent}<pony:compiler-err:print-unassigned>"),

			Type::Fun(_) => inf_writeln!(into, "{indent}ps_print_ptr(\"fun\", (uintptr_t){val}.fun);"),
			Type::FunRaw(_) => inf_writeln!(into, "{indent}ps_print_ptr(\"fun*\", (uintptr_t){val});"),
			Type::Class(_) => inf_writeln!(into, "{indent}ps_print_ptr(\"object\", (uintptr_t){val});"),

			// TODO: Consider simply making 10.0 a float and 10 an int..?
			// at least, unless assigned differently..?
			// The context system is getting increasingly awkward.
			Type::AssumeFloat => todo!(),
			Type::AssumeInt => todo!(),
			Type::UnboundIdent(_) => inf_writeln!(into, "{indent}<pony:compiler-err:print-unbound-ident>"),
		}
	}

	fn compile_partial_str(&mut self, val: &TypedVal, buf_val: &Val, into: &mut String) {
		let typ = self.db.get(val.typ);

		// No promotion is possible inside a str(), so simply unwrap the val
		// for printing.
		let val = &val.val;

		let indent = self.indent();

		match typ {
			Type::Int => inf_writeln!(into, "{indent}ps_strfmt_int({buf_val}, {val});"),
			Type::Float => inf_writeln!(into, "{indent}ps_strfmt_float({buf_val}, {val});"),
			Type::Void => inf_writeln!(into, "{indent}/* ps_strfmt_void */"),
			Type::Bool => inf_writeln!(into, "{indent}ps_strfmt_bool({buf_val}, {val});"),
			Type::StrConst | Type::Str => inf_writeln!(into, "{indent}ps_strfmt_str({buf_val}, {val});"),
			Type::StrBuf => inf_writeln!(into, "{indent}ps_strfmt_strbuf({buf_val}, {val});"),
			Type::Bottom => { },
			Type::Unassigned => inf_writeln!(into, "{indent}<pony:compiler-err:strfmt-unassigned>"),
			Type::Fun(_) => todo!("str() for functions"),
			Type::FunRaw(_) => todo!("str() for function pointers"),
			Type::Class(_) => todo!("str() for classes"),
			Type::AssumeFloat => todo!(),
			Type::AssumeInt => todo!(),
			Type::UnboundIdent(_) => inf_writeln!(into, "{indent}<pony:compiler-err:strfmt-unbound-ident>"),
		}
	}

	fn compile_if(&mut self, if_: &If, into: &mut String) -> TypedVal {
		let indent = self.indent();

		let cond = self.expr(&if_.condition, into);
		let cond = self.promote(cond, self.db.types.bool);

		// Generate storage for the value of the expression, if relevant.
		let own_val = self.new_val_typed(if_.typ);
		define_val!(self, into, own_val, ";\n");

		inf_writeln!(into, "{indent}if ({cond}) {{");
		self.indent_level += 1;
		let then_val = self.expr(&if_.then_branch, into);
		
		// Save the value, if relevant.
		// IMPORTANT: set_val will only call promote() if the value is needed.
		set_val!(self, into, own_val, " = {};\n", self.promote(then_val, if_.typ));
		self.indent_level -= 1;
		inf_writeln!(into, "{indent}}}");

		// Generate else branch.
		if let Some(else_branch) = if_.else_branch.as_ref() {
			inf_writeln!(into, "{indent}else {{");
			self.indent_level += 1;

			let else_val = self.expr(&else_branch, into);
			// Save the value, if relevant.
			set_val!(self, into, own_val, " = {};\n", self.promote(else_val, if_.typ));
			self.indent_level -= 1;
			inf_writeln!(into, "{indent}}}");
		}

		own_val
	}

	fn expr(&mut self, expr: &Expr, into: &mut String) -> TypedVal {
		let indent = self.indent();
		match expr {
			Expr::Binary(binary) => self.compile_binary(binary, into),
			Expr::Comparison(compare) => self.compile_comparison(compare, into),
			Expr::If(if_) => self.compile_if(if_, into),

			Expr::Logical(logical) => {
				// Compute the left value up-front. The right value will be
				// computed inside the if, for short-circuiting.
				let left = self.expr(&logical.left, into);
				if left.is_bottom() { return left; }

				let left = self.promote(left, self.db.types.bool);

				let own_val = self.new_val_typed(self.db.types.bool);
				define_val!(self, into, own_val, " = {left};\n");

				// Short-circuiting behavior:
				// If we're 'and', and lhs is false, we don't evaluate rhs.
				// If we're 'or', and lhs is true, we don't evaluate rhs.
				let bang = match logical.op {
					Tok::And => "",
					Tok::Or => "!",
					_ => unreachable!(),
				};

				// Safe because own_val is bool
				inf_writeln!(into, "{indent}if({bang}{}) {{", own_val.val);
				self.indent_level += 1;

				// Generate the right expression inside the if.
				let right = self.expr(&logical.right, into);

				// Only store the value if not Bottom.
				if !right.is_bottom() {
					println!("right.is_bottom(): {}, right.typ: {}", right.is_bottom(), self.db.repr_type(right.typ));
					let right = self.promote(right, self.db.types.bool);

					// Our value now evalutes to this other one.
					set_val!(self, into, own_val, " = {right};\n");
				}

				self.indent_level -= 1;
				inf_writeln!(into, "{indent}}}");

				own_val
			}

			Expr::Variable(variable) => {
				// If the variable is inside a class, we need to walk the chain
				// of classes to synthesize the correct accessor.
				//
				// Note that, if the type checking and binding stages are correct,
				// this code should be fine, as the varaible should be bound to
				// a variable inside a class that we are also inside now.

				let mut depth = 0;

				if let Some(class) = self.db.get(variable.identity).class {
					for inside in self.inside_class.iter().rev() {
						// Depth is at least one, because we're in a class, so
						// increment before checking.
						depth += 1;
						if *inside == class {
							break;
						}
					}

					// TODO: Panic if we run out of classes before finding the
					// right one.
				}

				Val::DirectVar { name: self.db.get_cname(variable.identity), depth }
					.typed(self.db.get_var_type(variable.identity))
			},
			Expr::Assign(assign) => {
				self.compile_assign(assign.identity, assign.value, into, false)
			},
			Expr::FunCall(call) => {
				let ret_type = self.db.get_fun_ret_type(call.identity);
				let val = self.new_val_typed(ret_type);

				// TODO: Figure out a way to re-use PromotedVal buffers, maybe..
				let mut vals = Vec::new();
				for idx in 0..call.args.len() {
					let arg = &call.args[idx];
					let val = self.expr(arg, into);
					if val.is_bottom() {
						return val;
					}

					let val = self.promote(val, self.db.get_fun_param_type(call.identity, idx));
					vals.push(val);
				}

				// TODO: An awkward thing about the define_val! syntax is that
				// it must be remembed that it does not always print. So,
				// for a function call, we have to be sure to always generate
				// the cname separately.
				define_val!(self, into, val, " = ");
				inf_write!(into, "{}(", self.db.get_fun_cname(call.identity));

				let mut comma = "";
				for val in vals {
					inf_write!(into, "{comma}{val}");
					comma = ", ";
				}
				// TODO: Implement closure, gc scoping, etc
				inf_writeln!(into, "{comma}NULL);");

				val
			},
			// TODO: Consider using a different Expr type for string literals
			Expr::NumLiteral(lit) => {
				Val::DirectLit {
					ctype: self.db.get_ctype(lit.typ),
					lit: self.db.get(lit.contents.lexeme),
				}.typed(lit.typ)
			},
			Expr::StrLiteral(lit) => {
				return Val::StringLit { id: lit.id }.typed(self.db.types.str_const)
			},
			Expr::BoolLiteral(lit) => {
				return Val::BoolLit { val: lit.value }.typed(self.db.types.bool);
			}
			Expr::Block(block) => {
				let val = if self.db.type_generates_value(block.typ) {
					let val = self.new_val();

					inf_writeln!(into, "{indent}{} {};",
						self.db.get_ctype(block.typ),
						val);

					val
				} else { if block.typ == self.db.types.void { Val::Void } else { Val::Bottom } };

				let all_but_last = match block.stmts.len() {
					0 => 0,
					n => n - 1,
				};
				inf_writeln!(into, "{indent}{{");
				self.indent_level += 1;
				for stmt in &block.stmts[0..all_but_last] {
					let val = self.compile_stmt(stmt, into);
					if let Some(val) = val {
						if val.is_bottom() {
							// If we see a Bottom val inside a block, we have
							// found an unconditional return. So, we can
							// immediately stop processing further code (which
							// will be relevant to avoid e.g. generating accesses
							// to nonexistent variables).
							self.indent_level -= 1;
							inf_writeln!(into, "{indent}}}");
							return val;
						}
					}
				}

				let val = match (block.stmts.last(), val) {
					// If the block has no val, then generate a statement
					// and return Val::Void.
					(last, Val::Void) => {
						last.map(|last| self.compile_stmt(last, into));
						Val::Void
					},

					// Simmilar case for Val::Bottom
					(last, Val::Bottom) => {
						last.map(|last| self.compile_stmt(last, into));
						Val::Bottom
					},

					// If the block has a val, then last MUST exist
					// (otherwise the type checker is broken)
					// so return its value.
					(last, val) => {
						let last = self.compile_stmt(last.unwrap(), into);
						let last = last.unwrap();
						if last.needs_storage() && val.needs_storage() {
							let last = self.promote(last, block.typ);
							// Add one to indent because we're in the block
							inf_writeln!(into, "{indent}\t{val} = {last};");
						}

						val
					}
				};

				self.indent_level -= 1;
				inf_writeln!(into, "{indent}}}");
				val.typed(block.typ)
			},
			Expr::Print(print) => {
				let mut vals = Vec::new();

				for expr in &print.exprs {
					let val = self.expr(expr, into);

					// Propogate bottom values up. As a rule of thumb, always
					// bail from compiling as early as possible, for the dead-code
					// elimination that results...
					if val.is_bottom() {
						return val;	
					}

					vals.push(val);
				}

				for val in &vals {
					println!("found val in print -- typ = {}", self.db.repr_type(val.typ));
					self.compile_partial_print(val, into);
				}

				// For now, the print expr always adds a newline. This is the
				// same as GDScript, but we could change it in the future.
				inf_writeln!(into, "{indent}ps_println();");
				
				// The print returns its first value. Right now, prints always
				// require at least one argument.
				return unsafe { vals.into_iter().nth(0).unwrap_unchecked() }
			},
			Expr::Str(str) => {
				// TODO: We can count the size of any literals and prealloc at
				// least that much space, for efficiency.

				let mut vals = Vec::new();

				for expr in &str.exprs {
					let val = self.expr(expr, into);

					// Propogate bottom values up. 
					if val.is_bottom() {
						return val;	
					}

					vals.push(val);
				}

				// str() always returns a StrBuf, so we can easily generate a new
				// one unconditionally.
				let buf_val = self.new_val();
				inf_writeln!(into, "{indent}ps_strbuf *{buf_val} = ps_strbuf_new(8);");
				
				for val in &vals {
					self.compile_partial_str(val, &buf_val, into);
				}

				buf_val.typed(self.db.types.str_buf)
			}
			Expr::Unbound(_) => {
				panic!("compiler-err:tried-to-codegen-an-unbound-identifier-expression");
			},
			Expr::UnboundAssign(_) => {
				panic!("compiler-err:tried-to-codegen-an-unbound-assign-expression");
			},
			Expr::UnboundFunCapture(_) => {
				panic!("compiler-err:tried-to-codegen-an-unbound-funcapture");
			},
			Expr::Undefined(_) => {
				panic!("Internal compiler error: Tried to codegen an 'Undefined' node");
			}

			Expr::FunCapture(capt) => {
				let val = self.new_val_typed(capt.typ);

				// BIG TODO: Support closures. Not exactly clear how that will work.

				define_val!(self, into, val,
					" = ({}) {{ .fun = {}, .closure = NULL }};\n",
					self.db.get_ctype(capt.typ), // TODO: Maybe use a sig-specific fucntion
					self.db.get_fun_cname(capt.identity));

				val
			},

			Expr::ValCall(call) => {
				let fun_val = self.expr(call.value, into);
				if fun_val.is_bottom() {
					return fun_val;
				}
				// TODO: Support FunRaw, etc
				let fun_val = self.promote(fun_val, self.db.must_get_type(Type::Fun(call.sig)));

				let ret_type = self.db.get(call.sig).return_type;
				let val = self.new_val_typed(ret_type);

				// TODO: Figure out a way to re-use PromotedVal buffers, maybe..
				let mut vals = Vec::new();
				for idx in 0..call.args.len() {
					let arg = &call.args[idx];
					let val = self.expr(arg, into);
					if val.is_bottom() {
						return val;
					}

					let val = self.promote(val, self.db.get_sig_param_type(call.sig, idx));
					vals.push(val);
				}

				// TODO: An awkward thing about the define_val! syntax is that
				// it must be remembed that it does not always print. So,
				// for a function call, we have to be sure to always generate
				// the cname separately.
				define_val!(self, into, val, " = ");
				inf_write!(into, "{fun_val}.fun(");

				let mut comma = "";
				for val in vals {
					inf_write!(into, "{comma}{val}");
					comma = ", ";
				}
				// TODO: Implement closure, gc scoping, etc
				inf_writeln!(into, "{comma}{fun_val}.closure);");

				val
			},

			Expr::FunDeclare(declare) => {
				let val = self.new_val_typed(declare.typ);

				self.compile_function(declare.identity, declare.value);

				// BIG TODO: Support closures. Not exactly clear how that will work.
				// Also, when we do this, either we probably want to desugar
				// FunDeclare to somehow be wrapped in FunCapture, or at least
				// have some helper methods..

				define_val!(self, into, val,
					" = ({}) {{ .fun = {}, .closure = NULL }};\n",
					self.db.get_ctype(declare.typ), // TODO: Maybe use a sig-specific fucntion
					self.db.get_fun_cname(declare.identity));

				val
			},

			Expr::New(new) => {
				let val = self.new_val_typed(new.typ);

				// TODO: We need the C size (or at least the type name) of
				// each class, so we can do e.g. sizeof(struct cl_Class) or
				// just directly generate 16 or whatever. For now, use 32 bytes,
				// which is terrible, but it's a start.
				define_val!(self, into, val, " = ps_gc_must_calloc(sizeof(struct {}), 0);\n",
					self.db.get_class_cname(new.class));
				// Initialize the value.
				if val.needs_storage() {
					// Note: The value is a pointer-to-struct cl_Thing, so
					// we want to pass the direct value to the preparer.
					// e.g. struct cl_Thing *thing = malloc(); icl_Thing(thing);
					inf_writeln!(into, "{}({});",
						self.db.get_class_preparer_cname(new.class),
						val.val);
				}

				val
			},

			Expr::Get(get) => {
				let typ = self.db.get_var_type(get.var);
				let val = self.new_val_typed(typ);
			
				let lhs = self.expr(get.lhs, into);
				
				let varname = self.db.get_cname(get.var);

				// TODO: Should lhs be promoted...??
				define_val!(self, into, val, " = {}->{};\n", lhs.val, varname);

				val
			}

			Expr::Set(set) => {
				let typ = self.db.get_var_type(set.var);
				let val = self.new_val_typed(typ);
			
				let rhs = self.expr(set.rhs, into);
				if rhs.is_bottom() {
					return rhs;
				}
				let lhs = self.expr(set.lhs, into);
				// TODO: What happens if lhs is Bottom?
				let rhs = self.promote(rhs, typ);
				
				let varname = self.db.get_cname(set.var);

				// TODO: Should lhs be promoted...??
				// This is a bit hacky (the double assign), but I think it is overall fine.
				define_val!(self, into, val, " = {}->{} = {};\n", lhs.val, varname, rhs);

				val
			}

			Expr::SelfVal(selfval) => {
				Val::DirectSelf.typed(selfval.typ)
			}
		}
	}

	fn compile_class(&mut self, class_declare: &ClassDeclare) {
		// For the class, it does not generate any direct code.
		// But, we do have to generate a struct for the class,
		// as well as each of its function definitions.

		self.inside_class.push(class_declare.identity);

		for fun in &class_declare.funs {
			self.compile_function(fun.identity, &fun.value);
		}

		// Write the struct definition.
		let mut struc = String::new();
		inf_writeln!(struc, "struct {} {{", self.db.get_class_cname(class_declare.identity));

		// Simultaneously write the variable generator. 
		let enclosing_indent = self.indent_level;
		self.indent_level = 1;
		let mut preparer = String::new();

		inf_writeln!(preparer, "void {}(struct {} *this) {{",
				self.db.get_class_preparer_cname(class_declare.identity),
				self.db.get_class_cname(class_declare.identity));
		
		for var in &class_declare.vars {
			// Compile the assignment into the 'preparer' function. This is where
			// the variable value will be initialized.
			self.compile_assign(var.identity, &var.value, &mut preparer, false);
			// Compile the variable declaration into the struct.
			inf_writeln!(struc, "\t{} {};", self.db.get_var_ctype(var.identity), self.db.get_cname(var.identity));
		}

		inf_writeln!(struc, "}};");
		self.structs.push(struc);

		inf_writeln!(preparer, "}}");
		self.functions.push(preparer);

		self.indent_level = enclosing_indent;

		self.inside_class.pop();
	}

	fn compile_stmt(&mut self, stmt: &Stmt, into: &mut String) -> Option<TypedVal> {
		let indent = self.indent();
		match stmt {
			Stmt::Declare(declare) => {
				self.compile_assign(declare.identity, &declare.value, into, true);
				None
			},
			Stmt::ClassDeclare(class_declare) => {
				self.compile_class(class_declare);
				None
			}
			Stmt::Expression(expression) => {
				// The value of the expression is unused inside a statement.
				// Note that this automatically results in some kinds of
				// dead-code elimination, such as 30; turning into nothing.
				//
				// And, because the expression itself generates any code,
				// this function simply has to delegate to it.
				Some(self.expr(&expression.expression, into))
			},
			Stmt::Return(ret) => {
				match &ret.expression {
					Some(value) => {
						let needed_type = *self.return_types.last().unwrap();
						let val = self.expr(value, into);

						// Promote to the needed return type
						let val = self.promote(val, needed_type);
						// If the inner value is also a bottom type,
						// then we can't really generate a return here.
						if !val.is_bottom() {
							inf_writeln!(into, "{indent}return {val};");
						}
					},
					None => {
						inf_writeln!(into, "{indent}return;");
					}
				}

				Some(Val::Bottom.typed(self.db.types.bottom))
			}
		}
	}

	fn get_direct_var(&mut self, var: VarId) -> TypedVal {
		let mut depth = 0;

		if let Some(class) = self.db.get(var).class {
			for inside in self.inside_class.iter().rev() {
				// Depth is at least one, because we're in a class, so
				// increment before checking.
				depth += 1;
				if *inside == class {
					break;
				}
			}

			// TODO: Panic if we run out of classes before finding the
			// right one.
		}

		Val::DirectVar { name: self.db.get_cname(var), depth }
			.typed(self.db.get_var_type(var))
	}

	fn compile_assign(&mut self, var: VarId, expr: &Expr, into: &mut String, is_declaration: bool) -> TypedVal {
		let needed_type = self.db.get_var_type(var);
		let value = self.expr(expr, into);

		// Don't compile anything at all for variables that are bottom.
		if value.is_bottom() || value.typ == self.db.types.bottom { // TODO: Fix the value thingyingy
			return value;
		}

		println!("compile assign: var = {}, var type = {}, rhs type = {}",
					self.db.repr_var(var),
					self.db.repr_var_type(var),
					self.db.repr_type(value.typ));
		let value: PromotedVal = self.promote(value, needed_type);

		let (declaration, space) = if is_declaration {
			(self.db.get_var_ctype(var), " ")
		} else { ("", "") };

		let var_lvalue = self.get_direct_var(var);

		let indent = self.indent();
		// Discard type as we can't meaningfully promote it.
		inf_writeln!(into, "{indent}{declaration}{space}{} = {value};", var_lvalue.val);

		var_lvalue
	}

	// Does not generate the code for a function declaration (e.g. assigning
	// it to a local).
	fn compile_function(&mut self, fun: FunId, body: &Expr) {
		let is_init = Some(fun) == self.db.fun_init;

		let enclosing_indent = self.indent_level;
		self.indent_level = 1;
		let indent = self.indent();

		let mut own_buffer = String::new();

		// init() fun has no surrounding definition -- it is poni_init()
		if !is_init {
			inf_writeln!(own_buffer, "{} {}({}) {{",
				self.db.get_fun_ret_ctype(fun),
				self.db.get_fun_cname(fun),
				self.db.get_fun_cparams(fun));
		}

		if let Some(class) = self.db.get(fun).class {
			inf_writeln!(own_buffer, "{indent}struct {} *const this = closure;", self.db.get_class_cname(class));
		}

		// Same idea as in codegen()
		let own_return_type = self.db.get_fun_return_typid(fun);
		self.return_types.push(own_return_type);

		let val = self.expr(body, &mut own_buffer);
		if val.needs_storage() {
			let val = self.promote(val, own_return_type);
			// If it does have a value, then we write it as a default
			// return value.
			inf_writeln!(own_buffer, "{indent}return {val};");
		}

		// Pop type value
		self.return_types.pop();

		self.indent_level = enclosing_indent;

		// init() fun has no surrounding scope
		if !is_init { inf_writeln!(own_buffer, "}}"); }

		if Some(fun) == self.db.fun_init {
			self.fun_init_buffer = own_buffer;
		}
		else {
			self.functions.push(own_buffer);
		}
	}

	fn compile_string_constant_init(&mut self, define: &mut String, init: &mut String) {
		inf_writeln!(init, "void poni_init_strings(void) {{");
		for id in self.db.str_const_range() {
			inf_writeln!(define, "const ps_str* ps_str_const{} = NULL;", id.to_usize());
			inf_writeln!(init, "\tps_str_const{} = ps_str_from_literal({});",
				id.to_usize(), self.db.get(id));
		}
		inf_writeln!(init, "}}");
	}

	fn codegen_to_buffers(&mut self, module: &Module, out: &mut CodegenOutputs) {
		// Use an indent level of 1 for the initialization code for all global variables.
		self.indent_level = 1;
		for global in &module.globals {
			// Just dierectly encode the indentation..
			inf_writeln!(out.global_define, "{} {};",
				self.db.get_var_ctype(global.identity), self.db.get_cname(global.identity));

			// For globals, the initializer is not itself a declaration. So,
			// do tell self.compile_assign() that it's not a declaration.
			self.compile_assign(global.identity,
				&global.value,
				&mut out.global_init,
				false);
		}

		self.indent_level = 0;
		for fun in &module.functions {
			let is_init = Some(fun.identity) == self.db.fun_init;

			// Don't write declaration for the init() function.
			if !is_init {
				inf_writeln!(out.fun_declare, "{} {}({});",
					self.db.get_fun_ret_ctype(fun.identity),
					self.db.get_fun_cname(fun.identity),
					self.db.get_fun_cparams(fun.identity));
			}
			
			self.compile_function(fun.identity, &fun.value);
		}

		for class in &module.classes {
			self.compile_class(class);

			// For now: Write the forward declarations for these icl's here.
			// We might need to change how this works when we have nested classes.
			inf_writeln!(out.fun_declare, "void {}(struct {} *this);",
				self.db.get_class_preparer_cname(class.identity),
				self.db.get_class_cname(class.identity));
		}

		self.compile_string_constant_init(&mut out.string_const_define, &mut out.string_const_init);
	}

	fn codegen(&mut self, modules: &Vec<Module>, output: &mut dyn std::io::Write) -> std::io::Result<()> {
		let mut outputs = CodegenOutputs::new();

		for module in modules {
			self.codegen_to_buffers(module, &mut outputs);
		}

		writeln!(output, "#include \"poni/poni.h\"")?;
		writeln!(output, "#include \"poni/poni_standalone.h\"")?;

		writeln!(output, "// --- string constants ---\n{}", outputs.string_const_define)?;
		writeln!(output, "// --- struct declarations ---\n{}", outputs.struct_declare)?;
		writeln!(output, "// --- sig types ---\n{}", self.db.sig_declare_code)?;
		writeln!(output, "// --- struct definitions ---")?;
		for struc in &self.structs {
			writeln!(output, "{}", struc)?;
		}
		writeln!(output, "// --- global variables ---\n{}", outputs.global_define)?;
		writeln!(output, "// --- function declarations ---\n{}", outputs.fun_declare)?;
		writeln!(output, "// --- function definitions ---")?;
		for fun in &self.functions {
			writeln!(output, "{}", fun)?;
		}
		writeln!(output, "{}", outputs.string_const_init)?;
		writeln!(output, "void poni_init() {{")?;
		writeln!(output, "\tponi_init_strings();")?;
		writeln!(output, "{}", outputs.global_init)?;
		writeln!(output, "{}", self.fun_init_buffer)?;
		writeln!(output, "}}")?;

		Ok(())
	}
}

pub fn codegen(db: &mut Db, modules: &Vec<Module>, output: &mut dyn std::io::Write) -> std::io::Result<()> {
	let mut codegen = Codegen::new(db);

	codegen.codegen(modules, output)
}