use std::collections::HashMap;
use std::fs::File;
use std::io::Write as _;

struct BuiltTests {
	modules: HashMap<String, String>
}

fn get_module<'a>(bt: &'a mut BuiltTests, path: &str) -> &'a mut String {
	if bt.modules.contains_key(path) {
		bt.modules.get_mut(path).unwrap()
	}
	else {
		bt.modules.insert(path.to_string(), "".to_string());
		bt.modules.get_mut(path).unwrap()
	}
}

fn generate_test(bt: &mut BuiltTests, bt_valgrind: &mut BuiltTests, path: &str, test_name: &str) {
	let out = get_module(bt, path);
	let out_vg = get_module(bt_valgrind, path);

	use std::fmt::Write;

	writeln!(out, "\t#[test]").unwrap();
	writeln!(out, "\tfn {test_name}() {{").unwrap();

	// We let the generated test code actually do the concat!.
	let exe_path = format!("concat!(env!(\"CARGO_TARGET_TMPDIR\"), \"/{test_name}\")");

	writeln!(out, "\t\trun_integration_test(\"tests/{path}{test_name}.poni\", {exe_path});").unwrap();
	writeln!(out, "\t}}").unwrap();

	// Generate a valgrind test case too.
	writeln!(out_vg, "\t#[test]").unwrap();
	writeln!(out_vg, "\t#[ignore]").unwrap(); // Ignore valgrind tests by default because they are slow.
	writeln!(out_vg, "\tfn {test_name}() {{").unwrap();

	writeln!(out_vg, "\t\trun_integration_test_valgrind(\"tests/{path}{test_name}.poni\", {exe_path});").unwrap();
	writeln!(out_vg, "\t}}").unwrap();
}

pub fn generate(tests_file: &mut File) {
	let tests = [
		("assign/", "addition_tuple"),
		("assign/", "arith_ops_float_promote"),
		("assign/", "arith_ops_int"),
		("assign/", "compound_in_tuple"),
		("assign/", "compound_set_basic"),
		("assign/", "compound_set_fun_call"),
		("assign/", "err_arith_ops"),

		("binary/", "binary_doubleblock"),
		("binary/", "binary_bottom"),
		("binary/", "binary_parens"),
		("binary/", "tuples"),
		("binary/", "err_class"),
		("binary/", "modulo"),

		("binder/", "sneaky"),
		("binder/", "arity_resolve"),
		("binder/", "type_resolve"),

		("block/", "declaration_at_end_for_fun"),
		("block/", "declaration_at_end_for_range"),

		("cyclic/", "class_members_and_fun_thru_param"),
		("cyclic/", "err_class_members_and_fun_thru_param"),
		("cyclic/", "class_members_and_fun_thru_param_nocycle"),
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

		("cyclic/", "class_members_thru_class_fun_thruself"),
		("cyclic/", "class_members_thru_class_fun"),
		("cyclic/", "err_class_members_thru_class_fun_thruself"),
		("cyclic/", "err_class_members_thru_class_fun"),
		("cyclic/", "err_class_members_thru_class_fun2"),

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
		("typecheck/", "nested_if_promote_int"),
		("typecheck/", "nested_if_promote_void"),

		("typecheck/", "err_try_assign_float_for_int"),
		("typecheck/", "err_try_return_float_for_int_short"),
		("typecheck/", "err_try_return_float_for_int"),

		("string/", "simple_idx"),
		("string/", "simple_str"),
		("string/", "str_of_strbuf"),
		("string/", "very_simple_str"),

		("tuple/", "assign"),
		("tuple/", "nested_assign"),
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
		("tuple/", "xyzw"),

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
		("functions/", "simple_fun_ref"),

		("functions/", "err_assign_to_fun"),

		("gc/", "simple_tuple"),
		("gc/", "tuple_as_member"),
		("gc/", "fun_as_member"),

		("scope/", "block_shadow"),

		("if/", "basic_if_expr_ret"),
		("if/", "basic_if_expr_var"),
		("if/", "basic_if_expr"),
		("if/", "basic_if"),
		("if/", "if_extra_parens"),
		("if/", "if_no_else"),
		("if/", "if_no_else_in_print"),
		("if/", "if_fun_calls"),
		("if/", "if_with_prints"),

		("if/", "err_if_bad_condition"),
		("if/", "err_if_incompat_types"),
		("if/", "err_if_no_else_bad_type"),

		("new/", "err_new_mandatory_without_type"),
		("new/", "err_new_mandatory"),
		("new/", "new_dotted"),
		("new/", "new_mandatory"),
		("new/", "new_bad_self_ints"),
		("new/", "new_bad_self_str"),
		("new/", "fun_call"),
		("new/", "nested_initializers"),
		("new/", "nested_initializers_unique_fun_names"),

		("comparison/", "compare_basic"),
		("comparison/", "compare_constants"),
		("comparison/", "compare_doubleblock"),
		("comparison/", "compare_equal_nums"),
		("comparison/", "compare_equal_classes"),

		("lerp/", "basic_including_bools"),
		("lerp/", "class"),
		("lerp/", "promote"),
		("lerp/", "tuple"),
		("lerp/", "tuple_nest"),

		("lexer/", "color_literal"),
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

		("loop/", "err_break_incompatible_hasval"),
		("loop/", "err_break_incompatible_types"),
		("loop/", "loop_with_return"),
		("loop/", "loop_with_break"),
		("loop/", "loop_with_multi_break_int"),
		("loop/", "loop_with_multi_break_noval"),
		("loop/", "loop_with_multi_break_noval_bracefix"),
		("loop/", "loop_with_multi_break_promo_a"),
		("loop/", "loop_with_multi_break_promo_b"),
		("loop/", "while_with_continue"),
		("loop/", "for_with_continue"),

		("misc/", "panicking_color_literal"),
		("misc/", "array_of_str"),
		("misc/", "array_of_strbuf"),
		("misc/", "array_of_tuple_of_opt_class_opt_strbuf"),
		("misc/", "array_of_tuple_of_opt_class_opt_strbuf_ez"),
		("misc/", "big_optional_type_ball"),
		("misc/", "complex_return_in_binop"),
		("misc/", "err_return_in_binop"),
		("misc/", "err_top_level_return"),
		("misc/", "noerr_return_in_binop"),
		("misc/", "noerr_return_in_binop_sidefx"),
		("misc/", "simple_var_exprs_and_infer"),
		("misc/", "test_init"),
		("misc/", "unused_expr"),
		("misc/", "big5_smaller"),

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

		("closure/", "simple"),
		("closure/", "simple_class"),
		("closure/", "self_in_class"),
		("closure/", "simple_no_param"),
		("closure/", "simple_no_param_psuedo_obj"),
		("closure/", "for_loop"),
		("closure/", "while_loop"),
		("closure/", "param_mut"),
		("closure/", "crazy_inner_fun"),
		("closure/", "crazy_inner_fun_both_levels"),
		("closure/", "funs_of_funs"),
		("closure/", "nested_class"),
		("closure/", "closure_in_new"),

		// old closure tests
		("closure/", "capture_fun_lambda"),
		("closure/", "capture_fun_with_closure_lambda"),
		("closure/", "capture_fun_with_closure"),
		("closure/", "capture_fun"),
		("closure/", "capture_strbuf"),
		("closure/", "double_nested_param"),
		("closure/", "double_nested"),
		("closure/", "nested_param"),
		("closure/", "nested_var"),
		("closure/", "returns_closure_notinit"),
		("closure/", "returns_closure"),

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
		("classes/", "basic_self"),
		("classes/", "class_member_that_is_fun"),
		("classes/", "class_member_function_capture"),
		("classes/", "noout_data_and_fun_assign"),
		("classes/", "noout_data_and_fun_read"),
		("classes/", "noout_pure_data_complex"),
		("classes/", "noout_pure_data_initializers"),
		("classes/", "noout_pure_data"),

		("classes/", "class_member_after_fun"),

		("classes/", "inner_static_simple"),
		("classes/", "inner_static_scoped"),
		("classes/", "inner_wrongscope_err"),
		("classes/", "inner_wrongscope_fullname"),
		("classes/", "inner_many_scopes"),

		("classes/", "inner_dyn_simple"),
		("classes/", "inner_dyn_construct"),
		("classes/", "inner_dyn_construct_weirder"),
		("classes/", "inner_dyn_funcapture"),
		("classes/", "inner_dyn_funcall"),
		("classes/", "inner_dyn_funcapture_getter"),
		("classes/", "inner_dyn_funcall_getter"),
		("classes/", "inner_dyn2_funcapture"),
		("classes/", "inner_dyn2_funcall"),
		("classes/", "inner_dyn2_funcapture_getter"),
		("classes/", "inner_dyn2_funcall_getter"),

		("classes/", "inner_err_not_dyn"),
		("classes/", "inner_err_not_dyn_get"),
		("classes/", "inner_err_not_dyn_set"),
		("classes/", "inner_err_not_dyn_funcapture"),

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

		("let/", "let_err_classmem_reassign"),
		("let/", "let_err_reassign"),
		("let/", "let_err_tuple"),
		("let/", "let_new"),
		("let/", "let_read"),
		("let/", "let_success_tuple"),

		("promote/", "err_array_assign"),
		("promote/", "err_array_assign_in_tuple"),
		("promote/", "err_array_lit"),
		("promote/", "err_dynarray_assign"),
		("promote/", "err_dynarray_from_array"),
		("promote/", "blocks_float_print"),
		("promote/", "blocks_float_var"),
		("promote/", "blocks_int_print"),
		("promote/", "blocks_int_var"),
		("promote/", "synth_promote_float"),
		("promote/", "tuples"),
		("promote/", "tuple_matrix"),
		("promote/", "weird_tuple"),
		("promote/", "tuple_of_optional"),
	
		("variable/", "assign_numbers"),
		("variable/", "assign_to_bottom_binop"),
		("variable/", "assign_to_bottom_binop_var"),
		("variable/", "assign_to_bottom_binop_var2"),
		("variable/", "assign_to_bottom"),
		("variable/", "simple_assign"),

		("while/", "while_basic"),
		("while/", "while_nested"),
		("while/", "while_true"),
		("while/", "array_search"),

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

		("dynarray/", "assign_lit"),
		("dynarray/", "dynarray_horse"),
		("dynarray/", "dynarray_optional_horse"),
		("dynarray/", "dynarray_tuple"),
		("dynarray/", "index"),
		("dynarray/", "push_many_times"),
		("dynarray/", "push_once"),
		("dynarray/", "set_index"),
		("dynarray/", "set_index_len"),
		("dynarray/", "typename"),
		("dynarray/", "builtin_any"),
		("dynarray/", "builtin_all"),
		("dynarray/", "builtin_clone_shallow"),

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

		("optional/", "array_of_opt"),
		("optional/", "confusing_implicit_some"),
		("optional/", "else_ret_assigns"),
		("optional/", "err_unknown_nil"),
		("optional/", "flip_flop"),
		("optional/", "flip_flop2"),
		("optional/", "horse_question_mark"),
		("optional/", "implicit_some"),
		("optional/", "nil"),
		("optional/", "opt_else_promote"),
		("optional/", "option_else"),
		("optional/", "option_else_ret"),
		("optional/", "option_else_ret_nobrace"),
		("optional/", "option_else_ret_shadow"),
		("optional/", "optional_array"),
		("optional/", "optional_strings"),
		("optional/", "optional_strings_promote"),
		("optional/", "optional_string_tuple"),
		("optional/", "or_panic_success"),
		("optional/", "tree"),
		("optional/", "tree2"),
		("optional/", "tree3"),
		("optional/", "builtin_is_some_array"),
		("optional/", "builtin_is_some_horse"),

		("vec/", "vec_types"),
		("vec/", "vec_ret"),
		("vec/", "vec_lerp"),
		("vec/", "vec_product"),
		("vec/", "weird_vec_product_promote"),
		("vec/", "weird_vec_product_expr_promote"),
		("vec/", "builtin_map"),

		("unary/", "unary_int_float"),
		("unary/", "unary_vec"),
		("unary/", "unary_not"),
		("unary/", "err_not_on_nonbool"),

		("range/", "basic_parse"),
		("range/", "basic_parse_properties"),
		("range/", "basic_var"),
		("range/", "two_main_types"),

		("readonly/", "readonly_properties"),

		("for/", "err_assign_fun"),
		("for/", "err_assign_range"),
		("for/", "correct_scope"),
		("for/", "correct_scope2"),
		("for/", "for_basic_i"),
		("for/", "for_basic"),
		("for/", "for_closed_i"),
		("for/", "for_closed"),
		("for/", "for_basic_i_nospace"),
		("for/", "for_basic_nospace"),
		("for/", "for_closed_i_nospace"),
		("for/", "for_closed_nospace"),

		("for/", "for_optfun"),
	];

	let mut bt = BuiltTests {
		modules: HashMap::new()
	};
	let mut bt_valgrind = BuiltTests {
		modules: HashMap::new()
	};
	for (path, test) in tests {
		generate_test(&mut bt, &mut bt_valgrind, path, test);
	}

	for (k, v) in &bt.modules {
		// get rid of slash
		let substr = &k[..k.len() - 1];
		writeln!(tests_file, "mod r#{substr} {{").unwrap();
		writeln!(tests_file, "\tuse crate::run_integration_test;").unwrap();
		writeln!(tests_file, "{v}").unwrap();
		writeln!(tests_file, "}}").unwrap();
	}
		
	writeln!(tests_file, "mod valgrind {{").unwrap();
	for (k, v) in &bt_valgrind.modules {
		// get rid of slash
		let substr = &k[..k.len() - 1];
		writeln!(tests_file, "mod r#{substr} {{").unwrap();
		writeln!(tests_file, "\tuse crate::run_integration_test_valgrind;").unwrap();
		writeln!(tests_file, "{v}").unwrap();
		writeln!(tests_file, "}}").unwrap();
	}
	writeln!(tests_file, "}}").unwrap();
}