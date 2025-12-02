use std::{fs::{self, File}, path::Path, process::Command};

use clap::{Parser, Subcommand};
use poni_build::{BuildConfig, ConfigReadError, EnvironmentConfig, read_configs};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand
}

#[derive(Subcommand)]
enum CliCommand {
    /// Regenerate the ninja file for the projects. Mostly invoked from the
    /// ninja file to keep itself up to date.
    Regenerate,
    /// Build all projects, or build a single project if a name is passed.
    Build {
        project: Option<String>,
    },
    /// Build and run the project with the given name.
    Run {
        project: String,
    }
}

fn show_error_msg(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn try_run_ninja(project: Option<&String>) -> Result<(), &'static str> {
    // Now, spawn the ninja process.
    // TODO: Configurable ninja path?
    let mut process = Command::new("ninja");
    process.arg("-f").arg(".build/build.ninja")
        .env("NINJA_STATUS", "%e /) ");

    // If we're only building one project, then pass that as an argument.
    if let Some(project) = project {
        // TODO: Make this less, like, stringly typed or whatever...
        process.arg(format!(".build/{project}-debug"));
    }
    
    // Spawn the process.
    let mut child = process.spawn()
        .map_err(|_| "couldn't spawn 'ninja' process")?;

    child.wait()
        .map_err(|_| "couldn't wait for 'ninja' process")?;

    Ok(())
}

fn do_regenerate(build: &BuildConfig, env: &EnvironmentConfig) {
    // First, create the .build folder and the build.ninja file.
    fs::create_dir_all(".build")
        .unwrap_or_else(|_| show_error_msg("couldn't create .build directory"));

    // TODO: Don't rebuild .ninja file when nothing has changed... :/
    let mut file = File::create(".build/build.ninja")
        .unwrap_or_else(|_| show_error_msg("couldn't create .build/build.ninja file"));

    build.generate_ninja_file(&mut file, env)
        .unwrap_or_else(|_| show_error_msg("couldn't write .build/build.ninja file"));
}

fn do_build(build: &BuildConfig, env: &EnvironmentConfig, project: Option<&String>) {
    // Check the project before doing anything else.
    if let Some(project) = project {
        if !build.projects.contains_key(project) {
            eprintln!("error: no such project '{}'", project);
            std::process::exit(1);
        }
    }

    // For speed, what we would like to do is attempt running ninja *first*,
    // and do the creation process if that fails. However, this doesn't quite
    // work because ninja will print errors and so forth.
    //
    // So instead, check if the build.ninja file exists, and if so, *then*
    // run ninja without thinking about it.
    if fs::exists(".build/build.ninja")
        .unwrap_or_else(|_| show_error_msg("couldn't check if .build/build.ninja exists")) {
        
        try_run_ninja(project).unwrap_or_else(|err| show_error_msg(err));
        return;
    }

    do_regenerate(build, env);

    try_run_ninja(project).unwrap_or_else(|err| show_error_msg(err));
}

fn show_error(err: ConfigReadError) -> ! {
    match err {
        ConfigReadError::NoPoniesToml => eprintln!("error: couldn't open 'ponies.toml'"),
        ConfigReadError::BadPoniesToml(err) => eprintln!("error: couldn't parse 'ponies.toml':\n{err}"),

        // TODO: These errors should include their path with them, if possible.
        ConfigReadError::NoEnvironmentToml(tried_path) => eprintln!("error: couldn't open {}", tried_path.display()),
        ConfigReadError::BadEnvironmentToml(err) => eprintln!("error: couldn't parse build-config.toml:\n{err}"),
        ConfigReadError::XdgError(err) => eprintln!("error: couldn't read XDG environment: {err}"),
    }

    std::process::exit(1);
}

fn read_files() -> (BuildConfig, EnvironmentConfig) {
    // Read the build config from the local directory
    let result = read_configs(Path::new("./"));

    match result {
        Ok(configs) => configs,
        Err(err) => show_error(err)
    }
}

fn main() {
    let cli = Cli::parse();

    // Right now, all the commands require all the configuration files.
    let (build, env) = read_files();

    // Now do stuff, depending on what the command was.
    match cli.command {
        CliCommand::Regenerate => {
            do_regenerate(&build, &env);
        }
        CliCommand::Build { project } => {
            do_build(&build, &env, project.as_ref());
        },
        CliCommand::Run { project } => {
            if !build.projects.contains_key(&project) {
                eprintln!("error: no such project '{}'", project);
                std::process::exit(1);
            }
            do_build(&build, &env, Some(&project));

            // Now run that specific project.
            // TODO: Support arguments to the project?
            let mut child = Command::new(format!(".build/{project}-debug"))
                .spawn()
                .unwrap_or_else(|_| show_error_msg("couldn't spawn child process."));

            child.wait()
                .unwrap_or_else(|_| show_error_msg("couldn't wait for child process."));
        },
    }
}
