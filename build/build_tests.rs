use std::fs::File;
use std::io::Write as _;

fn generate_test(file: &mut File, path: &str, test_name: &str) -> std::io::Result<()> {
	writeln!(file, "#[test]")?;
	writeln!(file, "fn {test_name}() {{")?;

	// We let the generated test code actually do the concat!.
	let exe_path = format!("concat!(env!(\"CARGO_TARGET_TMPDIR\"), \"/{test_name}\")");

	writeln!(file, "\trun_integration_test(\"tests/poni/{path}{test_name}.poni\", {exe_path});")?;
	writeln!(file, "}}")?;

	Ok(())
}

pub fn generate(tests_file: &mut File) {
	let tests = [
		// print_nested: the last line is 345 because it should re-print each of the inner print()s.
		("print/", "print_nested"),
		("print/", "print_string_literal"),
		("print/", "print_multiple_string_literal"),
		("print/", "print_number_literal"),
		("print/", "print_block"),
		("print/", "print_void_first"),
		("print/", "print_void_others"),
		("print/", "print_dif_funs"),
		("print/", "print_inner_return"),

		("globals/", "global_block"),
		// TODO: Figure out precise float output format we want.
		("globals/", "globals_3"),
		("globals/", "global_global"),

		//("typecheck/", "print_assume_int", Expected::Output("10\n")),
		//("typecheck/", "promote_assumes_inside_block", Expected::Output("15\n")),
		//("typecheck/", "promote_to_float_arithmetic", Expected::Output("30.000000\n")),
		//("typecheck/", "promote_to_float_assign", Expected::Output("10.000000\n")),
		//("typecheck/", "str_types", Expected::Output("hello my little ponies\n")),
		//("typecheck/", "return_block_return", Expected::Output("")), // Make sure this one at least compiles
		// TODO: Add other tests when we get function calls
	
		//("string/", "simple_str", Expected::Output("hello world25\n")),
		//("string/", "str_of_strbuf", Expected::Output("hello my little ponies\n")),
	
		("functions/", "parse_params"),
		("functions/", "param_trailing_comma"),
		("functions/", "call_basic"),
		("functions/", "call_nested"),
		("functions/", "call_promote"),
		("functions/", "void_fun"),
		("functions/", "fib"),

		//("scope/", "block_shadow", Expected::Output("hello ponies\nhello again ponies\nhello ponies\n")),

		//("if/", "basic_if_expr_ret"   , Expected::Output("10\n3\n")),
		//("if/", "basic_if_expr_var"   , Expected::Output("3\n10\n")),
		//("if/", "basic_if_expr"       , Expected::Output("10\n3\n")),
		//("if/", "basic_if"            , Expected::Output("true\n")),
		//("if/", "if_no_else"          , Expected::Output("little pony\nmy\nlittle pony\n")),
		// Shows that the plain 'if' never returns a value.
		//("if/", "if_no_else_in_print" , Expected::Output("my little pony\n")),

		("comparison/", "compare_basic"),
		("comparison/", "compare_constants")
	];

	for (path, test) in tests {
		generate_test(tests_file, path, test).unwrap();
	}
}