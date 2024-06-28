use std::fs::File;
use std::io::Write as _;
use std::fmt::Write as _;

static TY: &str = "u32";

fn generate_struct(struct_: &mut String, id: &str, ty: &str) -> String {
	// Chop off the "id" part then convert to lowercase
	let new_name = id[..id.len() - 2].to_ascii_lowercase();
	
	// Finally, format to create the whole name. TODO: Can we format strings
	// straight into lowercase? It doesn't seem like it.
	let new_name = format!("arena_{}", new_name);

	writeln!(struct_, "\t{new_name}: Vec<{ty}>,").unwrap();

	new_name
}

fn generate_id(file: &mut File, name: &str, ty: &str, arena: &str) -> std::io::Result<()> {
	writeln!(file, "#[derive(Clone, Copy, PartialEq, Eq, Hash)]")?;
	writeln!(file, "pub struct {name}({TY});\n")?;
	writeln!(file, "impl {name} {{")?;
	writeln!(file, "\tpub fn to_usize(self) -> usize {{ self.0 as usize }}")?;
	//writeln!(file, "\tpub fn from_usize(v: usize) -> Self {{ {name}(v as {TY}) }}")?;
	writeln!(file, "}}\n")?;

	writeln!(file, "impl IdFuncs<{name}, {ty}> for Db {{")?;

	writeln!(file, "\tfn get(&self, id: {name}) -> &{ty} {{")?;
	writeln!(file, "\t\tunsafe {{ self.arenas.{arena}.get_unchecked(id.to_usize()) }} ")?;
	writeln!(file, "\t}}")?;

	writeln!(file, "\tfn get_mut(&mut self, id: {name}) -> &mut {ty} {{")?;
	writeln!(file, "\t\tunsafe {{ self.arenas.{arena}.get_unchecked_mut(id.to_usize()) }} ")?;
	writeln!(file, "\t}}")?;

	writeln!(file, "\tfn new_id(&mut self, item: {ty}) -> {name} {{")?;
	writeln!(file, "\t\tlet index = self.arenas.{arena}.len() as {TY};")?;
	writeln!(file, "\t\tself.arenas.{arena}.push(item);")?;
	writeln!(file, "\t\t{name}(index)")?;
	writeln!(file, "\t}}")?;

	writeln!(file, "}}")?;

	Ok(())
}

fn generate_impl(file: &mut File, pairs: &Vec<(&str, &str)>) {
	let mut init = String::new();
	let mut struct_ = String::new();
	writeln!(file, "pub type IdType = {TY};").unwrap();
	writeln!(init, "impl DbArenas {{").unwrap();
	writeln!(init, "\tpub fn new() -> Self {{").unwrap();
	writeln!(init, "\t\treturn DbArenas {{").unwrap();

	writeln!(struct_, "struct DbArenas {{").unwrap();
	for pair in pairs {
		let arena = generate_struct(&mut struct_, pair.0, pair.1);
		generate_id(file, pair.0, pair.1, &arena).unwrap();

		writeln!(init, "\t\t\t{arena}: Vec::new(),").unwrap();
	}
	writeln!(struct_, "}}").unwrap();

	writeln!(init, "\t\t}}").unwrap();
	writeln!(init, "\t}}").unwrap();
	writeln!(init, "}}").unwrap();

	writeln!(file, "{}", struct_).unwrap();
	writeln!(file, "{}", init).unwrap();
}

pub fn generate(db_file: &mut File) {
	// Modify this array to add new Id types
	let pairs = vec![
		("StrId", "&'static str"),
		("VarId", "Var"),
		("FunId", "Fun"),
		("TypId", "Type"),
		("SourceId", "Source"),
	];

	generate_impl(db_file, &pairs);
}