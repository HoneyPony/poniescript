use std::{fs::File, path::Path};

mod build_expr;
mod build_db;
mod build_tests;

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

	let mut db_file = new_gen_file(&out_dir, "db.arenas.rs");
	build_db::generate(&mut db_file);

	let mut tests_file = new_gen_file(&out_dir, "tests.gen.rs");
	build_tests::generate(&mut tests_file);

	// Note: This setup is based in part off of
	// https://github.com/condekind/tokers/blob/main/src/world.rs
}