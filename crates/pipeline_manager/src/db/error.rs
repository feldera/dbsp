use super::{ConnectorId, PipelineId, ProgramId, Version};
use crate::auth::TenantId;
use actix_web::{
    body::BoxBody, http::StatusCode, HttpResponse, HttpResponseBuilder, ResponseError,
};
use dbsp_adapters::DetailedError;
use dbsp_adapters::ErrorResponse;
use deadpool_postgres::PoolError;
use refinery::Error as RefineryError;
use serde::{ser::SerializeStruct, Serialize, Serializer};
use std::{backtrace::Backtrace, borrow::Cow, error::Error as StdError, fmt, fmt::Display};
use tokio_postgres::error::Error as PgError;

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum DBError {
    #[serde(serialize_with = "serialize_pg_error")]
    PostgresError {
        error: Box<PgError>,
        backtrace: Backtrace,
    },
    #[serde(serialize_with = "serialize_pgpool_error")]
    PostgresPoolError {
        error: Box<PoolError>,
        backtrace: Backtrace,
    },
    #[serde(serialize_with = "serialize_refinery_error")]
    PostgresMigrationError {
        error: Box<RefineryError>,
        backtrace: Backtrace,
    },
    #[cfg(feature = "pg-embed")]
    #[serde(serialize_with = "serialize_pgembed_error")]
    PgEmbedError {
        error: Box<pg_embed::pg_errors::PgEmbedError>,
        backtrace: Backtrace,
    },
    // Catch-all error for unexpected invalid data extracted from DB.
    // We can split it into several separate error variants if needed.
    #[serde(serialize_with = "serialize_invalid_data")]
    InvalidData {
        error: String,
        backtrace: Backtrace,
    },
    #[serde(serialize_with = "serialize_invalid_status")]
    InvalidStatus {
        status: String,
        backtrace: Backtrace,
    },
    UnknownProgram {
        program_id: ProgramId,
    },
    OutdatedProgramVersion {
        expected_version: Version,
    },
    UnknownPipeline {
        pipeline_id: PipelineId,
    },
    UnknownConnector {
        connector_id: ConnectorId,
    },
    UnknownTenant {
        tenant_id: TenantId,
    },
    UnknownAttachedConnector {
        pipeline_id: PipelineId,
        name: String,
    },
    UnknownName {
        name: String,
    },
    DuplicateName,
    #[serde(serialize_with = "serialize_duplicate_key")]
    DuplicateKey {
        backtrace: Backtrace,
    },
    InvalidKey,
    #[serde(serialize_with = "serialize_unique_key_violation")]
    UniqueKeyViolation {
        constraint: &'static str,
        backtrace: Backtrace,
    },
    #[serde(serialize_with = "serialize_unknown_pipeline_status")]
    UnknownPipelineStatus {
        status: String,
        backtrace: Backtrace,
    },
    ProgramNotSet,
    ProgramNotCompiled,
    ProgramFailedToCompile,
    NoRevisionAvailable {
        pipeline_id: PipelineId,
    },
    RevisionNotChanged,
    TablesNotInSchema {
        missing: Vec<(String, String)>,
    },
    ViewsNotInSchema {
        missing: Vec<(String, String)>,
    },
}

impl DBError {
    pub fn invalid_data(error: String) -> Self {
        Self::InvalidData {
            error,
            backtrace: Backtrace::capture(),
        }
    }
    pub fn invalid_status(status: String) -> Self {
        Self::InvalidStatus {
            status,
            backtrace: Backtrace::capture(),
        }
    }
    pub fn unknown_pipeline_status(status: String) -> Self {
        Self::UnknownPipelineStatus {
            status,
            backtrace: Backtrace::capture(),
        }
    }
    pub fn unique_key_violation(constraint: &'static str) -> Self {
        Self::UniqueKeyViolation {
            constraint,
            backtrace: Backtrace::capture(),
        }
    }
    pub fn duplicate_key() -> Self {
        Self::DuplicateKey {
            backtrace: Backtrace::capture(),
        }
    }
}

fn serialize_pg_error<S>(
    error: &PgError,
    backtrace: &Backtrace,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("PgError", 2)?;
    ser.serialize_field("error", &error.to_string())?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

fn serialize_pgpool_error<S>(
    error: &PoolError,
    backtrace: &Backtrace,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("PgPoolError", 2)?;
    ser.serialize_field("error", &error.to_string())?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

fn serialize_refinery_error<S>(
    error: &RefineryError,
    backtrace: &Backtrace,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("RefineryError", 2)?;
    ser.serialize_field("error", &error.to_string())?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

#[cfg(feature = "pg-embed")]
fn serialize_pgembed_error<S>(
    error: &pg_embed::pg_errors::PgEmbedError,
    backtrace: &Backtrace,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("PgEmbedError", 2)?;
    ser.serialize_field("error", &error.to_string())?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

fn serialize_invalid_data<S>(
    error: &String,
    backtrace: &Backtrace,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("InvalidData", 2)?;
    ser.serialize_field("error", error)?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

fn serialize_invalid_status<S>(
    error: &String,
    backtrace: &Backtrace,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("InvalidStatus", 2)?;
    ser.serialize_field("error", error)?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

fn serialize_duplicate_key<S>(backtrace: &Backtrace, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("DuplicateKey", 1)?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

fn serialize_unique_key_violation<S>(
    constraint: &&str,
    backtrace: &Backtrace,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("UniqueKeyViolation", 2)?;
    ser.serialize_field("constraint", constraint)?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

fn serialize_unknown_pipeline_status<S>(
    status: &String,
    backtrace: &Backtrace,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("UnknownPipelineStatus", 2)?;
    ser.serialize_field("status", &status.to_string())?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

impl From<PgError> for DBError {
    fn from(error: PgError) -> Self {
        Self::PostgresError {
            error: Box::new(error),
            backtrace: Backtrace::capture(),
        }
    }
}

impl From<PoolError> for DBError {
    fn from(error: PoolError) -> Self {
        Self::PostgresPoolError {
            error: Box::new(error),
            backtrace: Backtrace::capture(),
        }
    }
}

impl From<RefineryError> for DBError {
    fn from(error: RefineryError) -> Self {
        Self::PostgresMigrationError {
            error: Box::new(error),
            backtrace: Backtrace::capture(),
        }
    }
}

#[cfg(feature = "pg-embed")]
impl From<pg_embed::pg_errors::PgEmbedError> for DBError {
    fn from(error: pg_embed::pg_errors::PgEmbedError) -> Self {
        Self::PgEmbedError {
            error: Box::new(error),
            backtrace: Backtrace::capture(),
        }
    }
}

impl Display for DBError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DBError::PostgresError { error, .. } => {
                write!(f, "Unexpected Postgres error: '{error}'")
            }
            DBError::PostgresPoolError { error, .. } => {
                write!(f, "Postgres connection pool error: '{error}'")
            }
            DBError::PostgresMigrationError { error, .. } => {
                write!(f, "DB schema migration error: '{error}'")
            }
            #[cfg(feature = "pg-embed")]
            DBError::PgEmbedError { error, .. } => {
                write!(f, "PG-embed error: '{error}'")
            }
            DBError::InvalidData { error, .. } => {
                write!(f, "Invalid DB data '{error}'")
            }
            DBError::InvalidStatus { status, .. } => {
                write!(f, "Invalid program status string '{status}'")
            }
            DBError::UnknownProgram { program_id } => {
                write!(f, "Unknown program id '{program_id}'")
            }
            DBError::OutdatedProgramVersion { expected_version } => {
                write!(
                    f,
                    "Outdated program version. Expected version: '{expected_version}'"
                )
            }
            DBError::UnknownPipeline { pipeline_id } => {
                write!(f, "Unknown pipeline id '{pipeline_id}'")
            }
            DBError::UnknownAttachedConnector { pipeline_id, name } => {
                write!(
                    f,
                    "Pipeline '{pipeline_id}' does not have a connector named '{name}'"
                )
            }
            DBError::UnknownConnector { connector_id } => {
                write!(f, "Unknown connector id '{connector_id}'")
            }
            DBError::UnknownTenant { tenant_id } => {
                write!(f, "Unknown tenant id '{tenant_id}'")
            }
            DBError::DuplicateName => {
                write!(f, "An entity with this name already exists")
            }
            DBError::DuplicateKey { .. } => {
                write!(f, "A key with the same hash already exists")
            }
            DBError::InvalidKey => {
                write!(f, "Could not validate API")
            }
            DBError::UnknownName { name } => {
                write!(f, "An entity with name {name} was not found")
            }
            DBError::UniqueKeyViolation { constraint, .. } => {
                write!(f, "Unique key violation for '{constraint}'")
            }
            DBError::UnknownPipelineStatus { status, .. } => {
                write!(f, "Unknown pipeline status '{status}' encountered")
            }
            DBError::ProgramNotSet => write!(f, "The pipeline does not have a program attached"),
            DBError::ProgramNotCompiled => {
                write!(
                    f,
                    "The program attached to the pipeline hasn't been compiled yet."
                )
            }
            DBError::ProgramFailedToCompile => {
                write!(
                    f,
                    "The program attached to the pipeline did not compile successfully"
                )
            }
            DBError::NoRevisionAvailable { pipeline_id } => {
                write!(
                    f,
                    "The pipeline {pipeline_id} does not have a committed revision"
                )
            }
            DBError::RevisionNotChanged => {
                write!(f, "There is no change to commit for pipeline")
            }
            DBError::TablesNotInSchema { missing } => {
                write!(
                    f,
                    "Pipeline configuration specifies invalid connector->table pairs '{}': The table(s) don't exist in the program",
                    missing.iter().map(|(ac, t)| format!("{} -> {}", ac, t)).collect::<Vec<String>>().join(", ").trim_end_matches(", ")
                )
            }
            DBError::ViewsNotInSchema { missing } => {
                write!(
                    f,
                    "Pipeline configuration specifies invalid connector->view pairs '{}': The view(s) don't exist in the program",
                    missing.iter().map(|(ac, v)| format!("{} -> {}", ac, v)).collect::<Vec<String>>().join(", ").trim_end_matches(", ")
                )
            }
        }
    }
}

impl DetailedError for DBError {
    fn error_code(&self) -> Cow<'static, str> {
        match self {
            Self::PostgresError { .. } => Cow::from("PostgresError"),
            Self::PostgresPoolError { .. } => Cow::from("PostgresPoolError"),
            Self::PostgresMigrationError { .. } => Cow::from("PostgresMigrationError"),
            #[cfg(feature = "pg-embed")]
            Self::PgEmbedError { .. } => Cow::from("PgEmbedError"),
            Self::InvalidData { .. } => Cow::from("InvalidData"),
            Self::InvalidStatus { .. } => Cow::from("InvalidStatus"),
            Self::UnknownProgram { .. } => Cow::from("UnknownProgram"),
            Self::OutdatedProgramVersion { .. } => Cow::from("OutdatedProgramVersion"),
            Self::UnknownPipeline { .. } => Cow::from("UnknownPipeline"),
            Self::UnknownConnector { .. } => Cow::from("UnknownConnector"),
            Self::UnknownTenant { .. } => Cow::from("UnknownTenant"),
            Self::UnknownAttachedConnector { .. } => Cow::from("UnknownAttachedConnector"),
            Self::UnknownName { .. } => Cow::from("UnknownName"),
            Self::DuplicateName => Cow::from("DuplicateName"),
            Self::DuplicateKey { .. } => Cow::from("DuplicateKey"),
            Self::InvalidKey => Cow::from("InvalidKey"),
            Self::UniqueKeyViolation { .. } => Cow::from("UniqueKeyViolation"),
            Self::UnknownPipelineStatus { .. } => Cow::from("UnknownPipelineStatus"),
            Self::ProgramNotSet => Cow::from("ProgramNotSet"),
            Self::ProgramNotCompiled => Cow::from("ProgramNotCompiled"),
            Self::ProgramFailedToCompile => Cow::from("ProgramFailedToCompile"),
            Self::NoRevisionAvailable { .. } => Cow::from("NoRevisionAvailable"),
            Self::RevisionNotChanged => Cow::from("RevisionNotChanged"),
            Self::TablesNotInSchema { .. } => Cow::from("TablesNotInSchema"),
            Self::ViewsNotInSchema { .. } => Cow::from("ViewsNotInSchema"),
        }
    }
}

impl StdError for DBError {}

impl ResponseError for DBError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::PostgresError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PostgresPoolError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PostgresMigrationError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(feature = "pg-embed")]
            Self::PgEmbedError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidData { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidStatus { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::UnknownProgram { .. } => StatusCode::NOT_FOUND,
            Self::DuplicateName => StatusCode::CONFLICT,
            Self::OutdatedProgramVersion { .. } => StatusCode::CONFLICT,
            Self::UnknownPipeline { .. } => StatusCode::NOT_FOUND,
            Self::UnknownConnector { .. } => StatusCode::NOT_FOUND,
            // TODO: should we report not found instead?
            Self::UnknownTenant { .. } => StatusCode::UNAUTHORIZED,
            Self::UnknownAttachedConnector { .. } => StatusCode::NOT_FOUND,
            // This error should never bubble up till here
            Self::DuplicateKey { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidKey => StatusCode::UNAUTHORIZED,
            Self::UnknownName { .. } => StatusCode::NOT_FOUND,
            Self::ProgramNotCompiled => StatusCode::SERVICE_UNAVAILABLE,
            Self::ProgramFailedToCompile => StatusCode::BAD_REQUEST,
            Self::ProgramNotSet => StatusCode::BAD_REQUEST,
            // should in practice not happen, e.g., would mean a Uuid conflict:
            Self::UniqueKeyViolation { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            // should in practice not happen, e.g., would mean invalid status in db:
            Self::UnknownPipelineStatus { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NoRevisionAvailable { .. } => StatusCode::NOT_FOUND,
            Self::RevisionNotChanged => StatusCode::BAD_REQUEST,
            Self::TablesNotInSchema { .. } => StatusCode::BAD_REQUEST,
            Self::ViewsNotInSchema { .. } => StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse<BoxBody> {
        HttpResponseBuilder::new(self.status_code()).json(ErrorResponse::from_error(self))
    }
}
