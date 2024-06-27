mod db;
mod expr;
mod typ;
mod source;
mod lexer;
mod module;
mod parser;
mod typecheck;
mod codegen;

use std::env;
use std::path::Path;
use std::process::exit;

use module::Module;

fn parse_all_modules(db: &mut db::Db, input_paths: Vec<&Path>) -> (Vec<Module>, bool) {
	let mut modules = vec![];

	let mut had_error = false;

	for path in input_paths {
		match module::parse_module(db, path) {
			Ok(module) => { modules.push(module) },
			Err(err) => {
				eprintln!("Unable to parse source file {}: {err}", path.display());
				had_error = true;
			}
		}
	}

	(modules, had_error)
}

fn main() {
	let mut db = db::Db::new();

	let args: Vec<String> = std::env::args().collect();

	let mut input_paths: Vec<&Path> = Vec::new();

	for arg in &args[1..] {
		input_paths.push(arg.as_ref());
	}
	
	if input_paths.is_empty() {
		eprintln!("Need at least one input file");
		exit(1);
	}

	// Pass 1: Parse
	let (mut modules, had_error) = parse_all_modules(&mut db, input_paths);

	if had_error { exit(1); }

	// Pass 2: Type check and infer
	let had_error = typecheck::typecheck(&mut db, &mut modules);

	if had_error { exit(2); }

	// Pass 3: Codegen
	// Generate any caches that require type checking info.
	db.generate_fun_cparams_cache();
	codegen::codegen(&mut db, &modules);

	// Temporary: Print out the type of every var.
	//for var in db.var_range() {
	//	println!("Type of {} -> {}", db.err_var(var), db.err_var_type(var));
	//}
}
