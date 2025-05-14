use std::fmt::Write;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::cell::RefCell;

use crate::error::Error;
// Import relevant things.
use crate::expr::Var;
use crate::expr::Fun;
use crate::expr::Sig;
use crate::expr::Class;
use crate::typ::Type;
use crate::source::{Source, SourceLocation};

use crate::lexer::{Tok, Token};

use clap::builder::Str;
use rustc_hash::{FxHashMap};

// Include arenas
include!(concat!(env!("OUT_DIR"), "/db.arenas.rs"));

#[derive(Clone, Copy)]
pub enum ScopeEntry {
	Var(VarId),
	Fun(FunId),
	Class(ClassId),

	None
}

pub struct StrProperties {
	length: VarId,
	length_key: StrId,
}

pub struct DbTypes {
	pub str_const: TypId,
	pub str: TypId,
	pub str_buf: TypId,
	pub void: TypId,
	pub unassigned: TypId,
	pub int: TypId,
	pub float: TypId,
	pub bool: TypId,
	pub bottom: TypId,

	pub assume_int: TypId,
	pub assume_float: TypId,

	pub fun_sig_unassigned: TypId,
}

/// The Db stores all of the arena-allocated objects that can be referenced
/// with Ids. Basically all of these objects live for the entire program.
pub struct Db {
	arenas: DbArenas,

	str_side_map: FxHashMap<String, StrId>,
	source_side_map: FxHashMap<PathBuf, SourceId>,
	type_side_map: FxHashMap<Type, TypId>,
	sig_side_map: FxHashMap<Sig, SigId>,

	sig_cname_cache: FxHashMap<SigId, (&'static str, &'static str)>,
	sig_cdeclared: FxHashMap<SigId, bool>,
	sig_cgenerated: FxHashMap<SigId, bool>,

	array_cname_cache: FxHashMap<TypId, &'static str>,
	//array_cgenerated: FxHashMap<TypId, bool>,

	str_simple_const_map: FxHashMap<String, StrConstId>,

	key_lookup_map: FxHashMap<StrId, Tok>,

	/// Keep a cache of all generated ctypes so that we can quickly re-use them.
	ctype_cache: Vec<&'static str>,

	known_var_cnames: FxHashMap<VarId, &'static str>,

	var_cname_cache: Vec<&'static str>,
	fun_cname_cache: Vec<&'static str>,
	class_cname_cache: Vec<&'static str>,
	class_preparer_cache: Vec<&'static str>,

	/// Keep a cache of generated type reprs also for re-using them.
	type_repr_cache: RefCell<FxHashMap<TypId, &'static str>>,

	fun_cparams_cache: Vec<&'static str>,

	pub fun_init: Option<FunId>,
	pub types: DbTypes,

	pub synthetic: SourceId,
	pub sig_unassigned: SigId,
	pub class_unassigned: ClassId,
	pub var_unassigned: VarId,

	pub errors: Vec<Error>,

	/// Whether we're compiling in a mode where we're testing the compiler.
	/// Useful for comments to support the "expected value" of the test.
	pub test_mode: bool,
	/// The expected lines of output from the program.
	/// Note that these do NOT include the newlines or carriage returns. Those
	/// are assumed to already exist.
	pub test_lines: Vec<String>,

	/// Maps names of the form "scope.scope.Item" to ScopeEntries. Used to bind
	/// names to specific objects.
	name_map: FxHashMap<StrId, ScopeEntry>,

	/// Some C code to declare each Sig type.
	pub sig_declare_code: String,

	/// Some C code to declare each Array type.
	/// TODO: This doesn't quite work, we really need to do a topological
	/// sort on this stuff.
	// pub arr_declare_code: String,

	pub str_anonymous: StrId,
	pub str_lambda: StrId,

	prop_str: StrProperties,
}

impl Db {
	pub fn new() -> Self {
		let mut db = Db {
			arenas: DbArenas::new(),

			str_side_map: FxHashMap::default(),
			source_side_map: FxHashMap::default(),
			type_side_map: FxHashMap::default(),
			sig_side_map: FxHashMap::default(),

			sig_cname_cache: FxHashMap::default(),
			sig_cdeclared: FxHashMap::default(),
			sig_cgenerated: FxHashMap::default(),

			array_cname_cache: FxHashMap::default(),

			str_simple_const_map: FxHashMap::default(),

			key_lookup_map: FxHashMap::default(),

			ctype_cache: Vec::new(),

			known_var_cnames: FxHashMap::default(),

			type_repr_cache: RefCell::new(FxHashMap::default()),
			fun_cparams_cache: Vec::new(),

			var_cname_cache: Vec::new(),
			fun_cname_cache: Vec::new(),
			class_cname_cache: Vec::new(),
			class_preparer_cache: Vec::new(),

			errors: Vec::new(),

			fun_init: None,
			types: DbTypes {
				str_const: TypId(0),
				str: TypId(0),
				str_buf: TypId(0),
				void: TypId(0),
				unassigned: TypId(0),
				int: TypId(0),
				float: TypId(0),
				bool: TypId(0),
				bottom: TypId(0),

				assume_int: TypId(0),
				assume_float: TypId(0),

				fun_sig_unassigned: TypId(0),
			},

			synthetic: SourceId(0),
			sig_unassigned: SigId(0),
			class_unassigned: ClassId(0),
			var_unassigned: VarId(0),

			name_map: FxHashMap::default(),

			test_mode: false,
			test_lines: Vec::new(),

			sig_declare_code: String::new(),

			str_anonymous: StrId(0),
			str_lambda: StrId(0),

			prop_str: StrProperties {
				length: VarId(0),
				length_key: StrId(0)
			}
		};

		db.types.str_const  = db.put_type(Type::StrConst);
		db.types.str        = db.put_type(Type::Str);
		db.types.str_buf    = db.put_type(Type::StrBuf);
		db.types.void       = db.put_type(Type::Void);
		db.types.unassigned = db.put_type(Type::Unassigned);
		db.types.int        = db.put_type(Type::Int);
		db.types.float      = db.put_type(Type::Float);
		db.types.bool       = db.put_type(Type::Bool);
		db.types.bottom     = db.put_type(Type::Bottom);

		db.types.assume_float = db.put_type(Type::AssumeFloat);
		db.types.assume_int = db.put_type(Type::AssumeInt);

		db.synthetic = db.new_id(Source::Synthetic);

		db.sig_unassigned = db.put_sig(&Sig {
			parameters: vec![],
			return_type: db.types.unassigned
		});

		// TODO: Maybe make class_unassigned a special value...?
		// For now it's going to cause some unsafety..

		db.types.fun_sig_unassigned = db.put_type(Type::Fun(db.sig_unassigned));

		db.str_anonymous = db.put_str("<anonymous>");
		db.str_lambda = db.put_str("lambda");

		// Technically, this does waste the initially created
		// HashMap, but the db is created once per whole program run,
		// so it's not a huge inefficiency.
		db.key_lookup_map = crate::lexer::build_key_lookup_map(&mut db);

		(db.prop_str.length_key, db.prop_str.length) = db.synthesize_property("length", "length", db.types.int);

		return db;
	}

	pub fn synthetic(&self) -> SourceLocation {
		SourceLocation {
			source: self.synthetic,
			offset: 0,
			length: 0,
		}
	}

	pub fn synthetic_id(&self, name: StrId) -> Token {
		Token {
			typ: Tok::Identifier,
			lexeme: name,
			location: self.synthetic(),
		}
	}

	pub fn synthesize_property(&mut self, str: &'static str, cname: &'static str, typ: TypId) -> (StrId, VarId) {
		let key = self.put_str(str);
		let var = Var {
			name: self.synthetic_id(key),
			typ,
			class: None,
			init: false,
		};
		let var = self.new_id(var);

		self.known_var_cnames.insert(var, cname);

		(key, var)
	}

	pub fn put_sig(&mut self, sig: &Sig) -> SigId {
		if let Some(existing) = self.sig_side_map.get(sig) {
			return *existing;
		}

		let id = self.new_id(sig.clone());

		self.sig_side_map.insert(sig.clone(), id);

		id
	}

	// TODO: Return a string, etc..
	pub fn repr_nth_idx(&self, idx: usize) -> usize {
		idx + 1 // 0 -> 1st
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

		//let ctype = typ.gen_ctype(self);

		// IMPORTANT: The pushes() here must line up with new_id() -> TypId
		//self.ctype_cache.push(ctype.leak());

		let id = self.new_id(typ.clone());
		self.type_side_map.insert(typ, id);

		return id;
	}

	pub fn must_get_type(&self, typ: Type) -> TypId {
		*self.type_side_map.get(&typ).unwrap()
	}

	fn gen_sig_ctype_impl(&mut self, sig_id: SigId) -> (&'static str, &'static str) {
		if let Some(existing) = self.sig_cname_cache.get(&sig_id) {
			return *existing;
		}

		let struct_name = format!("ps_sig_{}", sig_id.0);
		let fnptr_name = format!("ps_sigraw_{}", sig_id.0);

		let result: (&'static str, &'static str) = (fnptr_name.leak(), struct_name.leak());

		self.sig_cname_cache.insert(sig_id, result);
	
		result
	}

	fn gen_type_dependencies(&mut self, typ: TypId) {
		match self.get(typ) {
			Type::FunRaw(sig) | Type::Fun(sig) => {
				self.gen_sig(*sig)
			},

			_ => { }
		}
	}
	
	fn gen_sig(&mut self, sig_id: SigId) {
		use crate::inf_write;
		use crate::inf_writeln;

		if *self.sig_cgenerated.get(&sig_id).unwrap_or(&false) {
			// Return if we've already done it.
			// TODO: This could just be an FxHashSet...
			// Other TODO: Figure out cyclic references (e.g. throw an error
			// if gen_type_dependcies calls use_sig on the same value again)
			return;
		}

		// Now the sig has been generated
		self.sig_cgenerated.insert(sig_id, true);

		let (fnptr_name, struct_name) = self.gen_sig_ctype_impl(sig_id);

		// First, if that sig itself references any other sigs, we have to
		// use_sig() them. This is effectively a topological sort.
		let param_count = self.get(sig_id).parameters.len();
		self.gen_type_dependencies(self.get(sig_id).return_type);
		for i in 0..param_count {
			let param = self.get(sig_id).parameters[i];
			self.gen_type_dependencies(param);
		}

		// Now, we can generate the actual code for that sig.
		let sig = &self.arenas.arena_sig[sig_id.0 as usize];

		// TODO: We have to topologically sort these declarations so that ones
		// that use earlier ones work correctly.
		inf_write!(self.sig_declare_code, "typedef {} (*{})(",
			self.get_ctype(sig.return_type),
			fnptr_name);

		let mut comma = false;
		for param in &sig.parameters {
			if comma { inf_write!(self.sig_declare_code, ", "); }
			comma = true;

			inf_write!(self.sig_declare_code, "{}", self.get_ctype(*param));
		}

		// Add closure param
		if comma { inf_write!(self.sig_declare_code, ", "); }
		inf_writeln!(self.sig_declare_code, "void*);");

		inf_writeln!(self.sig_declare_code, "typedef struct {} {{ {} fun; void* closure; }} {};",
			struct_name, fnptr_name, struct_name);
	}

	pub fn use_sig(&mut self, sig_id: SigId) {
		if *self.sig_cdeclared.get(&sig_id).unwrap_or(&false) {
			// Return if we've already done it.
			// TODO: This could just be an FxHashSet...
			// Other TODO: Figure out cyclic references (e.g. throw an error
			// if gen_type_dependcies calls use_sig on the same value again)
			return;
		}

		// Now the sig has been used
		self.sig_cdeclared.insert(sig_id, true);
	}

	/// Generates the ctype for a Sig. Note that this ctype might be nonsense,
	/// but that's OK.
	pub fn gen_sig_ctype(&mut self, sig: SigId) -> &'static str {
		self.gen_sig_ctype_impl(sig).1
	}

	/// Generates the raw ctype for a Sig. Note that this ctype might be nonsense,
	/// but that's OK.
	pub fn gen_sig_raw_ctype(&mut self, sig: SigId) -> &'static str {
		self.gen_sig_ctype_impl(sig).0
	}

	pub fn gen_array_ctype(&mut self, inner_ty: TypId) -> &'static str {
		if let Some(existing) = self.array_cname_cache.get(&inner_ty) {
			return existing;
		}

		let name = format!("ps_arr_{}", inner_ty.0);
		let name = name.leak();
		self.array_cname_cache.insert(inner_ty, name);

		name
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

	/// Performs an efficient lookup by not generating new StrIds for names
	/// that do not exist.
	pub fn lookup_full_name(&self, name: &str) -> ScopeEntry {
		let Some(id) = self.str_side_map.get(name) else {
			return ScopeEntry::None;
		};

		let Some(entry) = self.name_map.get(id) else {
			return ScopeEntry::None;
		};

		return *entry;
	}

	// TODO: Return the old name for error reporting..?
	pub fn add_full_name(&mut self, name: &str, entry: ScopeEntry) -> Option<ScopeEntry> {
		let name = self.put_str(name);

		self.name_map.insert(name, entry)
	}

	pub fn new_var(&mut self, name: Token, typ: TypId, class: Option<ClassId>, init: bool) -> VarId {
		let var = Var {
			name,
			typ,
			class,
			init
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

		if let Some(existing) = self.ctype_cache.get(typ.to_usize()) {
			return *existing;
		}

		// Didn't get the type -- give a helpful panic message.
		let ty_name = self.get(typ).to_string(self);
		panic!("Tried to get invalid type in get_ctype: {} (TypId {})", ty_name, typ.to_usize());

		// Safety: AS LONG AS we don't call new_id outside of put_type,
		// the index must be valid.
		// TODO: Make this code exist in like a "#[cfg(release)] or whatever."
		// unsafe { self.ctype_cache.get_unchecked(typ.to_usize()) }
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

	pub fn get_fun_ret_type(&self, fun: FunId) -> TypId {
		self.get(fun).return_type
	}

	/// Should only be called when we know for sure we have the correct
	/// index.
	pub fn get_fun_param_type(&self, fun: FunId, param: usize) -> TypId {
		self.get_var_type(*unsafe { self.get(fun).parameters.get_unchecked(param) })
	}

	pub fn get_sig_param_type(&self, sig: SigId, param: usize) -> TypId {
		*unsafe { self.get(sig).parameters.get_unchecked(param) }
	}

	pub fn get_fun_name(&self, fun: FunId) -> &str {
		if let Some(name) = &self.get(fun).name {
			self.get(name.lexeme)
		}
		else {
			"<anonymous>"
		}
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
	
	pub fn get_class_cname(&self, class: ClassId) -> &'static str {
		unsafe { self.class_cname_cache.get_unchecked(class.to_usize()) }
	}

	pub fn get_class_preparer_cname(&self, class: ClassId) -> &'static str {
		unsafe { self.class_preparer_cache.get_unchecked(class.to_usize()) }
	}

	pub fn repr_var(&self, var: VarId) -> &str {
		self.get(self.get(var).name.lexeme)
	}

	pub fn repr_class(&self, class: ClassId) -> &str {
		self.get(self.get(class).name.lexeme)
	}

	pub fn repr_var_type(&self, var: VarId) -> &'static str {
		self.repr_type(self.get(var).typ)
	}

	pub fn repr_type(&self, typ: TypId) -> &'static str {
		if let Some(&cached) = self.type_repr_cache.borrow().get(&typ) {
			return cached;
		}

		let value = self.get(typ).to_string(self).leak();

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
	
	pub fn lookup_property(&self, typ: TypId, propname: StrId) -> Option<VarId> {
		let ty = self.get(typ);
		match ty {
			// HACK: Right now these all do the same thing.
			Type::Str | Type::StrConst | Type::StrBuf => {
				if propname == self.prop_str.length_key {
					return Some(self.prop_str.length);
				}

				None
			},
			Type::Class(class_id) => {
				let class = self.get(*class_id);
				let result = class.var_map.get(&propname).copied();
				result
			},

			_ => None
		}
	}

	pub fn lookup_member_fn(&self, typ: TypId, propname: StrId) -> Option<FunId> {
		let ty = self.get(typ);
		match ty {
			Type::Class(class_id) => {
				let class = self.get(*class_id);
				class.fun_map.get(&propname).copied()
			}

			_ => None
		}
	}
	
	pub fn generate_codegen_caches(&mut self) {
		// The order matters, as e.g. var cnames are used for fun cparams.
		self.generate_class_cnames_cache();
		self.generate_ctypes_cache();
		self.generate_var_cnames_cache();
		self.generate_fun_cnames_cache();
		self.generate_fun_cparams_cache();
		self.generate_sigs_cache();
	}

	fn generate_ctypes_cache(&mut self) {
		let range = self.arenas.arena_typ.len() as IdType;

		for id in 0..range {
			let id = TypId(id);
			// It's OK to clone here because types are lightweight
			// (specifically because we're doing all this TypId stuff).
			let ty = self.get(id).clone();
			let ctype = ty.gen_ctype(self);
			self.ctype_cache.push(ctype.leak());
		}
	}

	fn generate_var_cnames_cache(&mut self) {
		let range = self.arenas.arena_var.len() as IdType;

		let mut used_set = FxHashMap::<StrId, u64>::default();

		for id in 0..range {
			let var = VarId(id);

			if let Some(desired) = self.known_var_cnames.get(&var) {
				self.var_cname_cache.push(desired);
				continue;
			}

			let str_id = self.get(var).name.lexeme;
			let cname = match used_set.entry(str_id) {
				std::collections::hash_map::Entry::Occupied(mut val) => {
					let result = *val.get();
					*val.get_mut() += 1;
					format!("v_{}{}", self.get(str_id), result)
				},
				std::collections::hash_map::Entry::Vacant(val) => {
					val.insert(0);
					format!("v_{}", self.get(str_id))
				},
			};
			let cname = cname.leak();

			self.var_cname_cache.push(cname);
		}
	}

	fn generate_class_cnames_cache(&mut self) {
		let range = self.arenas.arena_class.len() as IdType;

		let mut used_set = FxHashMap::<StrId, u64>::default();

		for id in 0..range {
			let class = ClassId(id);

			let str_id = self.get(class).name.lexeme;
			let cname = match used_set.entry(str_id) {
				std::collections::hash_map::Entry::Occupied(mut val) => {
					let result = *val.get();
					*val.get_mut() += 1;
					format!("cl_{}{}", self.get(str_id), result)
				},
				std::collections::hash_map::Entry::Vacant(val) => {
					val.insert(0);
					format!("cl_{}", self.get(str_id))
				},
			};
			let cname = cname.leak();
			let preparer = format!("i{}", cname).leak();

			self.class_cname_cache.push(cname);
			self.class_preparer_cache.push(preparer);
		}
	}

	fn generate_fun_cnames_cache(&mut self) {
		let range = self.arenas.arena_fun.len() as IdType;

		let mut used_set = FxHashMap::<StrId, u64>::default();

		for id in 0..range {
			let fun = FunId(id);

			// TODO: Actual name mangling and such
			let cname_id = if let Some(name) = &self.get(fun).name {
				name.lexeme
			} else { self.str_lambda };

			let cname = match used_set.entry(cname_id) {
				std::collections::hash_map::Entry::Occupied(mut val) => {
					let result = *val.get();
					*val.get_mut() += 1;
					format!("f_{}{}", self.get(cname_id), result)
				},
				std::collections::hash_map::Entry::Vacant(val) => {
					val.insert(0);
					format!("f_{}", self.get(cname_id))
				},
			};

			let cname = cname.leak();
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
	
	fn generate_sigs_cache(&mut self) {
		let sigs = std::mem::take(&mut self.sig_cdeclared);

		for sig in sigs {
			self.gen_sig(sig.0);
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