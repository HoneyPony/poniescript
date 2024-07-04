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

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{exit, Child, Command, ExitStatus, Stdio};
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
}

enum CompileMode {
	ToCFile,
	ToExeFile,
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
		if extension == "o" { todo!("outputting to .o files"); }
		if extension == "dll" { todo!("outputting to .dll files"); }
		if extension == "so" { todo!("outputting to .so files"); }

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
			CompileMode::ToExeFile => {
				// TODO: Why is as_deref giving &str?? As long as it works...
				let compiler = args.c_compiler.as_deref().unwrap_or("gcc");
				let cc = Command::new(compiler)
					.stdin(Stdio::piped())
					// .arg("-std=c11") // TODO: Do we want this? It seems tcc does not support it.
					.arg("-o")
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
	db.test_mode = args.test_mode;

	// Pass 1: Parse
	let (mut modules, had_error) = parse_all_modules(&mut db, &args);

	if had_error {
		report_errors(&db);
		exit(1);
	}

	let timer = duration(timer, "parsing", &mut duration_set);

	// Pass 2: Binding
	let had_error = binder::bind(&mut db, &mut modules);

	if had_error {
		report_errors(&db);
		exit(1);
	}

	let timer = duration(timer, "binding", &mut duration_set);

	// Pass 3: Type check and infer
	let had_error = typecheck::typecheck(&mut db, &mut modules);

	if had_error {
		report_errors(&db);
		exit(2);
	}

	let timer = duration(timer, "type check", &mut duration_set);

	// Pass 4: Codegen
	// Generate any caches that require type checking info.
	db.generate_codegen_caches();

	let compile_mode = CompileMode::parse(&args.output_path);
	let (mut output, cc) = compile_mode.get_output(&args);

	if let Err(err) = codegen::codegen(&mut db, &modules, &mut output) {
		eprintln!("Unable to write output file: {err}");
		exit(4);
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
		assert_eq!(line, db.test_lines[idx]);
		idx += 1;
	}

	Ok(())
}