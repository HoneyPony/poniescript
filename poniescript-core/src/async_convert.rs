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

fn push_to_block(ast: &AstProxy, target_block: ExprId, new_stmt: StmtId) {
    let mut binding = ast.get_expr_mut(target_block);
    let Expr::Block(block) = binding.as_mut() else { panic!("ICE: Non-block target_block"); };

    block.stmts.push(new_stmt);
}

impl AsyncConvert {
    fn expr(&mut self, ast: &AstProxy, db: &mut Db, expr: ExprId, mut target_block: ExprId) -> ExprId {
        let mut binding = ast.get_expr_mut(expr);
        match binding.as_mut() {
            Expr::Binary(binary) => {
                target_block = self.expr(ast, db, binary.left, target_block);
                target_block = self.expr(ast, db, binary.right, target_block);
                return target_block;
            },
            Expr::Unary(unary) => {
                return self.expr(ast, db, unary.inner, target_block);
            }
            Expr::Comparison(comparison) => {
                target_block = self.expr(ast, db, comparison.left, target_block);
                target_block = self.expr(ast, db, comparison.right, target_block);
                return target_block;
            }
            Expr::FunCall(call) => {
                if let Some(obj) = call.object {
                    target_block = self.expr(ast, db, obj, target_block);
                }
                for arg in &call.args {
                    target_block = self.expr(ast, db, *arg, target_block);
                }

                if call.call_type == CallType::Await {
                    // If we are an Await call, we want to do the following:
                    // - Append a FunDeclare to the current block
                    // - Append a FunCall to the new continuation to our own argument list
                    // - Return the block for the new function as the new target_block
                    let loc = call.location.clone();
                    let fun = call.identity;
                    drop(binding); // So we don't double borrow soon
                    
                    let new_block = Expr::push_block(ast,
                        loc.clone(),
                        // The new block starts empty; it is filled up through the target_block system.
                        Vec::new(),
                        db.types.void);
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

                    let replacement_expr: Expr;

                    // We re-use this sig as the sig for the new function.
                    let mut parameters = vec![];
                    // If there is a single parameter in the cb_type, that is our
                    // return-type variable for our callback.
                    if let Some(param) = db.get(sig).parameters.first().copied() {
                        let new_var_name = db.put_str("await");
                        let new_var = db.new_var(new_var_name, param, true, None,
                            None, Some(closure), None, None, loc.clone(), None);
                        parameters.push(new_var);

                        replacement_expr = Expr::Variable(Variable {
                            location: loc.clone(),
                            identity: new_var,
                        });
                    }
                    else {
                        // For now, if we are a void, as our replacement expression use an empty block.
                        replacement_expr = Expr::Block(Block {
                            location: loc.clone(),
                            stmts: Vec::new(),
                            typ: db.types.void
                        });
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
                    
                    // THIS needs to be the OLD target_block.
                    push_to_block(ast, target_block, fundeclare_stmt);

                    // Add the fun capture to the parameters of the fun call.
                    let mut binding = ast.get_expr_mut(expr);
                    let Expr::FunCall(call) = binding.as_mut() else { unreachable!() };
                    call.args.push(fun_capture);

                    // Finally, replace our fun call with the replacement expr. The idea here is that,
                    // we have some code that looks like:
                    //
                    // fun_call().await + 123
                    // 
                    // Replacing 'fun_call().await' with the new variable that represents the return value
                    // will naturally fit it into the syntax tree. The overall expression will end up in the
                    // new target_block, inside of stmt().
                    let fun_call = std::mem::replace(binding.as_mut(), replacement_expr);

                    // Finally-finally, the fun_call itself still needs to occur, so append it as a new
                    // expr to the current block (not the new block). It occurs as a fun call that is assigned
                    // to nothing.
                    drop(binding);
                    let funcall_expr = ast.exprs.push(fun_call);
                    let funcall_stmt = Stmt::push_expression(ast, loc.clone(), funcall_expr);
                    push_to_block(ast, target_block, funcall_stmt);

                    // Interestingly, there is nothing to visit in the new function yet. (And in fact, there never will be).
                    // Instead, we simply have a new target_block, for the rest of the upcoming statements.
                    target_block = new_block;
                }

                return target_block;
            },
            Expr::Block(block) => {
                // Blocks are somewhat special.
                //
                // First we have to clear the current block. This is because our stmt() function will push
                // Expr's back into the target block.
                //
                // Then, we set the target block to this block (?)
                //
                // Finally, this block itself must go on the original target_block, maybe.
                let take = std::mem::take(&mut block.stmts);
                drop(binding); // We will re-borrow the block later.

                let mut inner_target: ExprId = expr;
                for stmt in take {
                    // This may or may not be correct...
                    inner_target = self.stmt(ast, db, stmt, inner_target);
                }

                // Original target block.
                return target_block;
            }
            Expr::AllocateClosure(alloc) => {
                return self.expr(ast, db, alloc.inner, target_block)
            }
            Expr::Print(print) => {
                for arg in &print.exprs {
                    target_block = self.expr(ast, db, *arg, target_block);
                }
                return target_block;
            }
            Expr::ArrayLit(lit) => {
                for arg in &lit.values {
                    target_block = self.expr(ast, db, *arg, target_block);
                }
                return target_block;
            }
            Expr::Assign(assign) => {
                target_block = self.expr(ast, db, assign.value, target_block);
                return target_block;
            }
            Expr::Index(index) => {
                target_block = self.expr(ast, db, index.index, target_block);
                target_block = self.expr(ast, db, index.value, target_block);
                return target_block;
            }
            Expr::StrLiteral(_) => { target_block }
            Expr::NumLiteral(_) => { target_block }
            Expr::Variable(_) => { target_block }
            Expr::Return(ret) => {
                // Returns are special, because they must be replaced with a call to the
                // async continuation.
                target_block = self.maybe_expr(ast, db, ret.expression, target_block);

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

                        *ast.get_expr_mut(expr).as_mut() = Expr::ValCall(as_valcall);
                    }
                }

                return target_block;
            }
            Expr::ValCall(call) => {
                // TODO: Actually synthesize the closure. :)
                for arg in &call.args {
                    target_block = self.expr(ast, db, *arg, target_block);
                }
                return target_block;
            }
            Expr::BuiltinCall(call) => {
                // For now, builtin calls cannot themselves be async, so there is nothing
                // to convert.
                target_block = self.expr(ast, db, call.object, target_block);
                for arg in &call.args {
                    target_block = self.expr(ast, db, *arg, target_block);
                }
                return target_block;
            }
            Expr::FunDeclare(fun) => {
                // TODO: We may want to avoid traversing these through the AST, and instead
                // do it in a top-level way. This would allow us to skip any functions that
                // do not contain any .await's entirely.
                self.function(ast, db, fun.identity, target_block);
                // Keep same target block.
                return target_block;
            }
            Expr::FunCapture(capt) => {
                return self.maybe_expr(ast, db, capt.object, target_block);
            }
            Expr::Get(get) => {
                // There is not actually much to do here.
                return self.expr(ast, db, get.lhs, target_block);
            }
            Expr::MakeRange(make) => {
                target_block = self.expr(ast, db, make.left, target_block);
                target_block = self.expr(ast, db, make.right, target_block);
                return target_block;
            }
            Expr::WhileLoop(loop_) => {
                // TODO: Synthesize continuations and stuff. This one will
                // be interesting. For now we just barely support it for reasons.
                target_block = self.expr(ast, db, loop_.condition, target_block);
                target_block = self.expr(ast, db, loop_.inner, target_block);
                return target_block;
            }
            oops @ _ => {
                todo!("{:#?}", std::mem::discriminant(oops))
            }
        }
    }

    fn maybe_expr(&mut self, ast: &AstProxy, db: &mut Db, expr: Option<ExprId>, target_block: ExprId) -> ExprId {
        if let Some(expr) = expr {
            return self.expr(ast, db, expr, target_block);
        }
        return target_block;
    }

    fn _do_stmt(&mut self, ast: &AstProxy, db: &mut Db, stmt: StmtId, mut target_block: ExprId) -> ExprId {
        let binding = ast.get_stmt(stmt);
        match binding.as_ref() {
            Stmt::Declare(declare) => {
                target_block = self.maybe_expr(ast, db, declare.value, target_block);
                return target_block;
            }
            Stmt::Expression(expression) => {
                target_block = self.expr(ast, db, expression.expression, target_block);
                return target_block;
            },
            Stmt::ClassDeclare(class_declare) => {
                // Need to visit each function.
                todo!()
            },
        }
    }

    fn stmt(&mut self, ast: &AstProxy, db: &mut Db, stmt: StmtId, mut target_block: ExprId) -> ExprId {
        target_block = self._do_stmt(ast, db, stmt, target_block);

        // Push the statement into the target block.
        push_to_block(ast, target_block, stmt);
    
        return target_block;
    }

    fn function(&mut self, ast: &AstProxy, db: &mut Db, fun: FunId, target_block: ExprId) -> ExprId {
        let enclosing = self.current_fun;
        let enclosing_closure = self.current_closure;
        self.current_fun = Some(fun);
        // The current closure is the param closure.
        //
        // That's because the param_closure is the one that we want to be the
        // parent of any new continuation we build.
        self.current_closure = Some(db.get(fun).param_closure);

        self.maybe_expr(ast, db, db.get(fun).expression, target_block);

        self.current_fun = enclosing;
        self.current_closure = enclosing_closure;

        // Return the enclosing target_block..?
        target_block
    }
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

        // TODO: We need to implicitly add a call to the return continuation
        // if the function has no return statements.
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
                    // This is, uh, O(n^2), and of course, still very wrong,
                    // but it does actually kind of work.
                    self.visit_expr(ast, db, new_alloc);

                    return;
                }
            }

            // Otherwise, visit the expr normally...
            //self.visit_expr(ast, db, call_id);

            idx += 1;
        }
    }
}

pub fn convert_awaits(ast: &mut Ast, db: &mut Db) {
    let dummy_block = Expr::put_block(ast, db.synthetic(), Vec::new(), db.types.void);
    
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
            convert.function(&proxy, db, fun.identity, dummy_block);
            // As a sanity check, make sure nothing was pushed into the dummy_block.
            let check = proxy.get_expr(dummy_block);
            let Expr::Block(block) = check.as_ref() else { unreachable!() };

            if !block.stmts.is_empty() {
                panic!("ICE: Async conversion for '{}' resulted in dummy_block with {} statements",
                    db.get_fun_name(fun.identity), block.stmts.len())
            }
        }

        // for class in &module.classes {
        //     convert.visit_classdeclare_any(&proxy, db, class);
        // }

        // for var in &module.globals {
        //     if let Some(value) = var.value {
        //         convert.visit_expr(&proxy, db, value);
        //     }
        // }

        drop(source);
        proxy.commit();

        // Put the module back
        //ast.sources.get_mut(source_id).module = module;
    }
}