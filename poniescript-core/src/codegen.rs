use rustc_hash::FxHashSet;

use crate::arena::ArenaKey;
use crate::{db::*, Args};
use crate::lexer::Tok;
use crate::module::Module;
use crate::typ::Type;

use crate::expr::*;

use crate::arena::IndexCell;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::rc::Rc;

/// Helper struct for keeping track of where to store pointers across GC safepoints
/// and function call boundaries.
struct GCFrame {
	/// The next slot to add to the Set if none are available. Note that this
	/// also equals the number of slots we need at the end.
	next_alloc_slot: Cell<usize>,

	// TODO: Is there elsewhere in the code that we should use a BTreeSet?
	avail: RefCell<BTreeSet<usize>>,

	saved: RefCell<BTreeMap<usize, String>>,

	/// Contains a map of handed-out slots that we have already written into
	/// the array. We track slots in this map when we write them, so that we
	/// can avoid re-writing into the same slot -- this is because nothing
	/// else is allowed to overwrite them, but there's nothing telling the C
	/// compiler that except for us.
	written: RefCell<BTreeSet<usize>>,
}

impl GCFrame {
	fn new() -> Self {
		GCFrame {
			next_alloc_slot: Cell::new(0),
			avail: RefCell::new(BTreeSet::new()),
			saved: RefCell::new(BTreeMap::new()),
			written: RefCell::new(BTreeSet::new()),
		}
	}

	fn allocate_slot_internal(&self) -> usize {
		let mut avail = self.avail.borrow_mut();
		if let Some(last) = avail.pop_last() {
			return last;
		}

		let slot = self.next_alloc_slot.get();
		self.next_alloc_slot.set(slot + 1);
		slot
	}

	fn allocate_slot(&self, val: String) -> usize {
		let slot = self.allocate_slot_internal();
		let mut saved = self.saved.borrow_mut();
		debug_assert!(saved.insert(slot, val).is_none());

		slot
	}

	fn free_slots(&self, slots: &Vec<usize>) {
		let mut avail = self.avail.borrow_mut();
		let mut saved = self.saved.borrow_mut();
		let mut written = self.written.borrow_mut();
		for slot in slots {
			// This should always return true, because this slot should only
			// have belonged to use when we freed it.
			debug_assert!(avail.insert(*slot));
			debug_assert!(saved.remove(slot).is_some());

			// This might not be some, if we never wrote that slot. (Although,
			// in that case, ideally it would not have taken up a slot at all.
			// Oh well!)
			written.remove(slot);
		}
	}

	/// Marks a slot in the "written" array and returns:
	/// - true if it was not there before;
	/// - false if it was.
	/// This lets us generate code to only write to a slot when it needs marking.
	fn mark(&self, slot: usize) -> bool {
		let mut written = self.written.borrow_mut();
		written.insert(slot)
	}
}

struct Codegen<'a> {
	functions: Vec<String>,
	fun_declares: Vec<String>,
	structs: Vec<String>,
	struct_declares: Vec<String>,

	return_types: Vec<TypId>,

	/// Helps us resolve variables to the correct thing.
	/// TODO: Do we want to instead synthesize AST nodes for variables that
	/// are inside classes..?
	inside_class: Vec<ClassId>,

	val_idx: usize,

	indent_level: usize,

	fun_init_buffer: String,

	/// For now, when we are generated values that are referencing 'this', and
	/// we need the this_val to be something other than what it is, we can
	/// store a Tmp(usize) here.
	this_val: Option<usize>,

	gc_frame: Rc<GCFrame>,

	/// A list of Vec<usize>, where the inner Vecs contain references to the
	/// current GCFrame.
	block_scopes: Vec<Vec<usize>>,

	/// A flag for disabling the generation of GC frames. In this case, whenever
	/// we would write to the GC frame, we simply don't. (Mainly relevant for
	/// global initialization)
	disable_gc_frames: bool,

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
		this_val: Option<usize>,
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
	pub fn typed(self, typ: TypId, gc_slots_and_frame: Option<(Vec<usize>, Rc<GCFrame>)>) -> TypedVal {
		return TypedVal {
			val: self,
			typ,
			gc_slots_and_frame
		}
	}
}

struct TypedVal {
	val: Val,
	typ: TypId,
	
	// TODO: To make this more efficient, what we should really do is have the frame
	// be &GCFrame, and then pass a &gcframe down the whole tree of codegen
	// functions. That avoids needing an Rc.
	
	gc_slots_and_frame: Option<(Vec<usize>, Rc<GCFrame>)>
}

impl<'f> Drop for TypedVal {
	fn drop(&mut self) {
		if let Some((slots, frame)) = &self.gc_slots_and_frame {
			frame.free_slots(slots)
		}
	}
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

impl Val {
	pub fn is_bottom(&self) -> bool {
		match self {
			Val::Bottom => true,
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
				panic!("ICE: Codegen: 'infallible' write to buffer failed");
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
				panic!("ICE: Codegen: 'infallible' write to buffer failed");
			}
		}
	}
}


impl std::fmt::Display for Val {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Val::Tmp(idx) => write!(f, "t{}", idx),
			Val::DirectLit {ctype, lit } => write!(f, "(({ctype}){lit})")	,
			Val::DirectVar { this_val, name, depth } => {
				if *depth > 0 {
					if let Some(idx) = this_val {
						let val = Val::Tmp(*idx);
						write!(f, "{val}->")?;
					}
					else {
						// TODO: The problem with this system is it doesn't seem
						// like it can meaningfully support static variables in a
						// clean way. We probably do want to change into synthesizing
						// AST nodes of some sort.
						write!(f, "this->")?;
					}
					let depth_loop = depth - 1;
					while depth_loop > 0 {
						todo!("nested class support");
						depth_loop -= 1;
					}
				}
				write!(f, "{name}")
			}
			Val::DirectSelf => {
				write!(f, "this")
			}

			// String literals are always stored in variables with a consistent naming scheme.
			Val::StringLit { id } => write!(f, "ps_str_const{}", id.to_index()),
			Val::BoolLit { val } => match val {
				true => write!(f, "((ps_bool)1)"),
				false => write!(f, "((ps_bool)0)"),
			}
			Val::Bottom => panic!("ICE: Tried to codegen Val::Bottom"),

			// Void values have no representation.
			Val::Void => Ok(())
		}
		
	}
}

// Now that we no longer have promote() in the compiler, we can directly
// write TypedVals into the output stream.
impl std::fmt::Display for TypedVal {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		 write!(f, "{}", self.val)
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
			fun_declares: Vec::new(),
			structs: Vec::new(),
			struct_declares: Vec::new(),

			return_types: Vec::new(),

			val_idx: 0,

			indent_level: 0,

			db,

			this_val: None,

			fun_init_buffer: String::new(),

			inside_class: Vec::new(),

			gc_frame: Rc::new(GCFrame::new()),
			block_scopes: Vec::new(),

			disable_gc_frames: false,
		}
	}

	fn indent(&self) -> Indenter { return Indenter { level: self.indent_level } }

	fn new_val(&mut self) -> Val {
		let val = Val::Tmp(self.val_idx);
		self.val_idx += 1;
		val
	}

	fn val_alloc_slots_recurse(&mut self, prefix: &str, typ: TypId, slots: &mut Vec<usize>) {
		match self.db.get(typ) {
			Type::Int | Type::Float | Type::Bool | Type::Void => {}
			Type::StrConst => {}

			Type::Str | Type::StrBuf | Type::Class(_) | Type::ArrayOf(_) => {
				slots.push(self.gc_frame.allocate_slot(prefix.to_string()));
			}

			Type::Fun(_) => {
				// For functions, we are specifically saving the closure.
				//
				// TODO: It seems we often save the closure alongside the value
				// that we used to form the closure. This might just keep happening
				// until we get FunCall functionality working again, or until we
				// get a better IR..?
				slots.push(self.gc_frame.allocate_slot(format!("{prefix}.closure")));
			}

			Type::FunRaw(_) => {}
			Type::Bottom => {}

			Type::Option(id) => {
				// If it's a reference type, it has the same representation as
				// the non-Option type, so just re-use the logic.

				// If it's a value type... I'm not sure yet. I am starting to
				// wonder if a more powerful IR would be useful here.

				if self.db.is_value_type(*id) {
					todo!()
				}
				else {
					self.val_alloc_slots_recurse(prefix, *id, slots);
				}
			}

			Type::Tuple(typ_ids) => {
				for (idx, typ) in typ_ids.iter().enumerate() {
					// Don't allocate a new prefix string for any of the types
					// that don't have any slots.

					// TODO: Maybe profile this method, see if it's being
					// problematic..?
					if self.db.type_gc_slots(*typ) == 0 {
						continue;
					}
					let prefix = format!("{prefix}.v_{idx}");
					self.val_alloc_slots_recurse(&prefix, *typ, slots);
				}
			},

			Type::Unassigned | Type::AssumeInt | Type::AssumeFloat => {}
			Type::UnboundIdent(_) | Type::UnboundCStructPtr(_) => {}
		}
	}

	/// Helper function for safely converting a Val into a TypedVal. Think of it
	/// as val.typed() but without needing to specify the GC metadata, because
	/// this function computes the metadata.
	fn val_alloc_slots(&mut self, val: Val, typ: TypId) -> TypedVal {
		// In this case, don't bother even creating the vector, just directly
		// create the val.
		if self.db.type_gc_slots(typ) == 0 {
			return val.typed(typ, None);
		}

		let mut slots = vec![];
		let prefix = format!("{val}");
		self.val_alloc_slots_recurse(&prefix, typ, &mut slots);

		val.typed(typ, Some((slots, self.gc_frame.clone())))
	}

	// TODO: MOve all uses of new_val() to this function
	fn new_val_typed(&mut self, typ: TypId) -> TypedVal {
		if typ == self.db.types.void { return Val::Void.typed(typ, None); }
		if typ == self.db.types.bottom { return Val::Bottom.typed(typ, None); }

		let val = self.new_val();
		self.val_alloc_slots(val, typ)
	}

	/// Create a Val that is assume to have no gc_slots_and_frame.
	/// 
	/// This should be paired with a call to tmp_to_used_val(typ) once the Val
	/// actually has a value.
	fn new_val_typed_tmp(&mut self, typ: TypId) -> TypedVal {
		if typ == self.db.types.void { return Val::Void.typed(typ, None); }
		if typ == self.db.types.bottom { return Val::Bottom.typed(typ, None); }

		let val = self.new_val();
		val.typed(typ, None)
	}

	// TODO: This is Jank & Kind of Unsafe ?
	fn tmp_to_used_val(&mut self, mut val: TypedVal) -> TypedVal {
		// For Bottom in particular, we actually can't format it to a prefix
		// and then do the thing, so instead do this.
		if val.typ == self.db.types.void { return val; }
		if val.typ == self.db.types.bottom { return val; }
		
		// Also, we might as well actually check if the value has any slots
		// to fill out before we do format!() and allocate a vector.
		if self.db.type_gc_slots(val.typ) == 0 {
			return val;
		}

		let prefix = format!("{val}");
		let mut slots = vec![];
		self.val_alloc_slots_recurse(&prefix, val.typ, &mut slots);
		
		debug_assert!(val.gc_slots_and_frame.is_none());

		val.gc_slots_and_frame = Some((slots, self.gc_frame.clone()));
		val
	}

	fn save_gc_values(&mut self, into: &mut String) {
		// If we're disabling gc frames, trying to save the values will cause
		// issues.
		if self.disable_gc_frames { return; }

		let saved = self.gc_frame.saved.borrow();
		let indent = self.indent();
		for (k, v) in saved.iter() {
			// TODO:
			// If we do something where we always allocate the smallest available
			// k, then we can do an optimization in the GC where we stop checking
			// each frame when we hit a NULL. It probably won't really save much
			// though.

			// Use the 'mark' functionality of GCFrame to make sure we don't
			// do redundant writes of the same pointer.
			if self.gc_frame.mark(*k) {
				inf_writeln!(into, "{indent}gc_frame.ptrs[{k}] = {v};");
			}
		}
	}

	fn compile_partial_binary(&mut self, result_val: &TypedVal, lhs_val: &TypedVal, rhs_val: &TypedVal, op: char, cur_typ: TypId, postfix: &String, into: &mut String) {
		let indent = self.indent();
		match self.db.get(cur_typ) {
			Type::Int | Type::Float => {
				inf_writeln!(into, "{indent}{result_val}{postfix} = {lhs_val}{postfix} {op} {rhs_val}{postfix};");
			}
			Type::Tuple(typ_ids) => {
				// Iterate over each tuple member and perform the operator.
				for i in 0..typ_ids.len() {
					let postfix = format!("{postfix}.v_{i}");
					self.compile_partial_binary(result_val, lhs_val, rhs_val,
						op,
							typ_ids[i],
						&postfix,
						into);
				}
			},
			_ => {
				panic!("ICE: Trying to codegen binary operator for invalid types")
			}
		}
	}

	fn compile_binary(&mut self, ast: &Ast, binary: &Binary, into: &mut String) -> TypedVal {
		let left = self.expr(ast, binary.left, into);
		if left.is_bottom() { return left; /* Val::Bottom */ }

		let right = self.expr(ast, binary.right, into);
		if right.is_bottom() { return right; /* Val::Bottom */ }

		let op = match binary.op {
			Tok::Star => '*',
			Tok::Plus => '+',
			Tok::Minus => '-',
			Tok::Slash => '/',
			_ => panic!("ICE: Tried to codegen unknown binary operator")
		};

		let val = self.new_val_typed(binary.typ);
		let ctype = self.db.get_ctype(binary.typ);
		let indent = self.indent();

		// For simple binary expressions, write them out as one line & make them
		// a constant value
		if binary.typ == self.db.types.int || binary.typ == self.db.types.float {
			inf_writeln!(into, "{indent}const {ctype} {val} = {left} {op} {right};");
		}
		else {
			// Otherwise, we have to generate them through a tree, so we can't
			// make them const. But that's OK
			define_val!(self, into, val, ";\n");

			let postfix = "".to_string();
			self.compile_partial_binary(&val, &left, &right, 
				op, val.typ, &postfix, into);
		}

		val
	}

	fn compile_partial_lerp(&mut self, bool_val: &TypedVal, float_val: &TypedVal, one_minus_val: &TypedVal, from_val: &TypedVal, to_val: &TypedVal, target_val: &TypedVal, cur_typ: TypId, postfix: &String, into: &mut String) {
		let indent = self.indent();
		
		// IMPORTANT:
		// To walk down the tree of tuple types, we must start at the root
		// result TypId, but then walk through different TypIds, so that
		// we make progress (no stack overflow).
		match self.db.get(cur_typ) {
			Type::Int => {
				// TODO: What is the best way to lerp ints based on a float?
				inf_writeln!(into, "{indent}{target_val}{postfix} = (ps_int)({from_val}{postfix} * {one_minus_val} + {to_val}{postfix} * {float_val});")
			},
			Type::Float => {
				// TODO: What is the best way to lerp ints based on a float?
				inf_writeln!(into, "{indent}{target_val}{postfix} = {from_val}{postfix} * {one_minus_val} + {to_val}{postfix} * {float_val};")
			},
			Type::Tuple(typ_ids) => {
				// Iterate over each tuple member and lerp.
				for i in 0..typ_ids.len() {
					let postfix = format!("{postfix}.v_{i}");
					self.compile_partial_lerp(bool_val, float_val, one_minus_val,
						from_val, to_val, target_val,
							typ_ids[i],
						&postfix,
						into);
				}
			}

			// Everything else does bool-based lerp.
			_ => {
				// To do a bool-based lerp, 
				//    lerp("a", "b", false) gives "a", and true gives "b".
				inf_writeln!(into, "{indent}if({bool_val}) {{ {target_val}{postfix} = {to_val}{postfix}; }}");
				inf_writeln!(into, "{indent}\telse {{ {target_val}{postfix} = {from_val}{postfix}; }}");
			}
		}
	}

	fn compile_lerp(&mut self, ast: &Ast, lerp: &Lerp, into: &mut String) -> TypedVal {
		// We need the boolean value of the lerp value if we have anything other
		// than ints, floats, or vectors of such. For now, we just generate it
		// even if it isn't needed. TODO: optimize that.
		// let needs_bool_val = match self.db.get(lerp.typ) {
		// 	Type::Int => false,
		// 	Type::Float => false,
		// 	_ => true
		// };

		// Always generate a float value too. At least for now.

		let from_val = self.expr(ast, lerp.from, into);
		let to_val = self.expr(ast, lerp.to, into);

		// The type of this tells us whether it is bool or float.
		let amount_val = self.expr(ast, lerp.amount, into);
		assert!(amount_val.typ == self.db.types.float || amount_val.typ == self.db.types.bool);

		// Decide the bool_val and float_val based on amount_val
		let (bool_val, float_val) = if amount_val.typ == self.db.types.float {
			let bool_val = self.new_val_typed(self.db.types.bool);
			// EXTREMELY IMPORTANT SEMANTIC DECISION:
			// What exactly counts as true?
			//
			// For now, we're saying >= 0.5. But it could be > 0.5. Or anything
			// else.
			define_val!(self, into, bool_val, " = {amount_val} >= 0.5;\n");

			(bool_val, amount_val)
		}
		else {
			let float_val = self.new_val_typed(self.db.types.float);
			// Casting should give the desired behavior.
			define_val!(self, into, float_val, " = (ps_float){amount_val};\n");

			(amount_val, float_val)
		};

		// For big lerps, it is more efficient to generate one val and re-use it.
		// TODO: Don't do that for smaller lerps? Maybe also consider using specialized
		// methods for vectors...?
		let one_minus_val = self.new_val_typed(self.db.types.float);
		define_val!(self, into, one_minus_val, " = 1.0 - {float_val};\n");

		let result = self.new_val_typed(lerp.typ);
		// The result val will be defined through compile_partial_lerp.
		//
		// Again, maybe we can simplify in some cases. (TODO)
		define_val!(self, into, result, ";\n");

		// TODO: Consider having postfix be a re-used variable or something so
		// we don't have to allocate a new string every time.
		let postfix = "".to_string();
		self.compile_partial_lerp(&bool_val, &float_val, &one_minus_val,
			&from_val, &to_val, &result,
			result.typ,
			&postfix, into);

		result
	}

	fn compile_comparison(&mut self, ast: &Ast, compare: &Comparison, into: &mut String) -> TypedVal {
		let left = self.expr(ast, compare.left, into);
		if left.is_bottom() { return left; }

		let right = self.expr(ast, compare.right, into);
		if right.is_bottom() { return right; }

		let op = match compare.op {
			Tok::Less => "<",
			Tok::LessEqual => "<=",
			Tok::Greater => ">",
			Tok::GreaterEqual => ">=",
			_ => panic!("ICE: Tried to codegen unknown comparison operator"),
		};

		let val = self.new_val();

		let indent = self.indent();

		inf_writeln!(into, "{indent}const ps_bool {val} = (ps_bool)({left} {op} {right});");

		// Bool: No gc slot
		val.typed(self.db.types.bool, None)
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
			Type::Fun(_) => inf_writeln!(into, "{indent}ps_print_ptr(\"fun\", (uintptr_t){val}.fun);"),
			Type::FunRaw(_) => inf_writeln!(into, "{indent}ps_print_ptr(\"fun*\", (uintptr_t){val});"),
			Type::Class(_) => inf_writeln!(into, "{indent}ps_print_ptr(\"object\", (uintptr_t){val});"),
			
			Type::Option(_) => todo!("print() for Option"),
			Type::ArrayOf(_) => todo!("print() for Array"),
			Type::Tuple(tup) => {
				inf_writeln!(into, "{indent}ps_print_const(\"(\");");
				for (idx, ty) in tup.iter().enumerate() {
					if idx > 0 {
						inf_writeln!(into, "{indent}ps_print_const(\", \");");
					}
					// TODO: We can probably optimize the niceness of the code
					// generated here by splitting compile_partial_print into 
					// another helper function that just prints *any* value
					// of a given type.
					let new_val = self.new_val_typed(*ty);
					define_val!(self, into, new_val, " = {val}.v_{idx};\n");
					self.compile_partial_print(&new_val, into);
				}
				if tup.len() == 1 {
					// Place a comma after the last element for 1-element
					// tuple.
					inf_writeln!(into, "{indent}ps_print_const(\",)\");");
				}
				else {
					inf_writeln!(into, "{indent}ps_print_const(\")\");");
				}
			},

			// TODO: Consider simply making 10.0 a float and 10 an int..?
			// at least, unless assigned differently..?
			// The context system is getting increasingly awkward.
			Type::AssumeFloat => panic!("ICE: Tried to codegen print(AssumeFloat)"),
			Type::AssumeInt => panic!("ICE: Tried to codegen print(AssumeInt)"),
			Type::UnboundIdent(_) => panic!("ICE: Tried to codegen print(UnboundIdent)"),
			Type::Unassigned => panic!("ICE: Tried to codegen print(Unassigned)"),
			Type::UnboundCStructPtr(_) => panic!("ICE: Tried to codegen print(UnboundCStructPtr)"),
		}
	}

	fn compile_partial_str(&mut self, val: &TypedVal, buf_val: &Val, into: &mut String) {
		let typ = self.db.get(val.typ);

		// No promotion is possible inside a str(), so simply unwrap the val
		// for printing.
		let val = &val.val;

		let indent = self.indent();

		match typ {
			Type::Int => inf_writeln!(into, "{indent}ps_strfmt_int(ctx, {buf_val}, {val});"),
			Type::Float => inf_writeln!(into, "{indent}ps_strfmt_float(ctx, {buf_val}, {val});"),
			Type::Void => inf_writeln!(into, "{indent}/* ps_strfmt_void */"),
			Type::Bool => inf_writeln!(into, "{indent}ps_strfmt_bool(ctx, {buf_val}, {val});"),
			Type::StrConst | Type::Str => inf_writeln!(into, "{indent}ps_strfmt_str(ctx, {buf_val}, {val});"),
			Type::StrBuf => inf_writeln!(into, "{indent}ps_strfmt_strbuf(ctx, {buf_val}, {val});"),
			Type::Bottom => { },
			
			Type::Fun(_) => todo!("str() for Fun"),
			Type::FunRaw(_) => todo!("str() for FunRaw"),
			Type::Class(_) => todo!("str() for Class"),
			Type::ArrayOf(_) => todo!("str() for Array"),
			Type::Tuple(_) => todo!("str() for Tuple"),
			Type::Option(_) => todo!("str() for Option"),

			Type::AssumeFloat => panic!("ICE: Tried to codegen str(AssumeFloat)"),
			Type::AssumeInt => panic!("ICE: Tried to codegen str(AssumeInt)"),
			Type::UnboundIdent(_) => panic!("ICE: Tried to codegen str(UnboundIdent)"),
			Type::Unassigned => panic!("ICE: Tried to codegen str(Unassigned)"),
			Type::UnboundCStructPtr(_) => panic!("ICE: Tried to codegen str(UnboundCStructPtr)"),
		}
	}

	fn compile_if(&mut self, ast: &Ast, if_: &If, into: &mut String) -> TypedVal {
		let indent = self.indent();

		let cond = self.expr(ast, if_.condition, into);

		// Generate storage for the value of the expression, if relevant.
		// Note that we cannot have this value interacting with the GC until
		// we actually write to it.
		let own_val = self.new_val_typed_tmp(if_.typ);
		define_val!(self, into, own_val, ";\n");

		inf_writeln!(into, "{indent}if ({cond}) {{");
		self.indent_level += 1;
		let then_val = self.expr(ast, if_.then_branch, into);
		
		// Save the value, if relevant.
		// IMPORTANT: set_val will only call promote() if the value is needed.
		set_val!(self, into, own_val, " = {};\n", then_val);
		self.indent_level -= 1;
		inf_writeln!(into, "{indent}}}");

		// Generate else branch.
		if let Some(else_branch) = if_.else_branch.as_ref() {
			inf_writeln!(into, "{indent}else {{");
			self.indent_level += 1;

			let else_val = self.expr(ast, *else_branch, into);
			// Save the value, if relevant.
			set_val!(self, into, own_val, " = {};\n", else_val);
			self.indent_level -= 1;
			inf_writeln!(into, "{indent}}}");
		}

		self.tmp_to_used_val(own_val)
	}

	fn push_block_scope(&mut self) {
		self.block_scopes.push(Vec::new());
	}
	fn pop_block_scope(&mut self) {
		let scope = self.block_scopes.pop().expect("unmatched push/pop pair");
		// When we leave a block scope, free the slots that we had stored for it.
		self.gc_frame.free_slots(&scope);
	}

	fn expr(&mut self, ast: &Ast, expr: ExprId, into: &mut String) -> TypedVal {
		let indent = self.indent();
		match ast.exprs.get(expr).as_ref() {
			Expr::Binary(binary) => self.compile_binary(ast, binary, into),
			Expr::Lerp(lerp) => self.compile_lerp(ast, lerp, into),
			Expr::Comparison(compare) => self.compile_comparison(ast, compare, into),
			Expr::If(if_) => self.compile_if(ast, if_, into),

			Expr::Logical(logical) => {
				// Compute the left value up-front. The right value will be
				// computed inside the if, for short-circuiting.
				let left = self.expr(ast, logical.left, into);
				if left.is_bottom() { return left; }

				assert!(left.typ == self.db.types.bool);

				let own_val = self.new_val_typed(self.db.types.bool);
				define_val!(self, into, own_val, " = {left};\n");

				// Short-circuiting behavior:
				// If we're 'and', and lhs is false, we don't evaluate rhs.
				// If we're 'or', and lhs is true, we don't evaluate rhs.
				let bang = match logical.op {
					Tok::And => "",
					Tok::Or => "!",
					_ => panic!("ICE: Tried to codegen unknown logical operator"),
				};

				// Safe because own_val is bool
				inf_writeln!(into, "{indent}if({bang}{}) {{", own_val.val);
				self.indent_level += 1;

				// Generate the right expression inside the if.
				let right = self.expr(ast, logical.right, into);

				// Only store the value if not Bottom.
				if !right.is_bottom() {
					assert!(right.typ == self.db.types.bool);

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

				// For GC Slots, DirectVars are special.
				//
				// The variable itself should already have a gc slot. So the
				// DirectVar does not need any additional slots.
				Val::DirectVar { this_val: self.this_val, name: self.db.get_cname(variable.identity), depth }
					.typed(self.db.get_var_type(variable.identity), None)
			},
			Expr::Assign(assign) => {
				self.compile_assign(ast, assign.identity, assign.value, into, false)
			},
			Expr::FunCall(call) => {
				// TODO: FIgure out a way to re-use these vec buffers, maybe...
				let mut vals = Vec::new();
				for idx in 0..call.args.len() {
					let arg = &call.args[idx];
					let val = self.expr(ast, *arg, into);
					if val.is_bottom() {
						return val;
					}

					vals.push(val);
				}

				// We must save values at this time.
				self.save_gc_values(into);

				// Don't create the FunCall val itself until the GC vals are saved.
				let ret_type = self.db.get_fun_ret_type(call.identity);
				let val = self.new_val_typed(ret_type);

				// TODO: An awkward thing about the define_val! syntax is that
				// it must be remembed that it does not always print. So,
				// for a function call, we have to be sure to always generate
				// the cname separately.
				define_val!(self, into, val, " = ");
				inf_write!(into, "{}(ctx", self.db.get_fun_cname(call.identity));

				let comma = ", ";
				for val in vals {
					inf_write!(into, "{comma}{val}");
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
				}.typed(lit.typ, None)
			},
			Expr::StrLiteral(lit) => {
				// Literals don't need a gc slot. If they get promoted by an
				// Expr::Promote, it should take the slot.
				return Val::StringLit { id: lit.id }.typed(self.db.types.str_const, None)
			},
			Expr::BoolLiteral(lit) => {
				return Val::BoolLit { val: lit.value }.typed(self.db.types.bool, None);
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

				// Each block contains its own list of var GC scopes. This
				// somewhat helps us have a more conservative GC frame. We
				// really need a more powerful IR to have an ideal GC frame
				// system.
				self.push_block_scope();

				for stmt in &block.stmts[0..all_but_last] {
					let val = self.compile_stmt(ast, *stmt, into);
					if let Some(val) = val {
						if val.is_bottom() {
							// If we see a Bottom val inside a block, we have
							// found an unconditional return. So, we can
							// immediately stop processing further code (which
							// will be relevant to avoid e.g. generating accesses
							// to nonexistent variables).
							self.indent_level -= 1;
							self.pop_block_scope();
							inf_writeln!(into, "{indent}}}");
							return val;
						}
					}
				}

				let val = match (block.stmts.last(), val) {
					// If the block has no val, then generate a statement
					// and return Val::Void.
					(last, Val::Void) => {
						last.map(|last| self.compile_stmt(ast, *last, into));
						Val::Void
					},

					// Simmilar case for Val::Bottom
					(last, Val::Bottom) => {
						last.map(|last| self.compile_stmt(ast, *last, into));
						Val::Bottom
					},

					// If the block has a val, then last MUST exist
					// (otherwise the type checker is broken)
					// so return its value.
					(last, val) => {
						let last = self.compile_stmt(ast, *last.unwrap(), into);
						let last = last.unwrap();
						if last.needs_storage() && val.needs_storage() {
							// Add one to indent because we're in the block
							inf_writeln!(into, "{indent}\t{val} = {last};");
						}

						val
					}
				};

				self.pop_block_scope();

				self.indent_level -= 1;
				inf_writeln!(into, "{indent}}}");
				self.val_alloc_slots(val, block.typ)
			},
			Expr::Print(print) => {
				let mut vals = Vec::new();

				for expr in &print.exprs {
					let val = self.expr(ast, *expr, into);

					// Propogate bottom values up. As a rule of thumb, always
					// bail from compiling as early as possible, for the dead-code
					// elimination that results...
					if val.is_bottom() {
						return val;	
					}

					vals.push(val);
				}

				for val in &vals {
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
					let val = self.expr(ast, *expr, into);

					// Propogate bottom values up. 
					if val.is_bottom() {
						return val;	
					}

					vals.push(val);
				}

				// str() always returns a StrBuf, so we can easily generate a new
				// one unconditionally.
				let buf_val = self.new_val();
				inf_writeln!(into, "{indent}ps_strbuf *{buf_val} = ps_strbuf_new(ctx, 8);");
				
				for val in &vals {
					self.compile_partial_str(val, &buf_val, into);
				}

				// Note that val_alloc_slots is essentially saying "give me
				// this Val with this TypId, safely"
				self.val_alloc_slots(buf_val, self.db.types.str_buf)
			}
			Expr::Unbound(_) => {
				panic!("ICE: Tried to codegen an Unbound");
			},
			Expr::UnboundAssign(_) => {
				panic!("ICE: Tried to codegen an UnboundAssign");
			},
			Expr::UnboundFunCapture(_) => {
				panic!("ICE: Tried to codegen an UnboundFunCapture");
			},
			Expr::Undefined(_) => {
				panic!("ICE: Tried to codegen an Undefined");
			}

			Expr::FunCapture(capt) => {
				// BIG TODO: Support closures. Not exactly clear how that will work.

				let closure = match &capt.object {
					Some(expr) => {
						Some(self.expr(ast, *expr, into))
					},
					None => None
				};

				// Define val after inner expression (for GC)
				let val = self.new_val_typed(capt.typ);

				define_val!(self, into, val,
					" = ({}) {{ .fun = {}, ",
					self.db.get_ctype(capt.typ), // TODO: Maybe use a sig-specific fucntion
					self.db.get_fun_cname(capt.identity));

				if val.needs_storage() {
					// Second half of definition: closure
					if let Some(closure) = closure {
						// TODO: We need to promote Closure into essentially
						// the class type for the function?
						inf_writeln!(into, ".closure = {} }};", closure.val);
					}
					else {
						inf_writeln!(into, ".closure = NULL }};");
					}
				}

				val
			},

			Expr::ValCall(call) => {
				let fun_val = self.expr(ast, call.value, into);
				if fun_val.is_bottom() {
					return fun_val;
				}
				// TODO: Support FunRaw, etc
				assert!(fun_val.typ == self.db.must_get_type(Type::Fun(call.sig)));

				// TODO: Figure out a way to re-use PromotedVal buffers, maybe..
				let mut vals = Vec::new();
				for idx in 0..call.args.len() {
					let arg = &call.args[idx];
					let val = self.expr(ast, *arg, into);
					if val.is_bottom() {
						return val;
					}

					assert!(val.typ == self.db.get_sig_param_type(call.sig, idx));
					vals.push(val);
				}

				self.save_gc_values(into);

				// Don't create the function val itself until we have saved
				// the GC values.
				let ret_type = self.db.get(call.sig).return_type;
				let val = self.new_val_typed(ret_type);

				// TODO: An awkward thing about the define_val! syntax is that
				// it must be remembed that it does not always print. So,
				// for a function call, we have to be sure to always generate
				// the cname separately.
				define_val!(self, into, val, " = ");
				inf_write!(into, "{fun_val}.fun(ctx");

				let comma = ", ";
				for val in vals {
					inf_write!(into, "{comma}{val}");
					//comma = ", ";
				}
				// TODO: Implement closure, gc scoping, etc
				inf_writeln!(into, "{comma}{fun_val}.closure);");

				val
			},

			Expr::FunDeclare(declare) => {
				self.compile_function(ast, declare.identity, declare.value);

				// BIG TODO: Support closures. Not exactly clear how that will work.
				// Also, when we do this, either we probably want to desugar
				// FunDeclare to somehow be wrapped in FunCapture, or at least
				// have some helper methods..

				// Define val after inner expression, even though it shouldn't
				// matter here..?
				let val = self.new_val_typed(declare.typ);

				define_val!(self, into, val,
					" = ({}) {{ .fun = {}, .closure = NULL }};\n",
					self.db.get_ctype(declare.typ), // TODO: Maybe use a sig-specific fucntion
					self.db.get_fun_cname(declare.identity));

				val
			},

			Expr::New(new) => {
				let val = self.new_val_typed_tmp(new.typ);

				// TODO: We need the C size (or at least the type name) of
				// each class, so we can do e.g. sizeof(struct cl_Class) or
				// just directly generate 16 or whatever. For now, use 32 bytes,
				// which is terrible, but it's a start.
				define_val!(self, into, val, " = poni_gc_alloc_tagged(ctx, sizeof(struct {}), {});\n",
					self.db.get_class_cname(new.class), self.db.get_type_ctag(new.typ));
				// Initialize the value.
				if val.needs_storage() {
					// Note: The value is a pointer-to-struct cl_Thing, so
					// we want to pass the direct value to the preparer.
					// e.g. struct cl_Thing *thing = malloc(); icl_Thing(thing);
					//inf_writeln!(into, "{indent}{}({});",
					//	self.db.get_class_preparer_cname(new.class),
					//	val.val);

					// TODO: Reuse this somehow?
					let mut dont_initialize = FxHashSet::default();

					let Val::Tmp(idx) = val.val else {
						panic!("ICE: New class val wasn't a Tmp");
					};

					let enclosing_this_val = self.this_val;
					self.this_val = Some(idx);
					self.inside_class.push(new.class);

					// Run all the initializers from the new{} first.
					for init in &new.initializers {
						let rhs = self.expr(ast, init.value, into);
						assert!(rhs.typ == self.db.get_var_type(init.var));
						let varname = self.db.get_cname(init.var);

						inf_writeln!(into, "{indent}{}->{varname} = {rhs};", val.val);

						dont_initialize.insert(init.var);
					}

					// Run all the initializers from the class second.
					for var in &self.db.get(new.class).vars {
						// Skip any variables from the new{} expression.
						if dont_initialize.contains(var) { continue; }

						// Compile the assignment.
						if let Some(initializer) = self.db.get(*var).initializer {
							// TODO: Some way to re-use compiled exprs?
							let rhs = self.expr(ast, initializer, into); 
							let varname = self.db.get_cname(*var);
							inf_writeln!(into, "{indent}{}->{varname} = {rhs};", val.val);
							//self.compile_assign(ast, *var, initializer, into, false);
						}
					}

					self.inside_class.pop();
					self.this_val = enclosing_this_val;
				}

				self.tmp_to_used_val(val)
			},

			Expr::Get(get) => {
				let lhs = self.expr(ast, get.lhs, into);
				let arrow = self.db.get_c_member_lookup(lhs.typ);
				
				let varname = self.db.get_cname(get.var);

				// Don't define our own val until we've evaluated inner expr,
				// for GC.
				let typ = self.db.get_var_type(get.var);
				let val = self.new_val_typed(typ);

				// TODO: Should lhs be promoted...??
				define_val!(self, into, val, " = {}{arrow}{};\n", lhs.val, varname);

				val
			}

			Expr::Set(set) => {
				let rhs = self.expr(ast, set.rhs, into);
				if rhs.is_bottom() {
					return rhs;
				}
				let lhs = self.expr(ast, set.lhs, into);
				// TODO: What happens if lhs is Bottom? (this TODO written when we are promoting)
				let arrow = self.db.get_c_member_lookup(lhs.typ);
				
				let varname = self.db.get_cname(set.var);

				// Define our own val as late as possible, for GC.
				let typ = self.db.get_var_type(set.var);
				let val = self.new_val_typed(typ);

				// TODO: Should lhs be promoted...??
				// This is a bit hacky (the double assign), but I think it is overall fine.
				define_val!(self, into, val, " = {}{arrow}{} = {};\n", lhs.val, varname, rhs);

				val
			}

			Expr::ArrayLit(lit) => {
				let val = self.new_val_typed_tmp(lit.arr_typ);

				define_val!(self, into, val, " =  poni_gc_alloc_tagged(ctx, sizeof(struct ps_array_header) + sizeof({}) * {}, PONI_TAG_ARRAY);\n",
					self.db.get_ctype(lit.elem_typ),
					lit.values.len());

				if val.needs_storage() {
					inf_writeln!(into, "{indent}{}->header.length = {};\n", val.val, lit.values.len());
					// Arrays keep track of their type at runtime, for the GC.
					inf_writeln!(into, "{indent}{}->header.type = {};\n", val.val, self.db.get_type_ctag(lit.elem_typ));

					let mut idx = 0;
					for value in &lit.values {
						let nth = self.expr(ast, *value, into);
						assert!(nth.typ == lit.elem_typ);
						inf_writeln!(into, "{indent}{}->contents[{idx}] = {nth};", val.val);

						idx += 1;
					}
				}

				self.tmp_to_used_val(val)
			}

			Expr::SelfVal(selfval) => {
				// Self is kind of special for the GC. We don't actually need
				// to keep a reference to it ourselves, because we're guaranteed
				// that it is either pointed to by a root, or by some other function
				// frame up the call stack.
				Val::DirectSelf.typed(selfval.typ, None)
			}

			Expr::Index(index) => {
				
				let arr_val = self.expr(ast, index.value, into);

				let idx_val = self.expr(ast, index.index, into);
				assert!(idx_val.typ == self.db.types.int);

				// Generate own val after inner expressions, for GC
				let val = self.new_val_typed(index.typ);

				// TODO: Generate bounds checks
				define_val!(self, into, val, " = {arr_val}->contents[{idx_val}];\n");

				val
			}

			Expr::SetIndex(set) => {
				let arr_val = self.expr(ast, set.value, into);

				let idx_val = self.expr(ast, set.index, into);
				assert!(idx_val.typ == self.db.types.int);

				let rhs_val = self.expr(ast, set.rhs, into);

				// Generate own val after inner expressions, for GC
				let val = self.new_val_typed(set.typ);

				// TODO: Generate bounds checks
				define_val!(self, into, val, " = {arr_val}->contents[{idx_val}] = {rhs_val};\n");

				val
			}

			Expr::MakeTuple(tuple) => {
				let val = self.new_val_typed_tmp(tuple.typ);
				let Type::Tuple(subtypes) = self.db.get(tuple.typ) else { unreachable!() };

				define_val!(self, into, val, ";\n");
				if val.needs_storage() {
					for (idx, expr) in tuple.values.iter().enumerate() {
						// TODO: Do we need to do all the exprs() first then
						// collect them after like for fun calls? I don't think so.
						let inner = self.expr(ast, *expr, into);

						assert!(inner.typ == subtypes[idx]);
						inf_writeln!(into, "{indent}{}.v_{idx} = {inner};", val.val);
					}
				}

				self.tmp_to_used_val(val)
			}

			Expr::Promote(promote) => {
				let val = self.new_val_typed_tmp(promote.promote_to);

				let inner = self.expr(ast, promote.inner, into);
				define_val!(self, into, val, ";\n");
				if val.needs_storage() {
					self.compile_partial_promote(&val.val, &inner.val,
						val.typ, inner.typ,
						&"".to_string(), &"".to_string(),
						into);
				}

				self.tmp_to_used_val(val)
			}

			Expr::MakeSumType(sum) => {
				let val = self.new_val_typed_tmp(sum.typ);

				// Right now, this is nothing but nil.
				define_val!(self, into, val, " = NULL;\n");

				self.tmp_to_used_val(val)
			}
		}
	}

	fn compile_partial_promote(&mut self, to: &Val, from: &Val, to_typ_id: TypId, from_typ_id: TypId, to_post: &String, from_post: &String, into: &mut String, ) {
		let to_typ = self.db.get(to_typ_id);
		let from_typ = self.db.get(from_typ_id);

		let indent = self.indent();

		// Helper function for doing the promotions
		let mut do_promote = |the_fn: &'static str| {
			inf_writeln!(into, "{indent}{to}{to_post} = {the_fn}{from}{from_post});");
		};

		// If we end up promoting a type to itself, that is just a no-op.
		// May happen with certain tuple values.
		if to_typ == from_typ {
			inf_writeln!(into, "{indent}{to}{to_post} = {from}{from_post};");
			return;
		}

		// This match statement should line up with the one in Expr::compute_assignable.
		match (to_typ, from_typ) {
			(_, Type::Bottom) => {
				/* Don't do any promotion, but don't panic? */
			}
			(Type::Bottom, _) => {
				/* Don't do any promotion, but don't panic? */
			}

			(Type::Float, Type::Int) => do_promote("ps_promote_int_to_float("),
			(Type::StrBuf, Type::StrConst) => do_promote("ps_promote_str_to_buf(ctx, "),
			(Type::StrBuf, Type::Str) => do_promote("ps_promote_str_to_buf(ctx, "),
			(Type::Str, Type::StrConst) => do_promote("ps_promote_str_const_to_str(ctx, "),

			// Tuples are where things get interesting. We have to recursively promote
			// every part of each tuple.
			(Type::Tuple(a), Type::Tuple(b)) => {
				if a.len() != b.len() {
					panic!("ICE: promotion between differently-sized tuples");
				}
				
				// Iterate over each tuple member and promote.
				for i in 0..a.len() {
					let to_post = format!("{to_post}.v_{i}");
					let from_post = format!("{from_post}.v_{i}");
					let to_typ = a[i];
					let from_typ = b[i];
					self.compile_partial_promote(to, from,
						to_typ, from_typ,
						&to_post, &from_post,
						into);
				}
			}

			_ => {
				panic!("ICE: Bad promotion in codegen. Unknown promotion {} -> {}",
					self.db.repr_type(from_typ_id), self.db.repr_type(to_typ_id));
			}
		}
	}

	fn compile_class(&mut self, ast: &Ast, class_declare: &ClassDeclare) {
		// For the class, it does not generate any direct code.
		// But, we do have to generate a struct for the class,
		// as well as each of its function definitions.

		self.inside_class.push(class_declare.identity);

		for fun in &class_declare.funs {
			self.compile_function(ast, fun.identity, fun.value);
		}

		// Write the struct definition.
		let mut struc = String::new();
		inf_writeln!(struc, "struct {} {{", self.db.get_class_cname(class_declare.identity));

		// Write the struct declaration. These must come before signature declarations
		// in case the signature needs to use the struct; The signature declarations
		// must then come before structs in case the struct needs to use the signature.
		let mut struc_declare = String::new();
		inf_writeln!(struc_declare, "struct {};", self.db.get_class_cname(class_declare.identity));
		self.struct_declares.push(struc_declare);

		// Simultaneously write the variable generator. 
		let enclosing_indent = self.indent_level;
		self.indent_level = 1;

		for var in &self.db.get(class_declare.identity).vars {
			// Compile the variable declaration into the struct.
			inf_writeln!(struc, "\t{} {};", self.db.get_var_ctype(*var), self.db.get_cname(*var));
		}

		inf_writeln!(struc, "}};");
		self.structs.push(struc);

		self.indent_level = enclosing_indent;

		self.inside_class.pop();
	}

	fn compile_stmt(&mut self, ast: &Ast, stmt: StmtId, into: &mut String) -> Option<TypedVal> {
		let indent = self.indent();
		match ast.stmts.get(stmt).as_ref() {
			Stmt::Declare(declare) => {
				self.compile_assign(ast, declare.identity, declare.value, into, true);

				// Each variable obtains a single GC slot for itself, if relevant.
				// These are stored in the "block scopes" vector.
				//
				// It is CRITICAL that we DON'T allocate the GC slot for the
				// variable until we have compiled its initial assignment.
				// Once the initial assignment has been compiled, the variable
				// actually exists, and so it can be saved.

				let slots = self.db.type_gc_slots(self.db.get(declare.identity).typ);
				if slots != 0 {
					// We re-use the slot allocation functionality from Vals to
					// get the slots. This is so we can handle Fun types and Tuple
					// types correctly.

					let mut tmp = vec![];

					let prefix = self.db.get_cname(declare.identity).to_string();
					self.val_alloc_slots_recurse(&prefix, self.db.get_var_type(declare.identity), &mut tmp);

					// We will write into the last block_scopes vec. This should
					// exist; if not, it's a bug.
					let last = self.block_scopes.last_mut().unwrap();
					last.append(&mut tmp);

					// It would be NICE to write directly into last, but unfortunately,
					// we cannot, thanks to the borrow checker.
				}

				None
			},
			Stmt::ClassDeclare(class_declare) => {
				self.compile_class(ast, class_declare);
				None
			}
			Stmt::Expression(expression) => {
				// The value of the expression is unused inside a statement.
				// Note that this automatically results in some kinds of
				// dead-code elimination, such as 30; turning into nothing.
				//
				// And, because the expression itself generates any code,
				// this function simply has to delegate to it.
				Some(self.expr(ast, expression.expression, into))
			},
			Stmt::Return(ret) => {
				inf_writeln!(into, "{indent}ctx->frame = gc_frame.prev;");

				match &ret.expression {
					Some(value) => {
						let needed_type = *self.return_types.last().unwrap();
						let val = self.expr(ast, *value, into);

						// If the inner value is also a bottom type,
						// then we can't really generate a return here.
						if !val.is_bottom() {
							// If it's not bottom, check the typechecker's work.
							assert!(val.typ == needed_type);

							inf_writeln!(into, "{indent}return {val};");
						}
					},
					None => {
						inf_writeln!(into, "{indent}return;");
					}
				}

				Some(Val::Bottom.typed(self.db.types.bottom, None))
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

		// Again, because this is a direct var, we don't need a GC slot for it.
		//
		// (We will need write barriers in the future..?)
		Val::DirectVar { this_val: self.this_val, name: self.db.get_cname(var), depth }
			.typed(self.db.get_var_type(var), None)
	}

	fn compile_assign(&mut self, ast: &Ast, var: VarId, expr: ExprId, into: &mut String, is_declaration: bool) -> TypedVal {
		let needed_type = self.db.get_var_type(var);
		let value = self.expr(ast, expr, into);

		// Don't compile anything at all for variables that are bottom.
		if value.is_bottom() || value.typ == self.db.types.bottom { // TODO: Fix the value thingyingy
			return value;
		}

		assert!(value.typ == needed_type);

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
	fn compile_function(&mut self, ast: &Ast, fun: FunId, body: ExprId) {
		let is_init = Some(fun) == self.db.fun_init;

		let enclosing_val = self.val_idx;
		// Reset vals for each function.
		self.val_idx = 0;
		let enclosing_indent = self.indent_level;
		self.indent_level = 1;
		let indent = self.indent();

		let enclosing_gc_frame = self.gc_frame.clone();
		self.gc_frame = Rc::new(GCFrame::new());

		// This resets block_scopes to empty, which is what we want.
		let enclosing_block_scopes = std::mem::take(&mut self.block_scopes);
		// Push a block scope for the parameters (?)
		// self.block_scopes.push(Vec::new());

		let enclosing_disable_gc_frames = self.disable_gc_frames;
		self.disable_gc_frames = false; // Don't disable gc frames, in general.

		// Separate out the beginning of the buffer from the rest so that
		// we can generate the GC frame once we know how deep it needs to be.
		let mut own_buffer_beginning = String::new();
		let mut own_buffer = String::new();

		// init() fun has no surrounding definition -- it is poni_init()
		if !is_init {
			inf_writeln!(own_buffer_beginning, "{} {}({}) {{",
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

		let val = self.expr(ast, body, &mut own_buffer);

		// Generate unconditional GC-frame pop
		inf_writeln!(own_buffer, "{indent}ctx->frame = gc_frame.prev;");

		if val.needs_storage() {
			assert!(val.typ == own_return_type);
			// If it does have a value, then we write it as a default
			// return value.
			inf_writeln!(own_buffer, "{indent}return {val};");
		}

		// Now that we have generated the inner expression, we know how big
		// of a GC frame we need. TODO: Actually generate the GC frame.
		let gc_frame_count = self.gc_frame.next_alloc_slot.get();
		inf_writeln!(own_buffer_beginning, "{indent}// gc frame count: {}", gc_frame_count);

		inf_writeln!(own_buffer_beginning, "{indent}struct {{");
		inf_writeln!(own_buffer_beginning, "{indent}\tstruct poni_gc_frame *prev;");
		inf_writeln!(own_buffer_beginning, "{indent}\tuint64_t ptr_count;");
		inf_writeln!(own_buffer_beginning, "{indent}\tvoid *ptrs[{}];", gc_frame_count);
		inf_writeln!(own_buffer_beginning, "{indent}}} gc_frame = {{0}};");
		inf_writeln!(own_buffer_beginning, "{indent}gc_frame.ptr_count = {};", gc_frame_count);
		inf_writeln!(own_buffer_beginning, "{indent}gc_frame.prev = ctx->frame;");
		inf_writeln!(own_buffer_beginning, "{indent}ctx->frame = (void*)&gc_frame;");
		
		// Pop type value
		self.return_types.pop();

		self.disable_gc_frames = enclosing_disable_gc_frames;
		self.block_scopes = enclosing_block_scopes;
		self.gc_frame = enclosing_gc_frame;
		self.indent_level = enclosing_indent;
		self.val_idx = enclosing_val;

		// init() fun has no surrounding scope
		if !is_init { inf_writeln!(own_buffer, "}}"); }

		if Some(fun) == self.db.fun_init {
			self.fun_init_buffer = format!("{}{}", own_buffer_beginning, own_buffer);
		}
		else {
			// Just let the big codegen function "concatenate" these togther,
			// no need for the copy. This might change if we get multithreading.
			self.functions.push(own_buffer_beginning);
			self.functions.push(own_buffer);
		}

		// Don't write declaration for the init() function.
		if !is_init {
			let mut declare = String::new();
			// TODO: Possibly write directly to Out::FunDeclare
			inf_writeln!(declare, "{} {}({});",
				self.db.get_fun_ret_ctype(fun),
				self.db.get_fun_cname(fun),
				self.db.get_fun_cparams(fun));
			self.fun_declares.push(declare);
		}
	}

	fn compile_string_constant_init(&mut self, define: &mut String, init: &mut String) {
		inf_writeln!(init, "void poni_init_strings(struct poni_gc_context *ctx) {{");
		for id in self.db.iter_strconst() {
			inf_writeln!(define, "const ps_str* ps_str_const{} = NULL;", id.to_index());
			inf_writeln!(init, "\tps_str_const{} = ps_str_from_literal(ctx, {});",
				id.to_index(), self.db.get(id));
		}
		inf_writeln!(init, "}}");
	}

	fn codegen_to_buffers(&mut self, ast: &Ast, module: &Module, out: &mut CodegenOutputs) {
		

		self.indent_level = 0;
		for fun in &module.functions {
			self.compile_function(ast, fun.identity, fun.value);
		}

		for class in &module.classes {
			self.compile_class(ast, class);
		}

		
	}

	fn codegen_gc_stride(&mut self) -> String {
		let mut type_stride = "static inline size_t
poni_get_type_stride(uint64_t tag) {
	switch(tag) {
		case PONI_TAG_STRCONST:
		case PONI_TAG_STR:
		case PONI_TAG_STRBUF:
		case PONI_TAG_ARRAY:
			return sizeof(void*);
		case PONI_TAG_FLOAT: return sizeof(ps_float);
		case PONI_TAG_INT:   return sizeof(ps_int);
		case PONI_TAG_BOOL:  return sizeof(ps_bool);
".to_string();

		let mut ptr_types = String::new();
		let mut fun_types = String::new();
		let mut funraw_types = String::new();

		for typ in self.db.iter_typ() {
			// Skip types that aren't cgen safe
			if !self.db.is_cgen_safe(typ) { continue; }

			let tag = self.db.get_type_ctag(typ);
			match self.db.get(typ) {
				// Primitive types already done
				Type::Int | Type::Float | Type::Bool => { continue; }

				// Illegal
				Type::Void | Type::Bottom => { continue; }

				// Already done
				Type::StrConst | Type::StrBuf | Type::Str | Type::ArrayOf(_) => { continue; }

				// Build up one big set of pointer types.
				Type::Class(_) => {
					inf_writeln!(ptr_types, "\t\tcase {tag}:");
				}

				Type::Option(id) => {
					if self.db.is_value_type(*id) {
						todo!()
					}
					else {
						// If it's a reference type, we're re-using the type id,
						// so we actually don't need a case at all.
					}
				}

				Type::Fun(_) => { inf_writeln!(fun_types, "\t\tcase {tag}:"); }
				Type::FunRaw(_) => { inf_writeln!(funraw_types, "\t\tcase {tag}:"); }

				// Value types should each return their sizeof.
				Type::Tuple(_) => {
					inf_writeln!(type_stride, "\t\tcase {tag}: return sizeof({});", self.db.get_ctype(typ));
				}

				Type::AssumeFloat | Type::AssumeInt | Type::Unassigned
				| Type::UnboundCStructPtr(_) | Type::UnboundIdent(_) => { continue; }
			}
		}

		if ptr_types.len() > 0 {
			inf_writeln!(ptr_types, "\t\t\treturn sizeof(void*);");
		}
		if fun_types.len() > 0 {
			// This should be valid on each compiler.
			inf_writeln!(fun_types, "\t\t\treturn sizeof(struct {{ void (*fn)(void); void *closure; }});");
		}
		if funraw_types.len() > 0 {
			inf_writeln!(funraw_types, "\t\t\treturn sizeof(void (*)(void))")
		}

		inf_write!(type_stride, "{ptr_types}{fun_types}{funraw_types}");

		inf_writeln!(type_stride, "\t}}\n}}");
		type_stride
	}


	fn codegen_gc_functions(&mut self) -> String {
		let type_stride = self.codegen_gc_stride();

		let mut is_valuetype = "static inline ps_bool
poni_is_value_type(uint64_t tag) { return !!(tag & 0x8000000000000000ULL); }
".to_string();

		let mut valuetype = "void
poni_gc_visit_valuetype(struct poni_gc *gc, void *object, uint64_t tag) {
	switch(tag) {
".to_string();
		let mut visit_object = "void
poni_gc_visit_object(struct poni_gc *gc, void *object) {
    uint64_t tag = *(uint64_t*)object;
    switch(tag & 0xFFFFFFFFFFFFFFFEULL) {
		case PONI_TAG_STRCONST:
		case PONI_TAG_STR:
			break; // Nothing to do
		case PONI_TAG_STRBUF: {
			struct ps_strbuf *self = object;
			poni_gc_mark(gc, self->buffer);
			break;
		}
		case PONI_TAG_ARRAY: {
			struct ps_array_header *header = object;
			char *elem_root = (char*)object + sizeof(struct ps_array_header);
			if(poni_is_value_type(header->type)) {
				// As an optimization, never visit any objects inside an
				// array of primitive types. We should probably have an additional
				// type info function that tells us whether we need to iterate
				// here.
				if(header->type == PONI_TAG_INT || header->type == PONI_TAG_FLOAT
					|| header->type == PONI_TAG_BOOL)
				{ break; }

				size_t stride = poni_get_type_stride(header->type);

				// For value types, the inner objects do not themselves need
				// to be marked; so instead of going through the gc marker,
				// instead just visit them directly.
				for(ps_int i = 0; i < header->length; ++i) {
					poni_gc_visit_valuetype(gc, elem_root, header->type);
					elem_root += stride;
				}
			}
			else {
				size_t stride = poni_get_type_stride(header->type);

				for(ps_int i = 0; i < header->length; ++i) {
					poni_gc_mark(gc, elem_root);
					elem_root += stride;
				}
			}
			break;
		}
".to_string();

	let mut visit_roots = "void
poni_gc_visit_roots(struct poni_gc *gc) {
".to_string();

	let mut allocation_size = "size_t
poni_gc_get_allocation_size(void *object) {
	uint64_t tag = *(uint64_t*)object;
	switch(tag) {
		case PONI_TAG_STRCONST:
		case PONI_TAG_STR:
		{
			struct ps_str *self = object;
			return sizeof(*self) + self->length;
		}
		case PONI_TAG_STRBUF: {
			return sizeof(struct ps_strbuf);
		}
		case PONI_TAG_ARRAY: {
			struct ps_array_header *header = object;
			size_t stride = poni_get_type_stride(header->type);
			return sizeof(*header) + stride * header->length;
		}
".to_string();

		for typ in self.db.iter_typ() {
			if !self.db.is_cgen_safe(typ) { continue; }

			let tag = self.db.get_type_ctag(typ);
			match self.db.get(typ) {
				Type::Class(id) => {
					inf_writeln!(visit_object, "\tcase {tag}: {{");
					inf_writeln!(visit_object, "\t\tstruct {} *self = object;", self.db.get_class_cname(*id));
					for field in &self.db.get(*id).vars {
						let field_ty = self.db.get(*field).typ;

						match self.db.get(field_ty) {
							Type::Int | Type::Float | Type::Bool => {}
							Type::Void | Type::Bottom => {}

							Type::StrConst => {
								// For now, we don't mark StrConst, because
								// they can't be deallocated.
							}

							Type::Str | Type::StrBuf | Type::Class(_) | Type::ArrayOf(_) => {
								inf_writeln!(visit_object, "\t\tponi_gc_mark(gc, self->{});", self.db.get_cname(*field));
							}

							Type::Option(id) => {
								match self.db.get(*id) {
									Type::Str | Type::StrBuf | Type::Class(_) | Type::ArrayOf(_) => {
										inf_writeln!(visit_object, "\t\tponi_gc_mark(gc, self->{});", self.db.get_cname(*field));
									},
									_ => todo!()
								}
							}

							// Nothing to visit.
							Type::FunRaw(_) => {}

							Type::Fun(_) | Type::Tuple(_) => {
								let inner_tag = self.db.get_type_ctag(field_ty);
								inf_writeln!(visit_object, "\t\tponi_gc_visit_valuetype(gc, &self->{}, {inner_tag});",
									self.db.get_cname(*field));
							}

							Type::Unassigned | Type::AssumeFloat | Type::AssumeInt | Type::UnboundIdent(_) | Type::UnboundCStructPtr(_) => {}
						}
					}

					inf_writeln!(visit_object, "\t\tbreak;");
					inf_writeln!(visit_object, "\t}}");
				},

				Type::Tuple(typs) => {
					inf_writeln!(valuetype, "\tcase {tag}: {{");
					inf_writeln!(valuetype, "\t\t{} *self = object;", self.db.get_ctype(typ));

					for (idx, typ) in typs.iter().enumerate() {
						match self.db.get(*typ) {
							Type::Int | Type::Float | Type::Bool => {}
							Type::Void | Type::Bottom => {}

							Type::StrConst => {
								// For now, we don't mark StrConst, because
								// they can't be deallocated.
							}

							Type::Str | Type::StrBuf | Type::Class(_) | Type::ArrayOf(_) => {
								inf_writeln!(valuetype, "\t\tponi_gc_mark(gc, self->v_{});", idx);
							}

							Type::Option(id) => {
								match self.db.get(*id) {
									Type::Str | Type::StrBuf | Type::Class(_) | Type::ArrayOf(_) => {
										inf_writeln!(visit_object, "\t\tponi_gc_mark(gc, self->v_{});", idx);
									},
									_ => todo!()
								}
							}

							// Nothing to visit.
							Type::FunRaw(_) => {}

							Type::Fun(_) | Type::Tuple(_) => {
								let inner_tag = self.db.get_type_ctag(*typ);
								inf_writeln!(valuetype, "\t\tponi_gc_visit_valuetype(gc, &self->v_{}, {inner_tag});",
									idx);
							}

							Type::Unassigned | Type::AssumeFloat | Type::AssumeInt | Type::UnboundIdent(_) | Type::UnboundCStructPtr(_) => {}
						}
					}

					inf_writeln!(valuetype, "\t\tbreak;");
					inf_writeln!(valuetype, "\t}}");
				},

				Type::Fun(sig) => {
					// Don't generate these for unused sigs -- they might not
					// be valid C, so we can't generate them; and they won't
					// be needed anyway.
					if !self.db.is_sig_used(*sig) { continue; }

					inf_writeln!(valuetype, "\tcase {tag}: {{");
					inf_writeln!(valuetype, "\t\t{} *self = object;", self.db.get_ctype(typ));
					// Visit the closure for each fun.
					// We could make this particular bit of code some sort of
					// helper function / case-that-falls-through for each function,
					// but this is fine for now.
					inf_writeln!(valuetype, "\t\tponi_gc_mark(gc, self->closure);");
					inf_writeln!(valuetype, "\t\tbreak;");
					inf_writeln!(valuetype, "\t}}");
				}

				_ => {}
			}
		}

		inf_writeln!(valuetype, "\t}}\n}}");
		inf_writeln!(visit_object, "\t}}\n}}");
		inf_writeln!(allocation_size, "\t}}\n}}");

		inf_writeln!(visit_roots, "}}");

		// Just concatenate everything together.
		//
		// It might be cleaner if we just append each of these as separate buffers...
		format!("{type_stride}{is_valuetype}{valuetype}{visit_object}{visit_roots}{allocation_size}")
	}

	fn codegen(&mut self, args: &Args, ast: &Ast, output: &mut dyn std::io::Write) -> std::io::Result<()> {
		let mut outputs = CodegenOutputs::new();

		// Generate global variables in one pass as their ordering is a global
		// property.

		// Disable GC frame for now; we don't bother with one in the globals
		// initializer (it shouldn't be able to GC).

		self.disable_gc_frames = true;

		// Use an indent level of 1 for the initialization code for all global variables.
		self.indent_level = 1;
		for global in &self.db.globals {
			let global = *global;

			let Some(initializer) = self.db.get(global).initializer else {
				// If there is no initializer, this must be an extern variable,
				// so we don't codegen its initializer.
				continue;
				//panic!("ICE: Codegen of global variable without initializer");
			};

			// Just dierectly encode the indentation..
			let mut global_name = self.db.get_cname(global);
			if args.hot {
				// To contend with the Hot option, we have to chop off the ( )
				// surrounding the variable name when declaring it.
				//
				// Note that we leave the * on, because it does stuff for us.
				global_name = &global_name[1..global_name.len() - 1];
			}
			inf_writeln!(outputs.global_define, "{} {};",
				self.db.get_var_ctype(global), global_name);

			

			// In hot-code reloading, we need to do two things:
			// 1. Allocate the variable based on a pointer.
			// 2. Initialize it, if it *didn't* already exist.
			if args.hot {
				let indent = self.indent();
				inf_writeln!(outputs.global_init, "{indent}{} = poni_hot_lookup(\"{} {};\", sizeof({}), &existed);",
					// HACK: Chop off the * (the () have already been chopped)
					&global_name[1..],
					// Replicate the initializer
					self.db.get_var_ctype(global), self.db.get_cname(global),
					// For sizeof() we do want the * cause that tells us the
					// real size
					self.db.get_cname(global));
				inf_writeln!(outputs.global_init, "{indent}if(!existed) {{");

				self.indent_level += 1;
			}

			// For globals, the initializer is not itself a declaration. So,
			// do tell self.compile_assign() that it's not a declaration.
			self.compile_assign(ast, global,
				initializer,
				&mut outputs.global_init,
				false);

			// Finish hot compilation
			if args.hot {
				self.indent_level -= 1;

				let indent = self.indent();
				inf_writeln!(outputs.global_init, "{indent}}}");
			}
		}

		self.disable_gc_frames = false;

		for source in ast.sources.iter() {
            let mut source = ast.sources.get_mut(source);
            let module = &mut source.module;
			self.codegen_to_buffers(ast, module, &mut outputs);
		}
		self.compile_string_constant_init(&mut outputs.string_const_define, &mut outputs.string_const_init);

		writeln!(output, "#include \"poni/poni.h\"")?;
		// Engine code does not include poni_standalone.h.
		if !args.engine {
			writeln!(output, "#include \"poni/poni_standalone.h\"")?;
		}
		if args.hot {
			writeln!(output, "#include \"poni/poni_hot.h\"")?;
		}

		writeln!(output, "// --- imports ---\n")?;
		for path in &args.imports {
			writeln!(output, "#include \"{}\"", path.display())?;
		}

		writeln!(output, "// --- tag definitions ---\n{}", self.db.tag_define_code)?;

		writeln!(output, "// --- string constants ---\n{}", outputs.string_const_define)?;
		writeln!(output, "// --- struct declarations ---\n")?;
		for struc_declare in &self.struct_declares {
			writeln!(output, "{}", struc_declare)?;
		}
		writeln!(output, "// --- struct declarations (ps_tuple) ---\n{}", self.db.valty_declare_code)?;
		writeln!(output, "// --- struct declarations (ps_array) ---\n{}", self.db.arr_declare_code)?;
		writeln!(output, "// --- sig types ---\n{}", self.db.sig_declare_code)?;
		
		writeln!(output, "// --- struct definitions (ps_tuple) ---\n{}", self.db.valty_define_code)?;
		writeln!(output, "// --- struct definitions (ps_array) ---\n{}", self.db.arr_define_code)?;

		// I believe these have to come after the ps_tuple, because they might
		// refer to tuples.
		writeln!(output, "// --- struct definitions ---")?;
		for struc in &self.structs {
			writeln!(output, "{}", struc)?;
		}

		writeln!(output, "// --- global variables ---\n{}", outputs.global_define)?;
		writeln!(output, "// --- function declarations ---\n{}", outputs.fun_declare)?;
		for dec in &self.fun_declares {
			writeln!(output, "{}", dec)?;
		}
		writeln!(output, "// --- function definitions ---")?;
		for fun in &self.functions {
			writeln!(output, "{}", fun)?;
		}
		writeln!(output, "{}", outputs.string_const_init)?;

		writeln!(output, "// --- gc support ---")?;
		writeln!(output, "{}", self.codegen_gc_functions())?;

		// The various poni initializer functions are split into several pieces,
		// so as to enable hot code reloading.
		writeln!(output, "void poni_init_globals(struct poni_gc_context *ctx) {{")?;
		if args.hot {
			// For hot-code reloading, we need the bool flag 'existed' to decide
			// whether to run each initializer
			writeln!(output, "\tbool existed = false;")?;
		}
		writeln!(output, "{}", outputs.global_init)?;
		writeln!(output, "}}")?;

		writeln!(output, "void poni_init(struct poni_gc_context *ctx) {{")?;
		writeln!(output, "{}", self.fun_init_buffer)?;
		writeln!(output, "}}")?;

		Ok(())
	}
}

pub fn codegen(args: &Args, db: &mut Db, ast: &Ast, output: &mut dyn std::io::Write) -> std::io::Result<()> {
	let mut codegen = Codegen::new(db);

	codegen.codegen(args, ast, output)
}