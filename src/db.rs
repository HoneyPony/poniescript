use std::any::TypeId;
use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::cell::RefCell;

// Import relevant things.
use crate::expr::Var;
use crate::typ::Type;
use crate::source::{Source, SourceLocation};

use crate::lexer::{Tok, Token};

// I guess we could just use one file, because we're not allowed to have the macro
// expand to struct fields, for some reason.
include!(concat!(env!("OUT_DIR"), "/db.code.rs"));
include!(concat!(env!("OUT_DIR"), "/db.struct.rs"));

/// The Db stores all of the arena-allocated objects that can be referenced
/// with Ids. Basically all of these objects live for the entire program.
pub struct Db {
	arenas: DbArenas,

	str_side_map: HashMap<String, StrId>,
	source_side_map: HashMap<PathBuf, SourceId>,
	type_side_map: HashMap<Type, TypId>,

	key_lookup_map: HashMap<StrId, Tok>,

	/// Keep a cache of all generated ctypes so that we can quickly re-use them.
	ctype_cache: HashMap<TypId, &'static str>,

	/// Keep a cache of generated type reprs also for re-using them.
	type_repr_cache: RefCell<HashMap<TypId, &'static str>>,
}

impl Db {
	pub fn new() -> Self {
		let mut db = Db {
			arenas: DbArenas::new(),

			str_side_map: HashMap::new(),
			source_side_map: HashMap::new(),
			type_side_map: HashMap::new(),

			key_lookup_map: HashMap::new(),

			ctype_cache: HashMap::new(),
			type_repr_cache: RefCell::new(HashMap::new()),
		};

		// Technically, this does waste the initially created
		// HashMap, but the db is created once per whole program run,
		// so it's not a huge inefficiency.
		db.key_lookup_map = crate::lexer::build_key_lookup_map(&mut db);

		return db;
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

		let id = self.new_id(typ.clone());
		self.type_side_map.insert(typ, id);

		return id;
	}

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

	pub fn get_cname(&self, var: VarId) -> &str {
		// TODO: Cname generation, as well as 'extern C' sort of thing
		return self.get(self.get(var).name.lexeme);
	}

	// These should definitely be cached rather than generated each time, but..
	//
	// TODO: Consider generating the ctypes as soon as we generate a new type
	pub fn get_ctype(&mut self, typ: TypId) -> &'static str {
		if let Some(&cached) = self.ctype_cache.get(&typ) {
			return cached;
		}

		let full_type = self.get(typ).clone();
		let value = full_type.gen_ctype(self).leak();

		// TODO: Look into ways to make this safe with &self rather than
		// &mut self. It should be fine...?
		self.ctype_cache.insert(typ, value);

		value
	}

	pub fn get_var_ctype(&mut self, var: VarId) -> &'static str {
		self.get_ctype(self.get(var).typ)
	}

	pub fn get_var_type(&self, var: VarId) -> TypId {
		self.get(var).typ
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

	pub fn err_locate(&self, location: &SourceLocation) {
		// TODO: Implement an actual system for showing error locations.
		eprintln!("at {}, offset {}", location.source.0, location.offset);
	}

	pub fn var_range(&self) -> VarIter {
		return VarIter { 
			len: self.arenas.arena_var.len(),
			current: 0
		};
	}
}

pub struct VarIter {
	len: usize,
	current: usize,
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

/// Lets the Db implement some functions for every type of id.
pub trait IdFuncs<Id, T> {
	/// TODO: Consider making 'T' a type of IdFuncs, so it is only parameterized
	/// by Id.
	fn get(&self, id: Id) -> &T;

	fn get_mut(&mut self, id: Id) -> &mut T;

	fn new_id(&mut self, t: T) -> Id;
}