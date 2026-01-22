use std::path::PathBuf;

use clap::Parser;

#[derive(clap::Parser)]
struct Args {
    /// For now, we take in a vector of input files, and delegate the actual
    /// collection of these input paths to the build system.
    input_files: Vec<PathBuf>,

    #[arg(short, long)]
    /// Imported .h files.
    import: Vec<PathBuf>,

    #[arg(short, long)]
    output_path: PathBuf,
}

fn main() {
    let args = Args::parse();
    match poni_doc::generate_docs(&args.input_files, &args.import, &args.output_path) {
        Ok(_) => {
            eprintln!("successfully wrote docs.");
        }
        Err(err) => {
            eprintln!("error writing docs: {err}");
        }
    }
}
