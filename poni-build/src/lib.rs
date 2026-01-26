use std::{collections::{HashMap, HashSet}, fs::{self, File}, io::Write, path::{Path, PathBuf}};

use microxdg::{XdgApp, XdgError};
use serde::{Deserialize, Serialize};

fn default_rust_profile() -> String {
    "debug".into()
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
    MacOS,
    Web,

    None,
}

impl Default for Platform {
    fn default() -> Self {
        Platform::None
    }
}

impl Platform {
    fn empty_args() -> &'static [&'static str] {
        static EMPTY: &[&'static str] = &[];
        EMPTY
    }

    /// Get platform-specific C arguments.
    pub fn get_c_args(&self) -> &'static [&'static str] {
        match self {
            Platform::Web => {
                static ARGS: &[&'static str] = &["-target", "wasm32", "-DPONI_WASM32", "-nostdinc", "-nostdlib"];
                ARGS
            },
            _ => {
                Self::empty_args()
            }
        }
    }

    pub fn get_link_args(&self) -> &'static [&'static str] {
        match self {
            Platform::Web => {
                static ARGS: &[&'static str] = &[
                    "--export-all",
                    "--allow-undefined",
                    // This arg taken from RustC
                    "--no-demangle",
                    // These args also taken from RustC
                    "-z", "stack-size=1048576",
                    "--stack-first",
                ];
                ARGS
            },
            _ => {
                static LM: &[&str] = &["-lm"];
                LM
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Toolchain {
    /// The command to use for compiling C files.
    pub cc: String,

    /// Arguments to apply to the end of the compiler command line.
    #[serde(default)]
    pub cc_args_extra: String,

    /// The command to use for linking. This is often an invocation of a C
    /// compiler.
    pub linker: String,

    /// Arguments to apply to the end of the linker command line.
    #[serde(default)]
    pub linker_args_extra: String,

    /// Whether the PonieScript compiler directly invokes the C compiler through
    /// a pipe. This can reduce latency of compilation.
    pub piped: bool,

    /// The number of object files that the compilation is split into.
    pub ways: usize,

    /// The name of the corresponding Rust toolchain, if any. If none is specified,
    /// we will simply invoke `cargo` without a --target argument.
    #[serde(default)]
    pub rust_name: Option<String>,

    /// The name of the Rust profile to use ('debug', 'release', something 
    /// else.)
    #[serde(default = "default_rust_profile")]
    pub rust_profile: String,

    /// A known platform.
    #[serde(default)]
    pub target_platform: Platform,
}

#[derive(Serialize, Deserialize)]
pub struct ToolchainSet(HashMap<String, Toolchain>);

#[derive(Serialize, Deserialize)]
pub struct EnvironmentConfig {
    /// Path to the PonieScript source code, on this system. We currently assume
    /// Cargo is using its default configuration and putting everything inside
    /// target/. If this is not true, well, that's unfortunate.
    pub poni_src_path: PathBuf,

    /// Path to the PonieScript executable. This is probably inside the src
    /// directory, but you may want to choose between debug/release, etc.
    /// 
    /// In the future, maybe we will have the build system automatically
    /// rebuild PonieScript as well...
    pub poniescript_path: PathBuf,

    pub toolchain: HashMap<String, ToolchainSet>,

    pub default_profile: String,
    pub default_target: String,

    /// Warning messages about the environment config.
    #[serde(skip)]
    pub warnings: Vec<String>,
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

    pub fn lookup_rust_artifact(&self, profile: Option<&String>, target: Option<&String>, path: &PathBuf) -> Option<PathBuf> {
        let profile = profile.unwrap_or(&self.default_profile);
        let target = target.unwrap_or(&self.default_target);

        let Some(set) = self.toolchain.get(target) else {
            return None;
        };

        let Some(toolchain) = set.0.get(profile) else {
            return None;
        };

        let poni_target_path = self.poni_src_path.join("target");

        let poni_gc_path = match &toolchain.rust_name {
            // E.g. target/x86_64-pc-windows-gnu/debug
            Some(name) => poni_target_path.join(name).join(&toolchain.rust_profile),
            // E.g. target/debug
            None => poni_target_path.join(&toolchain.rust_profile)
        };

        Some(poni_gc_path.join(path))
    }

    fn generate_warnings(&mut self) {
        let mut warnings = Vec::new();

        for (target, set) in &self.toolchain {
            for (profile, toolchain) in &set.0 {
                match toolchain.target_platform {
                    Platform::Web => {
                        if !toolchain.linker.starts_with("wasm-ld") {
                            warnings.push(format!("{}-{}: expected linker 'wasm-ld'",
                                target, profile))
                        }

                        if !toolchain.cc.starts_with("clang") {
                            warnings.push(format!("{}-{}: expected c compiler 'clang'",
                                target, profile))
                        }
                    },
                    _ => {}
                }
            }
        }

        self.warnings = warnings;
    }
}

/// The top-level configuration for the build, described by a .toml file,
/// at least for now.
#[derive(Serialize, Deserialize)]
pub struct BuildConfig {
    pub projects: HashMap<String, Project>,
}

#[derive(Serialize, Deserialize)]
pub enum ProjectKind {
    /// Project using the small Ponyquad "game engine" / macroquad wrapper.
    Ponyquad,
}

impl ProjectKind {
    /// Gets additional C imports that this project kind requires. These are
    /// local to the poni_src_path.
    pub fn get_imports(&self) -> Vec<PathBuf> {
        match self {
            ProjectKind::Ponyquad => vec!["ponyquad/ponyquad.h".into()],
        }
    }

    /// Gets additional scripts that this project kind requires. These are
    /// local to the poni_src_path.
    pub fn get_poniescripts(&self) -> Vec<PathBuf> {
        match self {
            ProjectKind::Ponyquad => vec!["ponyquad/keycodes.poni".into()],
        }
    }

    /// Gets the name of the runtime library that this project kind requires.
    /// 
    /// For standalone projects, we just link against poniescript_rt. Otherwise,
    /// we may require something more in-depth.
    fn get_runtime_lib(&self) -> &'static str {
        match self {
            ProjectKind::Ponyquad => "ponyquad".into()
        }
    }

    /// Returns a vector of bound function names for this project types,
    /// e.g. 'update'.
    fn get_required_binds(&self) -> Vec<&'static str> {
        match self {
            ProjectKind::Ponyquad => vec!["update"]
        }
    }

    /// Returns whether this kind of project expects the '-e' argument to
    /// PonieScript.
    fn is_engine(&self) -> bool {
        match self {
            ProjectKind::Ponyquad => true,
        }
    }

    /// Path to the host we would use for hot reloading.
    pub fn get_hot_reload_host(&self) -> Option<PathBuf> {
        match self {
            ProjectKind::Ponyquad => Some("ponyquad-hot-host".into()),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Project {
    /// The .poni files that make up this project.
    pub files: Vec<PathBuf>,

    /// .h files that are imported for this project.
    pub imports: Vec<PathBuf>,

    pub kind: Option<ProjectKind>,
}

pub struct GeneratedNinjaInfo {
    default_toolchain: Option<String>,

    /// Contains a list of target files that we have already generated ninja
    /// rules for. This is mostly used in case we have more than one setup that
    /// is using the same Rust toolchain configuration, in which case duplicate
    /// `build: /path/to/rust-binary` rules would otherwise be generated, which
    /// is invalid in ninja.
    generated_rust_rules: HashSet<PathBuf>,
}

pub const MAGENTA: &'static str = "\x1b[0;35m";
pub const RED    : &'static str = "\x1b[0;31m";
pub const GREEN  : &'static str = "\x1b[0;32m";
pub const BLUE   : &'static str = "\x1b[0;34m";
pub const DIM    : &'static str = "\x1b[2m";
pub const RESET  : &'static str = "\x1b[0m";

struct RustLibraryDep {
    src_path: PathBuf,
    package_name: String,
}

impl BuildConfig {
    pub fn empty() -> Self {
        BuildConfig { projects: HashMap::new() }
    }

    fn generate_toolchain(&self, ninja: &mut File, poni_src_path: &PathBuf, name: &String, profile: &String, toolchain: &Toolchain, info: &mut GeneratedNinjaInfo) -> std::io::Result<()> {
        // TODO: Ask cargo for where build artifacts are, or something?
        let poni_target_path = poni_src_path.join("target");

        // let gc_dep = RustLibraryDep {
        //     src_path: poni_src_path.clone(),
        //     package_name: "poniescript-gc".into(),
        // };
        
        let poni_gc_path = match &toolchain.rust_name {
            // E.g. target/x86_64-pc-windows-gnu/debug
            Some(name) => poni_target_path.join(name).join(&toolchain.rust_profile),
            // E.g. target/debug
            None => poni_target_path.join(&toolchain.rust_profile)
        };
        
        // TODO: What we will end up wanting is a bunch of different cc/linkers,
        // used for debug/release, and used for different target platforms.
        // (e.g. ideally you should be able to, from Linux, easily compile for
        // Linux, Windows, Web, and Android)
        //
        // This profile setup mostly accomplishes this, but we will need
        // a different compile of poniescript_gc and any other supporting libraries,
        // on each targeted platform.
        writeln!(ninja, "rule link-{name}-{profile}")?;
        // NOTE: Apparently we can't do this with a variable like this, so 
        // just encode it directly for now.
        //writeln!(ninja, "  command = {} $in -o $out -L$poni_gc_path -lponiescript_gc",
        //    toolchain.linker)?;
        //writeln!(ninja, "  poni_gc_path = {}", poni_gc_path.display())?;

        // For now, always add -lm, although this might be wrong.
        write!(ninja, "  command = {} $in -o $out -L{} -l$runtime_lib {}",
            toolchain.linker, poni_gc_path.display(), toolchain.linker_args_extra)?;
        for arg in toolchain.target_platform.get_link_args() {
            write!(ninja, " {}", arg)?;
        }
        writeln!(ninja, "")?;
        writeln!(ninja, "  description = {BLUE}link{RESET}{DIM}.{name}.{profile}{RESET} -> $outdesc")?;

        writeln!(ninja, "rule hot-link-{name}-{profile}")?;
        writeln!(ninja, "  command = {} $in -o $out -shared {}",
            toolchain.linker, toolchain.linker_args_extra)?;
        writeln!(ninja, "  description = {BLUE}hot {RESET}{DIM}.{name}.{profile}{RESET} -> $outdesc")?;

        writeln!(ninja, "rule cc-{name}-{profile}")?;
        write!(ninja, "  command = {} -c $in -o $out -MD -MF $out.d -I$poni_h_path -I. {}", toolchain.cc, toolchain.cc_args_extra)?;
        for arg in toolchain.target_platform.get_c_args() {
            write!(ninja, " {}", arg)?;
        }
        writeln!(ninja, "")?;
        writeln!(ninja, "  depfile = $out.d")?;
        writeln!(ninja, "  deps = gcc")?;
        writeln!(ninja, "  description = {GREEN}cc  {RESET}{DIM}.{name}.{profile}{RESET} $indesc")?;

        writeln!(ninja, "rule cargo-{name}-{profile}")?;
        write!(ninja, "  command = cargo build --manifest-path $cargotoml -p $package")?;
        if let Some(name) = &toolchain.rust_name {
            write!(ninja, " --target {name}")?;
        }
        match toolchain.rust_profile.as_str() {
            "debug" => { /* cargo doesn't accept --debug as an argument */ },
            "release" => { write!(ninja, " --release")?; }
            s => { write!(ninja, "--profile {s}")?; }
        }
        write!(ninja, "\n")?;
        writeln!(ninja, "  description = {RED}rust{RESET}{DIM}.{name}.{profile}{RESET} $package")?;

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
            
            // Include a dependency on any of the Rust runtime libraries. That
            // way, if they change, we will automatically rebuild. (Or we can
            // build the Rust library if it hasn't been built yet).

            // TODO: Is this the same on windows? :)
            let runtime_lib = project.kind.as_ref().map(|p| p.get_runtime_lib())
                .unwrap_or("poniescript_rt");
            let runtime_artefact_path = poni_gc_path.join(format!("lib{}.a", runtime_lib));
            write!(ninja, " | {}", runtime_artefact_path.display())?;

            write!(ninja, "\n")?;
            writeln!(ninja, "  outdesc = {project_name}")?;
            writeln!(ninja, "  runtime_lib = {runtime_lib}")?;

            // Link rule for hot reloading.
            write!(ninja, "build {dir}/{project_name}.so: hot-link-{name}-{profile}")?;
            for obj in &object_files {
                write!(ninja, " {dir}/hot-{obj}")?;
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

                    writeln!(ninja, "build {dir}/hot-{obj}: cc-{name}-{profile} {dir}/hot-{c} | {c_file_deps}")?;
                    writeln!(ninja, "  indesc = hot-{c}")?;
                }
            }

            // Now generate the PonieScript rule. If the output is piped, then
            // the PonieScript command generates all of the .o files; otherwise,
            // it generates all of the .c files.
            let mut build_poniescript_cmd = |hot| -> std::io::Result<()> {
                let outputs = if toolchain.piped { &object_files } else { &c_files };
                write!(ninja, "build")?;
                for output in outputs {
                    if hot {
                        write!(ninja, " {dir}/hot-{output}")?;
                    }
                    else {
                        write!(ninja, " {dir}/{output}")?;
                    }
                }
                write!(ninja, ": poni-{name}-{profile}")?;
                
                // TODO: Escape spaces in paths

                // Pass all the PonieScript files to the PonieScript compiler.
                for poni in &project.files {
                    write!(ninja, " {}", poni.display())?;
                }
                // ProjectKind-specific PonieScript source files..
                for poni in &project.kind.as_ref().map(|p| p.get_poniescripts()).unwrap_or(Vec::new()) {
                    write!(ninja, " {}", poni_src_path.join(poni).display())?;
                }

                if toolchain.piped {
                    write!(ninja, " | {c_file_deps}")?;
                }

                // Create the imports variable.
                write!(ninja, "\n  imports =")?;
                for import in &project.imports {
                    write!(ninja, " -i {}", import.display())?;
                }
                // ProjectKind-specific imports.
                for import in &project.kind.as_ref().map(|p: &ProjectKind| p.get_imports()).unwrap_or(Vec::new()) {
                    write!(ninja, " -i {}", poni_src_path.join(import).display())?;
                }
                // So, technically imports is just imports, not any arguments, but
                // we'll include the -e in imports for now.
                if project.kind.as_ref().map(|p| p.is_engine()).unwrap_or(false) {
                    write!(ninja, " -e")?;
                }
                if hot {
                    write!(ninja, " --hot")?;
                }
                for bind in &project.kind.as_ref().map(|p| p.get_required_binds()).unwrap_or(Vec::new()) {
                    write!(ninja, " --bind-fun {}", bind)?;
                }


                // Create the outputargs variable. This is similar to outputs,
                // except with -o in front of each one.
                write!(ninja, "\n  outputargs =")?;
                for output in outputs {
                    let hot = if hot { "hot-" } else { "" };
                    write!(ninja, " -o {dir}/{hot}{output}")?;
                }
                write!(ninja, "\n  indesc = {project_name}")?;
                write!(ninja, "\n\n")?;

                Ok(())
            };

            build_poniescript_cmd(false)?;
            build_poniescript_cmd(true)?;

            // Generate rules for building Rust dependencies.
            if info.generated_rust_rules.insert(runtime_artefact_path.clone()) {
                let cargotoml = poni_src_path.join("Cargo.toml");
                writeln!(ninja, "build {} : cargo-{name}-{profile}", runtime_artefact_path.display())?;
                    // We could depend on the cargo.toml path... seems a bit
                    // silly...
                    //cargotoml.display())?;
                writeln!(ninja, "  package = {}", runtime_lib.replace("_", "-"))?;
                writeln!(ninja, "  cargotoml = {}", cargotoml.display())?;
                writeln!(ninja, "")?;
            }
        }

        // Finally, generate a phony rule that lets us build all the projects
        // with this particular toolchain. This is helpful for the CLI.
        write!(ninja, "build {name}-{profile}: phony")?;
        for (project_name, _) in &self.projects {
            write!(ninja, " {dir}/{project_name}")?;
        }
        writeln!(ninja, "\n")?;

        Ok(())
    }

    fn generate_toolchain_set(&self, ninja: &mut File, poni_src_path: &PathBuf, name: &String, set: &ToolchainSet, info: &mut GeneratedNinjaInfo) -> std::io::Result<()> {
        // Generate the following for each toolchain:
        // - Rules 'link', 'cc', and 'poni'
        // - The build rules for that toolchain

        for (profile, toolchain) in &set.0 {
            self.generate_toolchain(ninja, poni_src_path, name, profile, toolchain, info)?;
        }

        Ok(())
    }

    // Returns the default toolchain name, if any.
    pub fn generate_ninja_file(&self, file: &mut File, env: &EnvironmentConfig) -> std::io::Result<GeneratedNinjaInfo> {
        let mut result = GeneratedNinjaInfo {
            default_toolchain: None,
            generated_rust_rules: HashSet::new(),
        };

        writeln!(file, "builddir = .build\n")?;

        let poni_h_path = env.poni_src_path.join("poniescript");

        // TODO: Make all the Paths Strings instead?
        writeln!(file, "poni_h_path = {}", poni_h_path.display())?;
        writeln!(file, "poniescript = {}\n", env.poniescript_path.display())?;

        writeln!(file, "rule poni-regenerate\n  command = ponies regenerate\n  generator = true\n  description = ponies regenerate\n")?;

        // Project regeneration is based on the ponies.toml file
        //
        // TODO: Make this also depend on the EnvironmentConfig's path
        writeln!(file, "build .build/build.ninja: poni-regenerate ponies.toml")?;
        
        for (name, set) in &env.toolchain {
            self.generate_toolchain_set(file, &env.poni_src_path, name, set, &mut result)?;
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

pub fn read_build_config_from_string(text: &str) -> Result<BuildConfig, ConfigReadError> {
    let build: BuildConfig = toml::from_str(text)
        .map_err(|err| ConfigReadError::BadPoniesToml(err.to_string()))?;

    Ok(build)
}

pub fn read_environment_config() -> Result<EnvironmentConfig, ConfigReadError> {
    let app = XdgApp::new("poniescript")?;

    let build_cfg_path = app.app_config_file("build-config.toml")?;

    let config_str = fs::read_to_string(&build_cfg_path)
        .map_err(|_| ConfigReadError::NoEnvironmentToml(build_cfg_path))?;

    let mut env: EnvironmentConfig = toml::from_str(&config_str)
        .map_err(|err| ConfigReadError::BadEnvironmentToml(err.to_string()))?;

    env.generate_warnings();

    return Ok(env);
}

pub fn read_configs(build_config_search_path: &Path) -> Result<(BuildConfig, EnvironmentConfig), ConfigReadError> {
    let app = XdgApp::new("poniescript")?;

    let build_cfg_path = app.app_config_file("build-config.toml")?;

    let config_str = fs::read_to_string(&build_cfg_path)
        .map_err(|_| ConfigReadError::NoEnvironmentToml(build_cfg_path))?;

    let mut env: EnvironmentConfig = toml::from_str(&config_str)
        .map_err(|err| ConfigReadError::BadEnvironmentToml(err.to_string()))?;
    env.generate_warnings();

    let build_str = fs::read_to_string(build_config_search_path.join("ponies.toml"))
        .map_err(|_| ConfigReadError::NoPoniesToml)?;

    let build: BuildConfig = toml::from_str(&build_str)
        .map_err(|err| ConfigReadError::BadPoniesToml(err.to_string()))?;

    Ok((build, env))
}