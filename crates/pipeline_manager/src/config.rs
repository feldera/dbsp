use crate::db::{PipelineId, ProgramId, Version};
use anyhow::{Error as AnyError, Result as AnyResult};
use clap::Parser;
use serde::Deserialize;
use std::{
    fs::{canonicalize, create_dir_all, File},
    path::{Path, PathBuf},
};

const fn default_server_port() -> u16 {
    8080
}

fn default_server_address() -> String {
    "127.0.0.1".to_string()
}

fn default_working_directory() -> String {
    ".".to_string()
}

fn default_sql_compiler_home() -> String {
    "../sql-to-dbsp-compiler".to_string()
}

fn default_db_connection_string() -> String {
    "".to_string()
}

/// Pipeline manager configuration read from a YAML config file or from command
/// line arguments.
#[derive(Parser, Deserialize, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct DatabaseConfig {
    /// Point to a relational database to use for state management. Accepted
    /// values are `postgres://<host>:<port>` or `postgres-embed`. For
    /// postgres-embed we create a DB in the current working directory. For
    /// postgres, we use the connection string as provided.
    #[serde(default = "default_db_connection_string")]
    #[arg(short, long, default_value_t = default_db_connection_string())]
    pub db_connection_string: String,

    /// [Developers only] Inject a SQL file into the database when starting the
    /// manager.
    ///
    /// This is useful to populate the DB with state for testing.
    #[serde(skip)]
    #[arg(short, long)]
    pub initial_sql: Option<String>,
}

impl DatabaseConfig {
    /// Database connection string.
    pub(crate) fn database_connection_string(&self) -> String {
        if self.db_connection_string.starts_with("postgres") {
            // this starts_with works for `postgres://` and `postgres-embed`
            self.db_connection_string.clone()
        } else {
            panic!("Invalid connection string {}", self.db_connection_string)
        }
    }
}

/// Pipeline manager configuration read from a YAML config file or from command
/// line arguments.
#[derive(Parser, Deserialize, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct ManagerConfig {
    /// Directory where the manager stores its filesystem state:
    /// generated Rust crates, pipeline logs, etc.
    #[serde(default = "default_working_directory")]
    #[arg(short, long, default_value_t = default_working_directory())]
    pub manager_working_directory: String,

    /// Port number for the HTTP service, defaults to 8080.
    #[serde(default = "default_server_port")]
    #[arg(short, long, default_value_t = default_server_port())]
    pub port: u16,

    /// Bind address for the HTTP service, defaults to 127.0.0.1.
    #[serde(default = "default_server_address")]
    #[arg(short, long, default_value_t = default_server_address())]
    pub bind_address: String,

    /// File to write manager logs to.
    ///
    /// This setting is only used when the `unix_daemon` option is set to
    /// `true`; otherwise the manager prints log entries to `stderr`.
    ///
    /// The default is `working_directory/manager.log`.
    #[arg(short, long)]
    pub logfile: Option<String>,

    /// Run as a UNIX daemon (detach from terminal).
    ///
    /// The default is `false`.
    ///
    /// # Compatibility
    /// This only has effect on UNIX OSs.
    #[serde(default)]
    #[arg(long)]
    pub unix_daemon: bool,

    /// Enable bearer-token based authorization.
    ///
    /// Usage depends on three environment variables to be set
    ///
    /// AUTH_CLIENT_ID, the client-id or application
    /// AUTH_ISSUER, the issuing service
    ///
    /// The default is `false`.
    #[serde(default)]
    #[arg(long, action = clap::ArgAction::Set, default_value_t=false)]
    pub use_auth: bool,

    /// [Developers only] dump OpenAPI specification to `openapi.json` file and
    /// exit immediately.
    #[serde(skip)]
    #[arg(long)]
    pub dump_openapi: bool,

    /// Server configuration YAML file.
    #[serde(skip)]
    #[arg(short, long)]
    pub config_file: Option<String>,

    /// [Developers only] Run in development mode.
    ///
    /// This runs with permissive CORS settings and allows the manager to be
    /// accessed from a different host/port.
    ///
    /// The default is `false`.
    #[serde(default)]
    #[arg(long)]
    pub dev_mode: bool,
}

impl ManagerConfig {
    /// Convert all directory paths in the `self` to absolute paths.
    ///
    /// Converts `working_directory` `sql_compiler_home`, and
    /// `dbsp_override_path` fields to absolute paths;
    /// fails if any of the paths doesn't exist or isn't readable.
    pub fn canonicalize(mut self) -> AnyResult<Self> {
        create_dir_all(&self.manager_working_directory).map_err(|e| {
            AnyError::msg(format!(
                "unable to create or open working directory '{}': {e}",
                self.manager_working_directory
            ))
        })?;

        self.manager_working_directory = canonicalize(&self.manager_working_directory)
            .map_err(|e| {
                AnyError::msg(format!(
                    "error canonicalizing working directory path '{}': {e}",
                    self.manager_working_directory
                ))
            })?
            .to_string_lossy()
            .into_owned();

        // Running as daemon and no log file specified - use default log file name.
        if self.logfile.is_none() && self.unix_daemon {
            self.logfile = Some(format!("{}/manager.log", self.manager_working_directory));
        }

        if let Some(logfile) = &self.logfile {
            let file = File::create(logfile).map_err(|e| {
                AnyError::msg(format!(
                    "unable to create or truncate log file '{}': {e}",
                    logfile
                ))
            })?;
            drop(file);

            self.logfile = Some(
                canonicalize(logfile)
                    .map_err(|e| {
                        AnyError::msg(format!(
                            "error canonicalizing log file path '{}': {e}",
                            logfile
                        ))
                    })?
                    .to_string_lossy()
                    .into_owned(),
            );
        };

        Ok(self)
    }

    /// Where Postgres embed stores the database.
    ///
    /// e.g., `<working-directory>/data`
    #[cfg(feature = "pg-embed")]
    pub(crate) fn postgres_embed_data_dir(&self) -> PathBuf {
        Path::new(&self.manager_working_directory).join("data")
    }

    /// Manager pid file.
    ///
    /// e.g., `<working-directory>/manager.pid`
    #[cfg(unix)]
    pub(crate) fn manager_pid_file_path(&self) -> PathBuf {
        Path::new(&self.manager_working_directory).join("manager.pid")
    }
}

/// Pipeline manager configuration read from a YAML config file or from command
/// line arguments.
#[derive(Parser, Deserialize, Debug, Clone)]
pub struct CompilerConfig {
    /// Directory where the manager stores its filesystem state:
    /// generated Rust crates, pipeline logs, etc.
    #[serde(default = "default_working_directory")]
    #[arg(long, default_value_t = default_working_directory())]
    pub compiler_working_directory: String,

    /// Compile pipelines in debug mode.
    ///
    /// The default is `false`.
    #[serde(default)]
    #[arg(long)]
    pub debug: bool,

    /// Location of the SQL-to-DBSP compiler.
    #[serde(default = "default_sql_compiler_home")]
    #[arg(long, default_value_t = default_sql_compiler_home())]
    pub sql_compiler_home: String,

    /// Override DBSP dependencies in generated Rust crates.
    ///
    /// By default the Rust crates generated by the SQL compiler
    /// depend on github versions of DBSP crates
    /// (`dbsp`, `dbsp_adapters`).  This configuration options
    /// modifies the dependency to point to a source tree in the
    /// local file system.
    #[arg(long)]
    pub dbsp_override_path: Option<String>,

    /// Precompile Rust dependencies in the working directory.
    ///
    /// Instructs the manager to download and compile all crates needed by
    /// the Rust code generated by the SQL compiler and exit immediately.
    /// This is useful to prepare the working directory, so that the first
    /// compilation job completes quickly.  Also creates the `Cargo.lock`
    /// file, making sure that subsequent `cargo` runs do not access the
    /// network.
    #[serde(skip)]
    #[arg(long)]
    pub precompile: bool,
}

impl CompilerConfig {
    /// Binary name for a project and version.
    ///
    /// Note: we rely on the program id and not name, so projects can
    /// be renamed without recompiling.
    pub(crate) fn binary_name(program_id: ProgramId, version: Version) -> String {
        format!("project_{program_id}_v{version}")
    }

    /// Directory where the manager maintains the generated cargo workspace.
    ///
    /// e.g., `<working-directory>/cargo_workspace`
    pub(crate) fn workspace_dir(&self) -> PathBuf {
        Path::new(&self.compiler_working_directory).join("cargo_workspace")
    }

    /// Directory where the manager stores binary artefacts needed to
    /// run versioned pipeline configurations.
    ///
    /// e.g., `<working-directory>/binaries`
    pub(crate) fn binaries_dir(&self) -> PathBuf {
        Path::new(&self.compiler_working_directory).join("binaries")
    }

    /// Location of the versioned executable.
    /// e.g., `<working-directory>/binaries/
    /// project0188e0cd-d8b0-71d5-bb5a-2f66c7b07dfb-v11`
    pub(crate) fn versioned_executable(&self, program_id: ProgramId, version: Version) -> PathBuf {
        Path::new(&self.binaries_dir()).join(Self::binary_name(program_id, version))
    }

    /// Location of the compiled executable for the project in the cargo target
    /// dir.
    /// Note: This is generally not an executable that's run as a pipeline.
    pub(crate) fn target_executable(&self, program_id: ProgramId) -> PathBuf {
        Path::new(&self.workspace_dir())
            .join("target")
            .join(if self.debug { "debug" } else { "release" })
            .join(Self::crate_name(program_id))
    }

    /// Crate name for a project.
    ///
    /// Note: we rely on the program id and not name, so projects can
    /// be renamed without recompiling.
    pub(crate) fn crate_name(program_id: ProgramId) -> String {
        format!("project{program_id}")
    }

    /// File name where the manager stores the SQL code of the project.
    pub(crate) fn sql_file_path(&self, program_id: ProgramId) -> PathBuf {
        self.project_dir(program_id).join("project.sql")
    }

    /// Directory where the manager generates the rust crate for the project.
    ///
    /// e.g., `<working-directory>/cargo_workspace/
    /// project0188e0cd-d8b0-71d5-bb5a-2f66c7b07dfb`
    pub(crate) fn project_dir(&self, program_id: ProgramId) -> PathBuf {
        self.workspace_dir().join(Self::crate_name(program_id))
    }

    /// The path to `schema.json` that contains a JSON description of input and
    /// output tables.
    pub(crate) fn schema_path(&self, program_id: ProgramId) -> PathBuf {
        const SCHEMA_FILE_NAME: &str = "schema.json";
        let sql_file_path = self.sql_file_path(program_id);
        let project_directory = sql_file_path.parent().unwrap();

        PathBuf::from(project_directory).join(SCHEMA_FILE_NAME)
    }

    /// Path to the generated `main.rs` for the project.
    pub(crate) fn rust_program_path(&self, program_id: ProgramId) -> PathBuf {
        self.project_dir(program_id).join("src").join("main.rs")
    }

    /// Path to the generated `Cargo.toml` file for the project.
    pub(crate) fn project_toml_path(&self, program_id: ProgramId) -> PathBuf {
        self.project_dir(program_id).join("Cargo.toml")
    }

    /// Top-level `Cargo.toml` file for the generated Rust workspace.
    pub(crate) fn workspace_toml_path(&self) -> PathBuf {
        self.workspace_dir().join("Cargo.toml")
    }
    /// Convert all directory paths in the `self` to absolute paths.
    ///
    /// Converts `working_directory` `sql_compiler_home`, and
    /// `dbsp_override_path` fields to absolute paths;
    /// fails if any of the paths doesn't exist or isn't readable.
    pub fn canonicalize(mut self) -> AnyResult<Self> {
        create_dir_all(&self.compiler_working_directory).map_err(|e| {
            AnyError::msg(format!(
                "unable to create or open working directory '{}': {e}",
                self.compiler_working_directory
            ))
        })?;
        create_dir_all(self.binaries_dir()).map_err(|e| {
            AnyError::msg(format!(
                "unable to create or open binaries directory '{:?}': {e}",
                self.binaries_dir()
            ))
        })?;

        self.sql_compiler_home = canonicalize(&self.sql_compiler_home)
            .map_err(|e| {
                AnyError::msg(format!(
                    "failed to access SQL compiler home '{}': {e}",
                    self.sql_compiler_home
                ))
            })?
            .to_string_lossy()
            .into_owned();

        if let Some(path) = self.dbsp_override_path.as_mut() {
            *path = canonicalize(&path)
                .map_err(|e| {
                    AnyError::msg(format!(
                        "failed to access dbsp override directory '{path}': {e}"
                    ))
                })?
                .to_string_lossy()
                .into_owned();
        }

        Ok(self)
    }

    /// SQL compiler executable.
    pub(crate) fn sql_compiler_path(&self) -> PathBuf {
        Path::new(&self.sql_compiler_home)
            .join("SQL-compiler")
            .join("sql-to-dbsp")
    }

    /// Location of the Rust libraries that ship with the SQL compiler.
    pub(crate) fn sql_lib_path(&self) -> PathBuf {
        Path::new(&self.sql_compiler_home).join("lib")
    }

    /// Location of the template `Cargo.toml` file that ships with the SQL
    /// compiler.
    pub(crate) fn project_toml_template_path(&self) -> PathBuf {
        Path::new(&self.sql_compiler_home)
            .join("temp")
            .join("Cargo.toml")
    }

    /// File to redirect compiler's stdout stream.
    pub(crate) fn compiler_stdout_path(&self, program_id: ProgramId) -> PathBuf {
        self.project_dir(program_id).join("out.log")
    }

    /// File to redirect compiler's stderr stream.
    pub(crate) fn compiler_stderr_path(&self, program_id: ProgramId) -> PathBuf {
        self.project_dir(program_id).join("err.log")
    }

    //// TODO: These might have to be runner specific

    /// Location to store pipeline files at runtime.
    pub(crate) fn pipeline_dir(&self, pipeline_id: PipelineId) -> PathBuf {
        Path::new(&self.compiler_working_directory)
            .join("pipelines")
            .join(format!("pipeline{pipeline_id}"))
    }

    /// Location to write the pipeline config file.
    pub(crate) fn config_file_path(&self, pipeline_id: PipelineId) -> PathBuf {
        self.pipeline_dir(pipeline_id).join("config.yaml")
    }

    /// Location to write the pipeline metadata file.
    pub(crate) fn metadata_file_path(&self, pipeline_id: PipelineId) -> PathBuf {
        self.pipeline_dir(pipeline_id).join("metadata.json")
    }

    /// Location for pipeline port file
    pub(crate) fn port_file_path(&self, pipeline_id: PipelineId) -> PathBuf {
        self.pipeline_dir(pipeline_id)
            .join(dbsp_adapters::server::SERVER_PORT_FILE)
    }
}
