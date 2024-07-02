use crate::expr::*;
use crate::module::Module;
use crate::db::*;


struct NumConc<'a> {
	db: &'a mut Db,
	return_type: TypId,
}

impl<'a> NumConc<'a> {

	fn compute(&self, known: Option<TypId>, current: TypId) -> TypId {
		if current == self.db.types.assume_int {
			// If it's assume int, it can be turned into either int or float.
			if let Some(ty) = known {
				if ty == self.db.types.int { return self.db.types.int }
				if ty == self.db.types.float { return self.db.types.float }
			}

			// Default to int if we have no info.
			return self.db.types.int
		}
		else if current == self.db.types.assume_float {
			// If it's assume float, then it is always turned in to float for
			// now. Later it may also be a fixed point, perhaps.

			// Default to float if we have no info. (and always, for now)
			return self.db.types.float
		}

		// The idea is that, after passing through this function, there should
		// be NO instances of AssumeInt or AssumeFloat remaining.

		// TODO: We may have to recursively type-check e.g. Lists?
		// Although, it would be best if we never generate a List with
		// an AssumeInt, etc...

		current
	}

	fn visit_declare(&mut self, declare: &mut Declare) {

	}

	fn visit_expr(&mut self, expr: &mut Expr, known_type: Option<TypId>) {
		match expr {
			Expr::Binary(binary) => {
				
			},
			Expr::Variable(_) => { /* nothing needed */ },
			Expr::Assign(_) => todo!(),
			Expr::NumLiteral(_) => todo!(),
			Expr::StrLiteral(_) => todo!(),
			Expr::Block(_) => todo!(),
			Expr::Unbound(_) => todo!(),
			Expr::Print(_) => todo!(),
		}
	}

	fn visit_stmt(&mut self, stmt: &mut Stmt) {
		match stmt {
			Stmt::Declare(declare) => self.visit_declare(declare),
			Stmt::Expression(expression) => self.visit_expr(&mut expression.expression, None),
			Stmt::FunDeclare(function) => self.visit_function(function),
			Stmt::Return(ret) => todo!(),
		}
	}

	fn visit_function(&mut self, function: &mut FunDeclare) {

	}

	fn visit_module(&mut self, module: &mut Module) {
		for var in &mut module.globals {
			self.visit_declare(var);
		}

		for fun in &mut module.functions {
			self.visit_function(fun);
		}
	}

}

pub fn make_concrete(db: &mut Db, modules: &mut Vec<Module>) {
	let mut pass = NumConc {
		return_type: db.types.void, db
	};

	for module in modules {
		pass.visit_module(module);
	}
}