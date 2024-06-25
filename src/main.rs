mod db;
mod expr;
mod typ;
mod source;
mod lexer;
mod module;

use std::env;
use std::path::Path;

fn main() {
	let mut db = db::Db::new();

	match module::parse_module(&mut db, "test.poni".as_ref()) {
		Ok(_) => {},
		Err(err) => {
			eprintln!("Error parsing module: {err}");
		}
	}
}
