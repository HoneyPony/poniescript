use std::{fs::{self, File}, io::Write, path::{Path, PathBuf}, process::{Command, ExitStatus}};

use clap::{Parser, Subcommand};
use poni_build::{BuildConfig, ConfigReadError, ConfigWriteError, EnvironmentConfig, Project, read_configs, write_build_config};

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
    },

    /// Create a new project in the current directory's ponies.toml, or
    /// create a new ponies.toml if there is none. Generates a single file to
    /// begin with that includes the project's name.
    New {
        name: String,
    }
}

fn show_error_msg(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

macro_rules! exit_with_error {
    ($($arg:tt)*) => {
        let msg = format!($($arg)*);
		show_error_msg(&msg);
	}
}

fn try_run_ninja(project: Option<&String>, toolchain: &String) -> Result<ExitStatus, &'static str> {
    // Now, spawn the ninja process.
    // TODO: Configurable ninja path?
    let mut process = Command::new("ninja");
    process.arg("-f").arg(".build/build.ninja")
        .env("NINJA_STATUS", "%e /) ");

    // If we're only building one project, then pass that as an argument.
    if let Some(project) = project {
        // TODO: Make this less, like, stringly typed or whatever...
        process.arg(format!(".build/{toolchain}/{project}"));
    }
    
    // Spawn the process.
    let mut child = process.spawn()
        .map_err(|_| "couldn't spawn 'ninja' process")?;

    child.wait()
        .map_err(|_| "couldn't wait for 'ninja' process")
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

/// Returns the exit status of the ninja process.
fn do_build(build: &BuildConfig, env: &EnvironmentConfig, toolchain: &String, project: Option<&String>) -> ExitStatus {
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
        
        return try_run_ninja(project, toolchain).unwrap_or_else(|err| show_error_msg(err));
    }

    do_regenerate(build, env);

    return try_run_ninja(project, toolchain).unwrap_or_else(|err| show_error_msg(err));
}

fn show_error(err: ConfigReadError) -> ! {
    match err {
        ConfigReadError::NoPoniesToml => eprintln!("error: couldn't open 'ponies.toml'"),
        ConfigReadError::BadPoniesToml(err) => eprintln!("error: couldn't parse 'ponies.toml':\n{err}"),

        // TODO: These errors should include their path with them, if possible.
        ConfigReadError::NoEnvironmentToml(tried_path) => eprintln!("error: couldn't open {}", tried_path.display()),
        ConfigReadError::BadEnvironmentToml(err) => eprintln!("error: couldn't parse build-config.toml:\n{err}"),
        ConfigReadError::XdgError(err) => eprintln!("error: couldn't read XDG environment: {err}"),
        ConfigReadError::FsError => eprintln!("error: problem reading filesystem."),
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

/// Handles any task that requires both the BuildConfig and EnvironmentConfig
/// and fails if they don't exist.
fn handle_build_cmd(cmd: CliCommand) {
    // Right now, all the commands require all the configuration files.
    let (build, env) = read_files();

    // Now do stuff, depending on what the command was.
    match cmd {
        CliCommand::Regenerate => {
            do_regenerate(&build, &env);
        }
        CliCommand::Build { project } => {
            let Some(toolchain) = env.lookup_default_toolchain() else {
                show_error_msg("no default toolchain found");
            };

            do_build(&build, &env, &toolchain, project.as_ref());
        },
        CliCommand::Run { project } => {
            if !build.projects.contains_key(&project) {
                eprintln!("error: no such project '{}'", project);
                std::process::exit(1);
            }

            let Some(toolchain) = env.lookup_default_toolchain() else {
                show_error_msg("no default toolchain found");
            };

            let status = do_build(&build, &env, &toolchain, Some(&project));
            // Only execute the child process if it successfully built.
            if status.success() {
                // Now run that specific project.
                // TODO: Support arguments to the project?
                let mut child = Command::new(format!(".build/{toolchain}/{project}"))
                    .spawn()
                    .unwrap_or_else(|_| show_error_msg("couldn't spawn child process."));

                child.wait()
                    .unwrap_or_else(|_| show_error_msg("couldn't wait for child process."));
            }
        },
        _ => unreachable!()
    }
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        CliCommand::Regenerate | CliCommand::Build { .. } | CliCommand::Run { .. } => {
            handle_build_cmd(cli.command);
        },
        CliCommand::New { name } => {
            let the_path = Path::new("./");

            // Depending on the error, it might not be a real error.
            let cfg = poni_build::read_build_config_precise(the_path);
            let mut cfg = match cfg {
                Ok(cfg) => cfg,
                // If there is not a ponies.toml, we can start from scratch.
                Err(ConfigReadError::NoPoniesToml) => {
                    BuildConfig::empty()
                },
                Err(err) => show_error(err),
            };

            // Now, add the new project to the config.
            if cfg.projects.contains_key(name) {
                exit_with_error!("project '{name}' already exists");
            }

            // Create a new project that contains a file based on the project
            // name.
            let project = Project {
                files: vec![PathBuf::from(format!("{name}.poni"))],
                imports: vec![],
            };

            cfg.projects.insert(name.clone(), project);
            let res = write_build_config(the_path, &cfg);
            match res {
                Ok(_) => {
                    eprintln!("success: created project '{}'", name);
                },
                Err(ConfigWriteError::CantSerialize(s) | ConfigWriteError::Io(s)) => {
                    eprintln!("error: couldn't write ponies.toml: {}", s);
                    std::process::exit(1);
                }
            }

            // Now, if the {name}.poni file doesn't exist, create it with some
            // default contents.
            let poni_path = format!("{name}.poni");
            if let Ok(mut f) = File::create_new(&poni_path) {
                match f.write_all(include_bytes!("template.poni")) {
                    Ok(_) => {
                        eprintln!("success: wrote template file to {}", poni_path);
                    },
                    Err(e) => {
                        eprintln!("error: couldn't write {}: {}", poni_path, e);
                    },
                }
            }
        }
    }
}
