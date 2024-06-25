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

fn generate_spec(name: &str, mut spec: &str, file: &mut File) -> std::fmt::Result {
	let mut enum_def = String::new();
	let mut struct_defs = String::new();
	let mut enum_impl = String::new();
	let mut enum_ok_funs = String::new();

	writeln!(enum_def, "pub enum {name} {{")?;
	writeln!(enum_impl, "impl Expr {{")?;

	while let Some(ty_name) = token(&mut spec) {
		token(&mut spec);

		// To get fields...
		let mut fields: Vec<(&str, &str)> = vec![];
		loop {
			if at_line_end(spec) { break; }

			let Some(mut ty) = token(&mut spec) else { return Ok(()); };

			if ty == "Expr" {
				ty = "Box<Expr>";
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

		write!(enum_impl, "\tpub fn mk_{}(", ty_name.to_ascii_lowercase())?;
		write!(enum_ok_funs, "\tpub fn mk_{}_ok(", ty_name.to_ascii_lowercase())?;

		let mut add_comma = false;
		for field in &fields {
			let mut param_ty = field.0;
			if param_ty == "Box<Expr>" { param_ty = "Expr"; }

			if add_comma { write!(enum_impl, ", ")?; write!(enum_ok_funs, ", ")?; }
			add_comma = true;

			write!(enum_impl, "{}: {}", field.1, param_ty)?;
			write!(enum_ok_funs, "{}: {}", field.1, param_ty)?;
		}
		writeln!(enum_impl, ") -> {ty_name} {{")?;
		writeln!(enum_ok_funs, ") -> crate::parser::Result<{ty_name}> {{")?;
		for field in &fields {
			if field.0 == "Box<Expr>" {
				writeln!(enum_impl, "\t\tlet {0} = Box::new({0});", field.1)?;
			}
		}
		write!(enum_impl, "\t\t{ty_name} {{")?;
		write!(enum_ok_funs, "\t\tOk(Expr::mk_{}(", ty_name.to_ascii_lowercase())?;

		let mut add_comma = false;

		for field in &fields {
			write!(enum_impl, "{}, ", field.1)?;

			if add_comma { write!(enum_ok_funs, ", ")?; }
			add_comma = true;

			write!(enum_ok_funs, "{}", field.1)?;
		}
		writeln!(enum_impl, "}}")?;
		writeln!(enum_ok_funs, "))")?;

		writeln!(enum_impl, "\t}}")?;
		writeln!(enum_ok_funs, "\t}}")?;
	}

	writeln!(enum_def, "}}")?;

	writeln!(enum_impl, "{}", enum_ok_funs)?;
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
	let spec = r#"

	Binary   : Expr left, Expr right, TypId typ
	Variable : VarId identity
	Assign   : VarId identity, Expr value
	Literal  : StrId contents, TypId typ
	Declare  : VarId identity, Expr value

	"#;

	generate_spec("Expr", spec, file).expect("couldn't codegen");
}