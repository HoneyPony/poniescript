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
    dummy_block: ExprId,
}

fn push_to_block(ast: &AstProxy, target_block: ExprId, new_stmt: StmtId) {
    let mut binding = ast.get_expr_mut(target_block);
    let Expr::Block(block) = binding.as_mut() else { panic!("ICE: Non-block target_block"); };

    log::trace!("push stmt {} to block {}", new_stmt.to_index(), target_block.to_index());

    block.stmts.push(new_stmt);
}

impl AsyncConvert {
    fn call_continuation(
        &mut self,
        ast: &AstProxy,
        db: &mut Db,
        fun: FunId,
        value: Option<ExprId>
    ) -> ExprId {
        // TODO: If we are calling a fun(), we actually need to essentially do
        // {
        //     <evaluate value>
        //     call();
        // }
        let loc = db.synthetic();
        Expr::push_funcall(ast, loc.clone(), loc.clone(),
            // TODO: The number of arguments in the value needs to match the call...
            fun, value.into_iter().collect(), None, CallType::Normal, Vec::new())
    }

    fn splice_continuation_call(
        &mut self,
        ast: &AstProxy,
        db: &mut Db,
        // The block that we're splicing the call into.
        dest_block: ExprId,
        continuation: FunId,
    ) {
        let mut binding = ast.get_expr_mut(dest_block);
        let Expr::Block(block) = binding.as_mut() else { panic!("ICE: Non-Block in splice_continuation_call"); };

        let last_expr = block.stmts.last();
        match last_expr {
            Some(s) => {
                let mut binding = ast.get_stmt_mut(*s);
                match binding.as_mut() {
                    Stmt::Declare(declare) => (),
                    Stmt::Expression(expression) => {
                        let call = self.call_continuation(ast, db, continuation, Some(expression.expression));
                        expression.expression = call;
                        return;
                    }
                    Stmt::ClassDeclare(class_declare) => (),
                }
            }
            None => (),
        };
        
        // Ok, well, we weren't able to replace the last expression of the block with a call, so synthesize
        // a new call.
        let call = self.call_continuation(ast, db, continuation, None);
        let call_stmt = Stmt::push_expression(ast, db.synthetic(), call);
        block.stmts.push(call_stmt);
    }

    /// Synthesizes a new continuation function.
    /// 
    /// The callback_param may be either None (for an empty callback, equivalent to void), or a void type.
    /// 
    /// If it is void, you should not call the callback with any parameters.
    fn synthesize_continuation(
        &mut self,
        ast: &AstProxy,
        db: &mut Db,
        tag: &'static str,
        current_closure: Option<ClosureId>,
        mut callback_param: Option<TypId>,
        target_block: ExprId,
    ) -> (ExprId, FunId, ClosureId, Expr) {
        let loc = db.synthetic(); // idk

        if let Some(cb) = callback_param {
            if cb == db.types.void {
                // Convert these into empty callback param so we handle it consistently
                callback_param = None;
            }
        }

        let new_block = Expr::push_block(ast,
            loc.clone(),
            // The new block starts empty; it is filled up through the target_block system.
            Vec::new(),
            db.types.void);
        let closure = Closure {
            class: None,
            parent: current_closure,
            parent_class: None,
        };
        let closure = db.push(closure);
        let new_alloc = Expr::push_allocateclosure(ast, loc.clone(), closure,
            new_block, db.types.void,
            // This closure is responsible for copying params.
            true);

        let replacement_expr: Expr;

        // We re-use this sig as the sig for the new function.
        let mut parameters = vec![];
        let mut the_new_var = None;
        // If there is a single parameter in the cb_type, that is our
        // return-type variable for our callback.
        if let Some(param) = callback_param {
            let new_var_name = db.put_str("await");
            let new_var = db.new_var(new_var_name, param, true, None,
                None, Some(closure), None, None, loc.clone(), None);
            parameters.push(new_var);

            // It is important to save this for later, because we have to assign
            // it's fun, otherwise closure_convert won't convert it
            the_new_var = Some(new_var);

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
        let sig = Sig {
            parameters: the_new_var.map(|v| db.get(v).typ).into_iter().collect(),
            return_type: db.types.void,
        };
        let sig = db.put_sig(&sig);
        db.use_sig(sig);

        // Use the name "continuation" for these functions, to make
        // them clearer in debug output
        let name = db.put_str(tag);

        let new_function = Fun {
            name: Some(name),
            sig,
            parameters,
            return_type: db.types.void,
            sugar_return_type: db.types.void,
            asyncness: Asyncness::Not,
            class: None,
            closure: current_closure, // I believe this is the enclosing closure
            param_closure: closure, // This would be the inner closure
            expression: Some(new_alloc),
            location: loc.clone(),
            doc_comment: None,
        };

        let new_function = db.push(new_function);
        
        if let Some(var) = the_new_var {
            // Make sure the var is assigned the function, otherwise closure conversion
            // won't work.
            db.get_mut(var).fun = Some(new_function);
            // ...And it is a parameter.
            db.get_mut(var).param_for = Some(new_function);
        }

        // We must add the fun declare to this block so that the closure
        // conversion pass will see it.
        let fundeclare = Expr::push_fundeclare(ast, loc.clone(), new_function, new_alloc, db.types.void);
        let fundeclare_stmt = Stmt::push_expression(ast, loc.clone(), fundeclare);
        
        // THIS needs to be the OLD target_block.
        push_to_block(ast, target_block, fundeclare_stmt);

        (
            new_block,
            new_function,
            closure,
            replacement_expr
        )
    }

    // Returns (target_block, own_dest_block)
    fn expr(&mut self, ast: &AstProxy, db: &mut Db, expr: ExprId, mut target_block: ExprId) -> (ExprId, ExprId) {
        let mut binding = ast.get_expr_mut(expr);
        match binding.as_mut() {
            Expr::Binary(binary) => {
                target_block = self.expr(ast, db, binary.left, target_block).0;
                target_block = self.expr(ast, db, binary.right, target_block).0;
                return (target_block, target_block);
            },
            Expr::Unary(unary) => {
                return self.expr(ast, db, unary.inner, target_block);
            }
            Expr::Comparison(comparison) => {
                target_block = self.expr(ast, db, comparison.left, target_block).0;
                target_block = self.expr(ast, db, comparison.right, target_block).0;
                return (target_block, target_block);
            }
            Expr::FunCall(call) => {
                if let Some(obj) = call.object {
                    target_block = self.expr(ast, db, obj, target_block).0;
                }
                for arg in &call.args {
                    target_block = self.expr(ast, db, *arg, target_block).0;
                }

                if call.call_type == CallType::Await {
                    // If we are an Await call, we want to do the following:
                    // - Append a FunDeclare to the current block
                    // - Append a FunCall to the new continuation to our own argument list
                    // - Return the block for the new function as the new target_block
                    let loc = call.location.clone();
                    let fun = call.identity;
                    drop(binding); // So we don't double borrow soon
                    
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

                    // The callback_param is literally the parameter of our callback, i.e. our async_continuation
                    // param. That is, it's the first parameter of the function type which is the last parameter.
                    let callback_param = db.get(sig).parameters.first().copied();
                    let (new_block, new_function, closure, replacement_expr)
                        = self.synthesize_continuation(ast, db, "funcall_continuation", self.current_closure, callback_param, target_block);

                    let fun_capture = Expr::push_funcapture(ast, loc.clone(),
                loc.clone(), new_function, db.put_type(Type::Fun(sig)), None);

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

                    // I'm not entirely sure if this is right, but it seems like it should be?
                    //
                    // We definitely need to keep track of the new closure SOMEWHERE. The question is whether
                    // we ever pop this value in some way.
                    //
                    // We can't update self.current_fun, though, because we still need the *real* current_fun to find
                    // the async_continuation for return statements. We will likely have to revisit this when we 
                    // implement loop handling.
                    self.current_closure = Some(closure);
                    //self.current_fun = Some(new_function);

                    // Interestingly, there is nothing to visit in the new function yet. (And in fact, there never will be).
                    // Instead, we simply have a new target_block, for the rest of the upcoming statements.
                    target_block = new_block;
                }

                return (target_block, target_block);
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
                log::trace!("clearing block {} (target_block = {}). inner stmts: ",
                    expr.to_index(), target_block.to_index());
                for stmt in &take {
                    let tmp = ast.get_stmt(*stmt);
                    match tmp.as_ref() {
                        Stmt::Declare(_) => log::trace!(" - {} (Declare)", stmt.to_index()),
                        Stmt::Expression(expression) => log::trace!(" - {} (ExprId {})",
                            stmt.to_index(), expression.expression.to_index()),
                        Stmt::ClassDeclare(_) => log::trace!(" - {} (ClassDeclare)", stmt.to_index()),
                    }
                    
                }
                drop(binding); // We will re-borrow the block later.

                let mut inner_target: ExprId = expr;
                for stmt in take {
                    // This may or may not be correct...
                    inner_target = self.stmt(ast, db, stmt, inner_target);
                }

                // let mut final_target_block = target_block;

                // if inner_target != expr {
                //     let mut binding = ast.get_expr_mut(expr);
                //     let Expr::Block(block) = binding.as_mut() else { unreachable!() };
                //     block.typ = db.types.void;

                //     final_target_block = inner_target;

                //     // Move all our stmts to our parent block...?
                //     // But that wouldn't be correct for if statements...
                //     //
                //     // We need to figure out how to avoid leaking blocks that are nested inside a block,
                //     // WITHOUT affecting how blocks inside control structures are moved. Weird...
                // }

                // log::trace!("block {}: final_target_block = {}, target_block = {}",
                //     expr.to_index(), final_target_block.to_index(), target_block.to_index());

                //return (final_target_block, target_block);
                // Revert to original target if there were no continuations.
                if inner_target == expr {
                    return (target_block, target_block);
                }

                // Otherwise, we do need to keep the continuation block.
                // But we have to write OURSELVES into our parent block...? IF AND ONLY IF WE ARE A stmt...
                // So we use a split target_block approach.
                //
                // This does seem a little bad, but the idea is we already double-block up the top-level
                // of each function, so this shouuuuuld work...
                if target_block != self.dummy_block {
                    //return inner_target;
                    let mut binding = ast.get_expr_mut(expr);
                    let Expr::Block(block) = binding.as_mut() else { unreachable!() };

                    // Sythesize a new block with our statements and push it to the parent. We remain empty.
                    let take = std::mem::take(&mut block.stmts);
                    let typ = block.typ;
                    block.typ = db.types.void;
                    drop(binding);

                    // TODO: Consider using Type::Bottom for everything instead of Void.
                    let new_block = Expr::push_block(ast, db.synthetic(), take, db.types.void);
                    let new_stmt = Stmt::push_expression(ast, db.synthetic(), new_block);
                    push_to_block(ast, target_block, new_stmt);

                    // Now, replace ourselves with whatever the last statement in the newest block is.

                    let mut binding_inner = ast.get_expr_mut(inner_target);
                    let Expr::Block(inner) = binding_inner.as_mut() else { unreachable!() };
                    // TODO: Does this still apply in the case of void?? Probably not.
                    let Some(last_expr) = inner.stmts.pop() else { panic!("ICE: inner stmts didn't have a last expression") };

                    let mut binding_stmt = ast.get_stmt(last_expr);
                    let Stmt::Expression(to_steal_expr) = binding_stmt.as_ref() else {
                        panic!("ICE: Last expression in async block wasn't an Expression.");
                    };

                    let mut to_steal = ast.get_expr_mut(to_steal_expr.expression);

                    // Re-bind ourselves.
                    let mut binding = ast.get_expr_mut(expr);

                    *binding = std::mem::take(to_steal.as_mut());
                }
                else {
                    // If we're not moving ourselves, then we need to change our type to be void.
                    let mut binding = ast.get_expr_mut(expr);
                    let Expr::Block(block) = binding.as_mut() else { unreachable!() };
                    block.typ = db.types.void;
                }
                return (inner_target, inner_target);
            }
            Expr::AllocateClosure(alloc) => {
                return self.expr(ast, db, alloc.inner, target_block)
            }
            Expr::Print(print) => {
                for arg in &print.exprs {
                    target_block = self.expr(ast, db, *arg, target_block).0;
                }
                return (target_block, target_block);
            }
            Expr::ArrayLit(lit) => {
                for arg in &lit.values {
                    target_block = self.expr(ast, db, *arg, target_block).0;
                }
                return (target_block, target_block);
            }
            Expr::Assign(assign) => {
                target_block = self.expr(ast, db, assign.value, target_block).0;
                return (target_block, target_block);
            }
            Expr::Index(index) => {
                target_block = self.expr(ast, db, index.index, target_block).0;
                target_block = self.expr(ast, db, index.value, target_block).0;
                return (target_block, target_block);
            }
            Expr::StrLiteral(_) => { (target_block, target_block) }
            Expr::NumLiteral(_) => { (target_block, target_block) }
            Expr::BoolLiteral(_) => { (target_block, target_block) }
            Expr::Variable(_) => { (target_block, target_block) }
            Expr::Return(ret) => {
                // Returns are special, because they must be replaced with a call to the
                // async continuation.
                target_block = self.maybe_expr(ast, db, ret.expression, target_block).0;

                // Whenever we encounter a return statement, instead call the function's
                // end_continuation.
                if let Some(fun) = self.current_fun {
                    if db.get(fun).asyncness == Asyncness::Implicit {
                        log::trace!("moving Return to block {}", target_block.to_index());
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

                return (target_block, target_block);
            }
            Expr::ValCall(call) => {
                // TODO: Actually synthesize the closure. :)
                for arg in &call.args {
                    target_block = self.expr(ast, db, *arg, target_block).0;
                }
                return (target_block, target_block);
            }
            Expr::BuiltinCall(call) => {
                // For now, builtin calls cannot themselves be async, so there is nothing
                // to convert.
                target_block = self.expr(ast, db, call.object, target_block).0;
                for arg in &call.args {
                    target_block = self.expr(ast, db, *arg, target_block).0;
                }
                return (target_block, target_block);
            }
            Expr::FunDeclare(fun) => {
                // TODO: We may want to avoid traversing these through the AST, and instead
                // do it in a top-level way. This would allow us to skip any functions that
                // do not contain any .await's entirely.
                self.function(ast, db, fun.identity, target_block);
                // In case we overwrote the function expressoin in-Fun, also do it here.
                if let Some(expr) = db.get(fun.identity).expression {
                    fun.value = expr;
                }
                // Keep same target block.
                return (target_block, target_block);
            }
            Expr::FunCapture(capt) => {
                return self.maybe_expr(ast, db, capt.object, target_block);
            }
            Expr::Get(get) => {
                // There is not actually much to do here.
                return self.expr(ast, db, get.lhs, target_block);
            }
            Expr::MakeRange(make) => {
                target_block = self.expr(ast, db, make.left, target_block).0;
                target_block = self.expr(ast, db, make.right, target_block).0;
                return (target_block, target_block);
            }
            Expr::WhileLoop(loop_) => {
                // TODO: Synthesize continuations and stuff. This one will
                // be interesting. For now we just barely support it for reasons.
                target_block = self.expr(ast, db, loop_.condition, target_block).0;
                target_block = self.expr(ast, db, loop_.inner, target_block).0;
                return (target_block, target_block);
            }
            Expr::Loop(loop_) => {
                // The basic idea here is as follows:
                // If there is an await inside the loop, we will need the loop body
                // itself to be a new continuation, as well as everything 'after' the loop.
                //
                // For this reason, we create a new block *immediately*. If it turns out that the
                // loop body was a continuation, we synthesize it into a continuation, then create
                // ANOTHER continuation, and make that the exit continuation for the loop.
                //
                // If it turns out the loop body was NOT a continuation, we simply append everything
                // to the parent target block.
                //
                // We figure this out by checking if target block changes. If it stays the same, there
                // is no need for any continuation synthesis.
                todo!()
            }
            Expr::If(if_) => {
                // The idea here is similar to the Loops.
                //
                // Basically, we want to see if either of our own inner expressions changes.
                // I actually think what we want to do is just make it a propert of if/else
                // that they always have an inner block, to keep the building of the continuations
                // more straightforward.
                //
                // In any case... What we want to do is, if the target_block changed, synthesize
                // one new continuation, and then write into the end of each target_block a jump
                // to this continuation.
                target_block = self.expr(ast, db, if_.condition, target_block).0;

                let if_closure = self.current_closure;

                // We pass the dummy_block as the target block because we are in control of the flow.
                let then_branch = self.expr(ast, db, if_.then_branch, self.dummy_block).0;
                let else_branch = match if_.else_branch {
                    Some(branch) => Some(self.expr(ast, db, branch, self.dummy_block).0),
                    None => None
                };

                log::trace!("if: got then_branch = {}, else_branch = {}, dummy_block = {}",
                    then_branch.to_index(), else_branch.map(|x| x.to_index()).unwrap_or(0xFFFFFFFFFF), self.dummy_block.to_index());

                let needs_continuation = then_branch != self.dummy_block || match else_branch {
                    Some(b) => b != self.dummy_block,
                    None => false
                };

                if needs_continuation {
                    log::trace!("async: if type = {}", db.repr_type(if_.typ));
                    let loc = db.synthetic();
                    let (new_block, new_function, closure, replacement_expr)
                        = self.synthesize_continuation(ast, db, "if_continuation", if_closure, Some(if_.typ), target_block);


                    // What we need to do is put our own continuation at the end of the converted blocks.
                    // 
                    // That is,
                    // if { let x = a().await; x + 2 } else { b().await }
                    //
                    // Should be converted into:
                    // ```
                    // if {
                    //     a(fun(a_result) {
                    //         let x = a_result;
                    //         if_continuation(x + 2)
                    //     })
                    // }
                    // else {
                    //     b(fun(b_result) {
                    //         if_continuation(b_result)
                    //     })
                    // }
                    // ```
                    //
                    // One reasonably clean way to do this should be (?) to 'simply' replace the last expression
                    // in the block with a call, with the old expression as an argument.

                    let else_branch = match else_branch {
                        Some(e) => e,
                        None => {
                            // If we have an empty else branch, we have to synthesize a new one.
                            let else_branch = Expr::push_block(ast, db.synthetic(), Vec::new(), db.types.void);
                            if_.else_branch = Some(else_branch); // This must be put on the if_ as well

                            else_branch
                        },
                    };

                    self.splice_continuation_call(ast, db, then_branch, new_function);
                    self.splice_continuation_call(ast, db, else_branch, new_function);

                    if_.typ = db.types.void;

                    // As with function calls, we in-place replace the old if statement with the new variable
                    // representing its value. Then, we push the if statement to the OLD target_block.
                    let if_ = std::mem::replace(binding.as_mut(), replacement_expr);

                    // Finally-finally, the fun_call itself still needs to occur, so append it as a new
                    // expr to the current block (not the new block). It occurs as a fun call that is assigned
                    // to nothing.
                    drop(binding);
                    let if_expr = ast.exprs.push(if_);
                    let if_stmt = Stmt::push_expression(ast, loc.clone(), if_expr);
                    push_to_block(ast, target_block, if_stmt);

                    // I'm not entirely sure if this is right, but it seems like it should be?
                    //
                    // We definitely need to keep track of the new closure SOMEWHERE. The question is whether
                    // we ever pop this value in some way.
                    //
                    // We can't update self.current_fun, though, because we still need the *real* current_fun to find
                    // the async_continuation for return statements. We will likely have to revisit this when we 
                    // implement loop handling.
                    self.current_closure = Some(closure);
                    //self.current_fun = Some(new_function);

                    // Interestingly, there is nothing to visit in the new function yet. (And in fact, there never will be).
                    // Instead, we simply have a new target_block, for the rest of the upcoming statements.
                    target_block = new_block;
                    log::trace!("if: target_block is now {}", target_block.to_index());
                }

                (target_block, target_block)
            }
            Expr::Promote(promote) => {
                return self.expr(ast, db, promote.inner, target_block);
            }
            oops @ _ => {
                todo!("{:#?}", std::mem::discriminant(oops))
            }
        }
    }

    fn maybe_expr(&mut self, ast: &AstProxy, db: &mut Db, expr: Option<ExprId>, target_block: ExprId) -> (ExprId, ExprId) {
        if let Some(expr) = expr {
            return self.expr(ast, db, expr, target_block);
        }
        return (target_block, target_block);
    }

    fn _do_stmt(&mut self, ast: &AstProxy, db: &mut Db, stmt: StmtId, mut target_block: ExprId) -> (ExprId, ExprId) {
        let binding = ast.get_stmt(stmt);
        match binding.as_ref() {
            Stmt::Declare(declare) => {
                return self.maybe_expr(ast, db, declare.value, target_block);
            }
            Stmt::Expression(expression) => {
                return self.expr(ast, db, expression.expression, target_block);
            },
            Stmt::ClassDeclare(class_declare) => {
                // Need to visit each function.
                todo!()
            },
        }
    }

    fn stmt(&mut self, ast: &AstProxy, db: &mut Db, stmt: StmtId, mut target_block: ExprId) -> ExprId {
        let (target, own) = self._do_stmt(ast, db, stmt, target_block);
        target_block = target;

        // Push the statement into the target block.
        log::trace!("push stmt {} to {} -- target_block = {}", stmt.to_index(), own.to_index(), target_block.to_index());
        push_to_block(ast, own, stmt);
    
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

        // For implicit functions, we want to desugar the inner block to have an explicit Return.
        //
        // This is because the implicit return has no obvious place to be converted into a continuation.
        // For example consider
        // ```
        // fun example(x: bool) {
        //     if(x) {
        //         some_fun().await
        //     }
        //     else {
        //         some_fun().await
        //     }
        // }
        // ```
        //
        // ... How does the if know that, for its new continuation, it needs to return a value to the
        // function?
        //
        // Adding the return explicitly should help this.
        //
        // The question, I guess, will be whether this same issue impacts any other expressions-as-values.
        // We will see.
        if db.get(fun).asyncness == Asyncness::Implicit {
            if let Some(expr) = db.get(fun).expression {
                // We need to sythesize a Return, but it must occur inside a block so that the target_block
                // logic works.
                // So, synthesize a new block, and put the inner block inside it.
                //
                // I believe we want the new block to be inside the AllocateClosure.
                let mut binding = ast.get_expr_mut(expr);
                let Expr::AllocateClosure(ac) = binding.as_mut() else { panic!("ICE: Fun without AllocateClosure"); };

                // TODO: We may have to change this slightly for void-returning functions.
                let ret = Expr::push_return(ast, db.synthetic(), Some(ac.inner));

                let mut og_block = ast.get_expr_mut(ac.inner);
                let Expr::Block(og) = og_block.as_mut() else { panic!("ICE: Fun without Block"); };
                // The og_block also must have its type changed to void, rather than the sugar return type.
                // og.typ = db.types.void;
                // No, wait, that doesn't make any sense. The og block is being moved inside our return value...

                let ret_stmt = Stmt::push_expression(ast, db.synthetic(), ret);
                let block = Expr::push_block(ast, db.synthetic(), vec![ret_stmt], db.types.void);
                log::trace!("Fun '{}': {} => Expr::Block {} / Ret StmtId {} / Ret ExprId {}", db.get_fun_name(fun),
                    ac.inner.to_index(), block.to_index(), ret_stmt.to_index(), ret.to_index());
                ac.inner = block;            
            }
        }

        //return target_block;
        self.maybe_expr(ast, db, db.get(fun).expression, target_block);

        self.current_fun = enclosing;
        self.current_closure = enclosing_closure;

        // Return the enclosing target_block..?
        target_block
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
            dummy_block,
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