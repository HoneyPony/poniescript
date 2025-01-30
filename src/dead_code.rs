use crate::{db::Db, expr::{ClassDeclare, Expr, Stmt}, module::Module};

struct DeadCodeElim<'db> {
	db: &'db mut Db,
}

impl<'db> DeadCodeElim<'db> {
	pub fn new(db: &'db mut Db) -> Self {
		return DeadCodeElim {
			db,
		}
	}

    fn elim_expr(&mut self, expr: &mut Expr) -> bool {
        // Counterintuitive but true:
        // For any binary expression, such as a + b, if the LHS is Bottom, 
        // we can replace the whole expression with the LHS (because the RHS
        // would never get to be evaluated anyway).
        //
        // If the RHS is Bottom, we cannot eliminate the LHS, as it must be
        // evaluated before the RHS. But we also can't eliminate the RHS, 
        // because it might have side-effects.
        //
        // So what we really have to do in the case that RHS is bottom is
        // turn the expression into a sequence, e.g. evaluate LHS first then
        // RHS but without any need to evaluate the higher level expression.
        // 
        // We can either implement this as a separate Expr::Sequence node or
        // something, or by making the codegen stage understand how to do that.
        match expr {
            Expr::Binary(binary) => {
                if self.elim_expr(&mut binary.left) {
                    let left = std::mem::take(binary.left);
                    *expr = left;
                    return true;
                }

                if self.elim_expr(&mut binary.right) {
                    return true;
                }

                false
                // else if binary.right.typ(self.db) == self.db.types.bottom {
                //     let left = std::mem::take(binary.left);
                //     *expr = left;
                // }
            },
            Expr::Comparison(comparison) => {
                

                if self.elim_expr(&mut comparison.left) {
                    let left = std::mem::take(comparison.left);
                    *expr = left;
                    return true;
                }

                if self.elim_expr(&mut comparison.right) {
                    return true;
                }

                false
                // else if comparison.right.typ(self.db) == self.db.types.bottom {
                //     let left = std::mem::take(comparison.left);
                //     *expr = left;
                // }
            },
            Expr::Variable(variable) => {
                false
            },
            Expr::Logical(logical) => {
                if self.elim_expr(&mut logical.left) {
                    let left = std::mem::take(logical.left);
                    *expr = left;
                    return true;
                }

                // Don't return true if logical.right is a Bottom, because it
                // might not always be evaluated.
                self.elim_expr(&mut logical.right);

                false
                // else if logical.right.typ(self.db) == self.db.types.bottom {
                //     let left = std::mem::take(logical.left);
                //     *expr = left;
                // }
            },
            Expr::FunCall(fun_call) => {
                let mut last_needed_idx = None;

                for idx in 0..fun_call.args.len() {
                    if self.elim_expr(&mut fun_call.args[idx]) {
                        last_needed_idx = Some(idx);
                        break;
                    }
                }

                if let Some(last) = last_needed_idx {
                    let old = std::mem::take(expr);
                    let Expr::FunCall(old) = old else { unreachable!() };

                    let (mut args, location) = (old.args, old.location);
                    
                    let mut new_exprs = Vec::new();
                    for arg in args.drain(0..=last) {
                        new_exprs.push(Stmt::mk_expression(arg.location().clone(), arg));
                    }

                    let new_block = Expr::mk_block(location, new_exprs, self.db.types.bottom);
                    *expr = new_block;

                    return true;
                }

                false
            },
            Expr::FunDeclare(fun_declare) => {
                self.elim_expr(&mut fun_declare.value)
            },
            Expr::ValCall(val_call) => {
                let mut last_needed_idx = None;

                for idx in 0..val_call.args.len() {
                    if self.elim_expr(&mut val_call.args[idx]) {
                        last_needed_idx = Some(idx);
                        break;
                    }
                }

                if let Some(last) = last_needed_idx {
                    let old = std::mem::take(expr);
                    let Expr::ValCall(old) = old else { unreachable!() };

                    let (mut args, location) = (old.args, old.location);
                    
                    let mut new_exprs = Vec::new();
                    for arg in args.drain(0..=last) {
                        new_exprs.push(Stmt::mk_expression(arg.location().clone(), arg));
                    }

                    let new_block = Expr::mk_block(location, new_exprs, self.db.types.bottom);
                    *expr = new_block;

                    return true;
                }

                false
            },
            Expr::FunCapture(fun_capture) => { false },
            Expr::Assign(assign) => {
                if self.elim_expr(&mut assign.value) {
                    let value = std::mem::take(assign.value);
                    *expr = value;
                    return true;
                }
                false
            },
            Expr::UnboundAssign(unbound_assign) => unreachable!("dead code UnboundAssign"),
            Expr::NumLiteral(num_literal) => false,
            Expr::StrLiteral(str_literal) => false,
            Expr::BoolLiteral(bool_literal) => false,
            Expr::Block(block) => {
                let mut last_needed_idx = None;

                for idx in 0..block.stmts.len() {
                    if self.elim_stmt(&mut block.stmts[idx]) {
                        last_needed_idx = Some(idx);
                        break;
                    }
                }

                if let Some(last) = last_needed_idx {
                    block.stmts.truncate(last + 1);
                    return true;
                }

                false
            },
            Expr::If(if_) => {
                // The if unconditionally evaluates its conditional, so if
                // the condition is itself a Bottom, then the if should just
                // be replaced with that. Otherwise, even if both branches are
                // Bottom, there isn't dead code, because the branches might be
                // evaluating different things that both end up as Bottom.
                //
                // Note: We might want to eventually also prune dead branches
                // if e.g. the conditional evaluates to true. But we might need
                // a constant folding pass for that to work.
                self.elim_expr(&mut if_.condition);
                if if_.condition.typ(&self.db) == self.db.types.bottom {
                    let replace = std::mem::take(if_.condition);
                    *expr = replace;
                    return true;
                }
                else {
                    // Note: We can only do this if the conditional didn't
                    // evaluate to Bottom (because otherwise, the if has
                    // been deleted at this point!)
                    //
                    // Although in this case Rust will yell at us if we try.
                    let mut is_bottom = self.elim_expr(&mut if_.then_branch);
                    if let Some(else_) = &mut if_.else_branch {
                        is_bottom = is_bottom && self.elim_expr(else_);
                    }

                    return is_bottom
                }
            },
            Expr::Unbound(unbound) => unreachable!("dead code Unbound"),
            Expr::UnboundCall(unbound_call) => unreachable!("dead code UnboundCall"),
            Expr::Print(print) => {
                let mut last_needed_idx = None;

                for idx in 0..print.exprs.len() {
                    if self.elim_expr(&mut print.exprs[idx]) {
                        last_needed_idx = Some(idx);
                        break;
                    }
                }

                if let Some(last) = last_needed_idx {
                    let old = std::mem::take(expr);
                    let Expr::Print(old) = old else { unreachable!() };

                    let (mut args, location) = (old.exprs, old.location);
                    
                    let mut new_exprs = Vec::new();
                    for arg in args.drain(0..=last) {
                        new_exprs.push(Stmt::mk_expression(arg.location().clone(), arg));
                    }

                    let new_block = Expr::mk_block(location, new_exprs, self.db.types.bottom);
                    *expr = new_block;

                    return true;
                }

                false
            },
            Expr::Str(str) => {
                let mut last_needed_idx = None;

                for idx in 0..str.exprs.len() {
                    if self.elim_expr(&mut str.exprs[idx]) {
                        last_needed_idx = Some(idx);
                        break;
                    }
                }

                if let Some(last) = last_needed_idx {
                    let old = std::mem::take(expr);
                    let Expr::Str(old) = old else { unreachable!() };

                    let (mut args, location) = (old.exprs, old.location);
                    
                    let mut new_exprs = Vec::new();
                    for arg in args.drain(0..=last) {
                        new_exprs.push(Stmt::mk_expression(arg.location().clone(), arg));
                    }

                    let new_block = Expr::mk_block(location, new_exprs, self.db.types.bottom);
                    *expr = new_block;

                    return true;
                }

                false
            },
            Expr::New(new) => {
                false // TODO
            },
            Expr::Get(get) => {
                if self.elim_expr(get.lhs) {
                    let replace = std::mem::take(get.lhs);
                    *expr = replace;
                    return true;
                }
                false
            },
            Expr::Set(set) => {
                if self.elim_expr(set.lhs) {
                    let replace = std::mem::take(set.lhs);
                    *expr = replace;
                    return true;
                }

                if self.elim_expr(set.rhs) {
                    return true;
                }

                return false;
                // if set.rhs.typ(&self.db) == self.db.types.bottom {
                //     // important: lhs
                //     let replace = std::mem::take(set.lhs);
                //     *expr = replace;
                //     return;
                // }
            },
            Expr::Undefined(undefined) => unreachable!("dead code Undefined"),
        }
    }

    // Returns whether the Stmt "evaluates" to Bottom.
    fn elim_stmt(&mut self, stmt: &mut Stmt) -> bool {
        match stmt {
            Stmt::Declare(declare) => {
                // So even though we can't declare variables as Bottom, we
                // can still do something like:
                // var x : int = { return; }
                // Which is valid.
                // In these cases, we do have to propogate whatever value
                // we found inside the assignment upwards.
                self.elim_expr(&mut declare.value);

                // If the eliminated expression is a Bottom, then we can replace
                // ourselves with it.
                if declare.value.typ(self.db) == self.db.types.bottom {
                    let value = std::mem::take(&mut declare.value);
                    *stmt = Stmt::mk_expression(declare.location.clone(), value);
                    return true
                }

                false
            },
            Stmt::Expression(expression) => {
                self.elim_expr(&mut expression.expression);
                expression.expression.typ(self.db) == self.db.types.bottom
            },
            Stmt::Return(ret) => {
                if let Some(inner) = &mut ret.expression {
                    self.elim_expr(inner);
                }

                true
            },

            // Awkward:
            // If we delete all the ClassDeclares in a Block that has been
            // dead-code-eliminated, those classes can't be accessed in that
            // block.
            //
            // But maybe that's fine? It's possible we want it so that classes
            // that are private to a Block can only be used by code after that
            // class has been declared..
            Stmt::ClassDeclare(class_declare) => {
                self.elim_class(class_declare);
                false
            },
        }
    }

    fn elim_class(&mut self, class: &mut ClassDeclare) {
        for fun in &mut class.funs {
            self.elim_expr(fun.value);
        }
    }

    fn elim_module(&mut self, module: &mut Module) {
        // TODO: I think that maybe variable declarations should not be
        // allowed to have type Bottom.
        // for var in &mut module.globals {
        //     self.elim_declare(var);
        // }

        for fun in &mut module.functions {
            self.elim_expr(fun.value);
        }
        for class in &mut module.classes {
            self.elim_class(class);
        }
    }

    fn elim_modules(&mut self, modules: &mut Vec<Module>) {
        for module in modules {
            self.elim_module(module);
        }
    }
}


pub fn eliminate_dead_code(db: &mut Db, modules: &mut Vec<Module>) {
	let mut dc = DeadCodeElim::new(db);

	dc.elim_modules(modules);
}