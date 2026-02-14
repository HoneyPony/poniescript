//! This is an unfortunately quite complicated desugaring.
//! 
//! Essentially, we want to convert async functions into continuation-passing
//! style. This allows us to desugar .await points.
//! 
//! There are a number of complications with this.
//! 1. Handling expressions. Our main answer to this is to not do it. We will
//!    need an additional linearization pass (i.e. converting 1 + fun().await
//!    into let tmp = fun.await(); 1 + tmp)
//! 2. Loops and branches. These require us to really carefully track our continuations.
//! 3. Closures. We rely on the closure synthesis pass to do most of the grunt
//!    work for actually making everything work. This is convenient, on the one
//!    hand, but it also means we have to be very careful about our ClosureId
//!    chaining on the other.
//!

use poni_arena::{ArenaKey, IndexCell};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{db::*, expr::*, lexer::Tok, typ::Type};

struct AsyncConvert {
    current_fun: Option<FunId>,
    current_closure: Option<ClosureId>,
}


impl AsyncConvert {
    fn visit_fundeclare_any(&mut self, ast: &AstProxy, db: &mut Db, declare: &FunDeclare) {
        log::trace!("async: visit fun {}", db.get_fun_name(declare.identity));
        let enclosing = self.current_fun;
        let enclosing_closure = self.current_closure;
        self.current_fun = Some(declare.identity);
        // The current closure is the param closure.
        //
        // That's because the param_closure is the one that we want to be the
        // parent of any new continuation we build.
        self.current_closure = Some(db.get(declare.identity).param_closure);

        self.visit_expr(ast, db, declare.value);

        self.current_fun = enclosing;
        self.current_closure = enclosing_closure;
    }

    fn visit_classdeclare_any(&mut self, ast: &AstProxy, db: &mut Db, declare: &ClassDeclare) {
        log::trace!("async: visit class {}", db.get(db.get(declare.identity).name));

        for fun in &declare.funs {
            self.visit_fundeclare_any(ast, db, &fun);
        }
        for class in &declare.classes {
            self.visit_classdeclare_any(ast, db, &class);
        }
        for var in &declare.vars {
            if let Some(value) = var.value {
                self.visit_expr(ast, db, value);
            }
        }
    }
}

impl VisitAstMut for AsyncConvert {
    fn visit_return(&mut self, ast: &AstProxy, db: &mut Db, id: ExprId) {
        let binding = ast.get_expr(id);
        let Expr::Return(ret) = binding.as_ref() else { unreachable!() };

        if let Some(inner) = ret.expression {
            self.visit_expr(ast, db, inner);
        }

        // Whenever we encounter a return statement, instead call the function's
        // end_continuation.
        if let Some(fun) = self.current_fun {
            if db.get(fun).asyncness == Asyncness::Implicit {
                // This must be a variable, otherwise something is broken.
                let continuation = db.get(fun).parameters.last().unwrap();

                let loc = ret.location.clone();
                let inner = ret.expression;
                drop(binding);

                let var = Expr::push_variable(ast, loc.clone(), *continuation);
                let var_type = db.get_var_type(*continuation);
                let Type::Fun(sig) = db.get(var_type) else { panic!("ICE: Non-Fun continuation"); };
                let mut as_valcall = ValCall {
                    location: loc.clone(),
                    value: var,
                    args: Vec::new(),
                    sig: *sig,
                    call_type: CallType::Normal,
                    arg_boundaries: Vec::new(),
                };
                
                if let Some(inner) = inner {
                    as_valcall.args.push(inner);
                }

                *ast.get_expr_mut(id).as_mut() = Expr::ValCall(as_valcall);
            }
        }
    }

    fn visit_fundeclare(&mut self, ast: &AstProxy, db: &mut Db, id: crate::db::ExprId) {
        let binding = ast.get_expr(id);
        let Expr::FunDeclare(declare) = binding.as_ref() else { unreachable!() };

        self.visit_fundeclare_any(ast, db, declare);
    }

    fn visit_classdeclare(&mut self, ast: &AstProxy, db: &mut Db, id: crate::db::StmtId) {
        let binding = ast.get_stmt(id);
        let Stmt::ClassDeclare(declare) = binding.as_ref() else { unreachable!() };

        self.visit_classdeclare_any(ast, db, declare);
    }

    fn visit_block(&mut self, ast: &AstProxy, db: &mut Db, id: ExprId) {
        let mut binding = ast.get_expr_mut(id);
        let Expr::Block(block) = binding.as_mut() else { unreachable!() };

        // First pass: Visit everything
        for stmt_id in &block.stmts {
            self.visit_stmt(ast, db, *stmt_id);
        }

        // Second pass: Split at await point
        //
        // Still need to visit the inner expression somehow...?
        let mut idx = 0;
        for stmt_id in &block.stmts {
            let stmt = ast.get_stmt(*stmt_id);
            let Stmt::Expression(expr) = stmt.as_ref() else {
                continue;
            };
            
            let call_id = expr.expression; // we'll need this later
            let expr = ast.get_expr(call_id);
            if let Expr::FunCall(call) = expr.as_ref() {
                if call.call_type == CallType::Await {
                    // Await point.
                    //
                    // What we want to do:
                    // Split the rest of the function body into a new closure.
                    // Add that function as a continuation to this.
                    let stmts = block.stmts.split_off(idx + 1);
                    let loc = call.location.clone();
                    let fun = call.identity;
                    drop(binding); drop(stmt); drop(expr);
                    
                    let new_block = Expr::push_block(ast, loc.clone(), stmts, db.types.void);
                    let closure = Closure {
                        class: None,
                        parent: self.current_closure,
                        parent_class: None,
                    };
                    let closure = db.push(closure);
                    let new_alloc = Expr::push_allocateclosure(ast, loc.clone(), closure,
                        new_block, db.types.void,
                        // This closure is responsible for copying params.
                        true);

                    // We can't just directly use the sugar return type or
                    // the normal return type. We need to extract the return
                    // type from the function.
                    //
                    // First get the end_continuation.
                    let end_continuation = db.get(fun).parameters.last().unwrap();

                    // Next, extract the type from the end_continuation.
                    let cb_type = db.get_var_type(*end_continuation);
                    let Type::Fun(sig) = db.get(cb_type) else { panic!("ICE: Non-Fun continuation"); };
                    let sig = *sig;

                    // We re-use this sig as the sig for the new function.
                    let mut parameters = vec![];
                    // If there is a single parameter in the cb_type, that is our
                    // return-type variable for our callback.
                    if let Some(param) = db.get(sig).parameters.first().copied() {
                        let new_var_name = db.put_str("await");
                        let new_var = db.new_var(new_var_name, param, true, None,
                            None, Some(closure), None, None, loc.clone(), None);
                        parameters.push(new_var);
                    }

                    // log::trace!("async: cb_type for {} = {}", db.get_fun_name(fun), db.repr_type(cb_type));
                    // let sig = Sig {
                    //     parameters: vec![cb_type],
                    //     return_type: db.types.void,
                    // };
                    // let sig = db.put_sig(&sig);
                    // db.use_sig(sig);

                    // Use the name "continuation" for these functions, to make
                    // them clearer in debug output
                    let name = db.put_str("continuation");

                    let new_function = Fun {
                        name: Some(name),
                        sig,
                        parameters,
                        return_type: db.types.void,
                        sugar_return_type: db.types.void,
                        asyncness: Asyncness::Not,
                        class: None,
                        closure: self.current_closure, // I believe this is the enclosing closure
                        param_closure: closure, // This would be the inner closure
                        expression: Some(new_alloc),
                        location: loc.clone(),
                        doc_comment: None,
                    };

                    let new_function = db.push(new_function);
                    let fun_capture = Expr::push_funcapture(ast, loc.clone(),
                        loc.clone(), new_function, db.put_type(Type::Fun(sig)), None);
                    
                    // We must add the fun declare to this block so that the closure
                    // conversion pass will see it.
                    let fundeclare = Expr::push_fundeclare(ast, loc.clone(), new_function, new_alloc, db.types.void);
                    let fundeclare_stmt = Stmt::push_expression(ast, loc.clone(), fundeclare);
                    let mut binding = ast.get_expr_mut(id);
                    let Expr::Block(block) = binding.as_mut() else { unreachable!() };
                    block.stmts.insert(block.stmts.len() - 1, fundeclare_stmt);
                    drop(binding);

                    // Add the fun capture to the parameters of the fun call.
                    let mut binding = ast.get_expr_mut(call_id);
                    let Expr::FunCall(call) = binding.as_mut() else { unreachable!() };
                    call.args.push(fun_capture);

                    // Now, visit the new function ??
                    drop(binding);
                    //self.visit_expr(ast, db, new_alloc);

                    break;
                }
            }

            // Otherwise, visit the expr normally...
            //self.visit_expr(ast, db, call_id);

            idx += 1;
        }
    }
}

pub fn convert_awaits(ast: &mut Ast, db: &mut Db) {
    for source_id in ast.sources.iter_existing() {
        //let mut source = ast.sources.get_mut(source_id);
        //let module = std::mem::take(&mut source.module);
        //drop(source); // Let us borrow this
        
        let proxy = ast.get_proxy();

        let source = proxy.sources.get(source_id);
        let module = &source.module;

        let mut convert = AsyncConvert {
            current_fun: None,
            current_closure: None,
        };

        for fun in &module.functions {
            convert.visit_fundeclare_any(&proxy, db, fun);
        }

        for class in &module.classes {
            convert.visit_classdeclare_any(&proxy, db, class);
        }

        for var in &module.globals {
            if let Some(value) = var.value {
                convert.visit_expr(&proxy, db, value);
            }
        }

        drop(source);
        proxy.commit();

        // Put the module back
        //ast.sources.get_mut(source_id).module = module;
    }
}