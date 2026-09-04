//! Public identifiers for the PipeWireAO format-agnostic discard sink.

/// Metrics published by the discard sink through `SPA_PARAM_Props`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscardMetric {
    Buffers,
    DataBlocks,
    Bytes,
    ProtocolErrors,
    ProcessCalls,
}

impl DiscardMetric {
    /// Return the public SPA property identifier defined by
    /// `pipewireao-plugins/discard.h`.
    #[must_use]
    pub const fn property_id(self) -> u32 {
        let first = spa::sys::SPA_PROP_START_CUSTOM;
        match self {
            Self::Buffers => first,
            Self::DataBlocks => first + 1,
            Self::Bytes => first + 2,
            Self::ProtocolErrors => first + 3,
            Self::ProcessCalls => first + 4,
        }
    }

    /// Return the public scientific metric name advertised by PropInfo.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Buffers => "discard.buffers",
            Self::DataBlocks => "discard.data-blocks",
            Self::Bytes => "discard.bytes",
            Self::ProtocolErrors => "discard.protocol-errors",
            Self::ProcessCalls => "discard.process-calls",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_ids_match_the_public_contiguous_contract() {
        let metrics = [
            DiscardMetric::Buffers,
            DiscardMetric::DataBlocks,
            DiscardMetric::Bytes,
            DiscardMetric::ProtocolErrors,
            DiscardMetric::ProcessCalls,
        ];
        for (offset, metric) in metrics.into_iter().enumerate() {
            assert_eq!(
                metric.property_id(),
                spa::sys::SPA_PROP_START_CUSTOM + u32::try_from(offset).unwrap()
            );
            assert!(metric.name().starts_with("discard."));
        }
    }
}
