use crate::db::*;
use crate::codegen::*;
use crate::define_val;
use crate::expr::BuiltinCall;
use crate::expr::Sig;
use crate::inf_writeln;
use crate::inf_write;
use crate::typ::Type;

pub struct VecMap;

/// Method for any Vector type, i.e. tuple of integers or floats.
/// 
/// Takes a function that takes the inner type, and then applies that function
/// to every element, creating a new vector.
impl BuiltinMethod for VecMap {
    fn get_types(&self, db: &mut Db, self_ty: TypId) -> (TypId, Vec<TypId>) {
        match db.get(self_ty) {
            Type::Tuple(tys) => {
                let elem_ty = tys[0];

                // We take a function that takes an Elem and returns an Elem
                let sig = db.put_sig(&Sig {
                    parameters: vec![elem_ty],
                    return_type: elem_ty,
                });

                let fun = db.put_type(Type::Fun(sig));

                // Return type is another of us
                return (self_ty, vec![fun]);
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
        let Type::Tuple(tys) = self_val.get_type(codegen.db) else { unreachable!() };
        let [fun] = arg_vals.as_slice() else { unreachable!() };

        let indent = codegen.indent();

        let val = codegen.new_val_typed(self_val.typ);
        define_val!(codegen, into, val, ";");
        if val.needs_storage() {
            for i in 0..tys.len() {
                // Because we're making a function call, we must save GC values.
                // TODO:
                // What we really should do is have some IR/way to say we are doing
                // a ValCall. Then, any optimization that would elide the GC save
                // for the ValCall can also apply here. 
                codegen.save_gc_values(into);

                inf_writeln!(into, "{}{}.v_{} = {}.fun(ctx, {}.v_{}, {}.closure);",
                    indent, val, i, fun, self_val, i, fun);
            }
        }

        val
    }
}