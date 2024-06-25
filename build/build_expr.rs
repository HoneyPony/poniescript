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

	writeln!(enum_def, "pub enum {name} {{")?;

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
		for field in fields {
			writeln!(struct_defs, "\tpub {}: {},", field.1, field.0)?;
		}
		writeln!(struct_defs, "}}")?;

		writeln!(enum_def, "\t{ty_name}({ty_name}),")?;
	}

	writeln!(enum_def, "}}")?;

	{
		use std::io::Write;
		write!(file, "{}\n", struct_defs).unwrap();
		write!(file, "{}", enum_def).unwrap();
	}

	Ok(())
}

pub fn generate(file: &mut File) {
	let spec = r#"

	Binary : Expr left, Expr right

	"#;

	generate_spec("Expr", spec, file).expect("couldn't codegen");
}