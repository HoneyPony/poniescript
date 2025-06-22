use rustc_hash::FxHashMap;

use crate::{db::*, error::Error, source::SourceLocation};
use crate::expr::*;

trait VisitExpr {
    fn visit_binary(ast: &Ast, db: &mut Db, id: ExprId, item: &Binary) {
        
    }

    fn visit(ast: &Ast, db: &mut Db, item: ExprId) {
        match ast.get_expr(item) {
            Expr::Binary(binary) => {
                
            },
            Expr::Comparison(comparison) => todo!(),
            Expr::Variable(variable) => todo!(),
            Expr::Logical(logical) => todo!(),
            Expr::FunCall(fun_call) => todo!(),
            Expr::FunDeclare(fun_declare) => todo!(),
            Expr::ValCall(val_call) => todo!(),
            Expr::FunCapture(fun_capture) => todo!(),
            Expr::Assign(assign) => todo!(),
            Expr::UnboundAssign(unbound_assign) => todo!(),
            Expr::NumLiteral(num_literal) => todo!(),
            Expr::StrLiteral(str_literal) => todo!(),
            Expr::BoolLiteral(bool_literal) => todo!(),
            Expr::Block(block) => todo!(),
            Expr::If(_) => todo!(),
            Expr::Unbound(unbound) => todo!(),
            Expr::UnboundFunCapture(unbound_fun_capture) => todo!(),
            Expr::Print(print) => todo!(),
            Expr::Str(_) => todo!(),
            Expr::New(_) => todo!(),
            Expr::Get(get) => todo!(),
            Expr::Set(set) => todo!(),
            Expr::SelfVal(self_val) => todo!(),
            Expr::ArrayLit(array_lit) => todo!(),
            Expr::Index(index) => todo!(),
            Expr::SetIndex(set_index) => todo!(),
            Expr::Undefined(undefined) => todo!(),
        }  
    }
}

struct InitOrdering {
    vars: Vec<VarId>,
}

fn push_dfs(ord: &mut InitOrdering, ast: &Ast, db: &mut Db, map: &mut FxHashMap<VarId, bool>, item: VarId) {
    // If the variable is in the map, then it's one of the ones in this InitOrdering.
    // Otherwise, it's an unrelated variable, so don't check it.
    let Some(entry) = map.get_mut(&item) else { return; };

    if *entry {
        // If the entry has already been visited, that means that we have a cycle
        // in the graph. Report an error.
        //
        // TODO: Store a location for each variable..?
        // let location = 
        // db.report_error(Error::simple("Cycle in variable initilization order".into(), );
        panic!("Cycle in variable initilization order");
    }
    map.insert(item, true);

    /* visit dfs */

    ord.vars.push(item);
}

fn topological_sort(ord: &mut InitOrdering, ast: &Ast, db: &mut Db) {
    let mut queue = std::mem::take(&mut ord.vars);

    let mut map: FxHashMap<VarId, bool> = FxHashMap::default();

    // Initialize the map to have a 'false' for each variable, so that we can
    // distinguish between variables that are unrelated and variables that have
    // already been visited.
    for item in &queue {
        map.insert(*item, false);
    }

    for item in &queue {
        push_dfs(ord, ast, db, &mut map, *item);
    }
}