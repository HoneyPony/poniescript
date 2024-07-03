use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::cell::RefCell;

use crate::error::Error;
// Import relevant things.
use crate::expr::Var;
use crate::expr::Fun;
use crate::typ::Type;
use crate::source::{Source, SourceLocation};

use crate::lexer::{Tok, Token};

use rustc_hash::{FxHashMap};

// Include arenas
include!(concat!(env!("OUT_DIR"), "/db.arenas.rs"));

pub struct DbTypes {
	pub str_const: TypId,
	pub void: TypId,
	pub unassigned: TypId,
	pub int: TypId,
	pub float: TypId,
	pub bottom: TypId,

	pub assume_int: TypId,
	pub assume_float: TypId,
}

/// The Db stores all of the arena-allocated objects that can be referenced
/// with Ids. Basically all of these objects live for the entire program.
pub struct Db {
	arenas: DbArenas,

	str_side_map: FxHashMap<String, StrId>,
	source_side_map: FxHashMap<PathBuf, SourceId>,
	type_side_map: FxHashMap<Type, TypId>,

	str_simple_const_map: FxHashMap<String, StrConstId>,

	key_lookup_map: FxHashMap<StrId, Tok>,

	/// Keep a cache of all generated ctypes so that we can quickly re-use them.
	ctype_cache: Vec<&'static str>,

	var_cname_cache: Vec<&'static str>,
	fun_cname_cache: Vec<&'static str>,

	/// Keep a cache of generated type reprs also for re-using them.
	type_repr_cache: RefCell<FxHashMap<TypId, &'static str>>,

	fun_cparams_cache: Vec<&'static str>,

	pub fun_init: Option<FunId>,
	pub types: DbTypes,

	pub synthetic: SourceId,

	pub errors: Vec<Error>
}

impl Db {
	pub fn new() -> Self {
		let mut db = Db {
			arenas: DbArenas::new(),

			str_side_map: FxHashMap::default(),
			source_side_map: FxHashMap::default(),
			type_side_map: FxHashMap::default(),

			str_simple_const_map: FxHashMap::default(),

			key_lookup_map: FxHashMap::default(),

			ctype_cache: Vec::new(),
			type_repr_cache: RefCell::new(FxHashMap::default()),
			fun_cparams_cache: Vec::new(),

			var_cname_cache: Vec::new(),
			fun_cname_cache: Vec::new(),

			errors: Vec::new(),

			fun_init: None,
			types: DbTypes {
				str_const: TypId(0),
				void: TypId(0),
				unassigned: TypId(0),
				int: TypId(0),
				float: TypId(0),
				bottom: TypId(0),

				assume_int: TypId(0),
				assume_float: TypId(0),
			},

			synthetic: SourceId(0),
		};

		db.types.str_const  = db.put_type(Type::StrConst);
		db.types.void       = db.put_type(Type::Void);
		db.types.unassigned = db.put_type(Type::Unassigned);
		db.types.int        = db.put_type(Type::Int);
		db.types.float      = db.put_type(Type::Float);
		db.types.bottom     = db.put_type(Type::Bottom);

		db.types.assume_float = db.put_type(Type::AssumeFloat);
		db.types.assume_int = db.put_type(Type::AssumeInt);

		db.synthetic = db.new_id(Source::Synthetic);

		// Technically, this does waste the initially created
		// HashMap, but the db is created once per whole program run,
		// so it's not a huge inefficiency.
		db.key_lookup_map = crate::lexer::build_key_lookup_map(&mut db);

		return db;
	}

	pub fn is_not_concrete(&self, id: TypId) -> bool {
		return id == self.types.assume_float || id == self.types.assume_int;
	}

	pub fn is_concrete(&self, id: TypId) -> bool {
		return !self.is_not_concrete(id)
	}

	pub fn put_str(&mut self, str: &str) -> StrId {
		if let Some(existing) = self.str_side_map.get(str) {
			return *existing;
		}

		let leaked = str.to_owned().leak();

		let id = IdFuncs::<StrId, &'static str>::new_id(self, leaked);
		self.str_side_map.insert(leaked.to_string(), id);

		return id;
	}

	pub fn put_source_path(&mut self, path: &Path) -> SourceId {
		if let Some(existing) = self.source_side_map.get(path) {
			return *existing;
		}

		let buf = path.to_path_buf();
		let source = Source::new(buf.clone());

		let id = self.new_id(source);
		self.source_side_map.insert(buf, id);

		return id;
	}

	pub fn put_type(&mut self, typ: Type) -> TypId {
		if let Some(existing) = self.type_side_map.get(&typ) {
			return *existing;
		}

		let ctype = typ.gen_ctype(self);

		// IMPORTANT: The pushes() here must line up with new_id() -> TypId
		self.ctype_cache.push(ctype.leak());

		let id = self.new_id(typ.clone());
		self.type_side_map.insert(typ, id);

		return id;
	}

	pub fn put_str_const_simple(&mut self, string: &str) -> StrConstId {
		if let Some(existing) = self.str_simple_const_map.get(string) {
			return *existing;
		}

		let id: StrConstId = IdFuncs::<StrConstId, &'static str>::new_id(self, string.to_string().leak());
		self.str_simple_const_map.insert(string.to_string(), id);
		id
	}

	//pub fn iter_str_const(&self) -> impl Iterator<Item = (StrConstId, &'static str)> + '_ {
	//	self.arenas.arena_strconst.iter().enumerate().map(|(k, v)| (StrConstId(k as u32), *v))
	//}

	pub fn lookup_key(&self, id: StrId) -> Option<Tok> {
		self.key_lookup_map.get(&id).map(|tok| *tok)
	}

	pub fn new_var(&mut self, name: Token, typ: TypId) -> VarId {
		let var = Var {
			name,
			typ,
		};

		return self.new_id(var);
	}

	pub fn get_cname(&self, var: VarId) -> &'static str {
		// TODO: Cname generation, as well as 'extern C' sort of thing
		unsafe { self.var_cname_cache.get_unchecked(var.to_usize()) }
	}

	// These should definitely be cached rather than generated each time, but..
	//
	// TODO: Consider generating the ctypes as soon as we generate a new type
	pub fn get_ctype(&self, typ: TypId) -> &'static str {
		// Safety: AS LONG AS we don't call new_id outside of put_type,
		// the index must be valid.
		unsafe { self.ctype_cache.get_unchecked(typ.to_usize()) }
	}

	pub fn get_var_ctype(&self, var: VarId) -> &'static str {
		self.get_ctype(self.get(var).typ)
	}

	pub fn get_var_type(&self, var: VarId) -> TypId {
		self.get(var).typ
	}

	pub fn get_fun_ret_ctype(&self, fun: FunId) -> &'static str {
		self.get_ctype(self.get(fun).return_type)
	}
	
	pub fn get_fun_cname(&self, fun: FunId) -> &str {
		// TODO: Cname generation
		unsafe { self.fun_cname_cache.get_unchecked(fun.to_usize()) }
	}

	pub fn get_fun_return_typid(&self, fun: FunId) -> TypId {
		self.get(fun).return_type
	}

	pub fn does_fun_return_void(&self, fun: FunId) -> bool {
		match self.get(self.get_fun_return_typid(fun)) {
			Type::Void => true, 
			_ => false
		}
	}
	
	pub fn get_fun_cparams(&self, fun: FunId) -> &'static str {
		// This is a bit less safe. It cannot be called until
		// db.generate_fun_cparams_cache() has been called, which can't be
		// called until after type-checking.
		unsafe { self.fun_cparams_cache.get_unchecked(fun.to_usize()) }
	}

	/// Computes the needed "context type" corresponding to the given
	/// type. This is the type used for UnassignedNumeric types while
	/// code-gening.
	/// 
	/// If there is a specific needed type, e.g. Int or Float, this will
	/// return that; otherwise, it will return Float, which is the most
	/// generic.
	/// 
	/// That said, if a type is not getting propagated, it is either the
	/// last value in a block, or dead code.
	pub fn get_context_type(&mut self, typ: TypId) -> TypId {
		let typ = match self.get(typ) {
			Type::Int => Type::Int,
			Type::Float => Type::Float,
			_ => Type::Float,
		};

		/// TODO: CACHE FLOAT AND INT VALUES SO THIS CAN BE &self
		return self.put_type(typ);
	}

	pub fn repr_var(&self, var: VarId) -> &str {
		self.get(self.get(var).name.lexeme)
	}

	pub fn repr_var_type(&self, var: VarId) -> &'static str {
		self.repr_type(self.get(var).typ)
	}

	pub fn repr_type(&self, typ: TypId) -> &'static str {
		if let Some(&cached) = self.type_repr_cache.borrow().get(&typ) {
			return cached;
		}

		let value = self.get(typ).to_string().leak();

		// Note: Using &'static str as the hash map value makes it possible
		// to do this with interior mutability. Maybe we should also do that
		// for ctypes -- although, those the overhead from RefCell is more
		// relevant because we have to generate a LOT of those in the compiled
		// code.
		self.type_repr_cache.borrow_mut().insert(typ, value);

		value
	}

	pub fn report_error(&mut self, error: Error) {
		self.errors.push(error);
	}

	pub fn type_generates_value(&self, id: TypId) -> bool {
		match self.get(id) {
			Type::Void => false,
			Type::Bottom => false,
			_ => true,
		}
	}

	pub fn var_range(&self) -> VarIter {
		return VarIter { 
			len: self.arenas.arena_var.len() as IdType,
			current: 0
		};
	}

	pub fn str_const_range(&self) -> StrConstIter {
		return StrConstIter {
			len: self.arenas.arena_strconst.len() as IdType,
			current: 0
		}
	}
	
	pub fn generate_codegen_caches(&mut self) {
		// The order matters, as e.g. var cnames are used for fun cparams.
		self.generate_var_cnames_cache();
		self.generate_fun_cnames_cache();
		self.generate_fun_cparams_cache();
	}

	fn generate_var_cnames_cache(&mut self) {
		let range = self.arenas.arena_var.len() as IdType;

		for id in 0..range {
			let var = VarId(id);

			// TODO: Actual name mangling and such
			let cname = self.get(self.get(var).name.lexeme).to_string().leak();
			self.var_cname_cache.push(cname);
		}
	}

	fn generate_fun_cnames_cache(&mut self) {
		let range = self.arenas.arena_fun.len() as IdType;

		for id in 0..range {
			let fun = FunId(id);

			// TODO: Actual name mangling and such
			let cname = self.get(self.get(fun).name.lexeme).to_string().leak();
			self.fun_cname_cache.push(cname);
		}
	}

	fn generate_fun_cparams_cache(&mut self) {
		let range = self.arenas.arena_fun.len() as IdType;

		for id in 0..range {
			let id = FunId(id);
			let mut buffer = String::new();

			let mut comma = false;

			for param in &self.get(id).parameters {
				if comma { buffer.push_str(", "); }
				comma = true;

				buffer.push_str(self.get_var_ctype(*param));
				buffer.push(' ');
				buffer.push_str(self.get_cname(*param));
			}

			if comma { buffer.push_str(", "); }
			buffer.push_str("void* closure");

			self.fun_cparams_cache.push(buffer.leak());
		}
	}
}

pub struct VarIter {
	len: IdType,
	current: IdType,
}

impl Iterator for VarIter {
	type Item = VarId;

	fn next(&mut self) -> Option<Self::Item> {
		let result = if self.current == self.len {
			None
		}
		else { Some(VarId(self.current)) };

		self.current += 1;

		result
	}
}

// TODO: Just generate these using build_db.rs
pub struct StrConstIter {
	len: IdType,
	current: IdType,
}

impl Iterator for StrConstIter {
	type Item = StrConstId;

	fn next(&mut self) -> Option<Self::Item> {
		let result = if self.current == self.len {
			None
		}
		else { Some(StrConstId(self.current)) };

		self.current += 1;

		result
	}
}

/// Lets the Db implement some functions for every type of id.
pub trait IdFuncs<Id, T> {
	/// TODO: Consider making 'T' a type of IdFuncs, so it is only parameterized
	/// by Id.
	fn get(&self, id: Id) -> &T;

	fn get_mut(&mut self, id: Id) -> &mut T;

	fn new_id(&mut self, t: T) -> Id;
}