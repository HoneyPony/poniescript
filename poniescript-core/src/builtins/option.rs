use crate::db::*;
use crate::codegen::*;
use crate::define_val;
use crate::expr::BuiltinCall;
use crate::inf_writeln;
use crate::inf_write;
use crate::typ::Type;
pub struct OptionUnwrap;

impl BuiltinMethod for OptionUnwrap {
    fn get_types(&self, db: &mut Db, self_ty: TypId) -> (TypId, Vec<TypId>) {
        // Option[T]::unwrap(T) -> T (or panic)
        match db.get(self_ty) {
            Type::Option(inner) => (*inner, vec![]),
            _ => unreachable!()
        }
    }

    fn compile(
        &self,
        codegen: &mut Codegen,
        ast_node: &BuiltinCall,
        ast: &AstReadonly,
        self_val: TypedVal,
        arg_vals: Vec<TypedVal>,
        into: &mut String
    ) -> TypedVal {
        let Type::Option(inner) = self_val.get_type(codegen.db) else { unreachable!() };
        // Essentially a safety check that we were called with the right args.
        let [] = arg_vals.as_slice() else { unreachable!() };

        let indent = codegen.indent();

        if codegen.db.is_value_type(*inner) {
            todo!("unwrap() for value-type options");
        }

        // Pointer-based unwrap.
        let val = codegen.new_val_typed_tmp(*inner);
        inf_writeln!(into, "{}if(!{}) {{", indent, self_val);
        codegen.make_panic(ast, into, "called .or_panic on a nil", &ast_node.location);
        inf_writeln!(into, "{}}}", indent);
        define_val!(codegen, into, val, " = {};\n", self_val);

        codegen.tmp_to_used_val(val)
    }
}