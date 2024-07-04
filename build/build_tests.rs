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

		//("globals/", "global_block" , Expected::Output("7\n")),
		// TODO: Figure out precise float output format we want.
		//("globals/", "globals_3"    , Expected::Output("3\n4.000000\n10\n")),
		//("globals/", "global_global", Expected::Output("55\n")),

		//("typecheck/", "print_assume_int", Expected::Output("10\n")),
		//("typecheck/", "promote_assumes_inside_block", Expected::Output("15\n")),
		//("typecheck/", "promote_to_float_arithmetic", Expected::Output("30.000000\n")),
		//("typecheck/", "promote_to_float_assign", Expected::Output("10.000000\n")),
		//("typecheck/", "str_types", Expected::Output("hello my little ponies\n")),
		//("typecheck/", "return_block_return", Expected::Output("")), // Make sure this one at least compiles
		// TODO: Add other tests when we get function calls
	
		//("string/", "simple_str", Expected::Output("hello world25\n")),
		//("string/", "str_of_strbuf", Expected::Output("hello my little ponies\n")),
	
		//("functions/", "parse_params", Expected::Output("")),
		//("functions/", "param_trailing_comma", Expected::Output("")),
		//("functions/", "call_basic", Expected::Output("3\n7\n11\n")),
		//("functions/", "call_nested", Expected::Output("my little pony!\n")),
		//("functions/", "call_promote", Expected::Output("1.000000\n2.000000\n3.000000\n")),
		//("functions/", "void_fun", Expected::Output("ponies\nhorses\nponies\nhorses\n")),
		// Note: Test inspired by Crafting Interpreters 
		//("functions/", "fib", Expected::Output("9227465\n")),

		//("scope/", "block_shadow", Expected::Output("hello ponies\nhello again ponies\nhello ponies\n")),

		//("if/", "basic_if_expr_ret"   , Expected::Output("10\n3\n")),
		//("if/", "basic_if_expr_var"   , Expected::Output("3\n10\n")),
		//("if/", "basic_if_expr"       , Expected::Output("10\n3\n")),
		//("if/", "basic_if"            , Expected::Output("true\n")),
		//("if/", "if_no_else"          , Expected::Output("little pony\nmy\nlittle pony\n")),
		// Shows that the plain 'if' never returns a value.
		//("if/", "if_no_else_in_print" , Expected::Output("my little pony\n")),

		//("comparison/", "compare_basic", Expected::Output("false\ntrue\ntrue\ntrue\n")),
		//("comparison/", "compare_constants", Expected::Output("true\ntrue\nfalse\nfalse\nfalse\ntrue\nfalse\ntrue\ntrue\ntrue\n"))
	];

	for (path, test) in tests {
		generate_test(tests_file, path, test).unwrap();
	}
}