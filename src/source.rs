use std::{fs::File, path::PathBuf};
use std::io;
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
	lines: Vec<usize>,
}

impl SourceMap {
	pub fn find_corresponding_line(idx: usize) {
		
	}
}

pub struct Source {
	name: String,
	path: PathBuf,
}

impl Source {
	pub fn new(path: PathBuf) -> Source {
		let name = path.file_name()
			// TODO: Consider using to_string_lossy..?
			.map(|name| name.to_str())
			.flatten()
			.unwrap_or("<unknown>")
			.to_string();
		return Source { name, path }
	}

	pub fn to_file(&self) -> io::Result<File> {
		File::open(&self.path)
	}
}