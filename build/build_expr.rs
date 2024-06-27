use std::fs::File;
use std::fmt::Write;

fn token<'a>(spec: &mut &'a str) -> Option<&'a str> {
	let Some(before) = spec.find(|c| !char::is_whitespace(c)) else { return None };
	
	*spec = &spec[before..];

	let Some(after) = spec.find(char::is_whitespace) else { return None };

	let result = Some(&spec[..after]);

	*spec = &spec[after..];

	return result;
}

fn at_line_end(spec: &str) -> bool {
	let newline = spec.find('\n');
	let before = spec.find(|c| !char::is_whitespace(c));

	match (newline, before) {
		(Some(newline), Some(before)) => { return newline < before; },
		(Some(_), None) => { return true; }
		(None, Some(_)) => { return false; }
		(None, None) => { return true; }
	}
}

struct Opt {
	box_exprs: bool,
}

struct ConstructOpt {
	wrap_ok: bool,
	to_enum: bool,
}

fn generate_constructor(enum_name: &str, ty_name: &str, copt: ConstructOpt, opt: &Opt, fields: &Vec<(&str, &str)>, into: &mut String) -> std::fmt::Result {
	let prefix = if copt.to_enum { "mk_" } else { "new_" };
	let suffix = if copt.wrap_ok { "_ok" } else { "" };

	let return_type = match copt {
		ConstructOpt { wrap_ok: false, to_enum: false } => ty_name.to_string(),
		ConstructOpt { wrap_ok: false, to_enum: true } => enum_name.to_string(),
		ConstructOpt { wrap_ok: true, to_enum: false } => format!("crate::parser::Result<{ty_name}>"),
		ConstructOpt { wrap_ok: true, to_enum: true } => format!("crate::parser::Result<{enum_name}>"),
	};
	
	write!(into, "\tpub fn {prefix}{}{suffix}(", ty_name.to_ascii_lowercase())?;

	let mut add_comma = false;
	for field in fields {
		let mut param_ty = field.0;
		if param_ty == "&'static mut Expr" && opt.box_exprs { param_ty = "Expr"; }
		if param_ty == "&'static mut Stmt" { param_ty = "Stmt"; }

		if add_comma { write!(into, ", ")?; }
		add_comma = true;

		write!(into, "{}: {}", field.1, param_ty)?;
	}
	writeln!(into, ") -> {return_type} {{")?;
	for field in fields {
		if field.0 == "&'static mut Expr" && opt.box_exprs {
			writeln!(into, "\t\tlet {0} = alloc_expr({0});", field.1)?;
		}
		if field.0 == "&'static mut Stmt" {
			writeln!(into, "\t\tlet {0} = alloc_stmt({0});", field.1)?;
		}
	}
	write!(into, "\t\t")?;
	if copt.wrap_ok { write!(into, "Ok(")?; }
	if copt.to_enum { write!(into, "{enum_name}::{ty_name}(")?; }
	write!(into, "{ty_name} {{")?;
	for field in fields {
		write!(into, "{}, ", field.1)?;
	}
	write!(into, "}}")?;
	if copt.to_enum { write!(into, ")")?; }
	if copt.wrap_ok { write!(into, ")")?; }
	writeln!(into, "")?;

	writeln!(into, "\t}}")?;
	
	Ok(())
}

fn generate_spec(name: &str, mut spec: &str, opt: Opt, file: &mut File) -> std::fmt::Result {
	let mut enum_def = String::new();
	let mut struct_defs = String::new();
	let mut enum_impl = String::new();

	writeln!(enum_def, "pub enum {name} {{")?;
	writeln!(enum_impl, "impl {name} {{")?;

	while let Some(ty_name) = token(&mut spec) {
		token(&mut spec);

		// To get fields...
		let mut fields: Vec<(&str, &str)> = vec![];

		// Always add a source location field.
		fields.push(("SourceLocation", "location"));

		loop {
			if at_line_end(spec) { break; }

			let Some(mut ty) = token(&mut spec) else { return Ok(()); };

			if ty == "Expr" && opt.box_exprs {
				ty = "&'static mut Expr";
			}
			if ty == "Stmt" {
				ty = "&'static mut Stmt";
			}

			let Some(mut name) = token(&mut spec) else { return Ok(()); };

			if name.ends_with(',') {
				name = &name[..name.len() - 1];
			}

			fields.push((ty, name));
		}

		

		writeln!(struct_defs, "pub struct {ty_name} {{")?;
		for field in &fields {
			writeln!(struct_defs, "\tpub {}: {},", field.1, field.0)?;
		}
		writeln!(struct_defs, "}}")?;

		writeln!(enum_def, "\t{ty_name}({ty_name}),")?;

		generate_constructor(name,
			ty_name,
			ConstructOpt { wrap_ok: false, to_enum: false },
			&opt, &fields, &mut enum_impl)?;
		generate_constructor(name,
			ty_name,
			ConstructOpt { wrap_ok: false, to_enum: true },
			&opt, &fields, &mut enum_impl)?;
		generate_constructor(name,
			ty_name,
			ConstructOpt { wrap_ok: true, to_enum: false },
			&opt, &fields, &mut enum_impl)?;
		generate_constructor(name,
			ty_name,
			ConstructOpt { wrap_ok: true, to_enum: true },
			&opt, &fields, &mut enum_impl)?;
	}

	writeln!(enum_def, "}}")?;

	writeln!(enum_impl, "}}")?;

	{
		use std::io::Write;
		write!(file, "{}\n", struct_defs).unwrap();
		write!(file, "{}\n", enum_def).unwrap();
		write!(file, "{}\n", enum_impl).unwrap();
	}

	Ok(())
}

pub fn generate(file: &mut File) {
	let expr_spec = r#"

	Binary   : Tok op, Expr left, Expr right, TypId typ
	Variable : VarId identity
	Assign   : VarId identity, Expr value
	Literal  : Token contents, TypId typ
	Block    : Vec<Stmt> stmts, bool has_value
	

	"#;

	// 	FunDeclare : FunId identity, Vec<VarId> args, 
	let stmt_spec = r#"
	
	Declare    : VarId identity, Expr value
	Expression : Expr expression
	FunDeclare : FunId identity, Expr value

	"#;

	let expr_opt = Opt {
		// Exprs inside exprs must be boxed.
		box_exprs: true,
	};

	let stmt_opt = Opt {
		box_exprs: false,
	};

	generate_spec("Expr", expr_spec, expr_opt, file).unwrap();
	generate_spec("Stmt", stmt_spec, stmt_opt, file).unwrap();
}