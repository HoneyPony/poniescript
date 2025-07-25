#[macro_use]
mod arena;
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
mod binder;
mod dead_code;
mod init_ordering;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{exit, Child, Command, Stdio};
use std::time::{Duration, SystemTime};

use db::*;
use module::Module;
use clap::Parser as _;

use crate::db::Ast;

#[derive(clap::Parser)]
struct Args {
	#[arg(short = 'o', long = "output")]
	/// Where the output file should be written.
	output_path: PathBuf,

	#[arg(long = "no-timing")]
	/// Whether to hide the timing information.
	no_timing: bool,

	#[arg(short = 'c', long = "compiler")]
	/// The C compiler to use (if generating an executable or object file).
	/// Expects gcc-style arguments.
	c_compiler: Option<String>,

	#[arg(required = true)]
	/// The list of input files to compile into one .C file or executable.
	input_paths: Vec<PathBuf>,

	#[arg(long = "test")]
	/// Whether to run the PonieScript in "test mode," a special mode used
	/// for integration testing the compiler.
	test_mode: bool,

	#[arg(short = 'e', long = "engine")]
	/// Compile this code for integration into the PonyGame engine. In particular,
	/// don't include poni_standalone.h.
	engine: bool
}

enum CompileMode {
	ToCFile,
	ToExeFile,
	ToObjectFile,
	ToSharedLibary,
	// Will be useful if we can do hot code reloading
	// ToSharedLibrary,
}

impl CompileMode {
	pub fn parse(output_path: &PathBuf) -> CompileMode {
		// By default, return ToExeFile. This corresponds to, for example,
		// -o my_program (which on Linux would suggest an executable)
		let Some(extension) = output_path.extension() else {
			return CompileMode::ToExeFile;
		};

		if extension == "c" { return CompileMode::ToCFile; }
		if extension == "exe" { return CompileMode::ToExeFile; }
		if extension == "o" { return CompileMode::ToObjectFile; }
		if extension == "dll" { return CompileMode::ToSharedLibary; }
		if extension == "so" { return CompileMode::ToSharedLibary; }

		// Any other extension, we'll happily do Exe, but also print a warning.
		eprintln!("warning: unknown output file extension '.{}' -- generating executable.", extension.to_string_lossy());
		return CompileMode::ToExeFile;
	}

	pub fn get_output(&self, args: &Args) -> (Box<dyn std::io::Write>, Option<Child>) {
		match self {
			CompileMode::ToCFile => {
				// Don't let us run in test mode if we're trying to output a C
				// file.
				if args.test_mode {
					eprintln!("Error: Running in test mode, but output is a C file.");
					exit(10);
				}

				match File::create(&args.output_path) {
					Ok(f) => (Box::new(f), None),
					Err(err) => {
						eprintln!("Unable to create output file {}: {}", args.output_path.display(), err);
						exit(3);
					}
				}
			},
			CompileMode::ToExeFile | CompileMode::ToObjectFile | CompileMode::ToSharedLibary => {
				// TODO: Why is as_deref giving &str?? As long as it works...
				let compiler = args.c_compiler.as_deref().unwrap_or("gcc");
				let mut cc = Command::new(compiler);
				let mut cc = cc.stdin(Stdio::piped());

				// .arg("-std=c11") // TODO: Do we want this? It seems tcc does not support it.

				match self {
					CompileMode::ToObjectFile => {
						cc = cc.arg("-c");
					},
					CompileMode::ToSharedLibary => {
						cc = cc.arg("-shared").arg("-fPIC");
					}
					_ => {}
				};
					
				let cc = cc.arg("-o")
					.arg(&args.output_path)
					.arg("-I.")
					.arg("-x")
					.arg("c")
					.arg("-")
					.spawn();
				let mut cc = match cc {
					Ok(cc) => cc,
					Err(err) => {
						eprintln!("Unable to spawn C compiler: {}", err);
						exit(4);
					}
				};

				match cc.stdin.take() {
					Some(stdin) => (Box::new(stdin), Some(cc)),
					None => {
						eprintln!("Unable to feed C compiler with input");
						exit(5);
					}
				}
			},
		}
	}
}

fn parse_all_modules(ast: &mut Ast, db: &mut db::Db, args: &Args) -> (Vec<Module>, bool) {
	let mut modules = vec![];

	let mut had_error = false;

	for path in &args.input_paths {
		match module::parse_module(ast, db, &path) {
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
	if db.test_mode && !db.test_errors.is_empty() {
		if db.errors.len() != db.test_errors.len() {
			eprintln!("Test failure: Wrong number of errors.");
			exit(15);
		}

		// For now, we simply expect every single error that the compiler generates.
		// This does mean tests are brittle, but on the other hand... we don't update
		// the error messages that often. And for the most part, changes to the error
		// messages shouldn't require huge numbers of test updates (or where they do,
		// the updates should be somewhat regexable).
		for i in 0..db.errors.len() {
			// TODO:
			// For some reason, the err_fun_missing_brace test is failing even though
			// the strings absolutely appear to be the same. Very strange...
			let want_str = &db.test_errors[i];//.trim();
			let got_str = &db.errors[i].main_message;//.trim();
			if want_str != got_str {
				eprintln!("Test failure: Error message mismatch (index {i}):\nExpected: [{}]\nGot:      [{}]",
					want_str, got_str);
				
				let mut j = 0;
				for (a, b) in want_str.chars().zip(got_str.chars()) {
					if a != b {
						eprintln!("Mismatch at index {j}: {a} vs {b}")
					}
					j += 1;
				}
				exit(15);
			}
		}

		// The test was successful.
		exit(0);
	}
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
	let mut ast = db::Ast::new();
	db.test_mode = args.test_mode;

	// Pass 1: Parse
	let (mut modules, had_error) = parse_all_modules(&mut ast, &mut db, &args);

	if had_error {
		report_errors(&db);
		exit(1);
	}

	let timer = duration(timer, "parsing", &mut duration_set);

	// Pass 2: Binding
	let had_error = binder::bind(&mut db, &mut ast, &mut modules);

	if had_error {
		report_errors(&db);
		exit(2);
	}

	let timer = duration(timer, "binding", &mut duration_set);

	// Pass 3: Initialization orders. Fix initialization order of various things,
	// including globals.
	//
	// This must come before type check, otherwise the type checker won't be
	// able to figure out the types of certain global patterns (e.g. cyclic/globals_same)
	//
	// It is also valid for it to come after binding, as after that, all the variables
	// are essentially lexically bound, and we don't actually care about type
	// information during the sorting stage.
	// TODO: Is there a way to make this pattern cleaner..?
	let mut globals = std::mem::take(&mut db.globals);
	init_ordering::topological_sort(&mut globals, &ast, &mut db);

	for class in db.iter_class() {
		let mut vars = std::mem::take(&mut db.get_mut(class).vars);

		init_ordering::topological_sort(&mut vars, &ast, &mut db);

		db.get_mut(class).vars = vars;
	}
	db.globals = globals;

	if !db.errors.is_empty() {
		report_errors(&db);
		exit(3);
	}

	let timer = duration(timer, "initializer sort", &mut duration_set);

	// Pass 4: Type check and infer
	let had_error = typecheck::typecheck(&mut db, &ast, &mut modules);

	if had_error {
		report_errors(&db);
		exit(4);
	}

	let timer = duration(timer, "type check", &mut duration_set);

	dead_code::eliminate_dead_code(&mut db, &mut ast, &mut modules);

	let timer = duration(timer, "dead code", &mut duration_set);

	// Pass 5: Codegen
	// Generate any caches that require type checking info.
	db.generate_codegen_caches();

	let compile_mode = CompileMode::parse(&args.output_path);
	let (mut output, cc) = compile_mode.get_output(&args);

	if let Err(err) = codegen::codegen(&args, &mut db, &mut ast, &modules, &mut output) {
		eprintln!("Unable to write output file: {err}");
		exit(5);
	}

	// Wait for the C compiler and exit with an error if it failed.
	if let Some(mut cc) = cc {
		drop(output);

		match cc.wait() {
			Ok(status) => {
				if !status.success() {
					eprintln!("Internal compiler error. C compiler failed.");
					exit(12);
				}
			},
			Err(err) => {
				eprintln!("C compiler IO error: {}", err);
			},
		}
	}

	duration(timer, "codegen (to c)", &mut duration_set);
	duration(timer_begin, "total compile time", &mut duration_set);

	if !args.no_timing {
		for info in duration_set {
			eprintln!("{}", info);
		}
	}

	// If we're in test mode, then we want to run the program and check its
	// output.
	if args.test_mode {
		if !db.test_errors.is_empty() {
			// In this case, we actually have an error: we expected the compilation
			// to result in an error, but it didn't. So, report that to the test
			// runner.
			eprintln!("Test failure: Expected an error, but compilation suceeded.");
			exit(15);
		}
		// We've already checked the output is an Exe, so just run it at
		// that path.
		match test_compiled(&args.output_path, &db) {
			Ok(_) => { eprintln!("Test succeeded"); },
			Err(err) => { 
				eprintln!("Test encountered IO error: {err}");
				exit(11);
			},
		}
	}
}

fn test_compiled(exe_path: &Path, db: &Db) -> std::io::Result<()> {
	// TODO: We may have a problem if we e.g. get an exe file on Windows.
	// But, so far it seems to work as expected.
	let mut testprog = Command::new(exe_path)
		.stdout(Stdio::piped())
		.spawn()?;

	let mut stdout = testprog.stdout.take().expect("stdout");
	testprog.wait()?;

	let mut got = String::new();
	stdout.read_to_string(&mut got)?;

	// Check every line.
	let mut idx = 0;
	for line in got.lines() {
		if idx >= db.test_lines.len() {
			eprintln!("Test failure: Too many output lines");
			exit(15);
		}
		assert_eq!(line, db.test_lines[idx]);
		idx += 1;
	}

	if idx != db.test_lines.len() {
		eprintln!("Test failure: Too few output lines");
		exit(15);
	}

	Ok(())
}