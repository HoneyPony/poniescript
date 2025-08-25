#[macro_use]
pub mod arena;
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
pub struct Args {
	#[arg(short = 'o', long = "output")]
	/// Where the output file should be written.
	pub output_path: PathBuf,

	#[arg(long = "no-timing")]
	/// Whether to hide the timing information.
	pub no_timing: bool,

	#[arg(short = 'c', long = "compiler")]
	/// The C compiler to use (if generating an executable or object file).
	/// Expects gcc-style arguments.
	pub c_compiler: Option<String>,

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
}