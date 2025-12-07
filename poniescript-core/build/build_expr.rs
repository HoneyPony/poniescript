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
	in_ast: bool,
}

fn generate_constructor(use_proxy: bool, id_name: &str, ast_field: &str, enum_name: &str, ty_name: &str, copt: ConstructOpt, opt: &Opt, fields: &Vec<(&str, &str)>, into: &mut String) -> std::fmt::Result {
	let mut prefix = if copt.to_enum { "mk_" } else { "new_" };
	if copt.in_ast { prefix = "put_"; if use_proxy { prefix = "push_" } };
	let prefix = prefix;
	let suffix = if copt.wrap_ok { "_ok" } else { "" };

	let return_type = match copt {
		ConstructOpt { wrap_ok: false, to_enum: false, in_ast: false } => ty_name.to_string(),
		ConstructOpt { wrap_ok: false, to_enum: true , in_ast: false } => enum_name.to_string(),
		ConstructOpt { wrap_ok: true , to_enum: false, in_ast: false } => format!("crate::parser::Result<{ty_name}>"),
		ConstructOpt { wrap_ok: true , to_enum: true , in_ast: false } => format!("crate::parser::Result<{enum_name}>"),

		ConstructOpt { wrap_ok: false, to_enum: false, in_ast: true  } => panic!("invalid combo"),
		ConstructOpt { wrap_ok: false, to_enum: true , in_ast: true  } => id_name.to_string(),
		ConstructOpt { wrap_ok: true , to_enum: false, in_ast: true  } => panic!("invalid combo"),
		ConstructOpt { wrap_ok: true , to_enum: true , in_ast: true  } => format!("crate::parser::Result<{id_name}>"),
	};
	
	write!(into, "\tpub fn {prefix}{}{suffix}(", ty_name.to_ascii_lowercase())?;

	if copt.in_ast {
		if use_proxy {
			write!(into, "ast: &AstProxy, ")?;
		}
		else {
			write!(into, "ast: &mut Ast, ")?;
		}
	}

	let mut add_comma = false;
	for field in fields {
		let mut param_ty = field.0;
		if param_ty == "&'static mut Expr"         && opt.box_exprs { param_ty = "Expr"; }
		if param_ty == "Option<&'static mut Expr>" && opt.box_exprs { param_ty = "Option<Expr>"; }
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
		if field.0 == "Option<&'static mut Expr>" && opt.box_exprs {
			writeln!(into, "\t\tlet {0} = {0}.map(|e| alloc_expr(e));", field.1)?;
		}
		if field.0 == "&'static mut Stmt" {
			writeln!(into, "\t\tlet {0} = alloc_stmt({0});", field.1)?;
		}
	}
	write!(into, "\t\t")?;
	if copt.wrap_ok { write!(into, "Ok(")?; }
	if copt.in_ast  { write!(into, "ast.{ast_field}.push(")?; }
	if copt.to_enum { write!(into, "{enum_name}::{ty_name}(")?; }
	write!(into, "{ty_name} {{")?;
	for field in fields {
		write!(into, "{}, ", field.1)?;
	}
	write!(into, "}}")?;
	if copt.to_enum { write!(into, ")")?; }
	if copt.in_ast  { write!(into, ")")?; }
	if copt.wrap_ok { write!(into, ")")?; }
	writeln!(into, "")?;

	writeln!(into, "\t}}")?;
	
	Ok(())
}

fn generate_spec(name: &str, ast_field: &str, mut spec: &str, opt: Opt, file: &mut File,
	visit_trait: &mut String, visit_immut_trait: &mut String, locate_trait: &mut String) -> std::fmt::Result {
	let mut enum_def = String::new();
	let mut struct_defs = String::new();
	let mut enum_impl = String::new();
	let mut loc_match = String::new();
	let mut debug_impl = String::new();
	let mut visit_trait_visit_fn = String::new();
	let mut visit_immut_trait_visit_fn = String::new();
	let mut locate_trait_visit_fn = String::new();

	writeln!(enum_def, "pub enum {name} {{")?;
	writeln!(enum_impl, "#[allow(unused)]\nimpl {name} {{")?;

	writeln!(loc_match, "\tpub fn location(&self) -> &SourceLocation {{")?;
	writeln!(loc_match, "\t\tmatch self {{")?;

	writeln!(debug_impl, "impl std::fmt::Debug for {name} {{")?;
	writeln!(debug_impl, "\tfn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {{")?;
	writeln!(debug_impl, "\t\tmatch self {{")?;

	let lname = name.to_ascii_lowercase();
	let id_name = format!("{name}Id");

	writeln!(visit_trait_visit_fn, "\tfn visit_{lname}(&mut self, ast: &Ast, db: &mut Db, id: {id_name}) {{")?;
	writeln!(visit_trait_visit_fn, "\t\tlet binding = ast.{lname}s.get(id);")?;
	writeln!(visit_trait_visit_fn, "\t\tmatch binding.as_ref() {{")?;

	writeln!(visit_immut_trait_visit_fn, "\tfn visit_{lname}(&mut self, ast: &Ast, db: &Db, id: {id_name}) {{")?;
	writeln!(visit_immut_trait_visit_fn, "\t\tlet binding = ast.{lname}s.get(id);")?;
	writeln!(visit_immut_trait_visit_fn, "\t\tmatch binding.as_ref() {{")?;

	writeln!(locate_trait_visit_fn, "\tfn visit_{lname}(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, id: {id_name}) -> bool {{")?;
	writeln!(locate_trait_visit_fn, "\t\tlet binding = ast.{lname}s.get(id);")?;
	writeln!(locate_trait_visit_fn, "\t\tmatch binding.as_ref() {{")?;
	
	

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
				ty = "ExprId";
			}
			if ty == "Option<Expr>" && opt.box_exprs {
				ty = "Option<ExprId>";
			}
			if ty == "Vec<Expr>" {
				ty = "Vec<ExprId>";
			}
			if ty == "Stmt" {
				ty = "StmtId";
			}
			if ty == "Vec<Stmt>" {
				ty = "Vec<StmtId>";
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

		writeln!(loc_match, "\t\t\t{name}::{ty_name}(inner) => &inner.location,")?;

		writeln!(debug_impl, "\t\t\t{name}::{ty_name}(_) => f.write_str(\"{ty_name}\"),")?;

		

		// TODO: We could pass both id and the node itself, although then whenever
		// we override a thing we have to also do that.
		writeln!(visit_trait, "\tfn visit_{}(&mut self, ast: &Ast, db: &mut Db, id: {id_name}) {{", ty_name.to_ascii_lowercase())?;
		writeln!(visit_trait, "\t\tlet binding = ast.{lname}s.get(id);")?;
		writeln!(visit_trait, "\t\tlet {name}::{ty_name}({lname}) = binding.as_ref() else {{ return; }};")?;

		writeln!(visit_immut_trait, "\tfn visit_{}(&mut self, ast: &Ast, db: &Db, id: {id_name}) {{", ty_name.to_ascii_lowercase())?;
		writeln!(visit_immut_trait, "\t\tlet binding = ast.{lname}s.get(id);")?;
		writeln!(visit_immut_trait, "\t\tlet {name}::{ty_name}({lname}) = binding.as_ref() else {{ return; }};")?;

		
		// The locate trait helps us find the smallest node overlapping a particular
		// cursor position
		
		// The locate_ method is what is actually called when an expression is located
		writeln!(locate_trait, "\tfn locate_{}(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, it: &{ty_name}) {{", ty_name.to_ascii_lowercase())?;
		writeln!(locate_trait, "\t}}")?;

		writeln!(locate_trait, "\tfn visit_{}(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation, {lname}: &{ty_name}) -> bool {{", ty_name.to_ascii_lowercase())?;
		writeln!(locate_trait, "\t\tlet own_loc = &{lname}.location;")?;
		writeln!(locate_trait, "\t\teprintln!(\"visit {ty_name}: {{}} ? {{}} ? {{}}\", own_loc.offset, loc.offset, own_loc.offset + own_loc.length);")?;
		writeln!(locate_trait, "\t\tif loc.offset < own_loc.offset {{ return false; }}")?;
		writeln!(locate_trait, "\t\tif loc.offset >= own_loc.offset + own_loc.length {{ return false; }}")?;
		for field in &fields {
			if field.0 == "ExprId" {
				writeln!(visit_trait, "\t\tself.visit_expr(ast, db, {lname}.{});", field.1)?;

				writeln!(visit_immut_trait, "\t\tself.visit_expr(ast, db, {lname}.{});", field.1)?;

				writeln!(locate_trait, "\t\tif self.visit_expr(ast, db, loc, {lname}.{}) {{ return true; }}", field.1)?;
			}
			if field.0 == "Vec<ExprId>" {
				writeln!(visit_trait, "\t\tfor item in &{lname}.{} {{", field.1)?;
				writeln!(visit_trait, "\t\t\tself.visit_expr(ast, db, *item);")?;
				writeln!(visit_trait, "\t\t}}")?;

				writeln!(visit_immut_trait, "\t\tfor item in &{lname}.{} {{", field.1)?;
				writeln!(visit_immut_trait, "\t\t\tself.visit_expr(ast, db, *item);")?;
				writeln!(visit_immut_trait, "\t\t}}")?;

				writeln!(locate_trait, "\t\tfor item in &{lname}.{} {{", field.1)?;
				writeln!(locate_trait, "\t\t\tif self.visit_expr(ast, db, loc, *item) {{ return true; }}")?;
				writeln!(locate_trait, "\t\t}}")?;
			}
			// TODO: Also do this for Option<StmtId>
			if field.0 == "Option<ExprId>" {
				writeln!(visit_trait, "\t\tif let Some(inner) = {lname}.{} {{ self.visit_expr(ast, db, inner); }}", field.1)?;

				writeln!(visit_immut_trait, "\t\tif let Some(inner) = {lname}.{} {{ self.visit_expr(ast, db, inner); }}", field.1)?;

				writeln!(locate_trait, "\t\tif let Some(inner) = {lname}.{} {{ if self.visit_expr(ast, db, loc, inner) {{ return true; }} }}", field.1)?;
			}
			if field.0 == "StmtId" {
				writeln!(visit_trait, "\t\tself.visit_stmt(ast, db, {lname}.{});", field.1)?;

				writeln!(visit_immut_trait, "\t\tself.visit_stmt(ast, db, {lname}.{});", field.1)?;

				writeln!(locate_trait, "\t\tif self.visit_stmt(ast, db, loc, {lname}.{}) {{ return true; }}", field.1)?;
			}
			if field.0 == "Vec<StmtId>" {
				writeln!(visit_trait, "\t\tfor item in &{lname}.{} {{", field.1)?;
				writeln!(visit_trait, "\t\t\tself.visit_stmt(ast, db, *item);")?;
				writeln!(visit_trait, "\t\t}}")?;

				writeln!(visit_immut_trait, "\t\tfor item in &{lname}.{} {{", field.1)?;
				writeln!(visit_immut_trait, "\t\t\tself.visit_stmt(ast, db, *item);")?;
				writeln!(visit_immut_trait, "\t\t}}")?;

				writeln!(locate_trait, "\t\tfor item in &{lname}.{} {{", field.1)?;
				writeln!(locate_trait, "\t\t\tif self.visit_stmt(ast, db, loc, *item) {{ return true; }}")?;
				writeln!(locate_trait, "\t\t}}")?;
			}
		}
		// For the locate trait, we always unconditionally locate ourself if none
		// of our inner nodes returned true.
		writeln!(locate_trait, "\t\teprintln!(\"-> found @ {ty_name}\");")?;
		writeln!(locate_trait, "\t\tself.locate_{}(ast, db, loc, &{lname});", ty_name.to_ascii_lowercase())?;
		writeln!(locate_trait, "\t\treturn true;")?;
		writeln!(locate_trait, "\t}}")?;

		writeln!(visit_trait, "\t}}")?;

		writeln!(visit_trait_visit_fn, "\t\t\t{name}::{ty_name}(_inner) => {{")?;
		writeln!(visit_trait_visit_fn, "\t\t\t\tdrop(binding);")?;
		writeln!(visit_trait_visit_fn, "\t\t\t\tself.visit_{}(ast, db, id);", ty_name.to_ascii_lowercase())?;
		writeln!(visit_trait_visit_fn, "\t\t\t}}")?;


		writeln!(visit_immut_trait, "\t}}")?;

		writeln!(visit_immut_trait_visit_fn, "\t\t\t{name}::{ty_name}(_inner) => {{")?;
		writeln!(visit_immut_trait_visit_fn, "\t\t\t\tdrop(binding);")?;
		writeln!(visit_immut_trait_visit_fn, "\t\t\t\tself.visit_{}(ast, db, id);", ty_name.to_ascii_lowercase())?;
		writeln!(visit_immut_trait_visit_fn, "\t\t\t}}")?;



		writeln!(locate_trait_visit_fn, "\t\t\t{name}::{ty_name}(inner) => {{")?;
		writeln!(locate_trait_visit_fn, "\t\t\t\tself.visit_{}(ast, db, loc, inner)", ty_name.to_ascii_lowercase())?;
		writeln!(locate_trait_visit_fn, "\t\t\t}}")?;

		generate_constructor(false, &id_name, ast_field, name,
			ty_name,
			ConstructOpt { wrap_ok: false, to_enum: false, in_ast: false },
			&opt, &fields, &mut enum_impl)?;
		generate_constructor(false, &id_name, ast_field, name,
			ty_name,
			ConstructOpt { wrap_ok: false, to_enum: true , in_ast: false },
			&opt, &fields, &mut enum_impl)?;
		generate_constructor(false, &id_name, ast_field, name,
			ty_name,
			ConstructOpt { wrap_ok: true , to_enum: false, in_ast: false },
			&opt, &fields, &mut enum_impl)?;
		generate_constructor(false, &id_name, ast_field, name,
			ty_name,
			ConstructOpt { wrap_ok: true , to_enum: true , in_ast: false },
			&opt, &fields, &mut enum_impl)?;

		generate_constructor(false, &id_name, ast_field, name,
			ty_name,
			ConstructOpt { wrap_ok: false, to_enum: true , in_ast: true  },
			&opt, &fields, &mut enum_impl)?;
		generate_constructor(false, &id_name, ast_field, name,
			ty_name,
			ConstructOpt { wrap_ok: true , to_enum: true , in_ast: true  },
			&opt, &fields, &mut enum_impl)?;

		generate_constructor(true, &id_name, ast_field, name,
			ty_name,
			ConstructOpt { wrap_ok: false, to_enum: true , in_ast: true  },
			&opt, &fields, &mut enum_impl)?;
		generate_constructor(true, &id_name, ast_field, name,
			ty_name,
			ConstructOpt { wrap_ok: true , to_enum: true , in_ast: true  },
			&opt, &fields, &mut enum_impl)?;
	}

	writeln!(loc_match, "\t\t}}")?;
	writeln!(loc_match, "\t}}")?;
	
	writeln!(enum_impl, "{}", loc_match)?;

	writeln!(enum_impl, "}}")?;

	writeln!(enum_def, "}}")?;

	writeln!(debug_impl, "\t\t}}")?;
	writeln!(debug_impl, "\t}}")?;
	writeln!(debug_impl, "}}")?;

	writeln!(visit_trait_visit_fn, "\t\t}}")?;	
	writeln!(visit_trait_visit_fn, "\t}}")?;
	writeln!(visit_trait, "{}", visit_trait_visit_fn)?;

	writeln!(visit_immut_trait_visit_fn, "\t\t}}")?;	
	writeln!(visit_immut_trait_visit_fn, "\t}}")?;
	writeln!(visit_immut_trait, "{}", visit_immut_trait_visit_fn)?;

	writeln!(locate_trait_visit_fn, "\t\t}}")?;	
	writeln!(locate_trait_visit_fn, "\t}}")?;
	writeln!(locate_trait, "{}", locate_trait_visit_fn)?;

	{
		use std::io::Write;
		write!(file, "{}\n", struct_defs).unwrap();
		write!(file, "{}\n", enum_def).unwrap();
		write!(file, "{}\n", enum_impl).unwrap();
		write!(file, "{}\n", debug_impl).unwrap();
	}

	Ok(())
}

pub fn generate(file: &mut File) {
	let expr_spec = r#"

	Binary        : Tok op, Expr left, Expr right, TypId typ
	Unary         : Tok op, Expr inner, TypId typ
	Comparison    : Tok op, Expr left, Expr right, TypId compare_as
	Variable      : VarId identity
	Logical       : Tok op, Expr left, Expr right
	FunCall       : SourceLocation fn_name, FunId identity, Vec<Expr> args, Option<Expr> object
	FunDeclare    : FunId identity, Expr value, TypId typ
	ValCall       : Expr value, Vec<Expr> args, SigId sig
	FunCapture    : SourceLocation fn_name, FunId identity, TypId typ, Option<Expr> object
	Assign        : SourceLocation var_name, VarId identity, Expr value
	UnboundAssign : Token identifier, Expr value
	NumLiteral    : Token contents, TypId typ
	StrLiteral    : StrConstId id
	BoolLiteral   : bool value
	Block         : Vec<Stmt> stmts, TypId typ
	If            : Expr condition, Expr then_branch, Option<Expr> else_branch, TypId typ
	Unbound       : Token identifier
	UnboundFunCapture : Token identifier, Option<Expr> object
	Print         : Vec<Expr> exprs, TypId typ
	Str           : Vec<Expr> exprs
	New           : Token identifier, ClassId class, TypId typ, Vec<NewInitElem> initializers
	Get           : Token identifier, Expr lhs, VarId var
	Set           : Token identifier, Expr lhs, VarId var, Expr rhs
	SelfVal       : TypId typ
	ArrayLit      : Vec<Expr> values, TypId elem_typ, TypId arr_typ
	Index         : Expr value, Expr index, TypId typ
	SetIndex      : Expr value, Expr index, TypId typ, Expr rhs
	MakeTuple     : Vec<Expr> values, TypId typ
	MakeRange     : Expr left, Expr right, RangeEnd left_end, RangeEnd right_end, TypId typ
	Promote       : Expr inner, TypId promote_to
	Lerp          : Expr from, Expr to, Expr amount, TypId typ
	MakeSumType   : TypId typ
	OptionElse    : Expr value, Expr otherwise, TypId typ
	Loop          : Expr inner, TypId typ, Vec<ExprId> breaks
	Break         : Option<Expr> value
	WhileLoop     : Expr condition, Expr inner, TypId typ, Vec<ExprId> breaks
	ForLoop       : SourceLocation ident, VarId identity, Expr iterator, bool has_explicit_type, Expr inner
	Undefined     : 

	"#;

	// 	FunDeclare : FunId identity, Vec<VarId> args, 
	let stmt_spec = r#"
	
	Declare      : SourceLocation ident, VarId identity, Expr value, bool has_explicit_type
	Expression   : Expr expression
	Return       : Option<Expr> expression
	ClassDeclare : ClassId identity, Vec<FunDeclare> funs, Vec<Declare> vars

	"#;

	let expr_opt = Opt {
		// Exprs inside exprs must be boxed.
		box_exprs: true,
	};

	let stmt_opt = Opt {
		box_exprs: true,
	};

	let mut visit_trait = String::new();
	let mut visit_immut_trait = String::new();
	let mut locate_trait = String::new();

	writeln!(visit_trait, "#[allow(unused_variables)]\npub trait VisitAst {{").unwrap();
	writeln!(visit_immut_trait, "#[allow(unused_variables)]\npub trait VisitAstImmut {{").unwrap();
	writeln!(locate_trait, "#[allow(unused_variables)]\npub trait LocateAst {{").unwrap();

	generate_spec("Expr", "exprs", expr_spec, expr_opt, file, &mut visit_trait, &mut visit_immut_trait, &mut locate_trait).unwrap();
	generate_spec("Stmt", "stmts", stmt_spec, stmt_opt, file, &mut visit_trait, &mut visit_immut_trait, &mut locate_trait).unwrap();

	writeln!(locate_trait, "	fn visit_ast(&mut self, ast: &Ast, db: &Db, loc: &SourceLocation) {{
		// Locate the value into the relevant source itself.
		// Unlike other kinds of visitors, we only have to visit one Source.
		let source = ast.sources.get(loc.source);
		let module = &source.module;

		for it in &module.globals {{
			self.visit_declare(ast, db, loc, it);
		}}

		for it in &module.functions {{
			self.visit_fundeclare(ast, db, loc, it);
		}}

		for it in &module.classes {{
			self.visit_classdeclare(ast, db, loc, it);
		}}
	}}").unwrap();

	writeln!(locate_trait, "}}").unwrap();
	writeln!(visit_trait, "}}").unwrap();
	writeln!(visit_immut_trait, "}}").unwrap();

	{
		use std::io::Write;
		write!(file, "{}\n", visit_trait).unwrap();
		write!(file, "{}\n", visit_immut_trait).unwrap();
		write!(file, "{}\n", locate_trait).unwrap();
	}
}