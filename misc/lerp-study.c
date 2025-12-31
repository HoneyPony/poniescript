// Lerp study:
// This is what we generate right now:
		ps_bool t2 = ((ps_float)0.788075) >= 0.5;
		ps_float t3 = 1.0 - ((ps_float)0.788075);
		struct ps_tuple_15 t4;
		t4.v_0 = (ps_int)(v_a27174.v_0 * t3 + v_b27174.v_0 * ((ps_float)0.788075));
		t4.v_1 = (ps_int)(v_a27174.v_1 * t3 + v_b27174.v_1 * ((ps_float)0.788075));
		t4.v_2 = (ps_int)(v_a27174.v_2 * t3 + v_b27174.v_2 * ((ps_float)0.788075));
// With some specialized tuple methods, though, we could generate at the very
// least:
		ps_bool t2 = ((ps_float)0.788075) >= 0.5;
		ps_float t3 = 1.0 - ((ps_float)0.788075);
		struct ps_vec3i t4;
		t4 = ps_lerp_vec3i(v_a27174, v_b27174, ((ps_float)0.788075));
// This saves sommething like 171 bytes PER LERP. It could save even more
// if we were able to skip the t2 and the t3, and assign the t4 directly; at
// that point, the C compiler would have to churn through much less stuff.