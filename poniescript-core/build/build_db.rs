use std::fs::File;
use std::io::Write as _;
use std::fmt::Write as _;

static TY: &str = "u32";

fn generate_struct(struct_: &mut String, id: &str, ty: &str) -> (String, String) {
	// Chop off the "id" part then convert to lowercase
	let lower_name = id[..id.len() - 2].to_ascii_lowercase();
	
	// Finally, format to create the whole name. TODO: Can we format strings
	// straight into lowercase? It doesn't seem like it.
	let arena_name = format!("arena_{}", lower_name);

	writeln!(struct_, "\t{arena_name}: Arena<{ty}, {id}>,").unwrap();

	(lower_name, arena_name)
}

fn generate_id(file: &mut File, name: &str, ty: &str, arena: &str) -> std::io::Result<()> {
	writeln!(file, "define_arena_key!({name});")?;

	writeln!(file, "impl IdFuncs<{name}> for Db {{")?;

	writeln!(file, "\ttype Object = {ty};")?;

	writeln!(file, "\t#[inline(always)]")?;
	writeln!(file, "\tfn get(&self, id: {name}) -> &{ty} {{")?;
	writeln!(file, "\t\tself.arenas.{arena}.get(id)")?;
	writeln!(file, "\t}}")?;

	writeln!(file, "\t#[inline(always)]")?;
	writeln!(file, "\tfn get_mut(&mut self, id: {name}) -> &mut {ty} {{")?;
	writeln!(file, "\t\tself.arenas.{arena}.get_mut(id)")?;
	writeln!(file, "\t}}")?;

	writeln!(file, "\t#[inline(always)]")?;	
	writeln!(file, "\tfn push(&mut self, item: {ty}) -> {name} {{")?;
	writeln!(file, "\t\tself.arenas.{arena}.push(item)")?;
	writeln!(file, "\t}}")?;

	writeln!(file, "}}")?;

	Ok(())
}

fn generate_impl(file: &mut File, pairs: &Vec<(&str, &str)>) {
	let mut init = String::new();
	let mut struct_ = String::new();
	let mut db_impl = String::new();
	writeln!(file, "use poni_arena::Arena;").unwrap();
	writeln!(file, "pub type IdType = {TY};").unwrap();
	writeln!(init, "impl DbArenas {{").unwrap();
	writeln!(init, "\tpub fn new() -> Self {{").unwrap();
	writeln!(init, "\t\treturn DbArenas {{").unwrap();

	writeln!(db_impl, "impl Db {{").unwrap();

	writeln!(struct_, "struct DbArenas {{").unwrap();
	for pair in pairs {
		let (lower, arena) = generate_struct(&mut struct_, pair.0, pair.1);
		generate_id(file, pair.0, pair.1, &arena).unwrap();

		writeln!(init, "\t\t\t{arena}: Arena::new(),").unwrap();

		// We don't care if the iterator for a particular type goes unused.
		writeln!(db_impl, "\t#[allow(unused)]").unwrap();
		writeln!(db_impl, "\t#[inline(always)]").unwrap();
		writeln!(db_impl, "\tpub fn iter_{lower}(&self) -> ArenaIterator<{}, {}> {{ self.arenas.{arena}.iter() }}", pair.1, pair.0).unwrap();
	}
	writeln!(struct_, "}}").unwrap();

	writeln!(db_impl, "}}").unwrap();

	writeln!(init, "\t\t}}").unwrap();
	writeln!(init, "\t}}").unwrap();
	writeln!(init, "}}").unwrap();

	writeln!(file, "{}", struct_).unwrap();
	writeln!(file, "{}", init).unwrap();
	writeln!(file, "{}", db_impl).unwrap();
}

pub fn generate(db_file: &mut File) {
	// Modify this array to add new Id types
	let pairs = vec![
		("StrId", "&'static str"),
		("StrConstId", "&'static str"),
		("VarId", "Var"),
		("FunId", "Fun"),
		("TypId", "Type"),
		("SigId", "Sig"),
		("ClassId", "Class"),
		("ClosureId", "Closure"),
	];

	generate_impl(db_file, &pairs);
}