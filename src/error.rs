use crate::source::SourceLocation;
use crate::db::*;

use anstream::eprintln;
use anstream::eprint;

use anstyle::Style;
use anstyle::Color;
use anstyle::AnsiColor;

const STYLE_ERROR: Style = Style::new()
	.fg_color(Some(Color::Ansi(AnsiColor::Red)));
const STYLE_WARNING: Style = Style::new()
	.fg_color(Some(Color::Ansi(AnsiColor::Yellow)));

pub struct Error {
	main_message: String,
	main_location: SourceLocation,

	is_warning: bool,
}

impl Error {
	pub fn simple(message: String, location: &SourceLocation) -> Error {
		Error {
			main_message: message,
			main_location: location.clone(),

			is_warning: false,
		}
	}
}

pub fn show_error(error: &Error, db: &Db) {
	let source = db.get(error.main_location.source);

	match error.is_warning {
		true => eprint!("{STYLE_WARNING}warning: {STYLE_WARNING:#}"),
		false => eprint!("{STYLE_ERROR}error: {STYLE_ERROR:#}")
	}

	eprintln!("{}", error.main_message);

	source.show_underlined_location(&error.main_location);
}