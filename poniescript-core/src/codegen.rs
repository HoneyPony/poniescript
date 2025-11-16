use crate::{Args, db::{Ast, Db}};

mod single_codegen;

pub fn codegen(args: &Args, db: &mut Db, ast: &Ast, output: &mut dyn std::io::Write) -> std::io::Result<()> {
	let mut codegen = single_codegen::Codegen::new(db);

	codegen.codegen(args, ast, output)
}