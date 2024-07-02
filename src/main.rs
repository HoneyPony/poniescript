mod db;
mod expr;
mod typ;
mod source;
mod lexer;
mod module;
mod parser;
mod typecheck;
mod codegen;
mod error;

use std::fs::File;
use std::path::{PathBuf};
use std::process::exit;
use std::time::{Duration, SystemTime};

use db::Db;
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

struct TimeHelper {
	duration: Duration
}

impl std::fmt::Display for TimeHelper {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		if self.duration.as_millis() < 10 {
			write!(f, "{}us", self.duration.as_micros())
		}
		else {
			write!(f, "{}ms", self.duration.as_millis())
		}
	}
}

fn duration(prev: SystemTime, message: &str) -> SystemTime {
	let now = SystemTime::now();

	if let Ok(duration) = now.duration_since(prev) {
		eprintln!("{} took {}",
			message, TimeHelper { duration });
	}

	now
}

fn report_errors(db: &Db) {
	for error in &db.errors {
		crate::error::show_error(&error, db);
	}
}

fn main() {
	let timer = SystemTime::now();

	// Use Clap to parse arguments
	let args = Args::parse();

	let mut db = db::Db::new();

	// Pass 1: Parse
	let (mut modules, had_error) = parse_all_modules(&mut db, &args);

	if had_error {
		report_errors(&db);
		exit(1);
	}

	let timer = duration(timer, "poni: parsing");

	// Pass 2: Type check and infer
	let had_error = typecheck::typecheck(&mut db, &mut modules);

	if had_error {
		report_errors(&db);
		exit(2);
	}

	let timer = duration(timer, "poni: type check");

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

	duration(timer, "poni: codegen (to c)");
}
