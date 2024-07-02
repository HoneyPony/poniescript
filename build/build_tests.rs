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

fn generate_test(file: &mut File, test_name: &str, expect: Expected) -> std::io::Result<()> {
	writeln!(file, "#[test]")?;
	writeln!(file, "fn {test_name}() {{")?;
	writeln!(file, "\trun_integration_test(\"tests/poni/{test_name}.poni\", {expect});")?;
	writeln!(file, "}}")?;

	Ok(())
}

pub fn generate(tests_file: &mut File) {
	let tests = [
		("test_print", Expected::Output("345\n345\n45\n5\n"))
	];

	for (test, expect) in tests {
		generate_test(tests_file, test, expect).unwrap();
	}
}