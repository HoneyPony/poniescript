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
		// print_nested: the last line is 345 because it should re-print each of the inner print()s.
		("print/", "print_nested"                 , Expected::Output("345\n45\n5\n345\n")),
		("print/", "print_string_literal"         , Expected::Output("hello world\n")),
		("print/", "print_multiple_string_literal", Expected::Output("hello world\n")),
		("print/", "print_number_literal"         , Expected::Output("3\n")),
		("print/", "print_block"                  , Expected::Output("hello pony folk\n")),
		("print/", "print_void_first"             , Expected::Output("oh no my pony\n")),
		("print/", "print_void_others"            , Expected::Output("oh no my pony\n")),
		("print/", "print_dif_funs"               , Expected::Output("debug: 123\ndebug behavior: calling helper: 10\n")),
		("print/", "print_inner_return"           , Expected::Output("afterwards\n")),

		("globals/", "global_block" , Expected::Output("7\n")),
		// TODO: Figure out precise float output format we want.
		("globals/", "globals_3"    , Expected::Output("3\n4.000000\n10\n")),
		("globals/", "global_global", Expected::Output("55\n")),

		("typecheck/", "print_assume_int", Expected::Output("10\n")),
		("typecheck/", "promote_assumes_inside_block", Expected::Output("15\n")),
		("typecheck/", "promote_to_float_arithmetic", Expected::Output("30.000000\n")),
		("typecheck/", "promote_to_float_assign", Expected::Output("10.000000\n")),
		("typecheck/", "str_types", Expected::Output("hello my little ponies\n")),
		("typecheck/", "return_block_return", Expected::Output("")), // Make sure this one at least compiles
		// TODO: Add other tests when we get function calls
	
		("string/", "simple_str", Expected::Output("hello world25\n")),
		("string/", "str_of_strbuf", Expected::Output("hello my little ponies\n")),
	
		("functions/", "parse_params", Expected::Output("")),
		("functions/", "param_trailing_comma", Expected::Output("")),
		("functions/", "call_basic", Expected::Output("3\n7\n11\n")),
		("functions/", "call_nested", Expected::Output("my little pony!\n")),
		("functions/", "call_promote", Expected::Output("1.000000\n2.000000\n3.000000\n")),
		("functions/", "void_fun", Expected::Output("ponies\nhorses\nponies\nhorses\n")),
		// Note: Test inspired by Crafting Interpreters 
		("functions/", "fib", Expected::Output("9227465")),

		("scope/", "block_shadow", Expected::Output("hello ponies\nhello again ponies\nhello ponies\n")),

		("if/", "basic_if_expr_ret", Expected::Output("10\n3\n")),
		("if/", "basic_if_expr_var", Expected::Output("3\n10\n")),
		("if/", "basic_if_expr"    , Expected::Output("10\n3\n")),
		("if/", "basic_if"         , Expected::Output("true\n")),

		("comparison/", "compare_basic", Expected::Output("false\ntrue\ntrue\ntrue\n")),
		("comparison/", "compare_constants", Expected::Output("true\ntrue\nfalse\nfalse\nfalse\ntrue\nfalse\ntrue\ntrue\ntrue\n"))
	];

	for (path, test, expect) in tests {
		generate_test(tests_file, path, test, expect).unwrap();
	}
}