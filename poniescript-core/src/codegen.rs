pub mod single_codegen;

use crossbeam::channel;
use std::fs::File;
use std::io::Write;

use poni_arena::ArenaKey;
use crate::expr::ImportKind;
use crate::{db::*, Args};
use crate::typ::Type;

use std::io::BufWriter;
use std::sync::Arc;

use crate::{inf_write, inf_writeln};

pub use single_codegen::*;

pub enum CodegenTask {
	CompileFunction(FunId),
}

pub struct CodegenCoordinator {
	db: &'static Db,
}

/// Some of the output buffers used for code generation. Separate from
/// Codegen so that we can pass them to self.methods() without borrow checker
/// errors.
struct CodegenOutputs {
	/// Buffer containing the declarations (e.g. extern ...) for all global variables.
	global_declare: String,

	/// Buffer containing the definitions for all global variables.
	global_define: String,
			
	/// Buffer containing the initialization code for all global variables.
	global_init: String,

	/// Declarations of string constants (e.g. extern const char*)
	string_const_declare: String,
	/// Definitions of string constants (e.g. const char * = NULL)
	string_const_define: String,
	string_const_init: String,

	fun_declare: String,
	struct_declare: String,
	struct_define: String,
}

impl CodegenOutputs {
	pub fn new() -> Self {
		CodegenOutputs {
			global_declare: String::new(),
			global_define: String::new(),
			global_init: String::new(),

			string_const_declare: String::new(),
			string_const_define: String::new(),
			string_const_init: String::new(),

			fun_declare: String::new(),
			struct_declare: String::new(),
			struct_define: String::new(),
		}
	}
}

impl CodegenCoordinator {
	fn compile_class_declare(&mut self, class: ClassId, out: &mut CodegenOutputs) {
		if class == self.db.class_unassigned { return; }
		
		// Write the struct declaration. These must come before signature declarations
		// in case the signature needs to use the struct; The signature declarations
		// must then come before structs in case the struct needs to use the signature.
		inf_writeln!(out.struct_declare, "struct {};", self.db.get_class_cname(class));
	}

	fn compile_class_define(&mut self, class: ClassId, out: &mut CodegenOutputs) {
		if class == self.db.class_unassigned { return; }
		
		// Don't generate struct definitinos for CHeader imports, for now.
		if matches!(self.db.get(class).import_kind, ImportKind::CHeader) { return; }

		// Write the struct definition.
		inf_writeln!(out.struct_define, "struct {} {{", self.db.get_class_cname(class));

		// The object used for garbage collection / virtual dispatch.
		inf_writeln!(out.struct_define, "\tstruct ps_object object;");

		if let Some(parent) = self.db.get(class).parent {
			inf_writeln!(out.struct_define, "\tstruct {} *parent;", self.db.get_class_cname(parent));
		}

		for var in &self.db.get(class).vars {
			// Compile the variable declaration into the struct.
			inf_writeln!(out.struct_define, "\t{} {};", self.db.get_var_ctype(*var), self.db.get_cname(*var));
		}

		inf_writeln!(out.struct_define, "}};");
	}

	fn compile_fun_declare(&mut self, fun: FunId, out: &mut CodegenOutputs) {
		let is_init = self.db.fun_init == Some(fun);

		// Don't write declaration for the init() function.
		if !is_init {
			// TODO: Possibly write directly to Out::FunDeclare
			inf_writeln!(out.fun_declare, "{} {}({});",
				self.db.get_fun_ret_ctype(fun),
				self.db.get_fun_cname(fun),
				self.db.get_fun_cparams(fun));
		}
	}

	fn compile_string_constant_init(&mut self, declare: &mut String, define: &mut String, init: &mut String) {
		inf_writeln!(init, "void poni_init_strings(struct poni_gc_context *ctx) {{");
		for id in self.db.iter_strconst() {
			inf_writeln!(declare, "extern const ps_str *ps_str_const{};", id.to_index());
			inf_writeln!(define, "const ps_str* ps_str_const{} = NULL;", id.to_index());
			inf_writeln!(init, "\tps_str_const{} = ps_str_from_literal(ctx, {});",
				id.to_index(), self.db.get(id));
		}
		inf_writeln!(init, "}}");
	}

	fn codegen_gc_stride(&self) -> String {
		let mut type_stride = "static inline size_t
poni_get_type_stride(uint64_t tag) {
	switch(tag) {
		// Memory safety: Don't let us access an invalid size.
		default:
			abort();
			return 0;
		case PONI_TAG_STRCONST:
		case PONI_TAG_STR:
		case PONI_TAG_STRBUF:
		case PONI_TAG_ARRAY:
		case PONI_TAG_DYNARRAY:
			return sizeof(void*);
		case PONI_TAG_FLOAT: return sizeof(ps_float);
		case PONI_TAG_INT:   return sizeof(ps_int);
		case PONI_TAG_BOOL:  return sizeof(ps_bool);
".to_string();

		let mut ptr_types = String::new();
		let mut fun_types = String::new();
		let mut funraw_types = String::new();

		for typ in self.db.iter_typ() {
			// Skip types that aren't cgen safe
			if !self.db.is_cgen_safe(typ) { continue; }

			let tag = self.db.get_type_ctag(typ);
			match self.db.get(typ) {
				// Primitive types already done
				Type::Int | Type::Float | Type::Bool => { continue; }

				// Illegal
				Type::Void | Type::Bottom => { continue; }

				// Already done
				Type::StrConst | Type::StrBuf | Type::Str | Type::ArrayOf(_) | Type::DynArrayOf(..) => { continue; }

				// Build up one big set of pointer types.
				Type::Class(_) => {
					inf_writeln!(ptr_types, "\t\tcase {}:", tag);
				}

				Type::Option(id) => {
					if self.db.is_value_type(*id) {
						todo!()
					}
					else {
						// If it's a reference type, we're re-using the type id,
						// so we actually don't need a case at all.
					}
				}

				Type::Fun(_) => { inf_writeln!(fun_types, "\t\tcase {}:", tag); }
				Type::FunRaw(_) => { inf_writeln!(funraw_types, "\t\tcase {}:", tag); }

				// Value types should each return their sizeof.
				Type::Tuple(_) | Type::RangeOf(..) => {
					inf_writeln!(type_stride, "\t\tcase {}: return sizeof({});", tag, self.db.get_ctype(typ));
				}

				Type::AssumeFloat | Type::AssumeInt | Type::Unassigned
				| Type::UnboundCStructPtr(_) | Type::UnboundIdent(_) => { continue; }
			}
		}

		if ptr_types.len() > 0 {
			inf_writeln!(ptr_types, "\t\t\treturn sizeof(void*);");
		}
		if fun_types.len() > 0 {
			// This should be valid on each compiler.
			inf_writeln!(fun_types, "\t\t\treturn sizeof(struct {{ void (*fn)(void); void *closure; }});");
		}
		if funraw_types.len() > 0 {
			inf_writeln!(funraw_types, "\t\t\treturn sizeof(void (*)(void))")
		}

		inf_write!(type_stride, "{}{}{}", ptr_types, fun_types, funraw_types);

		inf_writeln!(type_stride, "\t}}\n}}");
		type_stride
	}


	fn codegen_gc_functions(&self) -> String {
		let type_stride = self.codegen_gc_stride();

		let is_valuetype = "static inline ps_bool
poni_is_value_type(uint64_t tag) { return !!(tag & 0x8000000000000000ULL); }
".to_string();

		let mut valuetype = "void
poni_gc_visit_valuetype(struct poni_gc *gc, void *object, uint64_t tag) {
	switch(tag) {
".to_string();
		let mut visit_object = "void
poni_gc_visit_object(struct poni_gc *gc, void *object) {
    uint64_t tag = *(uint64_t*)object;
    switch(tag & 0xFFFFFFFFFFFFFFFEULL) {
		case PONI_TAG_STRCONST:
		case PONI_TAG_STR:
			break; // Nothing to do
		case PONI_TAG_STRBUF: {
			struct ps_strbuf *self = object;
			poni_gc_mark(gc, self->buffer);
			break;
		}
		case PONI_TAG_ARRAY: {
			struct ps_array_header *header = object;
			char *elem_root = (char*)object + sizeof(struct ps_array_header);
			if(poni_is_value_type(header->type)) {
				// As an optimization, never visit any objects inside an
				// array of primitive types. We should probably have an additional
				// type info function that tells us whether we need to iterate
				// here.
				if(header->type == PONI_TAG_INT || header->type == PONI_TAG_FLOAT
					|| header->type == PONI_TAG_BOOL)
				{ break; }

				size_t stride = poni_get_type_stride(header->type);

				// For value types, the inner objects do not themselves need
				// to be marked; so instead of going through the gc marker,
				// instead just visit them directly.
				for(ps_int i = 0; i < header->length; ++i) {
					poni_gc_visit_valuetype(gc, elem_root, header->type);
					elem_root += stride;
				}
			}
			else {
				size_t stride = poni_get_type_stride(header->type);

				for(ps_int i = 0; i < header->length; ++i) {
					poni_gc_mark(gc, elem_root);
					elem_root += stride;
				}
			}
			break;
		}
		case PONI_TAG_DYNARRAY: {
			struct ps_dynarray_header *header = object;
			// For memory safety reasons, we need to be walking a known-good
			// pointer. So, store the pointer ahead of time, as its size
			// is immutable.
			//
			// We also cannot directly mark the inner buffer. The problem is
			// that it may have undefined contents, outside of the boundaries
			// of this array. So, instead we directly mark the inner buffer,
			// and then walk the children manually.
			struct ps_array_header *inner = header->buffer;
			char *elem_root = (char*)inner + sizeof(struct ps_array_header);

			// Note that although the DynArray's length might be changed by
			// another thread, we shouldn't have any code that can invalidate
			// existing objects in the array (aside from maybe this code).
			//
			// So even if the length decreases after we read it, we shouldn't
			// end up reading an invalid object.
			//
			// (*writes* to the length will perhaps have to be atomic-acquire?
			// they *must* occur *after* any writes to the buffer contents).
			ps_int length = header->length;
			if(inner->length < length) { length = inner->length; }

			// Now the rest of the logic is essentially the same as the regular
			// arrays.
			if(poni_is_value_type(header->type)) {
				if(header->type == PONI_TAG_INT || header->type == PONI_TAG_FLOAT
					|| header->type == PONI_TAG_BOOL)
				{ break; }

				size_t stride = poni_get_type_stride(header->type);

				for(ps_int i = 0; i < length; ++i) {
					poni_gc_visit_valuetype(gc, elem_root, header->type);
					elem_root += stride;
				}
			}
			else {
				size_t stride = poni_get_type_stride(header->type);

				for(ps_int i = 0; i < length; ++i) {
					poni_gc_mark(gc, elem_root);
					elem_root += stride;
				}
			}

			break;
		}
".to_string();

	let mut visit_roots = "void
poni_gc_visit_roots(struct poni_gc *gc) {
".to_string();

	let mut allocation_size = "size_t
poni_gc_get_allocation_size(void *object) {
	uint64_t tag = *(uint64_t*)object;
	switch(tag) {
		// Memory safety: Don't let us access an invalid size.
		default:
			abort();
			return 0;
		case PONI_TAG_STRCONST:
		case PONI_TAG_STR:
		{
			struct ps_str *self = object;
			return sizeof(*self) + self->length;
		}
		case PONI_TAG_STRBUF: {
			return sizeof(struct ps_strbuf);
		}
		case PONI_TAG_ARRAY: {
			struct ps_array_header *header = object;
			size_t stride = poni_get_type_stride(header->type);
			return sizeof(*header) + stride * header->length;
		}
		case PONI_TAG_DYNARRAY: {
			return sizeof(struct ps_dynarray_header);
		}
".to_string();

		for typ in self.db.iter_typ() {
			if !self.db.is_cgen_safe(typ) { continue; }

			let tag = self.db.get_type_ctag(typ);
			match self.db.get(typ) {
				Type::Class(id) => {
					inf_writeln!(visit_object, "\tcase {}: {{", tag);
					inf_writeln!(visit_object, "\t\tstruct {} *self = object;", self.db.get_class_cname(*id));
					for field in &self.db.get(*id).vars {
						let field_ty = self.db.get(*field).typ;

						match self.db.get(field_ty) {
							Type::Int | Type::Float | Type::Bool => {}
							Type::Void | Type::Bottom => {}

							Type::StrConst => {
								// For now, we don't mark StrConst, because
								// they can't be deallocated.
							}

							Type::Str | Type::StrBuf | Type::Class(_) | Type::ArrayOf(_) | Type::DynArrayOf(..) => {
								inf_writeln!(visit_object, "\t\tponi_gc_mark(gc, self->{});", self.db.get_cname(*field));
							}

							Type::Option(id) => {
								match self.db.get(*id) {
									Type::Str | Type::StrBuf | Type::Class(_) | Type::ArrayOf(_) | Type::DynArrayOf(..) => {
										inf_writeln!(visit_object, "\t\tponi_gc_mark(gc, self->{});", self.db.get_cname(*field));
									},
									_ => todo!()
								}
							}

							// Nothing to visit.
							Type::FunRaw(_) => {}

							Type::Fun(_) | Type::Tuple(_) | Type::RangeOf(..) => {
								let inner_tag = self.db.get_type_ctag(field_ty);
								inf_writeln!(visit_object, "\t\tponi_gc_visit_valuetype(gc, &self->{}, {});",
									self.db.get_cname(*field),
									inner_tag);
							}

							Type::Unassigned | Type::AssumeFloat | Type::AssumeInt | Type::UnboundIdent(_) | Type::UnboundCStructPtr(_) => {}
						}
					}

					inf_writeln!(visit_object, "\t\tbreak;");
					inf_writeln!(visit_object, "\t}}");
				},

				Type::Tuple(typs) => {
					inf_writeln!(valuetype, "\tcase {}: {{", tag);
					inf_writeln!(valuetype, "\t\t{} *self = object;", self.db.get_ctype(typ));

					for (idx, typ) in typs.iter().enumerate() {
						match self.db.get(*typ) {
							Type::Int | Type::Float | Type::Bool => {}
							Type::Void | Type::Bottom => {}

							Type::StrConst => {
								// For now, we don't mark StrConst, because
								// they can't be deallocated.
							}

							Type::Str | Type::StrBuf | Type::Class(_) | Type::ArrayOf(_) | Type::DynArrayOf(..) => {
								inf_writeln!(valuetype, "\t\tponi_gc_mark(gc, self->v_{});", idx);
							}

							Type::Option(id) => {
								log::trace!("gc tuple field with type {}?", self.db.repr_type(*id));
								match self.db.get(*id) {
									Type::StrConst => {}
									Type::Str | Type::StrBuf | Type::Class(_) | Type::ArrayOf(_) => {
										inf_writeln!(valuetype, "\t\tponi_gc_mark(gc, self->v_{});", idx);
									},
									_ => todo!()
								}
							}

							// Nothing to visit.
							Type::FunRaw(_) => {}

							Type::Fun(_) | Type::Tuple(_) | Type::RangeOf(..) => {
								let inner_tag = self.db.get_type_ctag(*typ);
								inf_writeln!(valuetype, "\t\tponi_gc_visit_valuetype(gc, &self->v_{}, {});",
									idx, inner_tag);
							}

							Type::Unassigned | Type::AssumeFloat | Type::AssumeInt | Type::UnboundIdent(_) | Type::UnboundCStructPtr(_) => {}
						}
					}

					inf_writeln!(valuetype, "\t\tbreak;");
					inf_writeln!(valuetype, "\t}}");
				},

				Type::Fun(sig) => {
					// Don't generate these for unused sigs -- they might not
					// be valid C, so we can't generate them; and they won't
					// be needed anyway.
					if !self.db.is_sig_used(*sig) { continue; }

					inf_writeln!(valuetype, "\tcase {}: {{", tag);
					inf_writeln!(valuetype, "\t\t{} *self = object;", self.db.get_ctype(typ));
					// Visit the closure for each fun.
					// We could make this particular bit of code some sort of
					// helper function / case-that-falls-through for each function,
					// but this is fine for now.
					inf_writeln!(valuetype, "\t\tponi_gc_mark(gc, self->closure);");
					inf_writeln!(valuetype, "\t\tbreak;");
					inf_writeln!(valuetype, "\t}}");
				}

				_ => {}
			}
		}

		inf_writeln!(valuetype, "\t}}\n}}");
		inf_writeln!(visit_object, "\t}}\n}}");
		inf_writeln!(allocation_size, "\t}}\n}}");

		inf_writeln!(visit_roots, "}}");

		// Just concatenate everything together.
		//
		// It might be cleaner if we just append each of these as separate buffers...
		format!("{type_stride}{is_valuetype}{valuetype}{visit_object}{visit_roots}{allocation_size}")
	}

	fn write_prelude(
		&self,
		output: &mut BufWriter<Box<dyn std::io::Write + Send + 'static>>,
		outputs: &CodegenOutputs,
		args: &Args,
		do_support_fns: bool,
	) -> std::io::Result<()> {
		// Every output file needs the prelude.
		writeln!(output, "#include \"poni/poni.h\"")?;		
		if args.hot {
			writeln!(output, "#include \"poni/poni_hot.h\"")?;
		}

		writeln!(output, "// --- imports ---\n")?;
		for path in &args.imports {
			writeln!(output, "#include \"{}\"", path.display())?;
		}

		writeln!(output, "// --- tag definitions ---\n{}", self.db.tag_define_code)?;

		writeln!(output, "// --- string constants ---\n{}", if do_support_fns { &outputs.string_const_define }
			else { &outputs.string_const_declare })?;
		writeln!(output, "// --- struct declarations ---\n")?;
		
		output.write_all(outputs.struct_declare.as_bytes())?;
		writeln!(output, "// --- struct declarations (ps_tuple) ---\n{}", self.db.valty_declare_code)?;
		writeln!(output, "// --- struct declarations (ps_array) ---\n{}", self.db.arr_declare_code)?;
		writeln!(output, "// --- sig types ---\n{}", self.db.sig_declare_code)?;
		
		writeln!(output, "// --- struct definitions (ps_tuple) ---\n{}", self.db.valty_define_code)?;
		writeln!(output, "// --- struct definitions (ps_array) ---\n{}", self.db.arr_define_code)?;

		// I believe these have to come after the ps_tuple, because they might
		// refer to tuples.
		writeln!(output, "// --- struct definitions ---")?;
		
		output.write_all(outputs.struct_define.as_bytes())?;

		writeln!(output, "// --- global variables ---\n{}", if do_support_fns { &outputs.global_define }
			else { &outputs.global_declare })?;
		writeln!(output, "// --- function declarations ---")?;
		
		output.write_all(outputs.fun_declare.as_bytes())?;

		Ok(())
	}

	fn per_writer_codegen(
		&self,
		writer: BufWriter<Box<dyn std::io::Write + Send + 'static>>,
		recv: channel::Receiver<String>,
		outputs: &CodegenOutputs,
		args: &Args,
		do_support_fns: bool,
	) -> std::io::Result<()> {
		let mut output = writer;

		// Engine code does not include poni_standalone.h.
		if !args.engine {
			// Only one .c/o file should include poni_standalone.
			if do_support_fns {
				writeln!(output, "#include \"poni/poni_standalone.h\"")?;
			}
		}

		if let Some(prelude_h) = &args.prelude_h {
			writeln!(output, "#include \"{}\"", prelude_h.display())?;
			if do_support_fns {
				// THe support-fns writer in this case has to write the initializers
				// for various globals.
				writeln!(output, "// --- string constant definitions ---\n{}", outputs.string_const_define)?;
				writeln!(output, "// --- global variable definitions ---\n{}", outputs.global_define)?;
			}
		}
		else {
			self.write_prelude(&mut output, outputs, args, do_support_fns)?;
		}
		
		if do_support_fns {
			writeln!(output, "{}", outputs.string_const_init)?;

			writeln!(output, "// --- gc support ---")?;
			writeln!(output, "{}", self.codegen_gc_functions())?;

			// The various poni initializer functions are split into several pieces,
			// so as to enable hot code reloading.
			writeln!(output, "void poni_init_globals(struct poni_gc_context *ctx) {{")?;
			if args.hot {
				// For hot-code reloading, we need the bool flag 'existed' to decide
				// whether to run each initializer
				writeln!(output, "\tbool existed = false;")?;
			}
			writeln!(output, "{}", outputs.global_init)?;
			writeln!(output, "}}")?;

			if self.db.fun_init.is_none() {
				// For now, if there is no init function, we still have to define
				// an empty body of it to avoid a link error.
				writeln!(output, "void poni_init(struct poni_gc_context *ctx) {{}}")?;
			}
		}

		writeln!(output, "// --- function definitions ---")?;

		loop {
			let Ok(next) = recv.recv() else { break; };
			// Just blit buffers of text as we receive them. Round robin them
			// to the different writers, to keep the load about equal.
			output.write_all(next.as_bytes())?;
		}

		// Because we're using a BufWriter, it is important to flush it.
		output.flush()?;

		Ok(())
	}

	fn codegen(&mut self, args: &Args, ast: Arc<AstReadonly>, outputs: Vec<Box<dyn std::io::Write + Send + 'static>>) -> std::io::Result<()> {
		// Default to 8 if no value specified.
		let thread_count = if args.codegen_threads > 0 { args.codegen_threads } else { 8 };

		let mut task_sets: Vec<_> = std::iter::repeat_with(|| Vec::new())
			.take(thread_count)
			.collect();
		for (n, fun) in self.db.iter_fun().enumerate() {
			task_sets[n % thread_count].push(CodegenTask::CompileFunction(fun));
		}

		let mut sends = Vec::new();
		let mut recvs = Vec::new();
		for _ in &outputs {
			let (send, recv) = channel::unbounded();
			sends.push(send);
			recvs.push(recv);
		}

		let send0 = sends[0].clone();
		
		{
			// In order to improve upon the overhead of starting threads, we
			// start the first thread, then have it start the rest, as we move on
			// to other codegen tasks.
			let send = sends[0].clone();
			let ast = Arc::clone(&ast);
			let db = self.db;
			std::thread::spawn(move || {
				// Distribute senders to the main codegen threads in a round-robin
				// fashion. This should work decently well.
				let mut send_idx = 1 % sends.len();

				for (idx, task_set) in task_sets.into_iter().enumerate() {
					// For the last task, we will just handle it ourselves.
					if idx == thread_count - 1 {
						let mut cg = Codegen::new(db, send.clone());
						cg.handle_tasks(Arc::clone(&ast), task_set);
					}
					// Otherwise, spawn more threads.
					else {
						let send = sends[send_idx].clone();
						send_idx = (send_idx + 1) % sends.len();
						let ast = Arc::clone(&ast);
						std::thread::spawn(|| {
							let mut cg = Codegen::new(db, send);
							cg.handle_tasks(ast, task_set);
						});
					}
				}
			});
		}

		// Use a big capacity for our BufWriter, at least for now.
		//
		// In the future, probably what we will want to do is use a smaller capacity,
		// especially when writing to e.g. a piped compiler, so that it can start
		// receiving input faster.
		let mut writers = Vec::new();
		for writer in outputs {
			writers.push(BufWriter::with_capacity(1024 * 1024, writer));
		}
		let mut outputs = CodegenOutputs::new();

		// Generate global variables in one pass as their ordering is a global
		// property.

		// Disable GC frame for now; we don't bother with one in the globals
		// initializer (it shouldn't be able to GC).

		// For any extras in the globals code, just send it to the 0th channel.
		let mut cg = Codegen::new(&self.db, send0);

		cg.disable_gc_frames = true;

		// Use an indent level of 1 for the initialization code for all global variables.
		cg.indent_level = 1;
		for global in &self.db.globals {
			let global = *global;

			let Some(initializer) = self.db.get(global).initializer else {
				// If there is no initializer, this must be an extern variable,
				// so we don't codegen its initializer.
				continue;
				//panic!("ICE: Codegen of global variable without initializer");
			};

			// Just dierectly encode the indentation..
			let mut global_name = self.db.get_cname(global);
			if args.hot {
				// To contend with the Hot option, we have to chop off the ( )
				// surrounding the variable name when declaring it.
				//
				// Note that we leave the * on, because it does stuff for us.
				global_name = &global_name[1..global_name.len() - 1];
			}
			inf_writeln!(outputs.global_declare, "extern {} {};",
				self.db.get_var_ctype(global), global_name);
			inf_writeln!(outputs.global_define, "{} {};",
				self.db.get_var_ctype(global), global_name);

			

			// In hot-code reloading, we need to do two things:
			// 1. Allocate the variable based on a pointer.
			// 2. Initialize it, if it *didn't* already exist.
			if args.hot {
				let indent = cg.indent();
				inf_writeln!(outputs.global_init, "{}{} = poni_hot_lookup(\"{} {};\", sizeof({}), &existed);",
					indent,
					// HACK: Chop off the * (the () have already been chopped)
					&global_name[1..],
					// Replicate the initializer
					self.db.get_var_ctype(global), self.db.get_cname(global),
					// For sizeof() we do want the * cause that tells us the
					// real size
					self.db.get_cname(global));
				inf_writeln!(outputs.global_init, "{}if(!existed) {{", indent);

				cg.indent_level += 1;
			}

			// For globals, the initializer is not itself a declaration. So,
			// do tell self.compile_assign() that it's not a declaration.
			cg.compile_assign(&ast, global,
				initializer,
				&mut outputs.global_init,
				false);

			// Finish hot compilation
			if args.hot {
				cg.indent_level -= 1;

				let indent = cg.indent();
				inf_writeln!(outputs.global_init, "{}}}", indent);
			}
		}

		cg.disable_gc_frames = false;

		// Ensure there's no extra open senders.
		drop(cg);

		self.compile_string_constant_init(
			&mut outputs.string_const_declare,
			&mut outputs.string_const_define,
			&mut outputs.string_const_init
		);

		for class in self.db.iter_class() {
			self.compile_class_declare(class, &mut outputs);
		}

		for class in self.db.iter_class() {
			self.compile_class_define(class, &mut outputs);
		}

		for fun in self.db.iter_fun() {
			self.compile_fun_declare(fun, &mut outputs);
		}

		// If we're writing a prelude_h, we have to do that before any of
		// the other codegen threads.
		if let Some(prelude_h) = &args.prelude_h {
			let prelude = File::create(prelude_h)?;
			let mut writer = BufWriter::new(Box::new(prelude) as _);
			self.write_prelude(&mut writer, &outputs, args, false)?;
		}

		std::thread::scope(|s| {
			let mut do_support_fns = true;
			for (writer, recv) in writers.into_iter().zip(recvs.into_iter()) {
				{
					let do_support_fns = do_support_fns;
					let outputs = &outputs;
					let the_self = &self;
					s.spawn(move || {
						Self::per_writer_codegen(the_self, writer, recv, &outputs, args, do_support_fns)
					});
				}
				do_support_fns = false;
			}
		});

		Ok(())
	}
}

pub fn codegen(args: &Args, db: &'static Db, ast: Arc<AstReadonly>, output: Vec<Box<dyn std::io::Write + Send>>) -> std::io::Result<()> {
	//let mut codegen = Codegen::new(db);

	//codegen.codegen(args, ast, output)

	let mut coordinator = CodegenCoordinator {
		db,
	};

	coordinator.codegen(args, ast, output)
}