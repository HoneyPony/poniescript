use std::collections::HashMap;

// Import relevant things.
use crate::expr::Var;
use crate::typ::Typ;

// I guess we could just use one file, because we're not allowed to have the macro
// expand to struct fields, for some reason.
include!(concat!(env!("OUT_DIR"), "/db.code.rs"));
include!(concat!(env!("OUT_DIR"), "/db.struct.rs"));

/// The Db stores all of the arena-allocated objects that can be referenced
/// with Ids. Basically all of these objects live for the entire program.
pub struct Db {
	arenas: DbArenas,

	str_side_map: HashMap<Box<str>, StrId>,
}

impl Db {
	pub fn new() -> Self {
		return Db {
			arenas: DbArenas::new(),

			str_side_map: HashMap::new(),
		}
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
}

/// Lets the Db implement some functions for every type of id.
pub trait IdFuncs<Id, T> {
	fn get(&self, id: Id) -> &T;

	fn get_mut(&mut self, id: Id) -> &mut T;

	fn new_id(&mut self, t: T) -> Id;
}