include!(concat!(env!("OUT_DIR"), "/tests.gen.rs"));

use std::{path::PathBuf, process::{Command}};

const CC: Option<&str> = option_env!("PONI_CC");

fn test_should_print(input_file: &str, exe_path: &str) -> std::io::Result<()> {
	let _ = std::fs::remove_file(exe_path);

	// Additionally remove the exe path extended with ".exe", as on Windows
	// gcc will output the executable file to this path instead.
	let exe_path_exe = PathBuf::from(exe_path).with_extension("exe");
	let _ = std::fs::remove_file(exe_path_exe);

	// We need to link against the poniescript-gc library, written in Rust.
	// For now, assume it is next to the poniescript executable, but it would
	// be nice to make this better.
	//
	// To make sure GCC links against the library and doesn't try to interpret
	// it as a source, use "-L" for the library path and "-l" for the name.
	let bin_dir = PathBuf::from(env!("CARGO_BIN_EXE_poniescript"));
	let bin_dir = bin_dir.parent().unwrap();
	//let gc_lib = bin_dir.join("libponiescript_gc.a");
	
	let mut poniescript = Command::new(env!("CARGO_BIN_EXE_poniescript"))
		.arg("-o")
		.arg(exe_path)
		.arg("-c")
		.arg(CC.unwrap_or("gcc"))

		// To make Clap happy, we have to shove these arguments together in this way.
		.arg(format!("-C-L{}", bin_dir.to_string_lossy()))
		.arg("-C-lponiescript_gc")
		.arg(input_file)
		.arg("--no-timing")
		.arg("--test")
		.spawn()?;
	let code = poniescript.wait()?;

	assert!(code.success());

	Ok(())
}

fn run_integration_test(input_file: &str, exe_path: &str) {
	// TODO:
	// - Use CARGO_TARGET_TMPDIR to store compiled programs
	// - Have the test programs report the expected value/error
	assert!(test_should_print(input_file, exe_path).is_ok())
}

fn do_run_valgrind(input_file: &str, exe_path: &str) -> std::io::Result<()> {
	let _ = std::fs::remove_file(exe_path);

	// Additionally remove the exe path extended with ".exe", as on Windows
	// gcc will output the executable file to this path instead.
	let exe_path_exe = PathBuf::from(exe_path).with_extension("exe");
	let _ = std::fs::remove_file(exe_path_exe);

	// We need to link against the poniescript-gc library, written in Rust.
	// For now, assume it is next to the poniescript executable, but it would
	// be nice to make this better.
	//
	// To make sure GCC links against the library and doesn't try to interpret
	// it as a source, use "-L" for the library path and "-l" for the name.
	let bin_dir = PathBuf::from(env!("CARGO_BIN_EXE_poniescript"));
	let bin_dir = bin_dir.parent().unwrap();
	//let gc_lib = bin_dir.join("libponiescript_gc.a");
	
	let mut poniescript = Command::new(env!("CARGO_BIN_EXE_poniescript"))
		.arg("-o")
		.arg(exe_path)
		.arg("-c")
		.arg(CC.unwrap_or("gcc"))

		// To make Clap happy, we have to shove these arguments together in this way.
		.arg(format!("-C-L{}", bin_dir.to_string_lossy()))
		.arg("-C-lponiescript_gc")
		.arg(input_file)
		.arg("--no-timing")
		// Ensure the garbage collector is cleaning stuff up
		.arg("-C-DPONI_CLEAN_EXIT")
		.spawn()?;
	let code = poniescript.wait()?;

	// Only run valgrind if we successfully compiled the executable.
	if code.success() {
		let mut valgrind = Command::new("valgrind")
			.arg("--error-exitcode=1")
			.arg("--errors-for-leak-kinds=all")
			.arg("--leak-check=full")
			.arg(exe_path)
			.spawn()?;
		let code = valgrind.wait()?;
		assert!(code.success());
	}

	Ok(())
}

fn run_integration_test_valgrind(input_file: &str, exe_path: &str) {
	assert!(do_run_valgrind(input_file, exe_path).is_ok())
}