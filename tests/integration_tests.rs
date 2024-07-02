include!(concat!(env!("OUT_DIR"), "/tests.gen.rs"));

use std::{io::Read, process::{Child, Command, Stdio}};

enum Expected {
	Output(&'static str),
	Error
}

fn test_should_print(input_file: &str, output: &str) -> std::io::Result<()> {
	let c_path = concat!(env!("CARGO_TARGET_TMPDIR"), "/out.c");
	let exe_path = concat!(env!("CARGO_TARGET_TMPDIR"), "/out");

	let mut poniescript = Command::new("target/debug/poniescript")
		.arg("-o")
		.arg(c_path)
		.arg(input_file)
		.spawn()?;
	poniescript.wait()?;

	let mut cc = Command::new("gcc")
		.arg(c_path)
		.arg("-o")
		.arg(exe_path)
		.arg("-I")
		.arg(".")
		.spawn()?;
	cc.wait()?;

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

fn run_integration_test(input_file: &str, expect: Expected) {
	// TODO:
	// - Use CARGO_TARGET_TMPDIR to store compiled programs
	// - Have the test programs report the expected value/error
	match expect {
		Expected::Output(output) => assert!(test_should_print(input_file, output).is_ok()),
		Expected::Error => todo!(),
	}
}