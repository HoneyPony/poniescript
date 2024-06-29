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
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::exit;

use module::Module;
use clap::Parser as _;

#[derive(clap::Parser)]
struct Args {
	#[arg(short = 'o', long = "output")]
	output_path: PathBuf,

	#[arg(required = true)]
	input_paths: Vec<PathBuf>,
}

fn parse_all_modules(db: &mut db::Db, args: &Args) -> (Vec<Module>, bool) {
	let mut modules = vec![];

	let mut had_error = false;

	for path in &args.input_paths {
		match module::parse_module(db, &path) {
			Ok((module, false)) => { modules.push(module) },
			Ok((_, true)) => {
				had_error = true;
			}
			Err(err) => {
				eprintln!("Unable to parse source file {}: {err}", path.display());
				had_error = true;
			}
		}
	}

	(modules, had_error)
}

fn main() {
	// Use Clap to parse arguments
	let args = Args::parse();

	let mut db = db::Db::new();

	// Pass 1: Parse
	let (mut modules, had_error) = parse_all_modules(&mut db, &args);

	if had_error { exit(1); }

	// Pass 2: Type check and infer
	let had_error = typecheck::typecheck(&mut db, &mut modules);

	if had_error { exit(2); }

	// Pass 3: Codegen
	// Generate any caches that require type checking info.
	db.generate_codegen_caches();

	let mut output = match File::create(&args.output_path) {
		Ok(f) => f,
		Err(err) => {
			eprintln!("Unable to create output file {}: {}", args.output_path.display(), err);
			exit(3);
		}
	};
	if let Err(err) = codegen::codegen(&mut db, &modules, &mut output) {
		eprintln!("Unable to write output file: {err}");
		exit(4);
	}

	// Temporary: Print out the type of every var.
	//for var in db.var_range() {
	//	println!("Type of {} -> {}", db.err_var(var), db.err_var_type(var));
	//}
}
