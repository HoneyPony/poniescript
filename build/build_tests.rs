use std::fs::File;
use std::io::Write as _;

enum Expected {
	Output(&'static str),
	Error
}

impl std::fmt::Display for Expected {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Expected::Output(output) => write!(f, "Expected::Output(\"{output}\")"),
			Expected::Error => write!(f, "Expected::Error"),
		}
	}
}

fn generate_test(file: &mut File, path: &str, test_name: &str, expect: Expected) -> std::io::Result<()> {
	writeln!(file, "#[test]")?;
	writeln!(file, "fn {test_name}() {{")?;

	// We let the generated test code actually do the concat!.
	let c_path = format!("concat!(env!(\"CARGO_TARGET_TMPDIR\"), \"/{test_name}.c\")");
	let exe_path = format!("concat!(env!(\"CARGO_TARGET_TMPDIR\"), \"/{test_name}\")");

	writeln!(file, "\trun_integration_test(\"tests/poni/{path}{test_name}.poni\", {c_path}, {exe_path}, {expect});")?;
	writeln!(file, "}}")?;

	Ok(())
}

pub fn generate(tests_file: &mut File) {
	let tests = [
		("print/", "print_nested"                 , Expected::Output("345\n345\n45\n5\n")),
		("print/", "print_string_literal"         , Expected::Output("hello world\n")),
		("print/", "print_multiple_string_literal", Expected::Output("hello world\n")),
		("print/", "print_number_literal"         , Expected::Output("3\n")),

		("globals/", "global_block", Expected::Output("7\n")),
		// TODO: Figure out precise float output format we want.
		("globals/", "globals_3"   , Expected::Output("3\n4.000000\n10\n")),
	];

	for (path, test, expect) in tests {
		generate_test(tests_file, path, test, expect).unwrap();
	}
}