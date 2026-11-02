// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! Owned storage for SPA parameter lists.

use std::io::Cursor;

use crate::{
    pod::{serialize::PodSerializer, Object, Pod, Property, Value},
    utils::Id,
};

/// Serialized parameters retained long enough for a PipeWire API call.
pub struct Parameters(Vec<Vec<u8>>);

impl Parameters {
    /// Serializes values into owned POD storage.
    pub fn new(values: impl IntoIterator<Item = Value>) -> Self {
        Self(values.into_iter().map(serialize).collect())
    }

    /// Returns borrowed POD views valid for the lifetime of this collection.
    pub fn pods(&self) -> Vec<&Pod> {
        self.0
            .iter()
            .map(|bytes| Pod::from_bytes(bytes).expect("serializer produced an invalid POD"))
            .collect()
    }

    /// Replaces the range in an existing `SPA_PARAM_Buffers` parameter with
    /// one fixed buffer count.
    ///
    /// This does not allocate or resize a PipeWire buffer pool. The value is
    /// negotiated when the port is connected.
    pub fn with_fixed_buffer_count(mut self, count: u32) -> Result<Self, &'static str> {
        let count = i32::try_from(count).map_err(|_| "buffer count exceeds SPA Int")?;
        if count < 2 {
            return Err("buffer count must be at least two");
        }
        for bytes in &mut self.0 {
            let Ok((_, Value::Object(mut object))) =
                crate::pod::deserialize::PodDeserializer::deserialize_from::<Value>(bytes)
            else {
                continue;
            };
            if object.type_ != spa_sys::SPA_TYPE_OBJECT_ParamBuffers
                || object.id != spa_sys::SPA_PARAM_Buffers
            {
                continue;
            }
            let Some(buffers) = object
                .properties
                .iter_mut()
                .find(|property| property.key == spa_sys::SPA_PARAM_BUFFERS_buffers)
            else {
                return Err("SPA_PARAM_Buffers has no buffers property");
            };
            buffers.value = Value::Int(count);
            *bytes = serialize(Value::Object(object));
            return Ok(self);
        }
        Err("parameters do not contain SPA_PARAM_Buffers")
    }

    /// Request the version-one BufferProperties allocation on every buffer.
    pub fn with_buffer_properties(self, capacity: usize) -> Result<Self, &'static str> {
        if !(16..=4096).contains(&capacity) || capacity % 8 != 0 {
            return Err("Properties capacity must be 16 through 4096 and eight-byte aligned");
        }
        Ok(self.with_meta(
            spa_sys::SPA_META_Properties,
            capacity,
            Some(spa_sys::SPA_META_FEATURE_PROPERTIES_VERSION_1 as i32),
        ))
    }

    /// Removes the first metadata request for `meta_type`.
    pub fn without_meta(mut self, meta_type: u32) -> Result<Self, &'static str> {
        let Some(index) = self.0.iter().position(|bytes| {
            let Ok((_, Value::Object(object))) =
                crate::pod::deserialize::PodDeserializer::deserialize_from::<Value>(bytes)
            else {
                return false;
            };
            object.type_ == spa_sys::SPA_TYPE_OBJECT_ParamMeta
                && object.id == spa_sys::SPA_PARAM_Meta
                && object.properties.iter().any(|property| {
                    property.key == spa_sys::SPA_PARAM_META_type
                        && property.value == Value::Id(Id(meta_type))
                })
        }) else {
            return Err("parameters do not contain the requested metadata type");
        };
        self.0.remove(index);
        Ok(self)
    }

    fn with_meta(mut self, meta_type: u32, size: usize, features: Option<i32>) -> Self {
        let mut properties = vec![
            Property::new(spa_sys::SPA_PARAM_META_type, Value::Id(Id(meta_type))),
            Property::new(
                spa_sys::SPA_PARAM_META_size,
                Value::Int(i32::try_from(size).expect("metadata size exceeds SPA Int")),
            ),
        ];
        if let Some(features) = features {
            properties.push(Property::new(
                spa_sys::SPA_PARAM_META_features,
                Value::Int(features),
            ));
        }
        self.0.push(serialize(Value::Object(Object {
            type_: spa_sys::SPA_TYPE_OBJECT_ParamMeta,
            id: spa_sys::SPA_PARAM_Meta,
            properties,
        })));
        self
    }
}

fn serialize(value: Value) -> Vec<u8> {
    PodSerializer::serialize(Cursor::new(Vec::new()), &value)
        .expect("serializing an in-memory POD should succeed")
        .0
        .into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::meta::{MetaHeader, Metadata};

    fn object(type_: u32, id: u32, properties: Vec<Property>) -> Value {
        Value::Object(Object {
            type_,
            id,
            properties,
        })
    }

    fn value(parameter: &Parameters, index: usize) -> Value {
        crate::pod::deserialize::PodDeserializer::deserialize_from::<Value>(
            parameter.pods()[index].as_bytes(),
        )
        .unwrap()
        .1
    }

    #[test]
    fn fixed_buffer_count_replaces_the_negotiated_range() {
        let parameters = Parameters::new([object(
            spa_sys::SPA_TYPE_OBJECT_ParamBuffers,
            spa_sys::SPA_PARAM_Buffers,
            vec![Property::new(
                spa_sys::SPA_PARAM_BUFFERS_buffers,
                Value::Int(8),
            )],
        )])
        .with_fixed_buffer_count(3)
        .unwrap();

        let Value::Object(object) = value(&parameters, 0) else {
            panic!("expected object");
        };
        assert_eq!(object.properties[0].value, Value::Int(3));
    }

    #[test]
    fn metadata_request_can_be_removed_by_type() {
        let parameters = Parameters::new([object(
            spa_sys::SPA_TYPE_OBJECT_ParamMeta,
            spa_sys::SPA_PARAM_Meta,
            vec![Property::new(
                spa_sys::SPA_PARAM_META_type,
                Value::Id(Id(MetaHeader::META_TYPE)),
            )],
        )])
        .without_meta(MetaHeader::META_TYPE)
        .unwrap();

        assert!(parameters.pods().is_empty());
    }

    #[test]
    fn properties_metadata_request_carries_version_feature() {
        let parameters = Parameters::new([]).with_buffer_properties(512).unwrap();
        let Value::Object(object) = value(&parameters, 0) else {
            panic!("expected object");
        };
        assert!(object.properties.iter().any(|property| {
            property.key == spa_sys::SPA_PARAM_META_type
                && property.value == Value::Id(Id(spa_sys::SPA_META_Properties))
        }));
        assert!(object.properties.iter().any(|property| {
            property.key == spa_sys::SPA_PARAM_META_features
                && property.value
                    == Value::Int(spa_sys::SPA_META_FEATURE_PROPERTIES_VERSION_1 as i32)
        }));
    }
}
