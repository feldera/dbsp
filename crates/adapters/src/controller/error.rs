use crate::DetailedError;
use anyhow::Error as AnyError;
use dbsp::Error as DBSPError;
use serde::{ser::SerializeStruct, Serialize, Serializer};
use std::{
    backtrace::Backtrace,
    borrow::Cow,
    error::Error as StdError,
    fmt::{Display, Error as FmtError, Formatter},
    io::Error as IOError,
    string::ToString,
};

/// Controller configuration error.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ConfigError {
    /// Failed to parse pipeline configuration.
    ConfigParseError { error: String },

    /// Input endpoint with this name already exists.
    DuplicateInputEndpoint { endpoint_name: String },

    /// Output endpoint with this name already exists.
    DuplicateOutputEndpoint { endpoint_name: String },

    /// Endpoint configuration specifies unknown input format name.
    UnknownInputFormat { format_name: String },

    /// Endpoint configuration specifies unknown output format name.
    UnknownOutputFormat { format_name: String },

    /// Endpoint configuration specifies unknown input transport name.
    UnknownInputTransport { transport_name: String },

    /// Endpoint configuration specifies unknown output transport name.
    UnknownOutputTransport { transport_name: String },

    /// Endpoint configuration specifies an input stream name
    /// that is not found in the circuit catalog.
    UnknownInputStream { stream_name: String },

    /// Endpoint configuration specifies an output stream name
    /// that is not found in the circuit catalog.
    UnknownOutputStream { stream_name: String },
}

impl StdError for ConfigError {}

impl DetailedError for ConfigError {
    fn error_code(&self) -> Cow<'static, str> {
        match self {
            Self::ConfigParseError { .. } => Cow::from("ConfigParseError"),
            Self::DuplicateInputEndpoint { .. } => Cow::from("DuplicateInputEndpoint"),
            Self::DuplicateOutputEndpoint { .. } => Cow::from("DuplicateOutputEndpoint"),
            Self::UnknownInputFormat { .. } => Cow::from("UnknownInputFormat"),
            Self::UnknownOutputFormat { .. } => Cow::from("UnknownOutputFormat"),
            Self::UnknownInputTransport { .. } => Cow::from("UnknownInputTransport"),
            Self::UnknownOutputTransport { .. } => Cow::from("UnknownOutputTransport"),
            Self::UnknownInputStream { .. } => Cow::from("UnknownInputStream"),
            Self::UnknownOutputStream { .. } => Cow::from("UnknownOutputStream"),
        }
    }
}

impl Display for ConfigError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        match self {
            Self::ConfigParseError { error } => {
                write!(f, "Failed to parse pipeline configuration: '{error}'")
            }
            Self::DuplicateInputEndpoint { endpoint_name } => {
                write!(f, "Input endpoint '{endpoint_name}' already exists")
            }
            Self::UnknownInputFormat { format_name } => {
                write!(f, "Unknown input format '{format_name}'")
            }
            Self::UnknownInputTransport { transport_name } => {
                write!(f, "Unknown input transport '{transport_name}'")
            }
            Self::DuplicateOutputEndpoint { endpoint_name } => {
                write!(f, "Output endpoint '{endpoint_name}' already exists")
            }
            Self::UnknownOutputFormat { format_name } => {
                write!(f, "Unknown output format '{format_name}'")
            }
            Self::UnknownOutputTransport { transport_name } => {
                write!(f, "Unknown output transport '{transport_name}'")
            }
            Self::UnknownInputStream { stream_name } => {
                write!(f, "Unknown table '{stream_name}'")
            }
            Self::UnknownOutputStream { stream_name } => {
                write!(f, "Unknown output table or view '{stream_name}'")
            }
        }
    }
}

impl ConfigError {
    pub fn config_parse_error<E>(error: &E) -> Self
    where
        E: ToString,
    {
        Self::ConfigParseError {
            error: error.to_string(),
        }
    }

    pub fn duplicate_input_endpoint(endpoint_name: &str) -> Self {
        Self::DuplicateInputEndpoint {
            endpoint_name: endpoint_name.to_owned(),
        }
    }

    pub fn unknown_input_format(format_name: &str) -> Self {
        Self::UnknownInputFormat {
            format_name: format_name.to_owned(),
        }
    }

    pub fn unknown_input_transport(transport_name: &str) -> Self {
        Self::UnknownInputTransport {
            transport_name: transport_name.to_owned(),
        }
    }

    pub fn duplicate_output_endpoint(endpoint_name: &str) -> Self {
        Self::DuplicateOutputEndpoint {
            endpoint_name: endpoint_name.to_owned(),
        }
    }

    pub fn unknown_output_format(format_name: &str) -> Self {
        Self::UnknownOutputFormat {
            format_name: format_name.to_owned(),
        }
    }

    pub fn unknown_output_transport(transport_name: &str) -> Self {
        Self::UnknownOutputTransport {
            transport_name: transport_name.to_owned(),
        }
    }

    pub fn unknown_input_stream(stream_name: &str) -> Self {
        Self::UnknownInputStream {
            stream_name: stream_name.to_owned(),
        }
    }

    pub fn unknown_output_stream(stream_name: &str) -> Self {
        Self::UnknownOutputStream {
            stream_name: stream_name.to_owned(),
        }
    }
}

/// Controller error.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ControllerError {
    /// I/O error.
    #[serde(serialize_with = "serialize_io_error")]
    IoError {
        /// Describes the context where the error occurred.
        context: String,
        io_error: IOError,
        backtrace: Backtrace,
    },

    /// Error parsing CLI arguments.
    CliArgsError { error: String },

    /// Invalid controller configuration.
    Config { config_error: ConfigError },

    /// Parser error.
    ///
    /// Error parsing the last input batch.  Parser errors are expected to be
    /// recoverable, i.e., the parser should be able to successfully parse
    /// new valid inputs after an error.
    #[serde(serialize_with = "serialize_parse_error")]
    ParseError {
        endpoint_name: String,
        error: AnyError,
    },

    /// Encode error.
    ///
    /// Error encoding the last output batch.  Encoder errors are expected to
    /// be recoverable, i.e., the encoder should be able to successfully parse
    /// new valid inputs after an error.
    #[serde(serialize_with = "serialize_encode_error")]
    EncodeError {
        endpoint_name: String,
        error: AnyError,
    },

    /// Input transport endpoint error.
    #[serde(serialize_with = "serialize_input_transport_error")]
    InputTransportError {
        endpoint_name: String,
        fatal: bool,
        error: AnyError,
    },

    /// Output transport endpoint error.
    #[serde(serialize_with = "serialize_output_transport_error")]
    OutputTransportError {
        endpoint_name: String,
        fatal: bool,
        error: AnyError,
    },

    /// Error evaluating the DBSP circuit.
    DbspError { error: DBSPError },

    /// Error inside the Prometheus module.
    PrometheusError { error: String },

    // TODO: we currently don't have a way to include more info about the panic.
    /// Panic inside the DBSP runtime.
    DbspPanic,

    /// Panic inside the DBSP controller.
    ControllerPanic,
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
    let mut ser = serializer.serialize_struct("IOError", 4)?;
    ser.serialize_field("context", context)?;
    ser.serialize_field("kind", &io_error.kind().to_string())?;
    ser.serialize_field("os_error", &io_error.raw_os_error())?;
    ser.serialize_field("backtrace", &backtrace.to_string())?;
    ser.end()
}

fn serialize_parse_error<S>(
    endpoint: &String,
    error: &AnyError,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("ParseError", 3)?;
    ser.serialize_field("endpoint_name", endpoint)?;
    ser.serialize_field("error", &error.to_string())?;
    ser.serialize_field("backtrace", &error.backtrace().to_string())?;
    ser.end()
}

fn serialize_encode_error<S>(
    endpoint: &String,
    error: &AnyError,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("EncodeError", 3)?;
    ser.serialize_field("endpoint_name", endpoint)?;
    ser.serialize_field("error", &error.to_string())?;
    ser.serialize_field("backtrace", &error.backtrace().to_string())?;
    ser.end()
}

fn serialize_input_transport_error<S>(
    endpoint: &String,
    fatal: &bool,
    error: &AnyError,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("InputTransportError", 4)?;
    ser.serialize_field("endpoint_name", endpoint)?;
    ser.serialize_field("fatal", fatal)?;
    ser.serialize_field("error", &error.to_string())?;
    ser.serialize_field("backtrace", &error.backtrace().to_string())?;
    ser.end()
}

fn serialize_output_transport_error<S>(
    endpoint: &String,
    fatal: &bool,
    error: &AnyError,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut ser = serializer.serialize_struct("OutputTransportError", 4)?;
    ser.serialize_field("endpoint_name", endpoint)?;
    ser.serialize_field("fatal", fatal)?;
    ser.serialize_field("error", &error.to_string())?;
    ser.serialize_field("backtrace", &error.backtrace().to_string())?;
    ser.end()
}

impl DetailedError for ControllerError {
    // TODO: attempts to cast `AnyError` to `DetailedError`.
    fn error_code(&self) -> Cow<'static, str> {
        match self {
            Self::IoError { .. } => Cow::from("ControllerIOError"),
            Self::CliArgsError { .. } => Cow::from("ControllerCliArgsError"),
            Self::Config { config_error } => {
                Cow::from(format!("ConfigError.{}", config_error.error_code()))
            }
            Self::ParseError { .. } => Cow::from("ParseError"),
            Self::EncodeError { .. } => Cow::from("EncodeError"),
            Self::InputTransportError { .. } => Cow::from("InputTransportError"),
            Self::OutputTransportError { .. } => Cow::from("OutputTransportError"),
            Self::PrometheusError { .. } => Cow::from("PrometheusError"),
            Self::DbspError { error } => error.error_code(),
            Self::DbspPanic => Cow::from("DbspPanic"),
            Self::ControllerPanic => Cow::from("ControllerPanic"),
        }
    }
}

impl StdError for ControllerError {}

impl Display for ControllerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        match self {
            Self::IoError {
                context, io_error, ..
            } => {
                write!(f, "I/O error {context}: {io_error}")
            }
            Self::CliArgsError { error } => {
                write!(f, "Error parsing command line arguments: '{error}'")
            }
            Self::Config { config_error } => {
                write!(f, "invalid controller configuration: '{config_error}'")
            }
            Self::InputTransportError {
                endpoint_name,
                fatal,
                error,
            } => {
                write!(
                    f,
                    "{}error on input endpoint '{endpoint_name}': '{error}'",
                    if *fatal { "FATAL " } else { "" }
                )
            }
            Self::OutputTransportError {
                endpoint_name,
                fatal,
                error,
            } => {
                write!(
                    f,
                    "{}error on output endpoint '{endpoint_name}': '{error}'",
                    if *fatal { "FATAL " } else { "" }
                )
            }
            Self::ParseError {
                endpoint_name,
                error,
            } => {
                write!(
                    f,
                    "parse error on input endpoint '{endpoint_name}': '{error}'"
                )
            }
            Self::EncodeError {
                endpoint_name,
                error,
            } => {
                write!(
                    f,
                    "encoder error on output endpoint '{endpoint_name}': '{error}'"
                )
            }
            Self::PrometheusError { error } => {
                write!(f, "Error in the Prometheus metrics module: '{error}'")
            }
            Self::DbspError { error } => {
                write!(f, "DBSP error: '{error}'")
            }
            Self::DbspPanic => {
                write!(f, "Panic inside the DBSP runtime")
            }
            Self::ControllerPanic => {
                write!(f, "Panic inside the DBSP controller")
            }
        }
    }
}

impl ControllerError {
    pub fn io_error(context: String, io_error: IOError) -> Self {
        Self::IoError {
            context,
            io_error,
            backtrace: Backtrace::capture(),
        }
    }

    pub fn cli_args_error<E>(error: &E) -> Self
    where
        E: ToString,
    {
        Self::CliArgsError {
            error: error.to_string(),
        }
    }

    pub fn config_parse_error<E>(error: &E) -> Self
    where
        E: ToString,
    {
        Self::Config {
            config_error: ConfigError::config_parse_error(error),
        }
    }

    pub fn duplicate_input_endpoint(endpoint_name: &str) -> Self {
        Self::Config {
            config_error: ConfigError::duplicate_input_endpoint(endpoint_name),
        }
    }

    pub fn unknown_input_format(format_name: &str) -> Self {
        Self::Config {
            config_error: ConfigError::unknown_input_format(format_name),
        }
    }

    pub fn unknown_input_transport(transport_name: &str) -> Self {
        Self::Config {
            config_error: ConfigError::unknown_input_transport(transport_name),
        }
    }

    pub fn duplicate_output_endpoint(endpoint_name: &str) -> Self {
        Self::Config {
            config_error: ConfigError::duplicate_output_endpoint(endpoint_name),
        }
    }

    pub fn unknown_output_format(format_name: &str) -> Self {
        Self::Config {
            config_error: ConfigError::unknown_output_format(format_name),
        }
    }

    pub fn unknown_output_transport(transport_name: &str) -> Self {
        Self::Config {
            config_error: ConfigError::unknown_output_transport(transport_name),
        }
    }

    pub fn unknown_input_stream(stream_name: &str) -> Self {
        Self::Config {
            config_error: ConfigError::unknown_input_stream(stream_name),
        }
    }

    pub fn unknown_output_stream(stream_name: &str) -> Self {
        Self::Config {
            config_error: ConfigError::unknown_output_stream(stream_name),
        }
    }

    pub fn input_transport_error(endpoint_name: &str, fatal: bool, error: AnyError) -> Self {
        Self::InputTransportError {
            endpoint_name: endpoint_name.to_owned(),
            fatal,
            error,
        }
    }

    pub fn output_transport_error(endpoint_name: &str, fatal: bool, error: AnyError) -> Self {
        Self::OutputTransportError {
            endpoint_name: endpoint_name.to_owned(),
            fatal,
            error,
        }
    }

    pub fn parse_error(endpoint_name: &str, error: AnyError) -> Self {
        Self::ParseError {
            endpoint_name: endpoint_name.to_owned(),
            error,
        }
    }

    pub fn encode_error(endpoint_name: &str, error: AnyError) -> Self {
        Self::EncodeError {
            endpoint_name: endpoint_name.to_owned(),
            error,
        }
    }

    pub fn prometheus_error<E>(error: &E) -> Self
    where
        E: ToString,
    {
        Self::PrometheusError {
            error: error.to_string(),
        }
    }

    pub fn dbsp_error(error: DBSPError) -> Self {
        Self::DbspError { error }
    }

    pub fn dbsp_panic() -> Self {
        Self::DbspPanic
    }

    pub fn controller_panic() -> Self {
        Self::ControllerPanic
    }
}
