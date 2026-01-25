use poni_arena::{ArenaKey, IndexCell};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{db::{Ast, AstAbstract, ClassId, ClosureId, Db, FunId, IdFuncs, VarId}, expr::{Class, Expr, Get, VisitAst}, typ::Type};

struct ClosureConvert<'a> {
    ast: &'a Ast,
    db: &'a mut Db,

    /// Maps closures to ClassIds.
    class_map: FxHashMap<ClosureId, ClassId>,

    /// Map of variables that have already been converted.
    var_set: FxHashSet<VarId>,

    current_fun: Option<FunId>,
}

impl<'a> ClosureConvert<'a> {
    fn actually_convert_var(&mut self, var: VarId) {
        // TODO: Get var closure_id.
        let closure_id = unsafe { ClosureId::from_index(0) };

        let class = *self.class_map.entry(closure_id)
            .or_insert_with(|| {
                let class = Class {
                    name: self.db.str_anonymous,
                    vars: Vec::new(),
                    funs: Vec::new(),
                    classes: Vec::new(),

                    // TODO: How are we going to properly do this...?
                    // Maybe a secondary side map that we use to wire everything
                    // up at the end?
                    parent: None,
                    mandatory_vars: FxHashSet::default(),
                    import_kind: crate::expr::ImportKind::Not,
                    var_map: FxHashMap::default(),
                    fun_map: FxHashMap::default(),
                    class_map: FxHashMap::default(),
                    location: self.db.synthetic(),
                    doc_comment: None,
                };

                self.db.push(class)
            });

        // Add the variable into the class.
        self.db.get_mut(class).vars.push(var);
        // Ensure the variable knows its own class.
        self.db.get_mut(var).class = Some(class);
    }

    fn maybe_convert_var(&mut self, var: VarId) {
        let in_fn = self.current_fun;

        // If we're in the same function that this variable is defined in, 
        // then it does not need to be put into a closure.
        if self.db.get(var).fun == in_fn {
            return;
        }

        // Otherwise, convert the var. If it is already in the map, we're done.
        if self.var_set.contains(&var) {
            return;
        }

        // So now we actually convert the var.
        self.actually_convert_var(var);
    }
}

impl<'a> VisitAst for ClosureConvert<'a> {
    fn visit_fundeclare(&mut self, ast: &Ast, db: &mut Db, id: crate::db::ExprId) {
        let binding = ast.get_expr(id);
        let Expr::FunDeclare(declare) = binding.as_ref() else { unreachable!() };

        let enclosing = self.current_fun;
        self.current_fun = Some(declare.identity);

        self.visit_expr(ast, db, declare.value);

        self.current_fun = enclosing;
    }

    fn visit_variable(&mut self, ast: &Ast, db: &mut Db, id: crate::db::ExprId) {
        let binding = ast.get_expr(id);
        let Expr::Variable(var) = binding.as_ref() else { unreachable!() };

        self.maybe_convert_var(var.identity);
    }
}

/// Second pass for rewriting any references to now-closed variables with
/// Expr::Get and Expr::Set instead.
fn replace_vars(ast: &mut Ast, db: &mut Db) {
    // We only care to iter over the already-existing exprs.
    for expr in ast.exprs.iter_existing() {
        let binding = ast.exprs.get(expr);
        match binding.as_ref() {
            Expr::Variable(var) => {
                let id = var.identity;
                let var = db.get(var.identity);
                if let Some(class) = var.class {
                    let location = var.location.clone();
                    drop(binding);

                    // Rewrite variable into an Expr::Get on SelfVal.
                    let selfval = Expr::put_selfval(ast,
                        db.synthetic(), 
                        db.put_type(Type::Class(class)));

                    let get = Get {
                        location,
                        // Technically, we don't need the chain, as this is a
                        // rewriting pass, and we shouldn't need it anymore.
                        chain: Vec::new(),
                        lhs: selfval,
                        vars: vec![id],
                    };

                    *ast.exprs.get_mut(expr) = Expr::Get(get);
                }
            },
            _ => {

            }
        }
    }
}

pub fn convert_closures(ast: &mut Ast, db: &mut Db) {
    let convert = ClosureConvert {
        ast: ast,
        db,
        class_map: FxHashMap::default(),
        var_set: FxHashSet::default(),
        current_fun: None,
    };

    // We don't have this...?
    // convert.visit_ast(ast, db);

    // Then do the variable replacement pass.
    replace_vars(ast, db);

    // NOTE: We also want a new Expr, the Expr::AllocateClosure(ClosureId).
    // If the closure ends up having a class, it will allocate it at the
    // beginning of the function or scope.
    //
    // We create Expr::AllocateClosure at the top of each function, and
    // at loops.
    //
    // Also, we will probably need to be able to copy the parameters in to
    // the closure at the beginning of the function.
}