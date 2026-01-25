use poni_arena::{ArenaKey, IndexCell};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{db::{Ast, AstAbstract, ClassId, ClosureId, Db, FunId, IdFuncs, VarId}, expr::{Class, Expr, Expression, FunDeclare, Get, Stmt, VisitAst}, lexer::Tok, typ::Type};

struct ClosureConvert<'a> {
    ast: &'a Ast,

    /// Maps closures to ClassIds.
    class_map: FxHashMap<ClosureId, ClassId>,

    /// Map of variables that have already been converted.
    var_set: FxHashSet<VarId>,

    current_fun: Option<FunId>,
}

impl<'a> ClosureConvert<'a> {
    fn actually_convert_var(&mut self, db: &mut Db, var: VarId) {
        // TODO: also add it to the var_set if it doesn't have closure...?
        let Some(closure_id) = db.get(var).closure else { return; };

        let class = *self.class_map.entry(closure_id)
            .or_insert_with(|| {
                log::trace!("creating new closure class for var {}", db.repr_var(var));
                let class = Class {
                    name: db.str_closure,
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
                    location: db.synthetic(),
                    doc_comment: None,
                };

                db.push(class)
            });

        // Add the variable into the class.
        db.get_mut(class).vars.push(var);
        // Ensure the variable knows its own class.
        db.get_mut(var).class = Some(class);

        log::trace!("adding variable to closure: {}", db.repr_var(var));

        self.var_set.insert(var);
    }

    fn maybe_convert_var(&mut self, db: &mut Db, var: VarId) {
        let in_fn = self.current_fun;

        // If we're in the same function that this variable is defined in, 
        // then it does not need to be put into a closure.
        if db.get(var).fun == in_fn {
            return;
        }

        if db.get(var).fun.is_none() {
            // I believe this would always be incorrect...?
            log::trace!("refusing to convert non-fun var {}", db.repr_var(var));
            return;
        }

        // Otherwise, convert the var. If it is already in the map, we're done.
        if self.var_set.contains(&var) {
            return;
        }

        log::trace!("converting var: {} (in fun {}, var belongs to {})",
            db.repr_var(var),
            in_fn.map(|f| db.get_fun_name(f)).unwrap_or("<none>"),
            db.get(var).fun.map(|f| db.get_fun_name(f)).unwrap_or("<none>"));

        // So now we actually convert the var.
        self.actually_convert_var(db, var);
    }

    fn visit_fundeclare_any(&mut self, ast: &Ast, db: &mut Db, declare: &FunDeclare) {
        log::trace!("closure: visit {}", db.get_fun_name(declare.identity));
        let enclosing = self.current_fun;
        self.current_fun = Some(declare.identity);

        self.visit_expr(ast, db, declare.value);

        self.current_fun = enclosing;
    }
}

impl<'a> VisitAst for ClosureConvert<'a> {
    fn visit_fundeclare(&mut self, ast: &Ast, db: &mut Db, id: crate::db::ExprId) {
        let binding = ast.get_expr(id);
        let Expr::FunDeclare(declare) = binding.as_ref() else { unreachable!() };

        self.visit_fundeclare_any(ast, db, declare);
    }

    fn visit_variable(&mut self, ast: &Ast, db: &mut Db, id: crate::db::ExprId) {
        let binding = ast.get_expr(id);
        let Expr::Variable(var) = binding.as_ref() else { unreachable!() };

        self.maybe_convert_var(db, var.identity);
    }
}

struct ReplaceVars {
    current_fun: Option<FunId>,
}

impl ReplaceVars {
    fn visit_fundeclare_any(&mut self, ast: &Ast, db: &mut Db, declare: &FunDeclare) {
        log::trace!("replace-vars: visit {}", db.get_fun_name(declare.identity));
        let enclosing = self.current_fun;
        self.current_fun = Some(declare.identity);

        self.visit_expr(ast, db, declare.value);

        self.current_fun = enclosing;
    }
}

impl VisitAst for ReplaceVars {
    fn visit_fundeclare(&mut self, ast: &Ast, db: &mut Db, id: crate::db::ExprId) {
        let binding = ast.get_expr(id);
        let Expr::FunDeclare(declare) = binding.as_ref() else { unreachable!() };

        self.visit_fundeclare_any(ast, db, declare);
    }

    fn visit_variable(&mut self, ast: &Ast, db: &mut Db, id: crate::db::ExprId) {
        let binding = ast.get_expr(id);
        let Expr::Variable(var) = binding.as_ref() else { unreachable!() };

        self.maybe_convert_var(db, var.identity);
    }

    fn visit_funcall(&mut self, ast: &Ast, db: &mut Db, id: crate::db::ExprId) {
        let fun = db.get(call.identity);
                if fun.closure.is_some() {
                    // We need to look up the FUNCTION class, not hte CLOSURE
                    // class, because the FUNCTION class might have been the
                    // closure's parent.
                    if let Some(class) = fun.class {
                        drop(binding);
                        let selfval = Expr::put_selfval(ast, db.synthetic(), db.put_type(Type::Class(class)));

                        let mut binding = ast.exprs.get_mut(expr);
                        let Expr::FunCall(call) = binding.as_mut() else { unreachable!() };
                        call.object = Some(selfval);
                    }
                }
    }

    fn visit_funcapture(&mut self, ast: &Ast, db: &mut Db, id: crate::db::ExprId) {
        let binding = ast.exprs.get(id);
        let Expr::FunCapture(capt) = binding.as_ref() else { unreachable!() };

        let fun = db.get(capt.identity);
        // The logic here is this:
        // We already precisely computed the correct classes for each
        // function in identify_function_classes. Now we simply have to
        // synthesized the SelfVals for those.
        //
        // But, we only do this for functions that BOTH have a class and
        // a closure; functions that already have a class have already
        // been handled by the nature of being a class function.
        if fun.closure.is_some() {
            // We need to look up the FUNCTION class, not hte CLOSURE
            // class, because the FUNCTION class might have been the
            // closure's parent.
            if let Some(class) = fun.class {
                drop(binding);
                let selfval = Expr::put_selfval(ast, db.synthetic(), db.put_type(Type::Class(class)));

                let mut binding = ast.exprs.get_mut(expr);
                let Expr::FunCapture(capt) = binding.as_mut() else { unreachable!() };
                capt.object = Some(selfval);
            }
        }
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

                // NOTE: It is critical that we use the closure type of the
                // function that the variable is accessed from as the selfval
                // type. This is actually impossible to do with a flat
                // iteration, currently. We need to use another tree-based
                // visitor.
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
            // Expr::FunDeclare(declare) => {
            //     let fun = db.get(declare.identity);
            //     // The logic here is this:
            //     // We 
            //     if fun.closure.is_some() {
            //         if let Some(class) = db.get(closure).class {
            //             db.get_mut(declare.identity).class = Some(class);
            //         }
            //     }
            // },
            Expr::FunCapture(capt) => {
                
            },
            Expr::FunCall(call) => {
                // Same idea as above.
                
            }
            _ => {

            }
        }
    }

    for stmt in ast.stmts.iter_existing() {
        let binding = ast.stmts.get(stmt);
        match binding.as_ref() {
            Stmt::Declare(declare) => {
                let id = declare.identity;
                let var = db.get(declare.identity);
                // Not a possible candidate
                if var.fun.is_none() { continue; }

                if let Some(class) = var.class {
                    let location = var.location.clone();
                    let value = declare.value.unwrap();
                    drop(binding);

                    // Rewrite variable into an Expr::Set on SelfVal.
                    let selfval = Expr::put_selfval(ast,
                        db.synthetic(), 
                        db.put_type(Type::Class(class)));

                    let set = Expr::put_set(ast,
                        db.synthetic(), Vec::new(),
                            selfval, vec![id], value, Tok::Equal);

                    *ast.stmts.get_mut(stmt) = Stmt::Expression(Expression {
                        location,
                        expression: set,
                    });
                }
            },
            _ => {}
        }
    }
}

// An important note about this chain walking algorithm is that it will identify
// a class for a function even if that function did not itself touch any variables.
//
// For example,
//     fun outer() {
//         var x = 20;
//         fun mid() {
//             fun inner() { x += 30; }
//         }
//     }
//
// Even though mid() does not refer to x at all, because x gets put into a closure,
// and mid's ClosureId points to outer's ClosureId, mid will end up having its
// class set to the correct value.
fn identify_class_for_function_closure(ast: &mut Ast, db: &mut Db, mut closure: ClosureId) -> Option<ClassId> {
    loop {
        let cur = db.get(closure);
        if let Some(class) = cur.class {
            return Some(class);
        }

        if let Some(parent) = cur.parent {
            closure = parent;
            continue;
        }

        return None;
    }
}

fn identify_function_classes(ast: &mut Ast, db: &mut Db) {
    for fun_id in db.iter_fun() {
        let fun = db.get(fun_id);
        if let Some(closure) = fun.closure {
            db.get_mut(fun_id).class = identify_class_for_function_closure(ast, db, closure);
        }
    }
}

pub fn convert_closures(ast: &mut Ast, db: &mut Db) {
    let mut convert = ClosureConvert {
        ast: ast,
        class_map: FxHashMap::default(),
        var_set: FxHashSet::default(),
        current_fun: None,
    };

    for source in ast.sources.iter_existing() {
        let source = ast.sources.get(source);
        let module = &source.module;

        for fun in &module.functions {
            convert.visit_fundeclare_any(ast, db, fun);
        }
    }

    // Now that we have the map, push that info into the db.
    for (closure, class) in &convert.class_map {
        db.get_mut(*closure).class = Some(*class);

        if let Some(parent) = db.get(*closure).parent {
            if let Some(parent_class) = convert.class_map.get(&parent) {
                db.get_mut(*class).parent = Some(*parent_class);
            }
        }
        else if let Some(parent_class) = db.get(*closure).parent_class {
            db.get_mut(*class).parent = Some(parent_class);
        }
    }

    identify_function_classes(ast, db);

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