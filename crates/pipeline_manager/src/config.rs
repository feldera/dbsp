use crate::{PipelineId, ProjectId};
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
pub(crate) struct ManagerConfig {
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

    /// Directory where the manager stores its filesystem state:
    /// generated Rust crates, pipeline logs, etc.
    #[serde(default = "default_working_directory")]
    #[arg(short, long, default_value_t = default_working_directory())]
    pub working_directory: String,

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

    /// Compile pipelines in debug mode.
    ///
    /// The default is `false`.
    #[serde(default)]
    #[arg(long)]
    pub debug: bool,

    /// Run as a UNIX daemon (detach from terminal).
    ///
    /// The default is `false`.
    ///
    /// # Compatibility
    /// This only has effect on UNIX OSs.
    #[serde(default)]
    #[arg(long)]
    pub unix_daemon: bool,

    /// Point to a relational database to use for state management. Accepted
    /// values are `postgres://<host>:<port>` or `postgres-embed`. For
    /// postgres-embed we create a DB in the current working directory. For
    /// postgres, we use the connection string as provided.
    #[serde(default = "default_db_connection_string")]
    #[arg(short, long, default_value_t = default_db_connection_string())]
    pub db_connection_string: String,

    /// [Developers only] serve static content from the specified directory.
    /// Allows modifying JavaScript without restarting the server.
    #[arg(short, long)]
    pub static_html: Option<String>,

    /// [Developers only] dump OpenAPI specification to `openapi.json` file and
    /// exit immediately.
    #[serde(skip)]
    #[arg(long)]
    pub dump_openapi: bool,

    /// Server configuration YAML file.
    #[serde(skip)]
    #[arg(short, long)]
    pub config_file: Option<String>,

    /// [Developers only] Inject a SQL file into the database when starting the
    /// manager.
    ///
    /// This is useful to populate the DB with state for testing.
    #[serde(skip)]
    #[arg(short, long)]
    pub initial_sql: Option<String>,

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
    /// Converts `working_directory` `sql_compiler_home`,
    /// `dbsp_override_path`, and `static_html` fields to absolute paths;
    /// fails if any of the paths doesn't exist or isn't readable.
    pub(crate) fn canonicalize(mut self) -> AnyResult<Self> {
        create_dir_all(&self.working_directory).map_err(|e| {
            AnyError::msg(format!(
                "unable to create or open working directry '{}': {e}",
                self.working_directory
            ))
        })?;

        self.working_directory = canonicalize(&self.working_directory)
            .map_err(|e| {
                AnyError::msg(format!(
                    "error canonicalizing working directory path '{}': {e}",
                    self.working_directory
                ))
            })?
            .to_string_lossy()
            .into_owned();

        // Running as daemon and no log file specified - use default log file name.
        if self.logfile.is_none() && self.unix_daemon {
            self.logfile = Some(format!("{}/manager.log", self.working_directory));
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

        if let Some(path) = self.static_html.as_mut() {
            *path = canonicalize(&path)
                .map_err(|e| AnyError::msg(format!("failed to access '{path}': {e}")))?
                .to_string_lossy()
                .into_owned();
        }

        Ok(self)
    }

    /// Crate name for a project.
    ///
    /// Note: we rely on the project id and not name, so projects can
    /// be renamed without recompiling.
    pub(crate) fn crate_name(project_id: ProjectId) -> String {
        format!("project{project_id}")
    }

    /// Directory where the manager maintains the generated cargo workspace.
    pub(crate) fn workspace_dir(&self) -> PathBuf {
        Path::new(&self.working_directory).join("cargo_workspace")
    }

    /// Where Postgres embed stores the database.
    #[cfg(feature = "pg-embed")]
    pub(crate) fn postgres_embed_data_dir(&self) -> PathBuf {
        Path::new(&self.working_directory).join("data")
    }

    /// Manager pid file.
    #[cfg(unix)]
    pub(crate) fn manager_pid_file_path(&self) -> PathBuf {
        Path::new(&self.working_directory).join("manager.pid")
    }

    /// Database connection string.
    pub(crate) fn database_connection_string(&self) -> String {
        if self.db_connection_string.starts_with("postgres") {
            // this starts_with works for `postgres://` and `postgres-embed`
            self.db_connection_string.clone()
        } else {
            panic!("Invalid connection string {}", self.db_connection_string)
        }
    }

    /// Directory where the manager generates Rust crate for the project.
    pub(crate) fn project_dir(&self, project_id: ProjectId) -> PathBuf {
        self.workspace_dir().join(Self::crate_name(project_id))
    }

    /// File name where the manager stores the SQL code of the project.
    pub(crate) fn sql_file_path(&self, project_id: ProjectId) -> PathBuf {
        self.project_dir(project_id).join("project.sql")
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

    /// File to redirect compiler's stderr stream.
    pub(crate) fn compiler_stderr_path(&self, project_id: ProjectId) -> PathBuf {
        self.project_dir(project_id).join("err.log")
    }

    /// File to redirect compiler's stdout stream.
    pub(crate) fn compiler_stdout_path(&self, project_id: ProjectId) -> PathBuf {
        self.project_dir(project_id).join("out.log")
    }

    /// Path to the generated `main.rs` for the project.
    pub(crate) fn rust_program_path(&self, project_id: ProjectId) -> PathBuf {
        self.project_dir(project_id).join("src").join("main.rs")
    }

    /// Location of the template `Cargo.toml` file that ships with the SQL
    /// compiler.
    pub(crate) fn project_toml_template_path(&self) -> PathBuf {
        Path::new(&self.sql_compiler_home)
            .join("temp")
            .join("Cargo.toml")
    }

    /// Path to the generated `Cargo.toml` file for the project.
    pub(crate) fn project_toml_path(&self, project_id: ProjectId) -> PathBuf {
        self.project_dir(project_id).join("Cargo.toml")
    }

    /// Top-level `Cargo.toml` file for the generated Rust workspace.
    pub(crate) fn workspace_toml_path(&self) -> PathBuf {
        self.workspace_dir().join("Cargo.toml")
    }

    /// Location of the compiled executable for the project.
    pub(crate) fn project_executable(&self, project_id: ProjectId) -> PathBuf {
        Path::new(&self.workspace_dir())
            .join("target")
            .join(if self.debug { "debug" } else { "release" })
            .join(Self::crate_name(project_id))
    }

    /// Location to store pipeline files at runtime.
    pub(crate) fn pipeline_dir(&self, pipeline_id: PipelineId) -> PathBuf {
        Path::new(&self.working_directory)
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

    /// The path to `schema.json` that contains a JSON description of input and
    /// output tables.
    pub(crate) fn schema_path(&self, project_id: ProjectId) -> PathBuf {
        const SCHEMA_FILE_NAME: &str = "schema.json";
        let sql_file_path = self.sql_file_path(project_id);
        let project_directory = sql_file_path.parent().unwrap();

        PathBuf::from(project_directory).join(SCHEMA_FILE_NAME)
    }
}
