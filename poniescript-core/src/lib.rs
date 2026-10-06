#[macro_use]
pub mod builtins;
pub mod closure_convert;
pub mod db;
pub mod expr;
pub mod typ;
pub mod source;
pub mod lexer;
pub mod module;
pub mod parser;
pub mod typecheck;
pub mod codegen;
pub mod error;
pub mod binder;
pub mod dead_code;
pub mod init_ordering;
pub mod glue;
pub mod pretty_print;

use std::path::PathBuf;



#[derive(clap::Parser, std::default::Default)]
pub struct Args {
	#[arg(short = 'o', long = "output")]
	/// Where the output file should be written.
	pub output_paths: Vec<PathBuf>,

	#[arg(long)]
	/// Optionally, write the compiled prelude code as a header file, before
	/// starting other compilation. This could potentially speed up codegen
	/// in some cases (when using multiple output paths).
	pub prelude_h: Option<PathBuf>,

	#[arg(long = "no-timing")]
	/// Whether to hide the timing information.
	pub no_timing: bool,

	#[arg(short = 'c', long = "compiler")]
	/// The C compiler to use (if generating an executable or object file).
	/// Expects gcc-style arguments.
	pub c_compiler: Option<String>,

	#[arg(short = 'C', long = "c-opt")]
	/// Options to pass along to the C compiler.
	pub c_opt: Vec<String>,

	#[arg(required = true)]
	/// The list of input files to compile into one .C file or executable.
	pub input_paths: Vec<PathBuf>,

	#[arg(long = "test")]
	/// Whether to run the PonieScript in "test mode," a special mode used
	/// for integration testing the compiler.
	pub test_mode: bool,

	#[arg(short = 'e', long = "engine")]
	/// Compile this code for integration into the PonyGame engine. In particular,
	/// don't include poni_standalone.h.
	pub engine: bool,

	#[arg(short = 'i', long = "import")]
	/// List of C header files to read PonieScript declarations from. These
	/// header files will also be #include'd in the final PonieScript C code
	/// generated.
	pub imports: Vec<PathBuf>,

	#[arg(long = "hot")]
	/// Compile this code for hot reload (integration with poni_hot). Not recommended
	/// for release builds.
	pub hot: bool,

	#[arg(long = "check")]
	/// Whether to run the compiler in check mode. In this mode, it does not
	/// output any C code, it merely runs the semantic analysis.
	pub check_mode: bool,

	#[arg(long = "codegen-threads", default_value_t = 0)]
	/// The number of threads to use for codegen. Note that high numbers can
	/// result in a panic.
	pub codegen_threads: usize,

	#[arg(long = "bind-fun")]
	/// Special functions that we expect to have implemented.
	/// 
	/// These are treated specially in two ways: First, we MUST define a global
	/// function with this name, and second, its C name will also be this name
	/// (no f_ prefix), which should mean it's guaranteed to refer to the global
	/// function rather than e.g. a class member function of the same name.
	pub bind_funs: Vec<String>,

	#[arg(long="disable-gc-frames")]
	/// Completely disable the generation of GC frames.
	/// 
	/// This prevents any GC-frame based backtrace for panic messages. However,
	/// it also removes a lot of extra code for shuffling information around for
	/// the GC.
	/// 
	/// This option is only appropriate if EVERY PonieScript thread in your program
	/// will regularly safepoint.
	pub disable_gc_frames: bool,

	#[arg(long = "pretty-print")]
	/// Stop after semantic analysis and pretty-print the AST.
	/// 
	/// TODO: Also support printing it at other stages.
	pub pretty_print: bool,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use poni_arena::IndexCell;

	#[test]
	fn test_line_column() {
		use crate::{db::*, db::Db, db::Ast, module};

		let mut ast = Ast::new();
		let mut db = Db::new(&mut ast);
		

		let path = Path::new("tests/test_line_column.poni");
		let source_id = ast.new_source(path.to_path_buf());
		module::parse_module(&mut ast, &mut db, source_id).unwrap();

		for var in db.iter_var() {
			if db.put_str("hello") == db.get(var).name {
				let loc = db.get(var).location.clone();
				assert_eq!(loc.length, 5);
				assert_eq!(loc.offset, 10);

				let (line, col) = ast.sources.get(loc.source).get_line_column(&loc);
				assert_eq!(line, 3);
				assert_eq!(col, 8);
			}

			if db.put_str("ponies") == db.get(var).name {
				let loc = db.get(var).location.clone();
				assert_eq!(loc.length, 6);
				assert_eq!(loc.offset, 30);

				let (line, col) = ast.sources.get(loc.source).get_line_column(&loc);
				assert_eq!(line, 7);
				assert_eq!(col, 6);
			}
		}
	}
}