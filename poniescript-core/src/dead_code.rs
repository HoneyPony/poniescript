use crate::{db::{Ast, AstProxy, Db}, expr::*, module::Module};

use crate::db::ExprId;
use crate::db::StmtId;

use crate::arena::IndexCell;

struct DeadCodeElim<'db> {
	db: &'db mut Db,
}

macro_rules! into {
    ($value:expr, $variant:ident) => {
        {
            let Expr::$variant(v) = $value else { unreachable!() };
            v
        }
    };
}

macro_rules! elim_sequence {
    ($self:expr, $ast:expr, $expr:expr, $variant:ident, $seq_id:ident) => {
        $self.elim_sequence::<$variant, _, _>($ast, 
            into!($expr, $variant).$seq_id.len(),
            |it, idx| { &mut into!(it, $variant).$seq_id[idx] }, 
            |it| { into!(it, $variant).$seq_id },
            $expr)
    }
}

impl<'db> DeadCodeElim<'db> {
	pub fn new(db: &'db mut Db) -> Self {
		return DeadCodeElim {
			db,
		}
	}

    fn elim_sequence<T, F, G>(&mut self, ast: &AstProxy, exprs_count: usize, idx_exprs: F, take_exprs: G, expr: &mut Expr) -> bool
        where F: Fn(&mut Expr, usize) -> &mut ExprId,
        G: Fn(Expr) -> Vec<ExprId>
    {
        let mut last_needed_idx = None;

        for idx in 0..exprs_count {
            if self.elim_expr(ast, idx_exprs(expr, idx)) {
                last_needed_idx = Some(idx);
                break;
            }
        }

        if let Some(last) = last_needed_idx {
            let old = std::mem::take(expr);
            let location = old.location().clone();

            let mut old_exprs = take_exprs(old);

            let mut new_exprs = Vec::new();
            for expr in old_exprs.drain(0..=last) {
                new_exprs.push(Stmt::push_expression(ast, expr.location(ast).clone(), expr));
            }

            let new_block = Expr::mk_block(location, new_exprs, self.db.types.bottom);
            *expr = new_block;

            return true;
        }

        false
    }

    fn elim_expr(&mut self, ast: &AstProxy, expr_id: &mut ExprId) -> bool {
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

        let mut binding = ast.exprs.get_mut(*expr_id);
        let expr = binding.as_mut();

        match expr {
            Expr::Binary(binary) => {
                if self.elim_expr(ast, &mut binary.left) {
                    let left = binary.left;
                    *expr_id = left;
                    return true;
                }

                if self.elim_expr(ast, &mut binary.right) {
                    return true;
                }

                false
                // else if binary.right.typ(self.db) == self.db.types.bottom {
                //     let left = std::mem::take(binary.left);
                //     *expr = left;
                // }
            },
            Expr::OptionElse(opt_else) => {
                // If the value is dead, the whole expression is dead. If the
                // otherwise is dead, then the expression is just the value.
                if self.elim_expr(ast, &mut opt_else.value) {
                    *expr_id = opt_else.value;
                    return true;
                }

                if self.elim_expr(ast, &mut opt_else.otherwise) {
                    // In the case of OptionElse, we are not ourselves a
                    // Bottom value in this case. In fact, our type will end
                    // just being the non-else branch.
                    return false;
                }

                false
            }
            Expr::Lerp(_lerp) => {
                // TODO
                false
            }
            Expr::MakeSumType(_sum) => {
                // TODO
                // Currently nothing to do. This will change...
                false
            }
            Expr::Comparison(comparison) => {
                if self.elim_expr(ast, &mut comparison.left) {
                    *expr_id = comparison.left;
                    return true;
                }

                if self.elim_expr(ast, &mut comparison.right) {
                    return true;
                }

                false
                // else if comparison.right.typ(self.db) == self.db.types.bottom {
                //     let left = std::mem::take(comparison.left);
                //     *expr = left;
                // }
            },
            Expr::Variable(_) => {
                // Nothing to eliminate.
                false
            },
            Expr::Logical(logical) => {
                if self.elim_expr(ast, &mut logical.left) {
                    *expr_id = logical.left;
                    return true;
                }

                // Don't return true if logical.right is a Bottom, because it
                // might not always be evaluated.
                self.elim_expr(ast, &mut logical.right);

                false
                // else if logical.right.typ(self.db) == self.db.types.bottom {
                //     let left = std::mem::take(logical.left);
                //     *expr = left;
                // }
            },
            Expr::FunCall(_) => {
                elim_sequence!(self, ast, expr,
                    FunCall, args)
            },
            Expr::FunDeclare(fun_declare) => {
                self.elim_expr(ast, &mut fun_declare.value);

                // A FunDeclare itself never evaluates to Bottom. It always evaluates
                // to a function type.
                false
            },
            Expr::ValCall(_) => {
                elim_sequence!(self, ast, expr,
                    ValCall, args)
            },
            Expr::FunCapture(_) => { false },
            Expr::Assign(assign) => {
                if self.elim_expr(ast, &mut assign.value) {
                    *expr_id = assign.value;
                    return true;
                }
                false
            },
            Expr::UnboundAssign(_unbound_assign) => panic!("ICE: Tried to DCE UnboundAssign"),
            Expr::NumLiteral(_) => false,
            Expr::StrLiteral(_) => false,
            Expr::BoolLiteral(_) => false,
            Expr::Block(block) => {
                let mut last_needed_idx = None;

                for idx in 0..block.stmts.len() {
                    if self.elim_stmt(ast, &mut block.stmts[idx]) {
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
                self.elim_expr(ast, &mut if_.condition);
                if if_.condition.typ(ast, &self.db) == self.db.types.bottom {
                    *expr_id = if_.condition;
                    return true;
                }
                else {
                    // Note: We can only do this if the conditional didn't
                    // evaluate to Bottom (because otherwise, the if has
                    // been deleted at this point!)
                    //
                    // Although in this case Rust will yell at us if we try.
                    let mut is_bottom = self.elim_expr(ast, &mut if_.then_branch);
                    if let Some(else_) = &mut if_.else_branch {
                        is_bottom = is_bottom && self.elim_expr(ast, else_);
                    }

                    return is_bottom
                }
            },
            Expr::Unbound(_) => panic!("ICE: Tried to DCE Unbound"),
            Expr::UnboundFunCapture(_) => panic!("ICE: Tried to DCE UnboundFunCapture"),
            Expr::Print(_) => {
                elim_sequence!(self, ast, expr,
                    Print, exprs)
            },
            Expr::Str(_) => {
                elim_sequence!(self, ast, expr,
                    Str, exprs)
            },
            Expr::New(new) => {
                // Note: We don't really care about the performance impact of
                // creating a new vector here in take_exprs because that code
                // should almost never run.
                self.elim_sequence::<New, _, _>(ast, 
                    new.initializers.len(),
                    |it, idx| { &mut into!(it, New).initializers[idx].value }, 
                    |it| { into!(it, New).initializers.iter().map(|init| init.value).collect() },
                    expr)
            },
            Expr::Get(get) => {
                if self.elim_expr(ast, &mut get.lhs) {
                    *expr_id = get.lhs;
                    return true;
                }
                false
            },
            Expr::Set(set) => {
                if self.elim_expr(ast, &mut set.lhs) {
                    *expr_id = set.lhs;
                    return true;
                }

                if self.elim_expr(ast, &mut set.rhs) {
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
            Expr::SelfVal(_) => {
                false
            }
            Expr::ArrayLit(_) => {
                elim_sequence!(self, ast, expr,
                    ArrayLit, values)
            }
            Expr::Index(_) => {
                // TODO eliminate pair like binop
                false
            }
            Expr::SetIndex(_) => {
                // TODO eliminate
                false
            }
            Expr::Promote(promote) => {
                // Dead code comes after TypeCheck so we have to DCE promote
                if self.elim_expr(ast, &mut promote.inner) {
                    *expr_id = promote.inner;
                    return true;
                }
                return false;
            }
            Expr::Undefined(_) => panic!("ICE: Tried to DCE Undefined"),
            Expr::MakeTuple(_) => {
                elim_sequence!(self, ast, expr,
                    MakeTuple, values)
            }
        }
    }

    // Returns whether the Stmt "evaluates" to Bottom.
    fn elim_stmt(&mut self, ast: &AstProxy, stmt_id: &mut StmtId) -> bool {
        let mut binding = ast.stmts.get_mut(*stmt_id);
        let stmt = binding.as_mut();
        match stmt {
            Stmt::Declare(declare) => {
                // So even though we can't declare variables as Bottom, we
                // can still do something like:
                // var x : int = { return; }
                // Which is valid.
                // In these cases, we do have to propogate whatever value
                // we found inside the assignment upwards.
                self.elim_expr(ast, &mut declare.value);

                // If the eliminated expression is a Bottom, then we can replace
                // ourselves with it.
                if declare.value.typ(ast, self.db) == self.db.types.bottom {
                    *stmt = Stmt::mk_expression(declare.location.clone(), declare.value);
                    return true
                }

                false
            },
            Stmt::Expression(expression) => {
                self.elim_expr(ast, &mut expression.expression);
                expression.expression.typ(ast, self.db) == self.db.types.bottom
            },
            Stmt::Return(ret) => {
                if let Some(inner) = &mut ret.expression {
                    self.elim_expr(ast, inner);
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
                self.elim_class(ast, class_declare);
                false
            },
        }
    }

    fn elim_class(&mut self, ast: &AstProxy, class: &mut ClassDeclare) {
        for fun in &mut class.funs {
            self.elim_expr(ast, &mut fun.value);
        }
    }

    fn elim_module(&mut self, ast: &AstProxy, module: &mut Module) {
        // TODO: I think that maybe variable declarations should not be
        // allowed to have type Bottom.
        // for var in &mut module.globals {
        //     self.elim_declare(var);
        // }

        for fun in &mut module.functions {
            self.elim_expr(ast, &mut fun.value);
        }
        for class in &mut module.classes {
            self.elim_class(ast, class);
        }
    }

    fn elim_modules(&mut self, ast: &AstProxy) {
        for source in ast.sources.iter() {
            let mut source = ast.sources.get_mut(source);
            let module = &mut source.module;
            self.elim_module(ast, module);
        }
    }
}


pub fn eliminate_dead_code(db: &mut Db, ast: &mut Ast) {
	let mut dc = DeadCodeElim::new(db);

    let proxy = ast.get_proxy();

	dc.elim_modules(&proxy);

    proxy.commit();
}