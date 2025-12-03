use std::{collections::HashMap, fs::{self, File}, io::Write, path::{Path, PathBuf}};

use microxdg::{XdgApp, XdgError};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Toolchain {
    /// The command to use for compiling C files.
    pub cc: String,

    /// The command to use for linking. This is often an invocation of a C
    /// compiler.
    pub linker: String,

    /// Whether the PonieScript compiler directly invokes the C compiler through
    /// a pipe. This can reduce latency of compilation.
    pub piped: bool,

    /// The number of object files that the compilation is split into.
    pub ways: usize,
}

#[derive(Serialize, Deserialize)]
pub struct ToolchainSet(HashMap<String, Toolchain>);

#[derive(Serialize, Deserialize)]
pub struct EnvironmentConfig {
    pub poni_h_path: PathBuf,
    pub poni_gc_path: PathBuf,
    pub poniescript_path: PathBuf,

    pub toolchain: HashMap<String, ToolchainSet>,

    pub default_profile: String,
    pub default_target: String,
}

impl EnvironmentConfig {
    /// Returns the toolchain name, and a boolean of whether it is actually defined.
    pub fn lookup_toolchain(&self, profile: Option<&String>, target: Option<&String>) -> (String, bool) {
        let profile = profile.unwrap_or(&self.default_profile);
        let target = target.unwrap_or(&self.default_target);

        let name = format!("{target}-{profile}");

        let Some(set) = self.toolchain.get(target) else {
            return (name, false);
        };

        (name, set.0.contains_key(profile))
    }
}

/// The top-level configuration for the build, described by a .toml file,
/// at least for now.
#[derive(Serialize, Deserialize)]
pub struct BuildConfig {
    pub projects: HashMap<String, Project>,
}

#[derive(Serialize, Deserialize)]
pub struct Project {
    /// The .poni files that make up this project.
    pub files: Vec<PathBuf>,

    /// .h files that are imported for this project.
    pub imports: Vec<PathBuf>,
}

pub struct GeneratedNinjaInfo {
    default_toolchain: Option<String>,
}

pub const MAGENTA: &'static str = "\x1b[0;35m";
pub const GREEN  : &'static str = "\x1b[0;32m";
pub const BLUE   : &'static str = "\x1b[0;34m";
pub const DIM    : &'static str = "\x1b[2m";
pub const RESET  : &'static str = "\x1b[0m";

impl BuildConfig {
    pub fn empty() -> Self {
        BuildConfig { projects: HashMap::new() }
    }

    fn generate_toolchain(&self, ninja: &mut File, name: &String, profile: &String, toolchain: &Toolchain, info: &mut GeneratedNinjaInfo) -> std::io::Result<()> {
        // TODO: What we will end up wanting is a bunch of different cc/linkers,
        // used for debug/release, and used for different target platforms.
        // (e.g. ideally you should be able to, from Linux, easily compile for
        // Linux, Windows, Web, and Android)
        //
        // This profile setup mostly accomplishes this, but we will need
        // a different compile of poniescript_gc and any other supporting libraries,
        // on each targeted platform.
        writeln!(ninja, "rule link-{name}-{profile}")?;
        writeln!(ninja, "  command = {} $in -o $out -L$poni_gc_path -lponiescript_gc",
            toolchain.linker)?;
        writeln!(ninja, "  description = {BLUE}link{RESET}{DIM}.{name}.{profile}{RESET} -> $outdesc")?;

        writeln!(ninja, "rule cc-{name}-{profile}")?;
        writeln!(ninja, "  command = {} -c $in -o $out -MD -MF $out.d -I$poni_h_path -I.", toolchain.cc)?;
        writeln!(ninja, "  depfile = $out.d")?;
        writeln!(ninja, "  deps = gcc")?;
        writeln!(ninja, "  description = {GREEN}cc  {RESET}{DIM}.{name}.{profile}{RESET} $indesc")?;

        writeln!(ninja, "rule poni-{name}-{profile}")?;
        if toolchain.piped {
            // For the piped toolchain, we have to tell PonieScript what compiler
            // to use, and also pass a few command line arguments.
            writeln!(ninja, "  command = $poniescript $outputargs $in $imports --no-timing -c {} -C-I$poni_h_path -C-I.",
                toolchain.cc)?;
        }
        else {
            writeln!(ninja, "  command = $poniescript $outputargs $in $imports --no-timing")?;
        }
        writeln!(ninja, "  description = {MAGENTA}poni{RESET}{DIM}.{name}.{profile}{RESET} $indesc")?;

        // Now, we generate the rules for building each project with this toolchain.
        let dir = format!(".build/{name}-{profile}");
        
        for (project_name, project) in &self.projects {
            // Create a vector of object & C file names based on the number of 'ways'.
            let mut object_files = Vec::new();
            let mut c_files = Vec::new();
            for i in 0..toolchain.ways {
                object_files.push(format!("{project_name}-{i}.o"));
                c_files.push(format!("{project_name}-{i}.c"));
            }

            // Link rule: Based on object files. The executable is just called
            // based on the project name.
            write!(ninja, "build {dir}/{project_name}: link-{name}-{profile}")?;
            for obj in &object_files {
                write!(ninja, " {dir}/{obj}")?;
            }
            write!(ninja, "\n")?;
            writeln!(ninja, "  outdesc = {project_name}")?;

            let c_file_deps = "$poni_h_path/poni/poni.h $poni_h_path/poni/poni_standalone.h $poni_h_path/poni/poni_gc.h";

            // The object file rule depends on whether or not the output is piped.
            // If the output is not piped, we invoke the C compiler separately.
            //
            // In that case, generate one rule for each object file.
            if !toolchain.piped {
                for (c, obj) in c_files.iter().zip(object_files.iter()) {
                    writeln!(ninja, "build {dir}/{obj}: cc-{name}-{profile} {dir}/{c} | {c_file_deps}")?;
                    writeln!(ninja, "  indesc = {c}")?;
                }
            }

            // Now generate the PonieScript rule. If the output is piped, then
            // the PonieScript command generates all of the .o files; otherwise,
            // it generates all of the .c files.
            let outputs = if toolchain.piped { &object_files } else { &c_files };
            write!(ninja, "build")?;
            for output in outputs {
                write!(ninja, " {dir}/{output}")?;
            }
            write!(ninja, ": poni-{name}-{profile}")?;
            
            // TODO: Escape spaces in paths

            // Pass all the PonieScript files to the PonieScript compiler.
            for poni in &project.files {
                write!(ninja, " {}", poni.display())?;
            }

            if toolchain.piped {
                write!(ninja, " | {c_file_deps}")?;
            }

            // Create the imports variable.
            write!(ninja, "\n  imports =")?;
            for import in &project.imports {
                write!(ninja, " -i {}", import.display())?;
            }

            // Create the outputargs variable. This is similar to outputs,
            // except with -o in front of each one.
            write!(ninja, "\n  outputargs =")?;
            for output in outputs {
                write!(ninja, " -o {dir}/{output}")?;
            }
            write!(ninja, "\n  indesc = {project_name}")?;
            write!(ninja, "\n\n")?;
        }

        Ok(())
    }

    fn generate_toolchain_set(&self, ninja: &mut File, name: &String, set: &ToolchainSet, info: &mut GeneratedNinjaInfo) -> std::io::Result<()> {
        // Generate the following for each toolchain:
        // - Rules 'link', 'cc', and 'poni'
        // - The build rules for that toolchain

        for (profile, toolchain) in &set.0 {
            self.generate_toolchain(ninja, name, profile, toolchain, info)?;
        }

        Ok(())
    }

    // Returns the default toolchain name, if any.
    pub fn generate_ninja_file(&self, file: &mut File, env: &EnvironmentConfig) -> std::io::Result<GeneratedNinjaInfo> {
        let mut result = GeneratedNinjaInfo {
            default_toolchain: None,
        };

        writeln!(file, "builddir = .build\n")?;

        // TODO: Make all the Paths Strings instead?
        writeln!(file, "poni_h_path = {}", env.poni_h_path.display())?;
        writeln!(file, "poni_gc_path = {}", env.poni_gc_path.display())?;
        writeln!(file, "poniescript = {}\n", env.poniescript_path.display())?;

        writeln!(file, "rule poni-regenerate\n  command = ponies regenerate\n  generator = true\n  description = ponies regenerate\n")?;

        // Project regeneration is based on the ponies.toml file
        //
        // TODO: Make this also depend on the EnvironmentConfig's path
        writeln!(file, "build .build/build.ninja: poni-regenerate ponies.toml")?;
        
        for (name, set) in &env.toolchain {
            self.generate_toolchain_set(file, name, set, &mut result)?;
        }

        // Write a default rule if we have a default toolchain
        if let Some(default) = &result.default_toolchain {
            write!(file, "default")?;
            for (name, _) in &self.projects {
                write!(file, " .build/{default}/{name}")?;
            }
            write!(file, "\n")?;
        }

        Ok(result)
    }
}

pub enum ConfigReadError {
    NoPoniesToml,
    BadPoniesToml(String),
    NoEnvironmentToml(PathBuf),
    BadEnvironmentToml(String),
    XdgError(String),
    FsError,
}

pub enum ConfigWriteError {
    CantSerialize(String),
    Io(String),
}

impl From<XdgError> for ConfigReadError {
    fn from(err: XdgError) -> Self {
        ConfigReadError::XdgError(err.to_string())
    }
}

pub fn write_build_config(build_config_search_path: &Path, cfg: &BuildConfig) -> Result<(), ConfigWriteError> {
    let serialize = toml::to_string_pretty(cfg)
        .map_err(|e| ConfigWriteError::CantSerialize(e.to_string()))?;

    fs::write(build_config_search_path.join("ponies.toml"), serialize)
        .map_err(|e| ConfigWriteError::Io(e.to_string()))?;

    Ok(())
}

pub fn read_build_config_precise(build_config_search_path: &Path) -> Result<BuildConfig, ConfigReadError> {
    let file_path = build_config_search_path.join("ponies.toml");

    // In this case, only return NoPoniesToml if the file literally does not exist.
    if !fs::exists(&file_path).map_err(|_| ConfigReadError::FsError)? {
        return Err(ConfigReadError::NoPoniesToml);
    }

    let build_str = fs::read_to_string(&file_path)
        .map_err(|err| ConfigReadError::BadPoniesToml(err.to_string()))?;

    let build: BuildConfig = toml::from_str(&build_str)
        .map_err(|err| ConfigReadError::BadPoniesToml(err.to_string()))?;

    Ok(build)
}

pub fn read_configs(build_config_search_path: &Path) -> Result<(BuildConfig, EnvironmentConfig), ConfigReadError> {
    let app = XdgApp::new("poniescript")?;

    let build_cfg_path = app.app_config_file("build-config.toml")?;

    let config_str = fs::read_to_string(&build_cfg_path)
        .map_err(|_| ConfigReadError::NoEnvironmentToml(build_cfg_path))?;

    let env: EnvironmentConfig = toml::from_str(&config_str)
        .map_err(|err| ConfigReadError::BadEnvironmentToml(err.to_string()))?;

    let build_str = fs::read_to_string(build_config_search_path.join("ponies.toml"))
        .map_err(|_| ConfigReadError::NoPoniesToml)?;

    let build: BuildConfig = toml::from_str(&build_str)
        .map_err(|err| ConfigReadError::BadPoniesToml(err.to_string()))?;

    Ok((build, env))
}