use std::{collections::HashMap, fs::{self, File}, io::Write, path::{Path, PathBuf}};

use microxdg::{XdgApp, XdgError};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct EnvironmentConfig {
    pub poni_h_path: PathBuf,
    pub poni_gc_path: PathBuf,
    pub poniescript_path: PathBuf,

    // TODO: We probably want these per-platform in some way.
    pub cc: String,
    pub linker: String,
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

impl BuildConfig {
    pub fn generate_ninja_file(&self, file: &mut File, env: &EnvironmentConfig) -> std::io::Result<()> {
        writeln!(file, "builddir = .build\n")?;

        // TODO: Make all the Paths Strings instead?
        writeln!(file, "poni_h_path = {}", env.poni_h_path.display())?;
        writeln!(file, "poni_gc_path = {}", env.poni_gc_path.display())?;
        writeln!(file, "poniescript = {}\n", env.poniescript_path.display())?;

        // TODO: What we will end up wanting is a bunch of different cc/linkers,
        // used for debug/release, and used for different target platforms.
        // (e.g. ideally you should be able to, from Linux, easily compile for
        // Linux, Windows, Web, and Android)
        writeln!(file, "rule link\n  command = {} $in -o $out -L$poni_gc_path -lponiescript_gc\n  description = link\n",
            env.linker)?;
        writeln!(file, "rule cc\n  command = {} -c $in -o $out -I$poni_h_path -I.\n  description = cc\n",
            env.cc)?;

        writeln!(file, "rule poni-debug\n  command = $poniescript -o $out $in $imports --no-timing\n  description = poniescript\n")?;

        writeln!(file, "rule poni-regenerate\n  command = ponies regenerate\n  description = ponies regenerate\n")?;

        // Project regeneration is based on the ponies.toml file
        writeln!(file, "build .build/build.ninja: poni-regenerate ponies.toml")?;
        
        for (name, project) in &self.projects {
            writeln!(file, "build .build/{}-debug: link .build/{}-debug.o", name, name)?;
            writeln!(file, "build .build/{}-debug.o: cc .build/{}-debug.c", name, name)?;
            write!(file, "build .build/{}-debug.c: poni-debug", name)?;
            
            // TODO: Escape spaces in paths
            for poni in &project.files {
                write!(file, " {}", poni.display())?;
            }
            write!(file, "\n  imports =")?;
            for import in &project.imports {
                write!(file, " -i {}", import.display())?;
            }
            write!(file, "\n\n")?;
        }

        write!(file, "default")?;
        for (name, _) in &self.projects {
            write!(file, " .build/{name}-debug")?;
        }
        write!(file, "\n")?;

        Ok(())
    }
}

pub enum ConfigReadError {
    NoPoniesToml,
    BadPoniesToml(String),
    NoEnvironmentToml(PathBuf),
    BadEnvironmentToml(String),
    XdgError(String),
}

impl From<XdgError> for ConfigReadError {
    fn from(err: XdgError) -> Self {
        ConfigReadError::XdgError(err.to_string())
    }
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