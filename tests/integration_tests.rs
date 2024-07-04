include!(concat!(env!("OUT_DIR"), "/tests.gen.rs"));

use std::{io::Read, path::PathBuf, process::{Command, Stdio}};

const CC: Option<&str> = option_env!("PONI_CC");

enum Expected {
	Output(&'static str),
	Error
}

fn test_should_print(input_file: &str, c_path: &str, exe_path: &str, output: &str) -> std::io::Result<()> {
	let _ = std::fs::remove_file(c_path);
	let _ = std::fs::remove_file(exe_path);

	// Additionally remove the exe path extended with ".exe", as on Windows
	// gcc will output the executable file to this path instead.
	let exe_path_exe = PathBuf::from(exe_path).with_extension("exe");
	let _ = std::fs::remove_file(exe_path_exe);
	
	let mut poniescript = Command::new(env!("CARGO_BIN_EXE_poniescript"))
		.arg("-o")
		.arg(c_path)
		.arg(input_file)
		.arg("--no-timing")
		.spawn()?;
	let code = poniescript.wait()?;
	if !code.success() {
		panic!("poniescript should be able to compile this");
	}

	let mut cc = Command::new(CC.unwrap_or("gcc"))
		.arg("-std=c11")
		.arg(c_path)
		.arg("-o")
		.arg(exe_path)
		.arg("-I")
		.arg(".")
		.spawn()?;
	let code = cc.wait()?;
	if !code.success() {
		panic!("the c compiler should be able to compile this");
	}

	// Finally, actually test the program.
	let mut testprog = Command::new(exe_path)
		.stdout(Stdio::piped())
		.spawn()?;

	let mut stdout = testprog.stdout.take().expect("stdout");
	testprog.wait()?;

	let mut got = String::new();
	stdout.read_to_string(&mut got)?;

	// Unfortunately, we have to strip '\r' from the output. This does mean we
	// can't compare strings with '\r' in our tests, at least for now.
	let mut got_real = String::new();
	got_real.reserve(got.len());
	for c in got.chars() {
		if c != '\r' { got_real.push(c); }
	}

	assert_eq!(output, got_real);

	Ok(())
}

fn run_integration_test(input_file: &str, c_path: &str, exe_path: &str, expect: Expected) {
	// TODO:
	// - Use CARGO_TARGET_TMPDIR to store compiled programs
	// - Have the test programs report the expected value/error
	match expect {
		Expected::Output(output) => 
			assert!(test_should_print(input_file, c_path, exe_path, output).is_ok()),
		Expected::Error => todo!(),
	}
}