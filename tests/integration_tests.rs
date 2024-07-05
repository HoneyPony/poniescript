include!(concat!(env!("OUT_DIR"), "/tests.gen.rs"));

use std::{io::Read, path::PathBuf, process::{Command, Stdio}};

const CC: Option<&str> = option_env!("PONI_CC");

fn test_should_print(input_file: &str, exe_path: &str) -> std::io::Result<()> {
	let _ = std::fs::remove_file(exe_path);

	// Additionally remove the exe path extended with ".exe", as on Windows
	// gcc will output the executable file to this path instead.
	let exe_path_exe = PathBuf::from(exe_path).with_extension("exe");
	let _ = std::fs::remove_file(exe_path_exe);
	
	let mut poniescript = Command::new(env!("CARGO_BIN_EXE_poniescript"))
		.arg("-o")
		.arg(exe_path)
		.arg("-c")
		.arg(CC.unwrap_or("gcc"))
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