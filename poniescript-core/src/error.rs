// anstream's eprintln & eprint macros apparently have a cfg called 'test'. This
// gives us a warning, which isn't very helpful.
#![allow(unexpected_cfgs)]

use crate::arena::IndexCell;
use crate::source::SourceLocation;
use crate::db::*;

use anstream::eprintln;
use anstream::eprint;

use anstyle::Style;
use anstyle::Color;
use anstyle::AnsiColor;

const STYLE_ERROR: Style = Style::new()
	.fg_color(Some(Color::Ansi(AnsiColor::Red)))
	.bold();
const STYLE_WARNING: Style = Style::new()
	.fg_color(Some(Color::Ansi(AnsiColor::Yellow)))
	.bold();
const STYLE_NOTE: Style = Style::new()
	.bold();

struct Note {
	note: String,
	location: Option<SourceLocation>,
}

pub struct Error {
	pub main_message: String,
	pub main_location: SourceLocation,

	pub is_warning: bool,

	notes: Vec<Note>
}

impl Error {
	pub fn simple(message: String, location: SourceLocation) -> Error {
		Error {
			main_message: message,
			main_location: location,

			is_warning: false,

			notes: vec![],
		}
	}

	pub fn add_note(mut self, note: String, location: Option<SourceLocation>) -> Error {
		self.notes.push(Note { note, location });

		self
	}
}

pub fn show_error(error: &Error, ast: &Ast) {
	let source = ast.sources.get(error.main_location.source);

	match error.is_warning {
		true => eprint!("{STYLE_WARNING}warning: {STYLE_WARNING:#}"),
		false => eprint!("{STYLE_ERROR}error: {STYLE_ERROR:#}")
	}

	eprintln!("{}", error.main_message);

	source.show_underlined_location(&error.main_location);

	for note in &error.notes {
		eprintln!("{STYLE_NOTE}note: {STYLE_NOTE:#}{}", note.note);
		if let Some(location) = &note.location {
			ast.sources.get(location.source)
				.show_underlined_location(&location);
		}
	}
}