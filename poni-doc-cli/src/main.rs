use std::path::PathBuf;

use clap::Parser;

#[derive(clap::Parser)]
struct Args {
    output_path: PathBuf,
}

fn main() {
    let args = Args::parse();
    match poni_doc::generate_docs(&args.output_path) {
        Ok(_) => {
            eprintln!("successfully wrote docs.");
        }
        Err(err) => {
            eprintln!("error writing docs: {err}");
        }
    }
}
