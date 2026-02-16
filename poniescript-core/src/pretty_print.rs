//! A pretty printer for the PonieScript AST.
//! 
//! NOT a code formatter, of course. Just a pretty printed. The main use case
//! is to debug the parser, especially as we move towards more sophisticated
//! LSP support.

use poni_arena::{ArenaKey, IndexCell};

use crate::{codegen::Indenter, db::{Ast, AstAbstract, Db, ExprId, IdFuncs, StmtId}, expr::{ClassDeclare, Declare, FunDeclare}};

struct PrettyPrinter<'ad> {
    ast: &'ad Ast,
    db: &'ad Db,
    indent: usize,
}

impl<'ad> PrettyPrinter<'ad> {
    fn var(&mut self, decl: &Declare) {
        eprintln!("Declare {} = {}", decl.identity.to_index(), self.db.repr_var(decl.identity));
        self.inner_expr(decl.value);
    }

    fn fun(&mut self, decl: &FunDeclare) {
        eprint!("FunDeclare {} = {}", decl.identity.to_index(), self.db.get_fun_name(decl.identity));
        self.start("");
        self.expr(decl.value);
        self.end();
    }

    fn class(&mut self, decl: &ClassDeclare) {
        eprint!("ClassDeclare {} = {}", decl.identity.to_index(), self.db.get(self.db.get(decl.identity).name));
        self.start("");
        for var in &decl.vars {
            self.var(var);
        }
        for fun in &decl.funs {
            self.fun(fun);
        }
        for class in &decl.classes {
            self.class(class);
        }
        self.end();
    }

    fn visit_expr(&mut self, expr: ExprId) {
        let binding = self.ast.get_expr(expr);
        match binding.as_ref() {
            crate::expr::Expr::AllocateClosure(allocate_closure) => {
                eprint!("AllocateClosure [[{}",
                    allocate_closure.id.to_index());
                if let Some(parent) = self.db.get(allocate_closure.id).parent {
                    eprint!(", parent = {}", parent.to_index());
                }
                eprintln!("]]");
                self.inner_expr(Some(allocate_closure.inner));
            },
            crate::expr::Expr::Binary(binary) => {
                self.start("Binary");
                self.expr(binary.left);
                self.expr(binary.right);
                self.end();
            },
            crate::expr::Expr::Unary(unary) => {
                self.start("Unary");
                self.expr(unary.inner);
                self.end();
            },
            crate::expr::Expr::Comparison(comparison) => {
                self.start("Comparison");
                self.expr(comparison.left);
                self.expr(comparison.right);
                self.end();
            },
            crate::expr::Expr::Variable(variable) => {
                eprintln!("Variable [{} = {}]", variable.identity.to_index(), self.db.repr_var(variable.identity));
            },
            crate::expr::Expr::Logical(logical) => {
                self.start("Logical");
                self.expr(logical.left);
                self.expr(logical.right);
                self.end();
            },
            crate::expr::Expr::FunCall(fun_call) => {
                eprint!("FunCall [{} = {}]", fun_call.identity.to_index(), self.db.get_fun_name(fun_call.identity));
                self.start("");
                self.expr_obj(fun_call.object);
                for arg in &fun_call.args {
                    self.expr(*arg);
                }
                self.end();
            },
            crate::expr::Expr::BuiltinCall(builtin_call) => {
                self.start("BuiltinCall");
                self.expr_obj(Some(builtin_call.object));
                for arg in &builtin_call.args {
                    self.expr(*arg);
                }
                self.end();
            },
            crate::expr::Expr::BuiltinCapture(builtin_capture) => {
                self.start("BuiltinCapture");
                self.expr_obj(Some(builtin_capture.object));
                self.end();
            },
            crate::expr::Expr::FunDeclare(fun_declare) => {
                self.fun(fun_declare);
            },
            crate::expr::Expr::ValCall(val_call) => {
                self.start("ValCall");
                self.expr_obj(Some(val_call.value));
                for arg in &val_call.args {
                    self.expr(*arg);
                }
                self.end();
            },
            crate::expr::Expr::FunCapture(fun_capture) => {
                eprint!("FunCapture [{} = {}]", fun_capture.identity.to_index(), self.db.get_fun_name(fun_capture.identity));
                self.start("");
                self.expr_obj(fun_capture.object);
                self.end();
            },
            crate::expr::Expr::Assign(assign) => {
                eprint!("Assign {} = {}", assign.identity.to_index(), self.db.repr_var(assign.identity));
                self.start("");
                self.expr(assign.value);
                self.end();
            },
            crate::expr::Expr::UnboundAssign(unbound_assign) => {
                eprint!("UnboundAssign ? = {}", self.db.get(unbound_assign.identifier.lexeme));
                self.start("");
                self.expr(unbound_assign.value);
                self.end();
            },
            crate::expr::Expr::NumLiteral(num_literal) => {
                eprintln!("NumLiteral {}", self.db.get(num_literal.contents.lexeme));
            }
            crate::expr::Expr::StrLiteral(str_literal) => {
                eprintln!("StrLiteral \"{}\"", self.db.get(str_literal.id));
            },
            crate::expr::Expr::BoolLiteral(bool_literal) => {
                eprintln!("BoolLiteral {}", bool_literal.value);
            },
            crate::expr::Expr::Block(block) => {
                self.start("Block");
                for stmt in &block.stmts {
                    self.stmt(*stmt);
                }
                self.end();
            },
            crate::expr::Expr::If(if_) => {
                self.start("If");
                self.expr_obj(Some(if_.condition));
                self.expr(if_.then_branch);
                if let Some(else_) = if_.else_branch { self.expr(else_); }
                self.end();
            },
            crate::expr::Expr::Unbound(unbound) => {
                eprintln!("Unbound ? = {}", self.db.get(unbound.identifier.lexeme));
            },
            crate::expr::Expr::UnboundFunCapture(unbound_fun_capture) => {
                eprint!("UnboundFunCapture ? = {}", self.db.get(unbound_fun_capture.identifier.lexeme));
                self.start("");
                self.expr_obj(unbound_fun_capture.object);
                self.end();
            },
            crate::expr::Expr::Print(print) => {
                self.start("Print");
                for arg in &print.exprs {
                    self.expr(*arg);
                }
                self.end();
            },
            crate::expr::Expr::Str(str_) => {
                self.start("Str");
                for arg in &str_.exprs {
                    self.expr(*arg);
                }
                self.end();
            },
            crate::expr::Expr::New(new_) => {
                self.start("New"); // TODO: Also show class names...?
                for arg in &new_.initializers {
                    self.expr_prefix(arg.value, self.db.get(arg.ident.lexeme));
                }
                self.end();
            },
            crate::expr::Expr::Get(get) => {
                eprint!("Get [");
                let mut comma = false;
                for ident in &get.chain {
                    if comma { eprint!(", "); }
                    eprint!("{}", self.db.get(ident.lexeme));
                    comma = true;
                }
                eprint!("]");
                self.start("");
                self.expr_obj(Some(get.lhs));
                self.end();
            },
            crate::expr::Expr::Set(set) => {
                eprint!("Set [");
                let mut comma = false;
                for ident in &set.chain {
                    if comma { eprint!(", "); }
                    eprint!("{}", self.db.get(ident.lexeme));
                    comma = true;
                }
                eprint!("]");
                self.start("");
                self.expr_obj(Some(set.lhs));
                self.expr(set.rhs);
                self.end();
            },
            crate::expr::Expr::SelfVal(self_val) => {
                eprintln!("SelfVal");
            },
            crate::expr::Expr::ArrayLit(array_lit) => {
                self.start("ArrayLit");
                for arg in &array_lit.values {
                    self.expr(*arg);
                }
                self.end();
            },
            crate::expr::Expr::Index(index) => todo!(),
            crate::expr::Expr::SetIndex(set_index) => todo!(),
            crate::expr::Expr::MakeTuple(make_tuple) => todo!(),
            crate::expr::Expr::MakeRange(make_range) => todo!(),
            crate::expr::Expr::Promote(promote) => {
                self.start("Promote");
                self.expr(promote.inner);
                self.end();
            },
            crate::expr::Expr::Lerp(lerp) => todo!(),
            crate::expr::Expr::MakeSumType(make_sum_type) => todo!(),
            crate::expr::Expr::OptionElse(option_else) => todo!(),
            crate::expr::Expr::Loop(_) => todo!(),
            crate::expr::Expr::Break(_) => todo!(),
            crate::expr::Expr::Continue(_) => todo!(),
            crate::expr::Expr::Return(ret) => {
                self.start("Return");
                self.inner_expr(ret.expression);
                self.end();
            },
            crate::expr::Expr::WhileLoop(while_loop) => todo!(),
            crate::expr::Expr::ForLoop(for_loop) => todo!(),
            crate::expr::Expr::Undefined(_undefined) => {
                eprintln!("Undefined");
            },
        }
    }
    
    fn visit_stmt(&mut self, stmt: StmtId) {
        let binding = self.ast.get_stmt(stmt);
        match binding.as_ref() {
            crate::expr::Stmt::Declare(declare) => self.var(declare),
            crate::expr::Stmt::Expression(expression) => {
                eprint!("<{}> ", expression.expression.to_index());
                // visit_expr so we don't re-indent-and-newline
                self.visit_expr(expression.expression);
            }
            crate::expr::Stmt::ClassDeclare(class_declare) => self.class(class_declare),
        }
    }

    fn inner_expr(&mut self, expr: Option<ExprId>) {
        self.indent += 1;
        let indent = Indenter { level: self.indent };
        eprint!("{} - ", indent);
        match expr {
            Some(expr) => {
                eprint!("{} = ", expr.to_index());
                self.visit_expr(expr);
            }
            None => {
                eprintln!("None");
            }
        }
        self.indent -= 1;
    }

    fn start(&mut self, name: &'static str) {
        eprintln!("{}", name);
        self.indent += 1;
    }
    fn end(&mut self) {
        self.indent -= 1;
    }
    fn expr(&mut self, expr: ExprId) {
        let indent = Indenter { level: self.indent };
        eprint!("{} - {} = ", indent, expr.to_index());
        self.visit_expr(expr);
    }
    fn expr_prefix(&mut self, expr: ExprId, prefix: &str) {
        let indent = Indenter { level: self.indent };
        eprint!("{} - {}: {} = ", indent, prefix, expr.to_index());
        self.visit_expr(expr);
    }

    fn stmt(&mut self, stmt: StmtId) {
        let indent = Indenter { level: self.indent };
        eprint!("{} | {} = ", indent, stmt.to_index());
        self.visit_stmt(stmt);
    }

    fn expr_obj(&mut self, expr: Option<ExprId>) {
        if let Some(expr) = expr {
            let indent = Indenter { level: self.indent };
            eprint!("{} @ {} = ", indent, expr.to_index());
            self.visit_expr(expr);
        }
    }
}

pub fn pretty_print(ast: &Ast, db: &Db) {
    let mut print = PrettyPrinter {
        db, ast, indent: 0
    };

    for source_id in ast.sources.iter() {
        let source = ast.sources.get(source_id);
        eprintln!("--- Source {} ({}) ---", source_id.to_index(), source.repr_path());

        // TODO: Consider keeping the order of all top-level items, and all
        // class-level items
        //
        // (Would also let us get rid of the sorting pass in semantic tokens, 
        // probably)
        for var in &source.module.globals {
            print.var(var);
        }
        for fun in &source.module.functions {
            print.fun(fun);
        }
        for class in &source.module.classes {
            print.class(class);
        } 
    }
}