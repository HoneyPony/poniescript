use std::fs::File;
use std::io::Write as _;
use std::fmt::Write as _;

fn generate_struct(struct_file: &mut File, id: &str, ty: &str) -> String {
	// Chop off the "id" part then convert to lowercase
	let new_name = id[..id.len() - 2].to_ascii_lowercase();
	
	// Finally, format to create the whole name. TODO: Can we format strings
	// straight into lowercase? It doesn't seem like it.
	let new_name = format!("arena_{}", new_name);

	writeln!(struct_file, "\t{new_name}: Vec<{ty}>,").unwrap();

	new_name
}

fn generate_id(code_file: &mut File, name: &str, ty: &str, arena: &str) -> std::io::Result<()> {
	writeln!(code_file, "#[derive(Clone, Copy, PartialEq, Eq, Hash)]")?;
	writeln!(code_file, "pub struct {name}(usize);\n")?;

	writeln!(code_file, "impl IdFuncs<{name}, {ty}> for Db {{")?;

	writeln!(code_file, "\tfn get(&self, id: {name}) -> &{ty} {{")?;
	writeln!(code_file, "\t\tunsafe {{ self.arenas.{arena}.get_unchecked(id.0) }} ")?;
	writeln!(code_file, "\t}}")?;

	writeln!(code_file, "\tfn get_mut(&mut self, id: {name}) -> &mut {ty} {{")?;
	writeln!(code_file, "\t\tunsafe {{ self.arenas.{arena}.get_unchecked_mut(id.0) }} ")?;
	writeln!(code_file, "\t}}")?;

	writeln!(code_file, "\tfn new_id(&mut self, item: {ty}) -> {name} {{")?;
	writeln!(code_file, "\t\tlet index = self.arenas.{arena}.len();")?;
	writeln!(code_file, "\t\tself.arenas.{arena}.push(item);")?;
	writeln!(code_file, "\t\t{name}(index)")?;
	writeln!(code_file, "\t}}")?;

	writeln!(code_file, "}}")?;

	Ok(())
}

fn generate_impl(code_file: &mut File, struct_file: &mut File, pairs: &Vec<(&str, &str)>) {
	let mut init = String::new();
	writeln!(init, "impl DbArenas {{").unwrap();
	writeln!(init, "\tpub fn new() -> Self {{").unwrap();
	writeln!(init, "\t\treturn DbArenas {{").unwrap();

	writeln!(struct_file, "struct DbArenas {{").unwrap();
	for pair in pairs {
		let arena = generate_struct(struct_file, pair.0, pair.1);
		generate_id(code_file, pair.0, pair.1, &arena).unwrap();

		writeln!(init, "\t\t\t{arena}: Vec::new(),").unwrap();
	}
	writeln!(struct_file, "}}").unwrap();

	writeln!(init, "\t\t}}").unwrap();
	writeln!(init, "\t}}").unwrap();
	writeln!(init, "}}").unwrap();

	writeln!(code_file, "{}", init).unwrap();
}

pub fn generate(code_file: &mut File, struct_file: &mut File) {
	// Modify this array to add new Id types
	let pairs = vec![
		("StrId", "Box<str>"),
		("VarId", "Var"),
		("TypId", "Typ"),
		("SourceId", "Source"),
	];

	generate_impl(code_file, struct_file, &pairs);
}