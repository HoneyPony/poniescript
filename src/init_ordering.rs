use rustc_hash::FxHashMap;

use crate::{db::*, error::Error};
use crate::expr::*;

struct OrderVisitor<'a> {
    ord: &'a mut Vec<VarId>,
    map: FxHashMap<VarId, bool>,
}

impl<'a> OrderVisitor<'a> {
    fn push_dfs(&mut self, ast: &Ast, db: &mut Db, item: VarId) {
        // If the variable is in the map, then it's one of the ones in this InitOrdering.
        // Otherwise, it's an unrelated variable, so don't check it.
        let Some(entry) = self.map.get_mut(&item) else { return; };

        if *entry {
            // If the entry has already been visited, that means that we have a cycle
            // in the graph. Report an error.
            //
            // TODO: Store a location for each variable..?
            let location = db.get(item).location.clone();
            let mut error = Error::simple("Cycle in variable initialization order".into(), location.clone());

            for (k, v) in &self.map {
                if *k == item { continue; }
                if *v {
                    // Any items that are set to true in the map are part of
                    // the error. This is due to the map-removing logic below.
                    let location = db.get(*k).location.clone();
                    error = error.add_note("Additional variable in cycle".into(), Some(location.clone()));
                }
            }

            db.report_error(error);
            return;
        }
        self.map.insert(item, true);

        // Visit the expression initializing this variable in a DFS. 
        if let Some(var_initializer) = db.get(item).initializer {
            self.visit_expr(ast, db, var_initializer);
        }
        
        self.ord.push(item);
    }
}

impl<'a> VisitAst for OrderVisitor<'a> {
    fn visit_variable(&mut self, ast: &Ast, db: &mut Db, id:ExprId) {
        let borrow = ast.get_expr(id);
        let Expr::Variable(var) = borrow.as_ref() else { return; };
        self.push_dfs(ast, db, var.identity);
    }

    fn visit_funcall(&mut self,ast: &Ast,db: &mut Db,id:ExprId) {
        let borrow = ast.get_expr(id);
        let Expr::FunCall(call) = borrow.as_ref() else { return; };
        
        // Default behavior: Visit all args
        for arg in &call.args {
            self.visit_expr(ast, db, *arg);
        }

        // Additionally visit function body
        // TODO: Cache dependencies of a function body..?
        if let Some(expression) = db.get(call.identity).expression {
            self.visit_expr(ast, db, expression);
        }
    }

    fn visit_funcapture(&mut self,ast: &Ast, db: &mut Db, id:ExprId) {
        let borrow = ast.get_expr(id);
        let Expr::FunCapture(capt) = borrow.as_ref() else { return; };
        
        // Note: You could argue that this is too conservative. Like, you could
        // have something like:
        //
        // var a = {
        //     var b = a_fun;
        //     10;
        // };
        // fun a_fun() { print(a); }
        //
        // But I think that's OK.
        if let Some(expression) = db.get(capt.identity).expression {
            // Only visit functions that we have the source code to.
            //
            // Perhaps imported functions could be banned from being used in
            // variable initialization? On the other hand, imported functions
            // should simply not actually read from those variables...
            self.visit_expr(ast, db, expression);
        }
    }
}

pub fn topological_sort(ord: &mut Vec<VarId>, ast: &Ast, db: &mut Db) {
    let queue = std::mem::take(ord);

    let mut map: FxHashMap<VarId, bool> = FxHashMap::default();

    // Initialize the map to have a 'false' for each variable, so that we can
    // distinguish between variables that are unrelated and variables that have
    // already been visited.
    for item in &queue {
        map.insert(*item, false);
    }

    let mut visitor = OrderVisitor {
        ord, map
    };

    let mut last_pushed = 0;

    for item in &queue {
        visitor.push_dfs(ast, db, *item);

        // After visiting a variable, ALL the variables that were successfully
        // added to the final ordering should no longer count as real variables.
        //
        // This is because it is completely valid for any variables that come next
        // to reference them, and it is impossible for those "variables coming
        // next" to have been referenced earlier, as otherwise we would have seen
        // them already.
        //
        // So effectively, all the variables that were added to the ordering
        // by a particular graph traversal become "unrelated variables" to
        // any future graph traversals.

        for i in last_pushed..visitor.ord.len() {
            visitor.map.remove(&visitor.ord[i]);
        }
        last_pushed = visitor.ord.len();
    }
}