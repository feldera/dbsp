use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Command, FromArgMatches};

use colored::Colorize;
use dbsp_pipeline_manager::compiler::Compiler;
use dbsp_pipeline_manager::config::{CompilerConfig, DatabaseConfig};
use dbsp_pipeline_manager::db::ProjectDB;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let name = "[compiler]".cyan();
    dbsp_pipeline_manager::logging::init_logging(name);
    let cli = Command::new("Pipeline manager CLI");
    let cli = DatabaseConfig::augment_args(cli);
    let cli = CompilerConfig::augment_args(cli);
    let matches = cli.get_matches();

    let database_config = DatabaseConfig::from_arg_matches(&matches).unwrap();
    let compiler_config = CompilerConfig::from_arg_matches(&matches).unwrap();
    let compiler_config = compiler_config.canonicalize().unwrap();
    let db: ProjectDB = ProjectDB::connect(
        &database_config,
        #[cfg(feature = "pg-embed")]
        None,
    )
    .await
    .unwrap();
    let compiler = Compiler::new(&compiler_config, Arc::new(Mutex::new(db)))
        .await
        .unwrap();
    loop {
        if compiler.compiler_task.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
}
