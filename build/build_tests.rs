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
		("binary/", "binary_doubleblock"),

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

		("typecheck/", "print_assume_int"),
		("typecheck/", "promote_assumes_inside_block"),
		("typecheck/", "promote_to_float_arithmetic"),
		("typecheck/", "promote_to_float_assign"),
		("typecheck/", "str_types"),
		("typecheck/", "return_block_return"), // Make sure this one at least compiles
		// TODO: Add other tests when we get function calls
	
		("string/", "simple_str"),
		("string/", "str_of_strbuf"),
	
		("functions/", "parse_params"),
		("functions/", "param_trailing_comma"),
		("functions/", "call_basic"),
		("functions/", "call_nested"),
		("functions/", "call_promote"),
		("functions/", "void_fun"),
		("functions/", "fib"),

		("scope/", "block_shadow"),

		("if/", "basic_if_expr_ret"),
		("if/", "basic_if_expr_var"),
		("if/", "basic_if_expr"),
		("if/", "basic_if"),
		("if/", "if_no_else"),
		("if/", "if_no_else_in_print"),
		("if/", "if_fun_calls"),

		("comparison/", "compare_basic"),
		("comparison/", "compare_constants"),
		("comparison/", "compare_doubleblock"),

		("logical/", "basic_and"),
		("logical/", "basic_or"),
		("logical/", "short_and"),
		("logical/", "short_or"),
		("logical/", "short_and_expr"),
		("logical/", "short_or_expr"),
		("logical/", "short_or_expr_block"),
		("logical/", "short_or_expr_doubleblock"),
		("logical/", "or_bottom"),

		("call/", "call_captured_rev"),
		("call/", "call_captured"),
		("call/", "call_if_simple"),
		// TODO: Fix the non-blocked version
		("call/", "call_if_as_expr_blocked"),
		("call/", "returns_fun"),
		("call/", "returns_fun_fun"),
		("call/", "returns_fun_fun_weirder"),
		("call/", "call_bottom"),
	];

	for (path, test) in tests {
		generate_test(tests_file, path, test).unwrap();
	}
}