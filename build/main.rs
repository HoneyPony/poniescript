use std::{fs::File, path::Path};

mod build_expr;

fn new_gen_file(out_dir: &String, file_name: &str) -> File {
	 let path = Path::new(&out_dir).join(file_name);
	 return File::create(path).unwrap();
}

fn main() {
	// Only rerun when we update the build script source code
	println!("cargo::rerun-if-changed=build");

	let out_dir = std::env::var("OUT_DIR").unwrap();

    let mut expr_file = new_gen_file(&out_dir, "expr.gen.rs");
	build_expr::generate(&mut expr_file);

	// Note: This setup is based in part off of
	// https://github.com/condekind/tokers/blob/main/src/world.rs
}