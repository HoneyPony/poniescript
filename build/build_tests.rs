use std::collections::HashMap;
use std::fs::File;
use std::io::Write as _;

struct BuiltTests {
	modules: HashMap<String, String>
}

fn generate_test(bt: &mut BuiltTests, path: &str, test_name: &str) {
	let out = if let Some(out) = bt.modules.get_mut(path) {
		out
	}
	else {
		bt.modules.insert(path.to_string(), "".to_string());
		bt.modules.get_mut(path).unwrap()
	};

	use std::fmt::Write;

	writeln!(out, "\t#[test]").unwrap();
	writeln!(out, "\tfn {test_name}() {{").unwrap();

	// We let the generated test code actually do the concat!.
	let exe_path = format!("concat!(env!(\"CARGO_TARGET_TMPDIR\"), \"/{test_name}\")");

	writeln!(out, "\t\trun_integration_test(\"tests/{path}{test_name}.poni\", {exe_path});").unwrap();
	writeln!(out, "\t}}").unwrap();
}

pub fn generate(tests_file: &mut File) {
	let tests = [
		("binary/", "binary_doubleblock"),
		("binary/", "binary_bottom"),
		("binary/", "binary_parens"),

		("cyclic/", "class_members_and_fun_thru_param"),
		("cyclic/", "class_members_expr"),
		("cyclic/", "class_members_same"),
		("cyclic/", "class_members_same2"),
		("cyclic/", "globals_and_class_thru_access_2"),
		("cyclic/", "globals_and_class_thru_access"),
		("cyclic/", "globals_and_fun_thru_access_2"),
		("cyclic/", "globals_and_fun_thru_access"),
		("cyclic/", "globals_and_fun_thru_param"),
		("cyclic/", "globals_expr"),
		("cyclic/", "globals_same"),
		("cyclic/", "globals_and_class_thru_difficult"),

		("cyclic/", "err_cyclic"),
		("cyclic/", "err_granularity_fun_in_init"),
		("cyclic/", "err_cyclic_fun"),
		("cyclic/", "err_cyclic_multi_fun"),
		("cyclic/", "err_cyclic_fun_five"),

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

		("print/", "err_print_empty"),


		("globals/", "global_block"),
		// TODO: Figure out precise float output format we want.
		("globals/", "globals_3"),
		("globals/", "global_global"),
		("globals/", "global_increment"),
		("globals/", "global_increment_indirect"),
		("globals/", "global_mutate"),
		("globals/", "global_after_fun"),

		("globals/", "err_global_fun_redefine"),
		("globals/", "err_global_redefine"),

		("typecheck/", "print_assume_int"),
		("typecheck/", "promote_assumes_inside_block"),
		("typecheck/", "promote_to_float_arithmetic"),
		("typecheck/", "promote_to_float_assign"),
		("typecheck/", "promote_to_float_block_assign_assumeint"),
		("typecheck/", "promote_to_float_block_assign"),
		("typecheck/", "promote_to_float_block_return"),
		("typecheck/", "promote_to_float_return"),
		("typecheck/", "str_types"),
		("typecheck/", "return_block_return"), // Make sure this one at least compiles

		("typecheck/", "err_try_assign_float_for_int"),
		("typecheck/", "err_try_return_float_for_int_short"),
		("typecheck/", "err_try_return_float_for_int"),

	
		("string/", "simple_str"),
		("string/", "str_of_strbuf"),
		("string/", "very_simple_str"),

		("tuple/", "assign"),
		("tuple/", "big_tuple"),
		("tuple/", "create_tuple"),
		("tuple/", "print_tuple"),
		("tuple/", "fun_tuple"),
		("tuple/", "index_tuple"),
		("tuple/", "mixed"),
		("tuple/", "nested_promote_assign"),
		("tuple/", "nested_promote_existing"),
		("tuple/", "nested_promote_return"),
		("tuple/", "nested_silly_syntax"),

		("tuple/", "err_assign_big"),
		("tuple/", "err_assign_big_2"),
		("tuple/", "err_assign_inner"),
		("tuple/", "err_assign_small"),
		("tuple/", "err_big_index"),
		("tuple/", "err_string_index"),
	
		("functions/", "parse_params"),
		("functions/", "param_trailing_comma"),
		("functions/", "call_basic"),
		("functions/", "call_nested"),
		("functions/", "call_promote"),
		("functions/", "void_fun"),
		("functions/", "fib"),

		("functions/", "err_assign_to_fun"),

		("scope/", "block_shadow"),

		("if/", "basic_if_expr_ret"),
		("if/", "basic_if_expr_var"),
		("if/", "basic_if_expr"),
		("if/", "basic_if"),
		("if/", "if_extra_parens"),
		("if/", "if_no_else"),
		("if/", "if_no_else_in_print"),
		("if/", "if_fun_calls"),

		("if/", "err_if_bad_condition"),
		("if/", "err_if_incompat_types"),
		("if/", "err_if_no_else_bad_type"),

		("new/", "new_dotted"),

		("comparison/", "compare_basic"),
		("comparison/", "compare_constants"),
		("comparison/", "compare_doubleblock"),

		("lexer/", "err_unterminated_string"),
		("lexer/", "string_lit_basic_escapes"),
		("lexer/", "utf8"),
		("lexer/", "err_invalid_utf8_short"),
		("lexer/", "err_invalid_utf8_long"),

		("logical/", "basic_and"),
		("logical/", "basic_or"),
		("logical/", "short_and"),
		("logical/", "short_or"),
		("logical/", "short_and_expr"),
		("logical/", "short_or_expr"),
		("logical/", "short_or_expr_block"),
		("logical/", "short_or_expr_doubleblock"),
		("logical/", "or_bottom"),

		("misc/", "complex_return_in_binop"),
		("misc/", "err_return_in_binop"),
		("misc/", "err_top_level_return"),
		("misc/", "noerr_return_in_binop"),
		("misc/", "simple_var_exprs_and_infer"),
		("misc/", "test_init"),
		("misc/", "unused_expr"),

		("parser/", "err_fun_missing_brace"),
		("parser/", "err_missing_expr_paren"),

		("call/", "call_captured_rev"),
		("call/", "call_captured"),
		("call/", "call_if_simple"),
		("call/", "call_if_as_expr"),
		("call/", "call_if_as_expr_blocked"),
		("call/", "returns_fun"),
		("call/", "returns_fun_fun"),
		("call/", "returns_fun_fun_weirder"),
		("call/", "call_bottom"),
		("call/", "call_bottom_notreal"),
		("call/", "call_with_bottom_param"),
		("call/", "local_fun"),
		("call/", "local_fun_lambda"),
		("call/", "if_simple_lambda"),
		("call/", "uses_fun_with_class_retval"),
		("call/", "uses_fun_with_class_param"),

		("classes/", "basic_class"),
		("classes/", "basic_new_inferred_get"),
		("classes/", "basic_new_inferred_get_promote"),
		("classes/", "basic_new_explicit_get"),
		("classes/", "basic_new_explicit_get_promote"),
		("classes/", "class_as_param"),
		("classes/", "class_as_returnval"),
		("classes/", "class_as_returnval_params"),
		("classes/", "class_ref_semantics"),
		("classes/", "basic_nested_get"),
		("classes/", "basic_nested_get_ooo"),
		("classes/", "basic_nested_set_ooo"),
		("classes/", "basic_nested_get_explicit"),
		("classes/", "basicer_nested_get"),
		("classes/", "more_basicer_nested_get"),
		("classes/", "basic_class_call"),
		("classes/", "basic_call_with_member"),
		("classes/", "class_call_own_funs"),
		("classes/", "class_member_ref"),
		("classes/", "basic_new_list"),
		("classes/", "class_member_that_is_fun"),
		("classes/", "class_member_function_capture"),
		("classes/", "noout_data_and_fun_assign"),
		("classes/", "noout_data_and_fun_read"),
		("classes/", "noout_pure_data_complex"),
		("classes/", "noout_pure_data_initializers"),
		("classes/", "noout_pure_data"),

		("classes/", "err_assign_to_class"),
		("classes/", "err_get_nonexistent_member"),
		("classes/", "err_new_unknown_property"),
		("classes/", "err_new_wrong_ty_known"),
		("classes/", "err_new_wrong_ty_unknown"),
		("classes/", "err_pure_data_wrongty"),
		("classes/", "err_try_to_read_class_in_initializer"),
		("classes/", "err_weird_var"),

		("get/", "get_string_length"),
		("get/", "err_get_on_int"),
		("get/", "err_get_on_string"),

		("set/", "basic_set"),
		("set/", "set_bottom"),
		("set/", "set_bottom_etc"),
		("set/", "set_chain"),

		("promote/", "blocks_float_print"),
		("promote/", "blocks_float_var"),
		("promote/", "blocks_int_print"),
		("promote/", "blocks_int_var"),
		("promote/", "synth_promote_float"),
	
		("variable/", "assign_numbers"),
		("variable/", "assign_to_bottom_binop"),
		("variable/", "assign_to_bottom_binop_var"),
		("variable/", "assign_to_bottom_binop_var2"),
		("variable/", "assign_to_bottom"),
		("variable/", "simple_assign"),

		("dead_code/", "dead_block"),
		("dead_code/", "dead_new"),
		("dead_code/", "dead_binary1"),
		("dead_code/", "dead_ops"),
		("dead_code/", "dead_op_single"),
		("dead_code/", "dead_args"),
		("dead_code/", "dead_args_array"),
		("dead_code/", "dead_args_print"),
		("dead_code/", "dead_args_str"),
		("dead_code/", "dead_args_valcall"),

		("array/", "array_nested_empty_lhs"),
		("array/", "array_nested_empty_rhs"),
		("array/", "array_nested_empty"),
		("array/", "array_nested_infer"),
		("array/", "array_index_nested"),
		("array/", "array_init_and_print"),
		("array/", "array_of_classes"),
		("array/", "array_set_and_funcall"),
		("array/", "array_set"),
		("array/", "array_type_infer"),
		("array/", "array_type_override"),
		("array/", "array_type_parse"),
		("array/", "array_as_member"),
		("array/", "array_class_deep_nesting"),
		("array/", "array_class_deep_nesting_namedif"),
		("array/", "array_class_deep_simpler"),
		("array/", "array_of_funs"),
		("array/", "array_ref_semantics"),
		("array/", "array_length"),
		("array/", "array_complicated_signature"),
		("array/", "err_empty_arr_and_var"),
	];

	let mut bt = BuiltTests {
		modules: HashMap::new()
	};
	for (path, test) in tests {
		generate_test(&mut bt, path, test);
	}
	for (k, v) in &bt.modules {
		// get rid of slash
		let substr = &k[..k.len() - 1];
		writeln!(tests_file, "mod r#{substr} {{").unwrap();
		writeln!(tests_file, "\tuse crate::run_integration_test;").unwrap();
		writeln!(tests_file, "{v}").unwrap();
		writeln!(tests_file, "}}").unwrap();
	}
}