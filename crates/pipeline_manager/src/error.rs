//! This module implements [`enum ManagerError`], the top-level error
//! type returned by HTTP endpoints of the pipeline manager.
//!
//! At a high level, the error hierarchy looks like this:
//!
//! ```text
//! ManagerError (this module)
//! ├─ db::DBError (errors returned by the `db` module)
//! ├─ runner::RunnerError (errors retunred by the `runner` module)
//! ```
//!
//! We implement the `actix-web` `ResponseError` trait for all error types,
//! which allows them to be returned as error responses by HTTP endpoints.
//! Our `ResponseError` implementation generates an HTTP response whose body
//! is a JSON serialization of the
//! [`ErrorResponse`](`dbsp_adapters::ErrorResponse`) type from the
//! `dbsp_adapters` crate, i.e., errors returned by the pipeline manager and
//! by individual pipelines have the same format.

use crate::db::DBError;
use crate::runner::RunnerError;
use actix_web::{
    body::BoxBody, http::StatusCode, HttpResponse, HttpResponseBuilder, ResponseError,
};
use dbsp_adapters::{DetailedError, ErrorResponse};
use serde::{ser::SerializeStruct, Serialize, Serializer};
use std::{
    backtrace::Backtrace,
    borrow::Cow,
    error::Error as StdError,
    fmt::{Display, Error as FmtError, Formatter},
    io::Error as IOError,
};

/// Pipeline manager errors.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ManagerError {
    ProgramNotSpecified,
    PipelineNotSpecified,
    ConnectorNotSpecified,
    // I don't think this can ever happen.
    MissingUrlEncodedParam {
        param: &'static str,
    },
    InvalidUuidParam {
        value: String,
        error: String,
    },
    InvalidPipelineAction {
        action: String,
    },
    DBError {
        #[serde(flatten)]
        db_error: DBError,
    },
    RunnerError {
        #[serde(flatten)]
        runner_error: RunnerError,
    },
    #[serde(serialize_with = "serialize_io_error")]
    IoError {
        context: String,
        io_error: IOError,
        backtrace: Backtrace,
    },
    #[serde(serialize_with = "serialize_invalid_program_schema")]
    InvalidProgramSchema {
        error: String,
        backtrace: Backtrace,
    },
    RustCompilerError {
        error: String,
    },
}

impl ManagerError {
    pub fn io_error(context: String, io_error: IOError) -> Self {
        Self::IoError {
            context,
            io_error,
            backtrace: Backtrace::capture(),
        }
    }

    pub fn invalid_program_schema(error: String) -> Self {
        Self::InvalidProgramSchema {
            error,
            backtrace: Backtrace::capture(),
        }
    }
}

fn serialize_io_error<S>(
    context: &String,
    io_error: &IOError,
    backtrace: &Backtrace,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("IoError", 4)?;
    ser.serialize_field("context", &context)?;
    ser.serialize_field("kind", &io_error.kind().to_string())?;
    ser.serialize_field("os_error", &io_error.raw_os_error())?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

fn serialize_invalid_program_schema<S>(
    error: &String,
    backtrace: &Backtrace,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("InvalidProgramSchema", 2)?;
    ser.serialize_field("error", &error)?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

impl StdError for ManagerError {}

impl From<DBError> for ManagerError {
    fn from(db_error: DBError) -> Self {
        Self::DBError { db_error }
    }
}

impl From<RunnerError> for ManagerError {
    fn from(runner_error: RunnerError) -> Self {
        Self::RunnerError { runner_error }
    }
}

impl Display for ManagerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        match self {
            Self::ProgramNotSpecified => {
                f.write_str("Program not specified. Use ?id or ?name query strings in the URL.")
            }
            Self::PipelineNotSpecified => {
                f.write_str("Pipeline not specified. Use ?id or ?name query strings in the URL.")
            }
            Self::ConnectorNotSpecified => {
                f.write_str("Connector not specified. Use ?id or ?name query strings in the URL.")
            }
            Self::MissingUrlEncodedParam { param } => {
                write!(f, "Missing URL-encoded parameter '{param}'")
            }
            Self::InvalidUuidParam { value, error } => {
                write!(f, "Invalid UUID string '{value}': '{error}'")
            }
            Self::InvalidPipelineAction { action } => {
                write!(f, "Invalid pipeline action '{action}'; valid actions are: 'deploy', 'start', 'pause', or 'shutdown'")
            }
            Self::DBError { db_error } => db_error.fmt(f),
            Self::RunnerError { runner_error } => runner_error.fmt(f),
            Self::IoError {
                context, io_error, ..
            } => {
                write!(f, "I/O error {context}: {io_error}")
            }
            Self::InvalidProgramSchema { error, .. } => {
                write!(f, "Error parsing program schema: {error}")
            }
            Self::RustCompilerError { error } => {
                write!(f, "Error compiling generated Rust code: {error}")
            }
        }
    }
}

impl ResponseError for ManagerError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ProgramNotSpecified => StatusCode::BAD_REQUEST,
            Self::PipelineNotSpecified => StatusCode::BAD_REQUEST,
            Self::ConnectorNotSpecified => StatusCode::BAD_REQUEST,
            Self::MissingUrlEncodedParam { .. } => StatusCode::BAD_REQUEST,
            Self::InvalidUuidParam { .. } => StatusCode::BAD_REQUEST,
            Self::InvalidPipelineAction { .. } => StatusCode::BAD_REQUEST,
            Self::DBError { db_error } => db_error.status_code(),
            Self::RunnerError { runner_error } => runner_error.status_code(),
            Self::IoError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidProgramSchema { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::RustCompilerError { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse<BoxBody> {
        HttpResponseBuilder::new(self.status_code()).json(ErrorResponse::from_error(self))
    }
}

impl DetailedError for ManagerError {
    fn error_code(&self) -> Cow<'static, str> {
        match self {
            Self::ProgramNotSpecified => Cow::from("ProgramNotSpecified"),
            Self::PipelineNotSpecified => Cow::from("PipelineNotSpecified"),
            Self::ConnectorNotSpecified => Cow::from("ConnectorNotSpecified"),
            Self::MissingUrlEncodedParam { .. } => Cow::from("MissingUrlEncodedParam"),
            Self::InvalidUuidParam { .. } => Cow::from("InvalidUuidParam"),
            Self::InvalidPipelineAction { .. } => Cow::from("InvalidPipelineAction"),
            Self::DBError { db_error } => db_error.error_code(),
            Self::RunnerError { runner_error } => runner_error.error_code(),
            Self::IoError { .. } => Cow::from("ManagerIoError"),
            Self::InvalidProgramSchema { .. } => Cow::from("InvalidProgramSchema"),
            Self::RustCompilerError { .. } => Cow::from("RustCompilerError"),
        }
    }
}
