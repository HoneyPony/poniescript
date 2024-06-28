use std::fs::File;
use std::io::Write as _;

fn generate_test(file: &mut File, test_name: &str) -> std::io::Result<()> {
	writeln!(file, "#[test]")?;
	writeln!(file, "fn {test_name}() {{")?;
	writeln!(file, "\trun_integration_test(\"{test_name}\");")?;
	writeln!(file, "}}")?;

	Ok(())
}

pub fn generate(tests_file: &mut File) {
	let tests = [
		"test_assign_typecheck",
	];

	for test in tests {
		generate_test(tests_file, test);
	}
}