use std::collections::HashMap;
use std::path::{self, Path, PathBuf};

// Import relevant things.
use crate::expr::Var;
use crate::typ::Typ;
use crate::source::Source;

use crate::lexer::Tok;

// I guess we could just use one file, because we're not allowed to have the macro
// expand to struct fields, for some reason.
include!(concat!(env!("OUT_DIR"), "/db.code.rs"));
include!(concat!(env!("OUT_DIR"), "/db.struct.rs"));

/// The Db stores all of the arena-allocated objects that can be referenced
/// with Ids. Basically all of these objects live for the entire program.
pub struct Db {
	arenas: DbArenas,

	str_side_map: HashMap<Box<str>, StrId>,
	source_side_map: HashMap<PathBuf, SourceId>,

	key_lookup_map: HashMap<StrId, Tok>,
}

impl Db {
	pub fn new() -> Self {
		let mut db = Db {
			arenas: DbArenas::new(),

			str_side_map: HashMap::new(),
			source_side_map: HashMap::new(),

			key_lookup_map: HashMap::new(),
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

		let boxed: Box<str> = str.to_owned().into_boxed_str();
		// TODO: Figure out if this is working correctly...
		let id = self.new_id(boxed.clone());
		self.str_side_map.insert(boxed, id);

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

	pub fn lookup_key(&self, id: StrId) -> Option<Tok> {
		self.key_lookup_map.get(&id).map(|tok| *tok)
	}
}

/// Lets the Db implement some functions for every type of id.
pub trait IdFuncs<Id, T> {
	fn get(&self, id: Id) -> &T;

	fn get_mut(&mut self, id: Id) -> &mut T;

	fn new_id(&mut self, t: T) -> Id;
}