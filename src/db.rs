// I guess we could just use one file, because we're not allowed to have the macro
// expand to struct fields, for some reason.
include!(concat!(env!("OUT_DIR"), "/db.code.rs"));
include!(concat!(env!("OUT_DIR"), "/db.struct.rs"));

/// The Db stores all of the arena-allocated objects that can be referenced
/// with Ids. Basically all of these objects live for the entire program.
pub struct Db {
	arenas: DbArenas
}

/// Lets the Db implement some functions for every type of id.
pub trait IdFuncs<Id, T> {
	fn get(&self, id: Id) -> &T;

	fn get_mut(&mut self, id: Id) -> &mut T;

	fn new_id(&mut self, t: T) -> Id;
}