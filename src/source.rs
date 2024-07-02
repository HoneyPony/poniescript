use std::cell::{Ref, RefCell};
use std::{fs::File, path::PathBuf};
use std::io::{self, Read};
use crate::db::*;

//pub enum SourceLocation {
	// TODO: Consider reducing these things to u32 to keep this smaller, as
	// it's stored per-token and probably per-ast-node.
//	Real { source: SourceId, offset: u64, length: u64 },
//	Synthesized,
//}

#[derive(Clone)]
pub struct SourceLocation {
	pub source: SourceId,
	pub offset: u64,
	pub length: u64,
}

struct SourceMap {
	created: bool,
	lines: Vec<u64>,
}

impl SourceMap {
	pub fn empty() -> SourceMap {
		return SourceMap {
			created: false,
			lines: Vec::new(),
		}
	}

	pub fn generate(&mut self, mut file: File) -> std::io::Result<()> {
		self.lines.push(0);

		let mut buf = String::new();
		file.read_to_string(&mut buf)?;

		let mut offset = 0;

		for c in buf.chars() {
			offset += 1;
			if c == '\n' {
				self.lines.push(offset);
			}
		}

		Ok(())
	}

	pub fn get_line_column(&self, input: u64) -> (u64, u64) {
		// Can't really give meaningful info in this case...
		if self.lines.is_empty() { return (0, 0); }

		let mut line = self.lines.len() / 2;
		let mut low = 0;
		let mut high = self.lines.len();
		loop {
			// If the line is at the end, we can't be past it or before it.
			if line == self.lines.len() - 1 {
				break;
			}

			// If the input is "on the current line," we're done.
			if self.lines[line] <= input && input < self.lines[line + 1] {
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

		(line as u64 + 1, column as u64)
	}
}

pub enum Source {
	Real {
		name: String,
		path: PathBuf,
		source_map: RefCell<SourceMap>,
	},
	Synthetic,
}

impl Source {
	pub fn new(path: PathBuf) -> Source {
		let name = path.file_name()
			// TODO: Consider using to_string_lossy..?
			.map(|name| name.to_str())
			.flatten()
			.unwrap_or("<unknown>")
			.to_string();
		return Source::Real { name, path, source_map: RefCell::new(SourceMap::empty()) }
	}

	pub fn to_file(&self) -> io::Result<File> {
		let Source::Real { path, .. } = self else {
			panic!("trying to open synthetic source");
		};
		File::open(path)
	}

	fn cache_map(path: &PathBuf, source_map: &RefCell<SourceMap>) -> bool {
		if source_map.borrow().created { return true; }

		let file = File::open(path);

		if let Ok(file) = file {
			return source_map.borrow_mut().generate(file).is_ok();
		}

		false
	}

	pub fn get_line_column(&self, location: u64) -> Option<(u64, u64)> {
		match self {
			Source::Real { name, path, source_map } => {
				if Self::cache_map(path, source_map) {
					Some(source_map.borrow().get_line_column(location))
				}
				else { None }
			},
			Source::Synthetic => None,
		}
	}

	fn name(&self) -> &str {
		match self {
			Source::Real { name, path, source_map } => &name,
			Source::Synthetic => "<unknown>",
		}
	}

	pub fn show_brief_at(&self, location: &SourceLocation) {
		match self.get_line_column(location.offset) {
			Some((line, column)) => {
				eprintln!("in {}:{}:{}:", self.name(), line, column);
			},
			None => {
				eprintln!("in <unknown>:");
			}
		}
		
	}
}