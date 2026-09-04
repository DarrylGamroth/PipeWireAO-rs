//! Versioned owner-mediated processing-node run control.

use spa::param::ParamType;
use spa::pod::deserialize::PodDeserializer;
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Pod, Property, Value};
use spa::utils::SpaTypes;
use std::fmt;
use std::io::Cursor;

/// Version of the request/status wire contract implemented by this module.
pub const VERSION: i32 = 1;
/// Node property advertising support for owner-mediated run control.
pub const ENABLED_PROPERTY: &str = "pipewireao.run-control";

const VERSION_KEY: &str = "pipewireao.run-control.version";
const REQUEST_TOKEN_KEY: &str = "pipewireao.run-control.request-token";
const REQUESTED_STATE_KEY: &str = "pipewireao.run-control.requested-state";
const COMPLETED_TOKEN_KEY: &str = "pipewireao.run-control.completed-token";
const RESULT_KEY: &str = "pipewireao.run-control.result";
const ACTUAL_STATE_KEY: &str = "pipewireao.run-control.actual-state";

/// Stable processing state carried by the Version 1 contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunState {
    Unknown,
    Stopped,
    Running,
}

impl RunState {
    fn wire_name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Stopped => "stopped",
            Self::Running => "running",
        }
    }

    fn parse(value: &str) -> Result<Self, RunControlError> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "stopped" => Ok(Self::Stopped),
            "running" => Ok(Self::Running),
            _ => Err(RunControlError::InvalidState(value.to_owned())),
        }
    }
}

/// One controller request decoded from `SPA_PARAM_Props`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunControlRequest {
    pub token: i64,
    pub requested_state: RunState,
}

/// One owner completion decoded from `SPA_PARAM_Props`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunControlStatus {
    pub completed_token: i64,
    pub result: i32,
    pub actual_state: RunState,
}

/// A structural or semantic run-control POD error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunControlError {
    NotRunControl,
    InvalidObject,
    InvalidFields,
    InvalidToken(i64),
    InvalidState(String),
    UnsupportedVersion(i32),
    Serialization(String),
}

impl fmt::Display for RunControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRunControl => formatter.write_str("Props is not a run-control parameter"),
            Self::InvalidObject => formatter.write_str("run-control Props object is invalid"),
            Self::InvalidFields => formatter.write_str("run-control fields are invalid"),
            Self::InvalidToken(token) => write!(formatter, "run-control token {token} is invalid"),
            Self::InvalidState(state) => {
                write!(formatter, "run-control state {state:?} is invalid")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "run-control version {version} is unsupported")
            }
            Self::Serialization(message) => {
                write!(formatter, "cannot serialize run-control Props: {message}")
            }
        }
    }
}

impl std::error::Error for RunControlError {}

/// Encode a Version 1 running or stopped request.
pub fn build_request(token: i64, state: RunState) -> Result<Vec<u8>, RunControlError> {
    if token <= 0 {
        return Err(RunControlError::InvalidToken(token));
    }
    if state == RunState::Unknown {
        return Err(RunControlError::InvalidState(state.wire_name().to_owned()));
    }
    encode(vec![
        Value::String(VERSION_KEY.to_owned()),
        Value::Int(VERSION),
        Value::String(REQUEST_TOKEN_KEY.to_owned()),
        Value::Long(token),
        Value::String(REQUESTED_STATE_KEY.to_owned()),
        Value::String(state.wire_name().to_owned()),
    ])
}

/// Decode and strictly validate a Version 1 request.
pub fn parse_request(pod: &Pod) -> Result<RunControlRequest, RunControlError> {
    let fields = fields(pod)?;
    if fields.len() != 6 {
        return Err(RunControlError::InvalidFields);
    }
    let version = pair_int(&fields, 0, VERSION_KEY)?;
    validate_version(version)?;
    let token = pair_long(&fields, 2, REQUEST_TOKEN_KEY)?;
    if token <= 0 {
        return Err(RunControlError::InvalidToken(token));
    }
    let requested_state = RunState::parse(pair_string(&fields, 4, REQUESTED_STATE_KEY)?)?;
    if requested_state == RunState::Unknown {
        return Err(RunControlError::InvalidState("unknown".to_owned()));
    }
    Ok(RunControlRequest {
        token,
        requested_state,
    })
}

/// Decode and strictly validate a Version 1 completion status.
pub fn parse_status(pod: &Pod) -> Result<RunControlStatus, RunControlError> {
    let fields = fields(pod)?;
    if fields.len() != 8 {
        return Err(RunControlError::InvalidFields);
    }
    let version = pair_int(&fields, 0, VERSION_KEY)?;
    validate_version(version)?;
    let completed_token = pair_long(&fields, 2, COMPLETED_TOKEN_KEY)?;
    if completed_token < 0 {
        return Err(RunControlError::InvalidToken(completed_token));
    }
    let result = pair_int(&fields, 4, RESULT_KEY)?;
    let actual_state = RunState::parse(pair_string(&fields, 6, ACTUAL_STATE_KEY)?)?;
    Ok(RunControlStatus {
        completed_token,
        result,
        actual_state,
    })
}

fn encode(fields: Vec<Value>) -> Result<Vec<u8>, RunControlError> {
    let value = Value::Object(Object {
        type_: SpaTypes::ObjectParamProps.as_raw(),
        id: ParamType::Props.as_raw(),
        properties: vec![Property::new(
            spa::sys::SPA_PROP_params,
            Value::Struct(fields),
        )],
    });
    PodSerializer::serialize(Cursor::new(Vec::new()), &value)
        .map(|result| result.0.into_inner())
        .map_err(|error| RunControlError::Serialization(format!("{error:?}")))
}

fn fields(pod: &Pod) -> Result<Vec<Value>, RunControlError> {
    let (_, value) = PodDeserializer::deserialize_from::<Value>(pod.as_bytes())
        .map_err(|_| RunControlError::InvalidObject)?;
    let Value::Object(object) = value else {
        return Err(RunControlError::InvalidObject);
    };
    if object.type_ != SpaTypes::ObjectParamProps.as_raw()
        || object.id != ParamType::Props.as_raw()
        || object.properties.len() != 1
        || object.properties[0].key != spa::sys::SPA_PROP_params
    {
        return Err(RunControlError::InvalidObject);
    }
    let Some(property) = object.properties.into_iter().next() else {
        return Err(RunControlError::InvalidObject);
    };
    let Value::Struct(fields) = property.value else {
        return Err(RunControlError::InvalidFields);
    };
    let has_run_control_key = fields.iter().step_by(2).any(
        |field| matches!(field, Value::String(key) if key.starts_with("pipewireao.run-control.")),
    );
    if !has_run_control_key {
        return Err(RunControlError::NotRunControl);
    }
    Ok(fields)
}

fn pair_int(fields: &[Value], index: usize, expected_key: &str) -> Result<i32, RunControlError> {
    match (fields.get(index), fields.get(index + 1)) {
        (Some(Value::String(key)), Some(Value::Int(value))) if key == expected_key => Ok(*value),
        _ => Err(RunControlError::InvalidFields),
    }
}

fn pair_long(fields: &[Value], index: usize, expected_key: &str) -> Result<i64, RunControlError> {
    match (fields.get(index), fields.get(index + 1)) {
        (Some(Value::String(key)), Some(Value::Long(value))) if key == expected_key => Ok(*value),
        _ => Err(RunControlError::InvalidFields),
    }
}

fn pair_string<'a>(
    fields: &'a [Value],
    index: usize,
    expected_key: &str,
) -> Result<&'a str, RunControlError> {
    match (fields.get(index), fields.get(index + 1)) {
        (Some(Value::String(key)), Some(Value::String(value))) if key == expected_key => Ok(value),
        _ => Err(RunControlError::InvalidFields),
    }
}

fn validate_version(version: i32) -> Result<(), RunControlError> {
    if version == VERSION {
        Ok(())
    } else {
        Err(RunControlError::UnsupportedVersion(version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pod(bytes: &[u8]) -> &Pod {
        Pod::from_bytes(bytes).expect("serialized POD")
    }

    #[test]
    fn request_round_trip() {
        let bytes = build_request(42, RunState::Running).expect("request");
        assert_eq!(
            parse_request(pod(&bytes)),
            Ok(RunControlRequest {
                token: 42,
                requested_state: RunState::Running,
            })
        );
        assert!(matches!(
            build_request(0, RunState::Running),
            Err(RunControlError::InvalidToken(0))
        ));
        assert!(matches!(
            build_request(1, RunState::Unknown),
            Err(RunControlError::InvalidState(_))
        ));
    }

    #[test]
    fn status_is_strictly_validated() {
        let status = encode(vec![
            Value::String(VERSION_KEY.to_owned()),
            Value::Int(VERSION),
            Value::String(COMPLETED_TOKEN_KEY.to_owned()),
            Value::Long(42),
            Value::String(RESULT_KEY.to_owned()),
            Value::Int(-5),
            Value::String(ACTUAL_STATE_KEY.to_owned()),
            Value::String("stopped".to_owned()),
        ])
        .expect("status");
        assert_eq!(
            parse_status(pod(&status)),
            Ok(RunControlStatus {
                completed_token: 42,
                result: -5,
                actual_state: RunState::Stopped,
            })
        );

        let wrong_version = encode(vec![
            Value::String(VERSION_KEY.to_owned()),
            Value::Int(2),
            Value::String(COMPLETED_TOKEN_KEY.to_owned()),
            Value::Long(42),
            Value::String(RESULT_KEY.to_owned()),
            Value::Int(0),
            Value::String(ACTUAL_STATE_KEY.to_owned()),
            Value::String("running".to_owned()),
        ])
        .expect("wrong-version status");
        assert_eq!(
            parse_status(pod(&wrong_version)),
            Err(RunControlError::UnsupportedVersion(2))
        );
    }
}
