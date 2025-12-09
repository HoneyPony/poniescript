use crate::db::*;
use crate::codegen::*;
use crate::define_val;
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
        ast: &AstReadonly,
        self_val: TypedVal,
        arg_vals: Vec<TypedVal>,
        into: &mut String
    ) -> TypedVal {
        let Type::DynArrayOf(elem_ty, arr_ty) = self_val.get_type(codegen.db) else { unreachable!() };
        let [new_item] = arg_vals.as_slice() else { unreachable!() };

        let indent = codegen.indent();

        let idx = codegen.new_val_typed(codegen.db.types.int);

        let inner_array = format!("(({}){})->buffer",
            codegen.db.get_ctype(*arr_ty), self_val);

        // The index that we want to use is the current length of the array.
        define_val!(codegen, into, idx, " = {}->length;\n", self_val);
        inf_writeln!(into, "{}if({} < {}->header.length) {{",
            indent, idx, inner_array);
        inf_writeln!(into, "{}\t{}->contents[{}] = {}", indent, inner_array, idx, new_item);
        inf_writeln!(into, "{}}}", indent);
        inf_writeln!(into, "{}else {{", indent);
        // Ensure that we have idx room in the array.
        inf_writeln!(into, "{}\t{}->buffer = poni_array_ensure(ctx, {}, sizeof({}), {});",
            indent, self_val, inner_array, codegen.db.get_ctype(*elem_ty), idx);
        inf_writeln!(into, "{}\t{}->contents[{}] = {}", indent, inner_array, idx, new_item);
        inf_writeln!(into, "{}}}", indent);
        
        Val::Void.typed(codegen.db.types.void, None)
    }
}