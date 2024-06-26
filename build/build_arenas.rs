use std::fs::File;
use std::io::Write as _;
use std::fmt::Write as _;

fn generate_struct(struct_: &mut String, id: &str, ty: &str) -> String {
	// Chop off the "id" part then convert to lowercase
	let new_name = id[..id.len() - 2].to_ascii_lowercase();
	
	// Finally, format to create the whole name. TODO: Can we format strings
	// straight into lowercase? It doesn't seem like it.
	let new_name = format!("arena_{}", new_name);

	writeln!(struct_, "\t{new_name}: Vec<{ty}>,").unwrap();

	new_name
}

fn generate_id(file: &mut File, name: &str, ty: &str, arena: &str, parent_struct: &str) -> std::io::Result<()> {
	writeln!(file, "#[derive(Clone, Copy, PartialEq, Eq, Hash)]")?;
	writeln!(file, "pub struct {name}(usize);\n")?;

	writeln!(file, "impl IdFuncs<{name}, {ty}> for {parent_struct} {{")?;

	writeln!(file, "\tfn get(&self, id: {name}) -> &{ty} {{")?;
	writeln!(file, "\t\tunsafe {{ self.arenas.{arena}.get_unchecked(id.0) }} ")?;
	writeln!(file, "\t}}")?;

	writeln!(file, "\tfn get_mut(&mut self, id: {name}) -> &mut {ty} {{")?;
	writeln!(file, "\t\tunsafe {{ self.arenas.{arena}.get_unchecked_mut(id.0) }} ")?;
	writeln!(file, "\t}}")?;

	writeln!(file, "\tfn new_id(&mut self, item: {ty}) -> {name} {{")?;
	writeln!(file, "\t\tlet index = self.arenas.{arena}.len();")?;
	writeln!(file, "\t\tself.arenas.{arena}.push(item);")?;
	writeln!(file, "\t\t{name}(index)")?;
	writeln!(file, "\t}}")?;

	writeln!(file, "}}")?;

	Ok(())
}

fn generate_impl(file: &mut File, ty_name: &str, pairs: &Vec<(&str, &str)>) {
	let mut init = String::new();
	let mut struct_ = String::new();
	writeln!(init, "impl {ty_name}Arenas {{").unwrap();
	writeln!(init, "\tpub fn new() -> Self {{").unwrap();
	writeln!(init, "\t\treturn {ty_name}Arenas {{").unwrap();

	writeln!(struct_, "struct {ty_name}Arenas {{").unwrap();
	for pair in pairs {
		let arena = generate_struct(&mut struct_, pair.0, pair.1);
		generate_id(file, pair.0, pair.1, &arena, ty_name).unwrap();

		writeln!(init, "\t\t\t{arena}: Vec::new(),").unwrap();
	}
	writeln!(struct_, "}}").unwrap();

	writeln!(init, "\t\t}}").unwrap();
	writeln!(init, "\t}}").unwrap();
	writeln!(init, "}}").unwrap();

	writeln!(file, "{}", struct_).unwrap();
	writeln!(file, "{}", init).unwrap();
}

pub fn generate(db_file: &mut File, module_file: &mut File) {
	// Modify this array to add new Id types
	let db_pairs = vec![
		("StrId", "&'static str"),
		("VarId", "Var"),
		("TypId", "Type"),
		("SourceId", "Source"),
	];

	let module_pairs = vec![
		("ExprId", "Expr"),
		("StmtId", "Stmt"),
	];

	generate_impl(db_file, "Db", &db_pairs);
	generate_impl(module_file, "Module", &module_pairs);
}