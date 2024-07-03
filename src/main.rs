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
	/// Where the output file should be written.
	output_path: PathBuf,

	#[arg(long = "no-timing")]
	/// Whether to hide the timing information.
	no_timing: bool,

	#[arg(required = true)]
	/// The list of input files to compile into one .C file or executable.
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
			write!(f, "{}ms", self.duration.as_micros() as f64 / 1000.0)
		}
		else {
			write!(f, "{}ms", self.duration.as_millis())
		}
	}
}

fn duration(prev: SystemTime, message: &str, duration_set: &mut Vec<&'static str>) -> SystemTime {
	let now = SystemTime::now();

	if let Ok(duration) = now.duration_since(prev) {
		// NOTE: Keep padding in sync with longest message
		let info = format!("{:<18} = {}",
			message, TimeHelper { duration }).leak();
		duration_set.push(info);
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
	let timer_begin = SystemTime::now();
	let mut duration_set = Vec::new();

	// Use Clap to parse arguments
	let args = Args::parse();

	let mut db = db::Db::new();

	// Pass 1: Parse
	let (mut modules, had_error) = parse_all_modules(&mut db, &args);

	if had_error {
		report_errors(&db);
		exit(1);
	}

	let timer = duration(timer, "parsing", &mut duration_set);

	// Pass 2: Type check and infer
	let had_error = typecheck::typecheck(&mut db, &mut modules);

	if had_error {
		report_errors(&db);
		exit(2);
	}

	let timer = duration(timer, "type check", &mut duration_set);

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

	duration(timer, "codegen (to c)", &mut duration_set);
	duration(timer_begin, "total compile time", &mut duration_set);

	if !args.no_timing {
		for info in duration_set {
			eprintln!("{}", info);
		}
	}
}
