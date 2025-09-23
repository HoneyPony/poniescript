#![allow(unexpected_cfgs)]

use std::cell::RefCell;
use std::{fs::File, path::PathBuf};
use std::io::{self, Read};
use crate::db::*;
use crate::module::Module;

//pub enum SourceLocation {
	// TODO: Consider reducing these things to u32 to keep this smaller, as
	// it's stored per-token and probably per-ast-node.
//	Real { source: SourceId, offset: u64, length: u64 },
//	Synthesized,
//}

use anstream::eprintln;
use anstream::eprint;

use anstyle::Style;
use anstyle::Color;
use anstyle::AnsiColor;

const STYLE_LINE_NUM: Style = Style::new()
	.fg_color(Some(Color::Ansi(AnsiColor::Magenta)));

// Might change this later. For now: Pony-styled magenta!
const STYLE_SQUIGGLE: Style = Style::new()
	.fg_color(Some(Color::Ansi(AnsiColor::Magenta)));

#[derive(Clone)]
pub struct SourceLocation {
	pub source: SourceId,
	pub offset: u64,
	pub length: u64,
}

impl SourceLocation {
	/// Returns a new SourceLocation pointing just to the beginning of the current
	/// SourceLocation, useful for indicating the start of an expression.
	pub fn begin(&self) -> SourceLocation {
		return SourceLocation {
			source: self.source,
			offset: self.offset,
			length: 1
		};
	}

	/// Returns a new SourceLocation pointing just to the end of the current
	/// SourceLocation.
	pub fn end(&self) -> SourceLocation {
		return SourceLocation {
			source: self.source,
			offset: self.offset + self.length,
			length: 1
		}
	}
}

pub struct SourceMap {
	created: bool,
	contents_chars: Vec<char>,
	lines: Vec<u64>,
}

impl SourceMap {
	pub fn empty() -> SourceMap {
		return SourceMap {
			created: false,
			contents_chars: Vec::new(),
			lines: Vec::new(),
			
		}
	}

	pub fn generate(&mut self, reader: &mut dyn Read) -> std::io::Result<()> {
		self.lines.push(0);

		let mut buf = String::new();
		reader.read_to_string(&mut buf)?;

		let mut offset = 0;

		self.contents_chars = buf.chars().collect();

		for c in &self.contents_chars {
			offset += 1;
			if *c == '\n' {
				self.lines.push(offset + 1);
			}
		}

		self.created = true;

		Ok(())
	}

	pub fn get_line_column(&self, input: u64) -> (u64, u64) {
		// Can't really give meaningful info in this case...
		if self.lines.is_empty() { return (1, 0); }

		let mut line = self.lines.len() / 2;
		let mut low = 0;
		let mut high = self.lines.len();
		loop {
			// If the line is at the end, check the "current line" condition specially.
			if line == self.lines.len() - 1 {
				// If the input is on the current line, break.
				if self.lines[line] <= input {
					break;
				}
			}
			// If the input is "on the current line," we're done.
			else if self.lines[line] <= input && input < self.lines[line + 1] {
				break;
			}

			// Otherwise, binary search.
			if self.lines[line] > input {
				high = line;
			}
			else {
				low = line;
			}
			line = (low + high) / 2;
		}

		let column = input - self.lines[line];

		(line as u64 + 1, column as u64 + 1)
	}

	/// Weirdly, this is 0-based in all things. We should probably change
	/// get_line_column to also be 0-based.
	pub fn get_offset(&self, line: u64, column: u64) -> u64 {
		let line = if line >= self.lines.len() as u64 { (self.lines.len() - 1) as u64 } else { line };

		return self.lines[line as usize] + column;
	}

	fn show_underlined_location(&self, location: &SourceLocation, provider: &dyn SourceProvider) {
		let mut start = self.get_line_column(location.offset);
		let end_offset = location.offset + location.length;
		let mut end = self.get_line_column(end_offset);
		
		// Generate the "name" info
		// TODO: SourceProvider should show its path?
		let display_path = provider.repr_path();
		eprintln!("{STYLE_LINE_NUM}    --- {STYLE_LINE_NUM:#}{}:{}:{}:", display_path, start.0, start.1);
		eprintln!("{STYLE_LINE_NUM}     | {STYLE_LINE_NUM:#}");

		// Convert back to indices
		start.0 -= 1;
		end.0 -= 1;

		// Start at the beginning of the line
		let mut offset = self.lines[start.0 as usize] as usize;
		let mut line_start_offset = offset;

		// Keep track of offsets for underlining
		let start_offset = location.offset as usize;
		let end_offset = end_offset as usize;


		let mut line_number = start.0 + 1;
		let mut at_line_beginning: bool = true;
		let mut underline = false;

		

		loop {
			if at_line_beginning {
				line_start_offset = offset;
				eprint!("{STYLE_LINE_NUM}{:>4} | {STYLE_LINE_NUM:#}", line_number);
				at_line_beginning = false;
			}

			if offset >= start_offset && offset < end_offset {
				underline = true;
			}

			if offset >= self.contents_chars.len() || self.contents_chars[offset] == '\n' {
				at_line_beginning = true;
				line_number += 1;

				// Generate underline
				if underline {
					underline = false;
					eprintln!("");
					// Line up with the line numbers
					eprint!("{STYLE_LINE_NUM}     : {STYLE_LINE_NUM:#}");
					for i in line_start_offset..offset {
						if self.contents_chars[i] == '\r' { continue; }
						if i >= start_offset && i < end_offset {
							if self.contents_chars[i] == '\t' {
								// TODO: Only generate the STYLE when needed
								eprint!("{STYLE_SQUIGGLE}~~~~{STYLE_SQUIGGLE:#}");
							}
							else { eprint!("{STYLE_SQUIGGLE}~{STYLE_SQUIGGLE:#}"); }
						}
						else {
							if self.contents_chars[i] == '\t' {
								eprint!("    ");
							}
							else { eprint!(" "); }
						}
					}
				}

				// Read until we hit the end of the line after the end of the block.
				if offset >= self.contents_chars.len() || offset >= end_offset { break; }
			}

			if self.contents_chars[offset] == '\t' {
				eprint!("    ");
			}
			else if self.contents_chars[offset] == '\r' {
				/* do nothing  */
			}
			else {
				eprint!("{}", self.contents_chars[offset]);
			}
			offset += 1;
		}

		// Add one more line ending for the end of the string.
		eprintln!();
	}
}

pub trait SourceProvider {
	fn to_reader(&self) -> io::Result<Box<dyn Read>>;

	fn repr_path(&self) -> String;
}

pub struct PathBufFileSource { path: PathBuf }
pub struct SyntheticSource {}

impl SourceProvider for PathBufFileSource {
	fn to_reader(&self) -> io::Result<Box<dyn Read>> {
		let file = File::open(&self.path)?;
		Ok(Box::new(file))
	}

	fn repr_path(&self) -> String {
		self.path.to_string_lossy().to_string()
	}
}

impl SourceProvider for SyntheticSource {
	fn to_reader(&self) -> io::Result<Box<dyn Read>> {
		panic!("ICE: Trying to read from SyntheticSource")
	}

	fn repr_path(&self) -> String {
		"<synthetic>".to_string()
	}
}

impl PathBufFileSource {
	pub fn new(path: PathBuf) -> Box<dyn SourceProvider + Send> {
		Box::new(PathBufFileSource { path })
	}
}

impl SyntheticSource {
	pub fn new() -> Box<dyn SourceProvider + Send> {
		Box::new(SyntheticSource {})
	}
}

pub struct Source {
	provider: Box<dyn SourceProvider + Send>,
	source_map: RefCell<SourceMap>,

	// TODO: Consider moving everything out of Module into Source directly.
	pub module: Module,
}

impl Source {
	pub fn new(provider: Box<dyn SourceProvider + Send>) -> Source {
		Source { provider, source_map: RefCell::new(SourceMap::empty()), module: Module::new_empty() }
	}

	pub fn to_reader(&self) -> io::Result<Box<dyn Read>> {
		self.provider.to_reader()
	}

	fn cache_map(provider: &dyn SourceProvider, source_map: &RefCell<SourceMap>) -> bool {
		if source_map.borrow().created { return true; }

		let reader = provider.to_reader();

		if let Ok(mut reader) = reader {
			return source_map.borrow_mut().generate(&mut reader).is_ok();
		}

		false
	}

	pub fn show_underlined_location(&self, location: &SourceLocation) {
		Self::cache_map(self.provider.as_ref(), &self.source_map);

		self.source_map.borrow().show_underlined_location(location, self.provider.as_ref());
	}

	pub fn get_line_column(&self, location: &SourceLocation) -> (u64, u64) {
		Self::cache_map(self.provider.as_ref(), &self.source_map);

		self.source_map.borrow().get_line_column(location.offset)
	}

	pub fn get_offset(&self, line: u64, column: u64) -> u64 {
		Self::cache_map(self.provider.as_ref(), &self.source_map);

		self.source_map.borrow().get_offset(line, column)
	}
}