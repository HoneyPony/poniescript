use crate::source::SourceLocation;
use crate::db::*;

pub struct Error {
	main_message: String,
	main_location: SourceLocation,
}

impl Error {
	pub fn simple(message: String, location: &SourceLocation) -> Error {
		Error {
			main_message: message,
			main_location: location.clone()
		}
	}
}

pub fn show_error(error: &Error, db: &Db) {
	let source = db.get(error.main_location.source);
	source.show_brief_at(&error.main_location);
	eprintln!("{}", error.main_message);
}