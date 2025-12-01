use rustc_hash::FxHashSet;
use ufmt::uwrite;

use crate::arena::ArenaKey;
use crate::codegen::*;
use crate::db::*;
use crate::lexer::Tok;
use crate::typ::Type;

use crate::{inf_write, inf_writeln};

use crate::expr::*;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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
pub enum Val {
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
	/// A directly generated NULL literal.
	DirectNull,
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
	pub fn typed(self, typ: TypId, gc_slots_and_frame: Option<(Vec<usize>, Arc<GCFrame>)>) -> TypedVal {
		return TypedVal {
			val: self,
			typ,
			gc_slots_and_frame
		}
	}
}

pub struct TypedVal {
	val: Val,
	typ: TypId,
	
	// TODO: To make this more efficient, what we should really do is have the frame
	// be &GCFrame, and then pass a &gcframe down the whole tree of codegen
	// functions. That avoids needing an Arc.
	
	gc_slots_and_frame: Option<(Vec<usize>, Arc<GCFrame>)>
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
		ufmt::uwrite!($into, $($arg)*).unwrap()
	}
}

#[macro_export]
macro_rules! inf_writeln {
	($into:expr, $($arg:tt)*) => {
		ufmt::uwriteln!($into, $($arg)*).unwrap()
	}
}

impl ufmt::uDisplay for Val {
	fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
	where
		W: ufmt::uWrite + ?Sized {
		match self {
			Val::Tmp(idx) => uwrite!(f, "t{}", idx),
			Val::DirectLit {ctype, lit } => uwrite!(f, "(({}){})", ctype, lit),
			Val::DirectVar { this_val, name, depth } => {
				if *depth > 0 {
					if let Some(idx) = this_val {
						let val = Val::Tmp(*idx);
						uwrite!(f, "{}->", val)?;
					}
					else {
						// TODO: The problem with this system is it doesn't seem
						// like it can meaningfully support static variables in a
						// clean way. We probably do want to change into synthesizing
						// AST nodes of some sort.
						uwrite!(f, "this->")?;
					}
					let depth_loop = depth - 1;
					while depth_loop > 0 {
						todo!("nested class support");
						depth_loop -= 1;
					}
				}
				uwrite!(f, "{}", name)
			}
			Val::DirectSelf => {
				uwrite!(f, "this")
			}
			Val::DirectNull => {
				uwrite!(f, "NULL")
			}

			// String literals are always stored in variables with a consistent naming scheme.
			Val::StringLit { id } => uwrite!(f, "ps_str_const{}", id.to_index()),
			Val::BoolLit { val } => match val {
				true => uwrite!(f, "((ps_bool)1)"),
				false => uwrite!(f, "((ps_bool)0)"),
			}
			Val::Bottom => panic!("ICE: Tried to codegen Val::Bottom"),

			// Void values have no representation.
			Val::Void => Ok(())
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
			Val::DirectNull => {
				write!(f, "NULL")
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

impl ufmt::uDisplay for TypedVal {
	fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
	where
		W: ufmt::uWrite + ?Sized {
		uwrite!(f, "{}", self.val)
	}
}

// Now that we no longer have promote() in the compiler, we can directly
// write TypedVals into the output stream.
impl std::fmt::Display for TypedVal {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.val)
	}
}

pub struct Indenter {
	level: usize,
}

impl ufmt::uDisplay for Indenter {
	fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
	where
		W: ufmt::uWrite + ?Sized {
		// In order to keep this "somewhat" fast, instead of looping, use a
		// maximum allocation size.
		//
		// I think, however, that the main overhead from Indenter likely comes
		// from the fact that it exists at all...
		let len = match self.level {
			l @ 0..16 => 16 - l,
			_ => 0,
		};
		f.write_str(&"\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t"[len..16])
	}
}

impl std::fmt::Display for Indenter {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		// In order to keep this "somewhat" fast, instead of looping, use a
		// maximum allocation size.
		//
		// I think, however, that the main overhead from Indenter likely comes
		// from the fact that it exists at all...
		let len = match self.level {
			l @ 0..16 => 16 - l,
			_ => 0,
		};
		f.write_str(&"\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t"[len..16])
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

/// Helper struct for keeping track of where to store pointers across GC safepoints
/// and function call boundaries.
pub struct GCFrame {
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
	pub fn new() -> Self {
		GCFrame {
			next_alloc_slot: Cell::new(0),
			avail: RefCell::new(BTreeSet::new()),
			saved: RefCell::new(BTreeMap::new()),
			written: RefCell::new(BTreeSet::new()),
		}
	}

	pub fn allocate_slot_internal(&self) -> usize {
		let mut avail = self.avail.borrow_mut();
		if let Some(last) = avail.pop_last() {
			return last;
		}

		let slot = self.next_alloc_slot.get();
		self.next_alloc_slot.set(slot + 1);
		slot
	}

	pub fn allocate_slot(&self, val: String) -> usize {
		let slot = self.allocate_slot_internal();
		let mut saved = self.saved.borrow_mut();
		debug_assert!(saved.insert(slot, val).is_none());

		slot
	}

	pub fn free_slots(&self, slots: &Vec<usize>) {
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
	pub fn mark(&self, slot: usize) -> bool {
		let mut written = self.written.borrow_mut();
		written.insert(slot)
	}
}

pub struct Codegen<'a> {
	return_types: Vec<TypId>,

	/// Helps us resolve variables to the correct thing.
	/// TODO: Do we want to instead synthesize AST nodes for variables that
	/// are inside classes..?
	inside_class: Vec<ClassId>,

	val_idx: usize,

	pub indent_level: usize,

	/// For now, when we are generated values that are referencing 'this', and
	/// we need the this_val to be something other than what it is, we can
	/// store a Tmp(usize) here.
	this_val: Option<usize>,

	gc_frame: Arc<GCFrame>,

	/// A list of Vec<usize>, where the inner Vecs contain references to the
	/// current GCFrame.
	block_scopes: Vec<Vec<usize>>,

	/// A flag for disabling the generation of GC frames. In this case, whenever
	/// we would write to the GC frame, we simply don't. (Mainly relevant for
	/// global initialization)
	pub disable_gc_frames: bool,

	/// The 'return' value of the current Loop, if any.
	loop_val: Option<TypedVal>,

	db: &'a Db,

    send: channel::Sender<String>,

	/// In order to keep parallelism decent, we build up a single buffer of
	/// stuff and send it when it is a reasonable size.
	current_buffer: String,
}

impl<'a> Codegen<'a> {
	pub fn new(db: &'a Db, send: channel::Sender<String>) -> Self {
		return Codegen {
			return_types: Vec::new(),

			val_idx: 0,

			indent_level: 0,

			db,

			this_val: None,

			inside_class: Vec::new(),

			gc_frame: Arc::new(GCFrame::new()),
			block_scopes: Vec::new(),

			disable_gc_frames: false,

			loop_val: None,
            send,

			current_buffer: String::new(),
		}
	}

	pub fn indent(&self) -> Indenter { return Indenter { level: self.indent_level } }

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
				inf_writeln!(into, "{}gc_frame.ptrs[{}] = {};", indent, k, v);
			}
		}
	}

	fn compile_partial_binary(&mut self, result_val: &TypedVal, lhs_val: &TypedVal, rhs_val: &TypedVal, op: char, cur_typ: TypId, postfix: &String, into: &mut String) {
		let indent = self.indent();
		match self.db.get(cur_typ) {
			Type::Int | Type::Float => {
				inf_writeln!(into, "{}{}{} = {}{} {} {}{};",
					indent, result_val, postfix, lhs_val, postfix, op, rhs_val, postfix);
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

	fn compile_binary(&mut self, ast: &AstReadonly, binary: &Binary, into: &mut String) -> TypedVal {
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
			inf_writeln!(into, "{}const {} {} = {} {} {};",
				indent, ctype, val, left, op, right);
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

	/// Converts a type into a tuple of length, inner type, if the given type
	/// is a 'vec' type; otherwise returns None.
	fn get_vec_params(&self, typ: TypId) -> Option<(usize, TypId)> {
		match typ {
			typ if typ == self.db.types.vec2 => Some((2, self.db.types.float)),
			typ if typ == self.db.types.vec3 => Some((3, self.db.types.float)),
			typ if typ == self.db.types.vec4 => Some((4, self.db.types.float)),
			typ if typ == self.db.types.vec2i => Some((2, self.db.types.int)),
			typ if typ == self.db.types.vec3i => Some((3, self.db.types.int)),
			typ if typ == self.db.types.vec4i => Some((4, self.db.types.int)),
			_ => None
		}
	}

	/// Converts a type into a str such as 'vec3', 'vec3i', or into nothing
	/// if it is not a vector type.
	fn get_vec_cstr(&self, typ: TypId) -> Option<&'static str> {
		match typ {
			typ if typ == self.db.types.vec2 => Some("vec2"),
			typ if typ == self.db.types.vec3 => Some("vec3"),
			typ if typ == self.db.types.vec4 => Some("vec4"),
			typ if typ == self.db.types.vec2i => Some("vec2i"),
			typ if typ == self.db.types.vec3i => Some("vec3i"),
			typ if typ == self.db.types.vec4i => Some("vec4i"),
			_ => None
		}
	}

	fn compile_partial_lerp(&mut self, bool_val: &TypedVal, float_val: &TypedVal, one_minus_val: &TypedVal, from_val: &TypedVal, to_val: &TypedVal, target_val: &TypedVal, cur_typ: TypId, postfix: &String, into: &mut String) {
		let indent = self.indent();

		// Generate special cases for vector lerps, to make the generated code
		// nicer.
		if let Some(vec_cstr) = self.get_vec_cstr(cur_typ) {
			inf_writeln!(into, "{}{}{} = ps_lerp_{}({}{}, {}{}, {});",
				indent, target_val, postfix, vec_cstr, from_val, postfix, to_val, postfix, float_val);
			return;
		}

		// IMPORTANT:
		// To walk down the tree of tuple types, we must start at the root
		// result TypId, but then walk through different TypIds, so that
		// we make progress (no stack overflow).
		match self.db.get(cur_typ) {
			Type::Int => {
				// TODO: What is the best way to lerp ints based on a float?
				inf_writeln!(into, "{}{}{} = (ps_int)({}{} * {} + {}{} * {});",
					indent, target_val, postfix, from_val, postfix, one_minus_val, to_val, postfix, float_val)
			},
			Type::Float => {
				// TODO: What is the best way to lerp ints based on a float?
				inf_writeln!(into, "{}{}{} = {}{} * {} + {}{} * {};",
					indent, target_val, postfix, from_val, postfix, one_minus_val, to_val, postfix, float_val)
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
				inf_writeln!(into, "{}if({}) {{ {}{} = {}{}; }}",
					indent, bool_val, target_val, postfix, to_val, postfix);
				inf_writeln!(into, "{}\telse {{ {}{} = {}{}; }}",
					indent, target_val, postfix, from_val, postfix);
			}
		}
	}

	fn compile_lerp(&mut self, ast: &AstReadonly, lerp: &Lerp, into: &mut String) -> TypedVal {
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

		let result = self.new_val_typed(lerp.typ);

		// Decide the bool_val and float_val based on amount_val
		let (bool_val, float_val) = if amount_val.typ == self.db.types.float {
			// If the amount_val is a float, we can generate a couple special
			// cases to keep the generated code simpler.
			if let Some(vec_cstr) = self.get_vec_cstr(lerp.typ) {
				define_val!(self, into, result, " = ps_lerp_{}({}, {}, {});\n",
					vec_cstr, from_val, to_val, amount_val);
				return result;
			}

			let bool_val = self.new_val_typed(self.db.types.bool);
			// EXTREMELY IMPORTANT SEMANTIC DECISION:
			// What exactly counts as true?
			//
			// For now, we're saying >= 0.5. But it could be > 0.5. Or anything
			// else.
			define_val!(self, into, bool_val, " = {} >= 0.5;\n", amount_val);

			(bool_val, amount_val)
		}
		else {
			let float_val = self.new_val_typed(self.db.types.float);
			// Casting should give the desired behavior.
			define_val!(self, into, float_val, " = (ps_float){};\n", amount_val);

			(amount_val, float_val)
		};

		// For big lerps, it is more efficient to generate one val and re-use it.
		// TODO: Don't do that for smaller lerps? Maybe also consider using specialized
		// methods for vectors...?
		let one_minus_val = self.new_val_typed(self.db.types.float);
		define_val!(self, into, one_minus_val, " = 1.0 - {};\n", float_val);

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

	fn compile_comparison(&mut self, ast: &AstReadonly, compare: &Comparison, into: &mut String) -> TypedVal {
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

		inf_writeln!(into, "{}const ps_bool {} = (ps_bool)({} {} {});",
			indent, val, left, op, right);

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

		if let Some(vec_str) = self.get_vec_cstr(typid) {
			// Special case for vecs, to make them generate cleaner code.
			inf_writeln!(into, "{}ps_print_{}({});", indent, vec_str, val);
			return;
		}

		match typ {
			Type::Int => inf_writeln!(into, "{}ps_print_int({});", indent, val),
			Type::Float => inf_writeln!(into, "{}ps_print_float({});", indent, val),
			Type::Bool => inf_writeln!(into, "{}ps_print_bool({});", indent, val),
			Type::Void => inf_writeln!(into, "{}/* ps_print_void */", indent),
			Type::StrConst | Type::Str => inf_writeln!(into, "{}ps_print_str({});", indent, val),
			Type::StrBuf => inf_writeln!(into, "{}ps_print_str({}->buffer);", indent, val),
			Type::Bottom => { },
			Type::Fun(_) => inf_writeln!(into, "{}ps_print_ptr(\"fun\", (uintptr_t){}.fun);", indent, val),
			Type::FunRaw(_) => inf_writeln!(into, "{}ps_print_ptr(\"fun*\", (uintptr_t){});", indent, val),
			Type::Class(_) => inf_writeln!(into, "{}ps_print_ptr(\"object\", (uintptr_t){});", indent, val),
			
			Type::Option(_) => todo!("print() for Option"),
			Type::ArrayOf(_) => todo!("print() for Array"),
			Type::Tuple(tup) => {
				inf_writeln!(into, "{}ps_print_const(\"(\");", indent);
				for (idx, ty) in tup.iter().enumerate() {
					if idx > 0 {
						inf_writeln!(into, "{}ps_print_const(\", \");", indent);
					}
					// TODO: We can probably optimize the niceness of the code
					// generated here by splitting compile_partial_print into 
					// another helper function that just prints *any* value
					// of a given type.
					let new_val = self.new_val_typed(*ty);
					define_val!(self, into, new_val, " = {}.v_{};\n",
						val, idx);
					self.compile_partial_print(&new_val, into);
				}
				if tup.len() == 1 {
					// Place a comma after the last element for 1-element
					// tuple.
					inf_writeln!(into, "{}ps_print_const(\",)\");",
						indent);
				}
				else {
					inf_writeln!(into, "{}ps_print_const(\")\");",
						indent);
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
			Type::Int => inf_writeln!(into, "{}ps_strfmt_int(ctx, {}, {});", indent, buf_val, val),
			Type::Float => inf_writeln!(into, "{}ps_strfmt_float(ctx, {}, {});", indent, buf_val, val),
			Type::Void => inf_writeln!(into, "{}/* ps_strfmt_void */", indent),
			Type::Bool => inf_writeln!(into, "{}ps_strfmt_bool(ctx, {}, {});", indent, buf_val, val),
			Type::StrConst | Type::Str => inf_writeln!(into, "{}ps_strfmt_str(ctx, {}, {});", indent, buf_val, val),
			Type::StrBuf => inf_writeln!(into, "{}ps_strfmt_strbuf(ctx, {}, {});", indent, buf_val, val),
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

	fn compile_if(&mut self, ast: &AstReadonly, if_: &If, into: &mut String) -> TypedVal {
		let indent = self.indent();

		let cond = self.expr(ast, if_.condition, into);

		// Generate storage for the value of the expression, if relevant.
		// Note that we cannot have this value interacting with the GC until
		// we actually write to it.
		let own_val = self.new_val_typed_tmp(if_.typ);
		define_val!(self, into, own_val, ";\n");

		inf_writeln!(into, "{}if ({}) {{",
			indent, cond);
		self.indent_level += 1;
		// If exprs are always blocks
		let then_val = self.expr_block_unwrapped(ast, if_.then_branch, into);
		
		// Save the value, if relevant.
		// IMPORTANT: set_val will only call promote() if the value is needed.
		set_val!(self, into, own_val, " = {};\n", then_val);
		self.indent_level -= 1;
		inf_writeln!(into, "{}}}", indent);

		// Generate else branch.
		if let Some(else_branch) = if_.else_branch.as_ref() {
			inf_writeln!(into, "{}else {{", indent);
			self.indent_level += 1;

			let else_val = self.expr_block_unwrapped(ast, *else_branch, into);
			// Save the value, if relevant.
			set_val!(self, into, own_val, " = {};\n", else_val);
			self.indent_level -= 1;
			inf_writeln!(into, "{}}}", indent);
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

	/// Allocates a Val for usage with block_into.
	fn block_begin(&mut self, typ: TypId, into: &mut String) -> Val {
		let val = if self.db.type_generates_value(typ) {
			let val = self.new_val();

			inf_writeln!(into, "{}{} {};",
				self.indent(),
				self.db.get_ctype(typ),
				val);

			val
		} else { if typ == self.db.types.void { Val::Void } else { Val::Bottom } };

		val
	}

	/// Compiles an Expr::Block into the given value, using fewer braces. The idea
	/// here is to use fewer temporaries and fewer braces/indentation for nested
	/// blocks.
	fn block_into(&mut self, ast: &AstReadonly, expr: ExprId, into: &mut String, val: Val, do_own_block: bool) -> TypedVal {
		let binding = ast.exprs.get(expr);
		let Expr::Block(block) = binding else { unreachable!() };
		let indent = self.indent();

		let all_but_last = match block.stmts.len() {
			0 => 0,
			n => n - 1,
		};
		if do_own_block {
			inf_writeln!(into, "{}{{", indent);
			self.indent_level += 1;
		}

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
					self.pop_block_scope();
					if do_own_block {
						self.indent_level -= 1;
						inf_writeln!(into, "{}}}", indent);
					}
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
					// Grab fresh copy of indent() because we're in the block,
					// and may or may not have +1'd it
					inf_writeln!(into, "{}{} = {};",
						self.indent(), val, last);
				}

				val
			}
		};

		self.pop_block_scope();

		if do_own_block {
			self.indent_level -= 1;
			inf_writeln!(into, "{}}}", indent);
		}
		self.val_alloc_slots(val, block.typ)
	}

	/// Assumes that the given ExprId is a Block, and then compiles it into the
	/// `into`, assuming that it does not need to generate its own C-level block
	/// (usually because we have already generated a block, e.g. in an if
	/// statement).
	/// 
	/// Should only be called by expr_block_unwrapped, as that makes sure that
	/// e.g. the DCE pass which can change the AST structure doesn't do anything
	/// weird.
	fn __block_unwrapped(&mut self, ast: &AstReadonly, expr: ExprId, into: &mut String) -> TypedVal {
		// TODO: No need to unwrap the binding multiple times. We should probably
		// make block_into take an &Block.
		let binding = ast.exprs.get(expr);
		let Expr::Block(block) = binding else { unreachable!(); };
		let val = self.block_begin(block.typ, into);
		self.block_into(ast, expr, into, val, false)
	}

	/// If the given expr is a Block, generates it in an unwrapped fashion,
	/// otherwise generates it normally.
	fn expr_block_unwrapped(&mut self, ast: &AstReadonly, expr: ExprId, into: &mut String) -> TypedVal {
		let binding = ast.exprs.get(expr);
		match binding {
			Expr::Block(_) => {
				self.__block_unwrapped(ast, expr, into)
			}
			_ => {
				self.expr(ast, expr, into)
			}
		}
	}

	fn expr(&mut self, ast: &AstReadonly, expr: ExprId, into: &mut String) -> TypedVal {
		let indent = self.indent();
		match ast.exprs.get(expr) {
			Expr::Binary(binary) => self.compile_binary(ast, binary, into),
			Expr::Lerp(lerp) => self.compile_lerp(ast, lerp, into),
			Expr::Comparison(compare) => self.compile_comparison(ast, compare, into),
			Expr::If(if_) => self.compile_if(ast, if_, into),
			Expr::Loop(loop_) => {
				// TODO: We will probably need to generate labels or something
				// for multi-level break.
				let enclosing_loop = self.loop_val.take();

				if loop_.typ != self.db.types.bottom {
					// Create a value for the loop if it has a value.
					let val = self.new_val_typed_tmp(loop_.typ);
					
					define_val!(self, into, val, ";\n");
					self.loop_val = Some(val);
				}

				inf_writeln!(into, "{}for(;;) {{", indent);
				self.indent_level += 1;
				self.expr_block_unwrapped(ast, loop_.inner, into);
				self.indent_level -= 1;
				inf_writeln!(into, "{}}}", indent);

				let own_val = match self.loop_val.take() {
					Some(val) => self.tmp_to_used_val(val),
					// If the Loop doesn't create a val, then it is a Never.
					None => Val::Bottom.typed(self.db.types.bottom, None),
				};

				self.loop_val = enclosing_loop;

				own_val
			}
			Expr::WhileLoop(while_) => {
				// We don't have a value yet; just generate a simple loop.
				inf_writeln!(into, "{}for(;;) {{", indent);
				self.indent_level += 1;
				let cond = self.expr(ast, while_.condition, into);
				inf_writeln!(into, "{}\tif(!{}) {{ break; }}",
					indent, cond);
				let _inner = self.expr_block_unwrapped(ast, while_.inner, into);
				self.indent_level -= 1;
				inf_writeln!(into, "{}}}", indent);

				self.val_alloc_slots(Val::Void, while_.typ)
			}
			Expr::Break(break_) => {
				if let Some(inner) = break_.value {
					let val = self.expr(ast, inner, into);
					let Some(loop_val) = &self.loop_val else {
						panic!("ICE: break inside a loop with no Val");
					};
					if loop_val.needs_storage() {
						inf_writeln!(into, "{}{} = {};",
							indent, loop_val, val);
					}
				}
				inf_writeln!(into, "{}break;", indent);

				// The Break itself is always Never.
				Val::Bottom.typed(self.db.types.bottom, None)
			}
			Expr::OptionElse(opt_else) => {
				let own_val = self.new_val_typed_tmp(opt_else.typ);
				define_val!(self, into, own_val, ";\n");

				let value_val = self.expr(ast, opt_else.value, into);

				// Here we need to check if the optional value is nil or not.
				// For pointers this is easy; for everything else, we don't
				// know yet.
				if self.db.is_value_type(opt_else.typ) { todo!("option type 'is nil?' for value types") };

				// If the 'value' is non-null, then we take on that value.
				inf_writeln!(into, "{}if({}) {{",
					indent, value_val);
				if own_val.needs_storage() {
					inf_writeln!(into, "{}\t{} = {};",
						indent, own_val, value_val);
				}
				inf_writeln!(into, "{}}}", indent);

				// Otherwise, we evaluate the 'otherwise' branch, and take on
				// that value (if there was one).
				inf_writeln!(into, "{}else {{", indent);
				self.indent_level += 1;

				let otherwise_val = self.expr_block_unwrapped(ast, opt_else.otherwise, into);
				// If the otherwise value is bottom, that means that we do NOT write
				// it into our own value, because the else branch should have diverged.
				if own_val.needs_storage() && !otherwise_val.is_bottom() {
					inf_writeln!(into, "{}\t{} = {};",
						indent, own_val, otherwise_val);
				}

				self.indent_level -= 1;
				inf_writeln!(into, "{}}}", indent);
				self.tmp_to_used_val(own_val)
			}

			Expr::Logical(logical) => {
				// Compute the left value up-front. The right value will be
				// computed inside the if, for short-circuiting.
				let left = self.expr(ast, logical.left, into);
				if left.is_bottom() { return left; }

				assert!(left.typ == self.db.types.bool);

				let own_val = self.new_val_typed(self.db.types.bool);
				define_val!(self, into, own_val, " = {};\n", left);

				// Short-circuiting behavior:
				// If we're 'and', and lhs is false, we don't evaluate rhs.
				// If we're 'or', and lhs is true, we don't evaluate rhs.
				let bang = match logical.op {
					Tok::And => "",
					Tok::Or => "!",
					_ => panic!("ICE: Tried to codegen unknown logical operator"),
				};

				// Safe because own_val is bool
				inf_writeln!(into, "{}if({}{}) {{",
					indent, bang, own_val.val);
				self.indent_level += 1;

				// Generate the right expression inside the if.
				let right = self.expr(ast, logical.right, into);

				// Only store the value if not Bottom.
				if !right.is_bottom() {
					assert!(right.typ == self.db.types.bool);

					// Our value now evalutes to this other one.
					set_val!(self, into, own_val, " = {};\n", right);
				}

				self.indent_level -= 1;
				inf_writeln!(into, "{}}}", indent);

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

				let object = match call.object {
					Some(object) => {
						Some(self.expr(ast, object, into))
					}
					None => None,
				};

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
					inf_write!(into, "{}{}", comma, val);
				}
				// TODO: Implement closure, gc scoping, etc
				if let Some(object) = object {
					inf_writeln!(into, "{}{});", comma, object);
				}
				else {
					inf_writeln!(into, "{}NULL);", comma);
				}

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
				let val = self.block_begin(block.typ, into);
				self.block_into(ast, expr, into, val, true)
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
				inf_writeln!(into, "{}ps_println();", indent);
				
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
				inf_writeln!(into, "{}ps_strbuf *{} = ps_strbuf_new(ctx, 8);",
					indent, buf_val);
				
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
				inf_write!(into, "{}.fun(ctx", fun_val);

				let comma = ", ";
				for val in vals {
					inf_write!(into, "{}{}", comma, val);
					//comma = ", ";
				}
				// TODO: Implement closure, gc scoping, etc
				inf_writeln!(into, "{}{}.closure);", comma, fun_val);

				val
			},

			Expr::FunDeclare(declare) => {
				//self.compile_function(ast, declare.identity, declare.value);
                // No need to recursively compile functions due to how the
                // multithreading architecture works.

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

						inf_writeln!(into, "{}{}->{} = {};", 
							indent, val.val, varname, rhs);

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
							inf_writeln!(into, "{}{}->{} = {};",
								indent, val.val, varname, rhs);
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
				define_val!(self, into, val, " = {}{}{};\n", lhs.val, arrow, varname);

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
				define_val!(self, into, val, " = {}{}{} = {};\n", lhs.val, arrow, varname, rhs);

				val
			}

			Expr::ArrayLit(lit) => {
				let val = self.new_val_typed_tmp(lit.arr_typ);

				define_val!(self, into, val, "; PONI_INIT_ARRAY({}, sizeof({}), {}, {})\n",
					val,
					self.db.get_ctype(lit.elem_typ),
					lit.values.len(),
					self.db.get_type_ctag(lit.elem_typ));

				if val.needs_storage() {
					let mut idx = 0;
					for value in &lit.values {
						let nth = self.expr(ast, *value, into);
						assert!(nth.typ == lit.elem_typ);
						inf_writeln!(into, "{}{}->contents[{}] = {};",
							indent, val.val, idx, nth);

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
				define_val!(self, into, val, " = {}->contents[{}];\n",
					arr_val, idx_val);

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
				define_val!(self, into, val, " = {}->contents[{}] = {};\n"
					arr_val, idx_val, rhs_val);

				val
			}

			Expr::MakeTuple(tuple) => {
				let val = self.new_val_typed_tmp(tuple.typ);
				let Type::Tuple(subtypes) = self.db.get(tuple.typ) else { unreachable!() };

				// Because vector types are very widely used, generate a bit
				// nicer initializer for them.
				if let Some(_) = self.get_vec_params(tuple.typ) {
					if val.needs_storage() {
						// For the "nicer" inner values, we have to generate the
						// values first, so that they can appear before the compound
						// initializer.
						let mut inners = Vec::new();
						for (idx, expr) in tuple.values.iter().enumerate() {
							let inner = self.expr(ast, *expr, into);
							assert!(inner.typ == subtypes[idx]);
							inners.push(inner);
						}

						define_val!(self, into, val, " = {{");

						// It should be the case that this is exactly the same
						// as the vec type.
						let mut comma = false;
						for (idx, inner) in inners.iter().enumerate() {
							if comma { inf_write!(into, ", "); }

							inf_write!(into, ".v_{} = {}",
								idx, inner);

							comma = true;
						}
						inf_writeln!(into, "}};");
					}
					return self.tmp_to_used_val(val);
				}

				define_val!(self, into, val, ";\n");
				if val.needs_storage() {
					for (idx, expr) in tuple.values.iter().enumerate() {
						// TODO: Do we need to do all the exprs() first then
						// collect them after like for fun calls? I don't think so.
						let inner = self.expr(ast, *expr, into);

						assert!(inner.typ == subtypes[idx]);
						inf_writeln!(into, "{}{}.v_{} = {};",
							indent, val.val, idx, inner);
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
				// Right now, this is nothing but nil, which cannot need a GC frame.
				Val::DirectNull.typed(sum.typ, None) 
			}
		}
	}

	fn compile_partial_promote(&mut self, to: &Val, from: &Val, to_typ_id: TypId, from_typ_id: TypId, to_post: &String, from_post: &String, into: &mut String, ) {
		let to_typ = self.db.get(to_typ_id);
		let from_typ = self.db.get(from_typ_id);

		let indent = self.indent();

		// Helper function for doing the promotions
		let do_promote = |the_fn: &'static str, into: &mut String| {
			inf_writeln!(into, "{}{}{} = {}{}{});",
				indent, to, to_post, the_fn, from, from_post);
		};

		// If we end up promoting a type to itself, that is just a no-op.
		// May happen with certain tuple values.
		if to_typ == from_typ {
			inf_writeln!(into, "{}{}{} = {}{};",
				indent, to, to_post, from, from_post);
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

			(Type::Float, Type::Int) => do_promote("ps_promote_int_to_float(", into),
			(Type::StrBuf, Type::StrConst) => do_promote("ps_promote_str_to_buf(ctx, ", into),
			(Type::StrBuf, Type::Str) => do_promote("ps_promote_str_to_buf(ctx, ", into),
			(Type::Str, Type::StrConst) => do_promote("ps_promote_str_const_to_str(ctx, ", into),

			(Type::Option(inner), _) => {
				if *inner != from_typ_id {
					// Synthesize a new temporary and promote to the inner
					// temporary first.
					let inner_val = self.new_val_typed_tmp(*inner);
					define_val!(self, into, inner_val, ";\n");

					self.compile_partial_promote(&inner_val.val, from, 
						*inner, from_typ_id, 
						&"".to_string(), from_post,
						into);

					let inner_val = self.tmp_to_used_val(inner_val);

					// Now, we can promote from the temporary we created, into
					// the real into.
					self.compile_partial_promote(to, &inner_val.val, 
						to_typ_id, *inner, 
						to_post, &"".to_string(), // From post is nothing because this is a standalone val (?)
						into);

					return;
				};

				if self.db.is_value_type(*inner) {
					todo!("implement value type promotions in option")
				}
				else {
					// For reference types, there is actually no work at all
					// to promote -- a pointer of type X is already a valid value
					// of type X?.

					// For now, I guess we still do through an extra temporary,
					// which is a little sad. (Maybe a real IR will save us??)
					do_promote("(", into);
				}

			}

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

	
	// fn compile_class(&mut self, ast: &AstReadonly, class_declare: &ClassDeclare) {
	// 	// For the class, it does not generate any direct code.
	// 	// But, we do have to generate a struct for the class,
	// 	// as well as each of its function definitions.

	// 	self.inside_class.push(class_declare.identity);

	// 	for fun in &class_declare.funs {
	// 		self.compile_function(ast, fun.identity, fun.value);
	// 	}

	// 	self.inside_class.pop();
	// }

	fn compile_stmt(&mut self, ast: &AstReadonly, stmt: StmtId, into: &mut String) -> Option<TypedVal> {
		let indent = self.indent();
		match ast.stmts.get(stmt) {
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
			Stmt::ClassDeclare(_) => {
				// We don't actually have anything to do for class declares any more.
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
				inf_writeln!(into, "{}ctx->frame = gc_frame.prev;", indent);

				match &ret.expression {
					Some(value) => {
						let needed_type = *self.return_types.last().unwrap();
						let val = self.expr(ast, *value, into);

						// If the inner value is also a bottom type,
						// then we can't really generate a return here.
						if !val.is_bottom() {
							// If it's not bottom, check the typechecker's work.
							assert!(val.typ == needed_type);

							inf_writeln!(into, "{}return {};",
								indent, val);
						}
					},
					None => {
						inf_writeln!(into, "{}return;", indent);
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

	pub fn compile_assign(&mut self, ast: &AstReadonly, var: VarId, expr: ExprId, into: &mut String, is_declaration: bool) -> TypedVal {
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
		inf_writeln!(into, "{}{}{}{} = {};",
			indent, declaration, space, var_lvalue.val, value);

		var_lvalue
	}

	// Does not generate the code for a function declaration (e.g. assigning
	// it to a local).
	fn compile_function(&mut self, ast: &AstReadonly, fun: FunId) {
        // Don't compile body-less functions (extern functions). Tbh, these
        // should probably be thrown away by the queue builder.
        let Some(body) = self.db.get(fun).expression else { return; };

        // Necessary due to the new structure of the code
        self.inside_class = self.db.get(fun).class.iter().copied().collect();

		let is_init = Some(fun) == self.db.fun_init;

		let enclosing_val = self.val_idx;
		// Reset vals for each function.
		self.val_idx = 0;
		let enclosing_indent = self.indent_level;
		self.indent_level = 1;
		let indent = self.indent();

		let enclosing_gc_frame = self.gc_frame.clone();
		self.gc_frame = Arc::new(GCFrame::new());

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

		log::trace!("compile_function: {}", self.db.get_fun_cname(fun));

		// init() fun has no surrounding definition -- it is poni_init()
		if !is_init {
			inf_writeln!(own_buffer_beginning, "{} {}({}) {{",
				self.db.get_fun_ret_ctype(fun),
				self.db.get_fun_cname(fun),
				self.db.get_fun_cparams(fun));
		}

		if let Some(class) = self.db.get(fun).class {
			inf_writeln!(own_buffer, "{}struct {} *const this = closure;",
				indent, self.db.get_class_cname(class));
		}

		// Same idea as in codegen()
		let own_return_type = self.db.get_fun_return_typid(fun);
		self.return_types.push(own_return_type);

		//let val = self.block_begin(own_return_type, &mut own_buffer);//self.expr(ast, body, &mut own_buffer);
		//let val = self.block_into(ast, body, &mut own_buffer, val, false);
		let val = self.expr_block_unwrapped(ast, body, &mut own_buffer);

		// Generate unconditional GC-frame pop
		inf_writeln!(own_buffer, "{}ctx->frame = gc_frame.prev;", indent);

		if val.needs_storage() {
			assert!(val.typ == own_return_type);
			// If it does have a value, then we write it as a default
			// return value.
			inf_writeln!(own_buffer, "{}return {};", indent, val);
		}

		// Now that we have generated the inner expression, we know how big
		// of a GC frame we need. TODO: Actually generate the GC frame.
		let gc_frame_count = self.gc_frame.next_alloc_slot.get();
		// inf_writeln!(own_buffer_beginning, "{}// gc frame count: {}", indent, gc_frame_count);

		// inf_writeln!(own_buffer_beginning, "{}struct {{", indent);
		// inf_writeln!(own_buffer_beginning, "{}\tstruct poni_gc_frame *prev;", indent);
		// inf_writeln!(own_buffer_beginning, "{}\tuint64_t ptr_count;", indent);
		// inf_writeln!(own_buffer_beginning, "{}\tvoid *ptrs[{}];", indent, gc_frame_count);
		// inf_writeln!(own_buffer_beginning, "{}}} gc_frame = {{0}};", indent);
		// inf_writeln!(own_buffer_beginning, "{}gc_frame.ptr_count = {};", indent, gc_frame_count);
		// inf_writeln!(own_buffer_beginning, "{}gc_frame.prev = ctx->frame;", indent);
		// inf_writeln!(own_buffer_beginning, "{}ctx->frame = (void*)&gc_frame;", indent);

		// Instead of generating the code directly, use a macro.
		inf_writeln!(own_buffer_beginning, "{}PONI_GC_FRAME({});",
			indent ,gc_frame_count);
		
		// Pop type value
		self.return_types.pop();

		self.disable_gc_frames = enclosing_disable_gc_frames;
		self.block_scopes = enclosing_block_scopes;
		self.gc_frame = enclosing_gc_frame;
		self.indent_level = enclosing_indent;
		self.val_idx = enclosing_val;

		// init() fun has no surrounding scope
		if !is_init { inf_writeln!(own_buffer, "}}"); }

		if is_init {
			// TODO: Less bad this
			inf_writeln!(self.current_buffer, "void poni_init(struct poni_gc_context *ctx) {{");
		}

		inf_write!(self.current_buffer, "{}{}", own_buffer_beginning, own_buffer);

		if is_init {
			inf_writeln!(self.current_buffer, "}}");
		}

        //let result = format!("{}{}", own_buffer_beginning, own_buffer);
        //let _ = self.send.send(CodegenResult::Function((fun, result)));
    }

	pub fn handle_tasks(&mut self, ast: Arc<AstReadonly>, my_tasks: Vec<CodegenTask>) {
		const CHUNK_SIZE: usize = 8192 * 4;

		for task in my_tasks {
            match task {
                CodegenTask::CompileFunction(fun) => {
                    self.compile_function(&ast, fun);
                }
            }

			if self.current_buffer.len() >= CHUNK_SIZE {
				// Note that the buffer, no matter the chunk size, is still
				// going to be only valid C code, because we build entire
				// correct things at a time.
				let _ = self.send.send(std::mem::take(&mut self.current_buffer));
			}
        }

		// After completing the tasks, send one final buffer.
		if self.current_buffer.len() > 0 {
			let _ = self.send.send(std::mem::take(&mut self.current_buffer));
		}
    }
}
