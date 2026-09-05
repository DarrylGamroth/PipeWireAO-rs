//! Versioned owner-mediated processing-state reset control.

use spa::param::ParamType;
use spa::pod::deserialize::PodDeserializer;
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Pod, Property, Value};
use spa::utils::SpaTypes;
use std::fmt;
use std::io::Cursor;

/// Version of the request/status wire contract implemented by this module.
pub const VERSION: i32 = 1;
/// Node property advertising support for owner-mediated reset.
pub const ENABLED_PROPERTY: &str = "pipewireao.reset-control";

const VERSION_KEY: &str = "pipewireao.reset-control.version";
const REQUEST_TOKEN_KEY: &str = "pipewireao.reset-control.request-token";
const COMPLETED_TOKEN_KEY: &str = "pipewireao.reset-control.completed-token";
const RESULT_KEY: &str = "pipewireao.reset-control.result";

/// One controller request decoded from `SPA_PARAM_Props`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResetControlRequest {
    pub token: i64,
}

/// One owner completion decoded from `SPA_PARAM_Props`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResetControlStatus {
    pub completed_token: i64,
    pub result: i32,
}

/// A structural or semantic reset-control POD error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResetControlError {
    NotResetControl,
    InvalidObject,
    InvalidFields,
    InvalidToken(i64),
    UnsupportedVersion(i32),
    Serialization(String),
}

impl fmt::Display for ResetControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotResetControl => formatter.write_str("Props is not a reset-control parameter"),
            Self::InvalidObject => formatter.write_str("reset-control Props object is invalid"),
            Self::InvalidFields => formatter.write_str("reset-control fields are invalid"),
            Self::InvalidToken(token) => {
                write!(formatter, "reset-control token {token} is invalid")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "reset-control version {version} is unsupported")
            }
            Self::Serialization(message) => {
                write!(formatter, "cannot serialize reset-control Props: {message}")
            }
        }
    }
}

impl std::error::Error for ResetControlError {}

/// Encode a Version 1 reset request.
pub fn build_request(token: i64) -> Result<Vec<u8>, ResetControlError> {
    if token <= 0 {
        return Err(ResetControlError::InvalidToken(token));
    }
    encode(vec![
        Value::String(VERSION_KEY.to_owned()),
        Value::Int(VERSION),
        Value::String(REQUEST_TOKEN_KEY.to_owned()),
        Value::Long(token),
    ])
}

/// Decode and strictly validate a Version 1 reset request.
pub fn parse_request(pod: &Pod) -> Result<ResetControlRequest, ResetControlError> {
    let fields = fields(pod)?;
    if fields.len() != 4 {
        return Err(ResetControlError::InvalidFields);
    }
    validate_version(pair_int(&fields, 0, VERSION_KEY)?)?;
    let token = pair_long(&fields, 2, REQUEST_TOKEN_KEY)?;
    if token <= 0 {
        return Err(ResetControlError::InvalidToken(token));
    }
    Ok(ResetControlRequest { token })
}

/// Decode and strictly validate a Version 1 reset completion.
pub fn parse_status(pod: &Pod) -> Result<ResetControlStatus, ResetControlError> {
    let fields = fields(pod)?;
    if fields.len() != 6 {
        return Err(ResetControlError::InvalidFields);
    }
    validate_version(pair_int(&fields, 0, VERSION_KEY)?)?;
    let completed_token = pair_long(&fields, 2, COMPLETED_TOKEN_KEY)?;
    if completed_token < 0 {
        return Err(ResetControlError::InvalidToken(completed_token));
    }
    let result = pair_int(&fields, 4, RESULT_KEY)?;
    Ok(ResetControlStatus {
        completed_token,
        result,
    })
}

fn encode(fields: Vec<Value>) -> Result<Vec<u8>, ResetControlError> {
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
        .map_err(|error| ResetControlError::Serialization(format!("{error:?}")))
}

fn fields(pod: &Pod) -> Result<Vec<Value>, ResetControlError> {
    let (_, value) = PodDeserializer::deserialize_from::<Value>(pod.as_bytes())
        .map_err(|_| ResetControlError::InvalidObject)?;
    let Value::Object(object) = value else {
        return Err(ResetControlError::InvalidObject);
    };
    if object.type_ != SpaTypes::ObjectParamProps.as_raw()
        || object.id != ParamType::Props.as_raw()
        || object.properties.len() != 1
        || object.properties[0].key != spa::sys::SPA_PROP_params
    {
        return Err(ResetControlError::InvalidObject);
    }
    let Some(property) = object.properties.into_iter().next() else {
        return Err(ResetControlError::InvalidObject);
    };
    let Value::Struct(fields) = property.value else {
        return Err(ResetControlError::InvalidFields);
    };
    let has_reset_control_key = fields.iter().step_by(2).any(
        |field| matches!(field, Value::String(key) if key.starts_with("pipewireao.reset-control.")),
    );
    if !has_reset_control_key {
        return Err(ResetControlError::NotResetControl);
    }
    Ok(fields)
}

fn pair_int(fields: &[Value], index: usize, expected_key: &str) -> Result<i32, ResetControlError> {
    match (fields.get(index), fields.get(index + 1)) {
        (Some(Value::String(key)), Some(Value::Int(value))) if key == expected_key => Ok(*value),
        _ => Err(ResetControlError::InvalidFields),
    }
}

fn pair_long(fields: &[Value], index: usize, expected_key: &str) -> Result<i64, ResetControlError> {
    match (fields.get(index), fields.get(index + 1)) {
        (Some(Value::String(key)), Some(Value::Long(value))) if key == expected_key => Ok(*value),
        _ => Err(ResetControlError::InvalidFields),
    }
}

fn validate_version(version: i32) -> Result<(), ResetControlError> {
    if version == VERSION {
        Ok(())
    } else {
        Err(ResetControlError::UnsupportedVersion(version))
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
        let bytes = build_request(42).expect("request");
        assert_eq!(
            parse_request(pod(&bytes)),
            Ok(ResetControlRequest { token: 42 })
        );
        assert_eq!(build_request(0), Err(ResetControlError::InvalidToken(0)));
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
        ])
        .expect("status");
        assert_eq!(
            parse_status(pod(&status)),
            Ok(ResetControlStatus {
                completed_token: 42,
                result: -5,
            })
        );

        let wrong_version = encode(vec![
            Value::String(VERSION_KEY.to_owned()),
            Value::Int(2),
            Value::String(COMPLETED_TOKEN_KEY.to_owned()),
            Value::Long(42),
            Value::String(RESULT_KEY.to_owned()),
            Value::Int(0),
        ])
        .expect("wrong-version status");
        assert_eq!(
            parse_status(pod(&wrong_version)),
            Err(ResetControlError::UnsupportedVersion(2))
        );
    }
}
