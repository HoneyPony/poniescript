include!(concat!(env!("OUT_DIR"), "/tests.gen.rs"));

fn run_integration_test(input_file: &str) {
	// TODO:
	// - Use CARGO_TARGET_TMPDIR to store compiled programs
	// - Have the test programs report the expected value/error
	eprintln!("Running integration test {input_file}");
}