use crate::db::*;
use crate::codegen::*;
use crate::define_val;
use crate::expr::BuiltinCall;
use crate::expr::Sig;
use crate::inf_writeln;
use crate::inf_write;
use crate::typ::Type;

pub struct DynarrayPush;

impl BuiltinMethod for DynarrayPush {
    fn get_types(&self, db: &mut Db, self_ty: TypId) -> (TypId, Vec<TypId>) {
        // DynArray[T]::push(T) -> void
        match db.get(self_ty) {
            Type::DynArrayOf(elem_ty, _) => (db.types.void, vec![*elem_ty]),
            _ => unreachable!()
        }
    }

    fn compile(
        &self,
        codegen: &mut Codegen,
        _ast_node: &BuiltinCall,
        _ast: &AstReadonly,
        self_val: TypedVal,
        arg_vals: Vec<TypedVal>,
        into: &mut String
    ) -> TypedVal {
        let Type::DynArrayOf(elem_ty, arr_ty) = self_val.get_type(codegen.db) else { unreachable!() };
        let [new_item] = arg_vals.as_slice() else { unreachable!() };

        let indent = codegen.indent();

        let idx = codegen.new_val_typed(codegen.db.types.int);

        let inner_array = format!("(({}){}->header.buffer)",
            codegen.db.get_ctype(*arr_ty), self_val);

        // The index that we want to use is the current length of the array.
        define_val!(codegen, into, idx, " = {}->header.length;\n", self_val);
        inf_writeln!(into, "{}if({} < {}->header.length) {{",
            indent, idx, inner_array);
        inf_writeln!(into, "{}\t{}->contents[{}] = {};", indent, inner_array, idx, new_item);
        inf_writeln!(into, "{}}}", indent);
        inf_writeln!(into, "{}else {{", indent);
        // Ensure that we have idx room in the array.
        inf_writeln!(into, "{}\t{}->header.buffer = poni_array_ensure(ctx, {}->header.buffer, sizeof({}), {});",
            indent, self_val, self_val, codegen.db.get_ctype(*elem_ty), idx);
        inf_writeln!(into, "{}\t{}->contents[{}] = {};", indent, inner_array, idx, new_item);
        inf_writeln!(into, "{}}}", indent);

        // This is one of the most subtle things, at least it will be.
        //
        // We probably (?) want this to be an atomic increment, and one that
        // is guaranteed to be occur-after the buffer write.
        //
        // That said, it is safe to under-estimate the size.
        //
        // The other thing TODO is to check that we don't overflow the
        // length variable. This is very unlikely to happen any time soon,
        // however.
        inf_writeln!(into, "{}{}->header.length += 1;", indent, self_val);
        
        Val::Void.typed(codegen.db.types.void, None)
    }
}

pub struct DynarrayAny {
    /// Instead of doing any(), do all(). (They are very similar operations).
    pub all: bool,
}

impl BuiltinMethod for DynarrayAny {
    fn get_types(&self, db: &mut Db, self_ty: TypId) -> (TypId, Vec<TypId>) {
        // DynArray[T]::push(T) -> void
        match db.get(self_ty) {
            Type::DynArrayOf(elem_ty, _) => {
                // We want a fun(Elem) -> bool
                let sig = Sig {
                    parameters: vec![*elem_ty],
                    return_type: db.types.bool
                };

                let sig = db.put_sig(&sig);
                let fun_typ = db.put_type(Type::Fun(sig));

                (db.types.bool, vec![fun_typ])
            }
            _ => unreachable!()
        }
    }

    fn compile(
        &self,
        codegen: &mut Codegen,
        _ast_node: &BuiltinCall,
        _ast: &AstReadonly,
        self_val: TypedVal,
        arg_vals: Vec<TypedVal>,
        into: &mut String
    ) -> TypedVal {
        let Type::DynArrayOf(_, arr_ty) = self_val.get_type(codegen.db) else { unreachable!() };
        let [fun] = arg_vals.as_slice() else { unreachable!() };

        let indent = codegen.indent();

        let idx = codegen.new_val_typed(codegen.db.types.int);

        let inner_array = format!("(({}){}->header.buffer)",
            codegen.db.get_ctype(*arr_ty), self_val);

        let val = codegen.new_val_typed(codegen.db.types.bool);
        // any starts at false, all starts at true.
        define_val!(codegen, into, val, " = {};", if self.all { "1" } else { "0" });

        // The index that we want to use is the current length of the array.
        define_val!(codegen, into, idx, " = 0;\n");

        // TODO: Memory safety: What if the array shrinks while we're iterating?
        // We might want to grab the inner array as a temporary. Or, we could
        // do the thing where we capture the value.
        inf_writeln!(into, "{}while({} < {}->header.length) {{",
            indent, idx, inner_array);
        if self.all {
            inf_writeln!(into, "{}\tif(!{}.fun(ctx, {}->contents[{}], {}.closure)) {{",
                indent, fun, inner_array, idx, fun);
            inf_writeln!(into, "{}\t\t{} = 0;", indent, val);
        }
        else {
            inf_writeln!(into, "{}\tif({}.fun(ctx, {}->contents[{}], {}.closure)) {{",
                indent, fun, inner_array, idx, fun);
            inf_writeln!(into, "{}\t\t{} = 1;", indent, val);
        }
        inf_writeln!(into, "{}\t\tbreak;", indent);
        inf_writeln!(into, "{}\t}}", indent);
        inf_writeln!(into, "{}\t{} += 1;", indent, idx);
        inf_writeln!(into, "{}}}", indent);
        
        val
    }
}

pub struct DynarrayCloneShallow;

impl BuiltinMethod for DynarrayCloneShallow {
    fn get_types(&self, db: &mut Db, self_ty: TypId) -> (TypId, Vec<TypId>) {
        // DynArray[T]::clone() -> DynArray[T]
        match db.get(self_ty) {
            Type::DynArrayOf(_, _) => {
                (self_ty, vec![])
            }
            _ => unreachable!()
        }
    }

    fn compile(
        &self,
        codegen: &mut Codegen,
        _ast_node: &BuiltinCall,
        _ast: &AstReadonly,
        self_val: TypedVal,
        arg_vals: Vec<TypedVal>,
        into: &mut String
    ) -> TypedVal {
        let Type::DynArrayOf(elem_ty, arr_ty) = self_val.get_type(codegen.db) else { unreachable!() };
        let [] = arg_vals.as_slice() else { unreachable!() };

        let indent = codegen.indent();

        let idx = codegen.new_val_typed(codegen.db.types.int);

        let inner_array = format!("(({}){}->header.buffer)",
            codegen.db.get_ctype(*arr_ty), self_val);

        // It is VERY IMPORTANT that we read the length value from the *DynArray*,
        // not from the *Array*. After all, the Array's length includes elements
        // that may be uninitialized.
        let stored_len_val = codegen.new_val_typed(codegen.db.types.int);
        define_val!(codegen, into, stored_len_val, " = {}->header.length;\n",
            self_val);

        // Read from a specific inner array temporary. This ensures memory safety
        // (the array can't shrink while we're constructing it).
        let inner_arr_val = codegen.new_val_typed(*arr_ty);
        define_val!(codegen, into, inner_arr_val, " = {};\n", inner_array);

        // No need for _tmp as we are not claling any functions.
        // NOTE: This is the buffer we're allocating for the new array.
        let buf_val = codegen.new_val_typed(*arr_ty);
        define_val!(codegen, into, buf_val, ";\n");

        let val = codegen.new_val_typed(self_val.typ);
        define_val!(codegen, into, val, "; PONI_INIT_DYNARRAY({}, {}, sizeof({}), {}, {}, {})\n",
            buf_val,
            val,
            codegen.db.get_ctype(*elem_ty),
            // For now, we use the length we got from the old array as the
            // new length value and the new allocated size.
            stored_len_val, // elem_cnt
            stored_len_val, // real_cnt
            codegen.db.get_type_ctag(*elem_ty));


        // The index that we want to use is the current length of the array.
        define_val!(codegen, into, idx, " = 0;\n");
        inf_writeln!(into, "{}while({} < {}) {{",
            indent, idx, stored_len_val);
        // Copy
        inf_writeln!(into, "{}\t{}->contents[{}] = {}->contents[{}];",
            indent, buf_val, idx, inner_arr_val, idx);
        inf_writeln!(into, "{}\t{} += 1;", indent, idx);
        inf_writeln!(into, "{}}}", indent);

        // TODO: Use memcpy() instead.
        
        val
    }
}