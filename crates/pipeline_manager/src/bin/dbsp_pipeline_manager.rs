use clap::{Args, Command, FromArgMatches};

use colored::Colorize;
use dbsp_pipeline_manager::compiler::Compiler;
use dbsp_pipeline_manager::config::{CompilerConfig, DatabaseConfig, ManagerConfig};
use dbsp_pipeline_manager::pipeline_manager::ApiDoc;
use utoipa::OpenApi;

fn main() -> anyhow::Result<()> {
    // Stay in single-threaded mode (no tokio) until calling `daemonize`.

    // Create env logger.
    let name = "[manager]".cyan();
    dbsp_pipeline_manager::logging::init_logging(name);

    let cli = Command::new("Pipeline manager CLI");
    let cli = DatabaseConfig::augment_args(cli);
    let cli = ManagerConfig::augment_args(cli);
    let cli = CompilerConfig::augment_args(cli);
    let matches = cli.get_matches();

    let mut manager_config = ManagerConfig::from_arg_matches(&matches)
        .map_err(|err| err.exit())
        .unwrap();
    if manager_config.dump_openapi {
        let openapi_json = ApiDoc::openapi().to_json()?;
        std::fs::write("openapi.json", openapi_json.as_bytes())?;
        return Ok(());
    }

    if let Some(config_file) = &manager_config.config_file {
        let config_yaml = std::fs::read(config_file).map_err(|e| {
            anyhow::Error::msg(format!("error reading config file '{config_file}': {e}"))
        })?;
        let config_yaml = String::from_utf8_lossy(&config_yaml);
        manager_config = serde_yaml::from_str(&config_yaml).map_err(|e| {
            anyhow::Error::msg(format!("error parsing config file '{config_file}': {e}"))
        })?;
    }
    let compiler_config = CompilerConfig::from_arg_matches(&matches)
        .map_err(|err| err.exit())
        .unwrap();

    let manager_config = manager_config.canonicalize()?;
    let compiler_config = compiler_config.canonicalize()?;

    if compiler_config.precompile {
        actix_web::rt::System::new()
            .block_on(Compiler::precompile_dependencies(&compiler_config))?;
        return Ok(());
    }
    let database_config = DatabaseConfig::from_arg_matches(&matches)?;
    dbsp_pipeline_manager::pipeline_manager::run(database_config, manager_config, compiler_config)
}
