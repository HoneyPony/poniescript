#![allow(unexpected_cfgs)]

use std::cell::RefCell;
use std::sync::RwLock;
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

use anstyle::{RgbColor, Style};
use anstyle::Color;
use anstyle::AnsiColor;

const STYLE_LINE_NUM: Style = Style::new()
	.fg_color(Some(Color::Ansi(AnsiColor::Magenta)));

// Might change this later. For now: Pony-styled magenta!
const STYLE_SQUIGGLE: Style = Style::new()
	.fg_color(Some(Color::Ansi(AnsiColor::Magenta)));

const STYLE_RESET: Style = Style::new();

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

fn gradient(a: (f32, f32, f32), b: (f32, f32, f32), f: f32) -> (f32, f32, f32) {
	(a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f, a.2 + (b.2 - a.2) * f)
}

fn style(c: (f32, f32, f32)) -> Style {
	Style::new()
		.fg_color(Some(Color::Rgb(RgbColor(c.0 as u8, c.1 as u8, c.2 as u8))))
}

fn gradient_style(a: (f32, f32, f32), b: (f32, f32, f32), f: f32) -> Style {
	style(gradient(a, b, f))
}

const ERR_COLOR_V0: (f32, f32, f32) = (255.0, 79.0, 144.0);
const ERR_COLOR_V1: (f32, f32, f32) = (255.0, 138.0, 220.0);
const ERR_COLOR_RIGHT: (f32, f32, f32) = (252.0, 38.0, 56.0);

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
				// TODO: Getting rid of the +1 here seems to help with
				// the CLI error reporting. Does that still work with the Language
				// Server?
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

	fn show_squiggle(&self, idx: u64, v: (f32, f32, f32)) {
		let f = (idx as f32 / 40.0).clamp(0.0, 1.0);
		let style = gradient_style(v, ERR_COLOR_RIGHT, f);
		
		eprint!("{style}─");
	}

	fn show_underlined_location(&self, location: &SourceLocation, provider: &dyn SourceProvider) {
		let mut start = self.get_line_column(location.offset);
		let end_offset = location.offset + location.length;
		let mut end = self.get_line_column(end_offset);
		
		let mut lines_rendered = 0;
		let mut line_style_tup = ERR_COLOR_V0;
		let mut line_style: Style = style(ERR_COLOR_V0);

		// Generate the "name" info
		// TODO: SourceProvider should show its path?
		let display_path = provider.repr_path();
		eprintln!("{line_style}    ─┬─ {line_style:#}{}:{}:{}:", display_path, start.0, start.1);
		eprintln!("{line_style}     │ {line_style:#}");

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

				eprint!("{line_style}{:>4} │ {line_style:#}", line_number);
				at_line_beginning = false;
				lines_rendered += 1;

				line_style_tup = gradient(ERR_COLOR_V0, ERR_COLOR_V1,
					(lines_rendered as f32 / 20.0).clamp(0.0, 1.0));
				line_style = style(line_style_tup);
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
					let mut squig = 0;
					eprintln!("");
					// Line up with the line numbers
					eprint!("{line_style}     ╰ {line_style:#}");

					// Sad code dupe :(
					// Maybe we need some sort of Renderer struct
					lines_rendered += 1;

					line_style_tup = gradient(ERR_COLOR_V0, ERR_COLOR_V1,
						(lines_rendered as f32 / 20.0).clamp(0.0, 1.0));
					line_style = style(line_style_tup);

					for i in line_start_offset..offset {
						if self.contents_chars[i] == '\r' { continue; }
						if i >= start_offset && i < end_offset {
							if self.contents_chars[i] == '\t' {
								// TODO: Only generate the STYLE when needed
								//eprint!("{STYLE_SQUIGGLE}────{STYLE_SQUIGGLE:#}");
								for _ in 0..4 {
									self.show_squiggle(squig, line_style_tup);
									squig += 1;
								}
							}
							else { //eprint!("{STYLE_SQUIGGLE}─{STYLE_SQUIGGLE:#}"); }
								self.show_squiggle(squig, line_style_tup);
								squig += 1;
							}
						}
						else {
							if self.contents_chars[i] == '\t' {
								eprint!("    ");
								squig += 4;
							}
							else { eprint!(" "); squig += 1; }
						}
					}
					eprint!("{STYLE_RESET}");
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
				// TODO:
				// Empirically, it seems like we need to subtract 1 from the
				// offset here, otherwise we get truncated file contents
				// (see e.g. tests/cyclic/err_cyclic.poni).
				// It's not really clear why that would be.
				//
				// Also, in practice, subtracting one causes a panic if the
				// file is too short?
				//
				// So I guess instead the line number offsets might be wrong
				// (see above).
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
	pub fn new(path: PathBuf) -> Box<dyn SourceProvider + Send + Sync> {
		Box::new(PathBufFileSource { path })
	}
}

impl SyntheticSource {
	pub fn new() -> Box<dyn SourceProvider + Send + Sync> {
		Box::new(SyntheticSource {})
	}
}

pub struct Source {
	provider: Box<dyn SourceProvider + Send + Sync>,
	source_map: RwLock<SourceMap>,

	// TODO: Consider moving everything out of Module into Source directly.
	pub module: Module,
}

impl Source {
	pub fn new(provider: Box<dyn SourceProvider + Send + Sync>) -> Source {
		Source { provider, source_map: RwLock::new(SourceMap::empty()), module: Module::new_empty() }
	}

	pub fn to_reader(&self) -> io::Result<Box<dyn Read>> {
		self.provider.to_reader()
	}

	pub fn repr_path(&self) -> String {
		self.provider.repr_path()
	}

	fn cache_map(provider: &dyn SourceProvider, source_map: &RwLock<SourceMap>) -> bool {
		{
			let source_map = source_map.read().unwrap();
			if source_map.created { return true; }
		}
		

		let reader = provider.to_reader();

		if let Ok(mut reader) = reader {
			let mut source_map = source_map.write().unwrap();
			return source_map.generate(&mut reader).is_ok();
		}

		false
	}

	pub fn show_underlined_location(&self, location: &SourceLocation) {
		Self::cache_map(self.provider.as_ref(), &self.source_map);

		let source_map = self.source_map.read().unwrap();
		source_map.show_underlined_location(location, self.provider.as_ref());
	}

	pub fn get_line_column(&self, location: &SourceLocation) -> (u64, u64) {
		Self::cache_map(self.provider.as_ref(), &self.source_map);

		let source_map = self.source_map.read().unwrap();
		source_map.get_line_column(location.offset)
	}

	pub fn get_offset(&self, line: u64, column: u64) -> u64 {
		Self::cache_map(self.provider.as_ref(), &self.source_map);

		let source_map = self.source_map.read().unwrap();
		source_map.get_offset(line, column)
	}
}