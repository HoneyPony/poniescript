mod build_tests;

use std::{fs::File, path::Path};

fn new_gen_file(out_dir: &String, file_name: &str) -> File {
	 let path = Path::new(&out_dir).join(file_name);
	 return File::create(path).unwrap();
}

fn main() {
	// Only rerun when we update the build script source code
	println!("cargo::rerun-if-changed=build");

	let out_dir = std::env::var("OUT_DIR").unwrap();

	let mut tests_file = new_gen_file(&out_dir, "tests.gen.rs");
	build_tests::generate(&mut tests_file);
}