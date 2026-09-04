//! Public identifiers for the PipeWireAO FITS source.

/// Read-only `SPA_PARAM_Props` identifier reporting normal finite completion.
pub const COMPLETED_PROPERTY: u32 = spa::sys::SPA_PROP_START_CUSTOM;
/// Scientific property name advertised by the FITS source through PropInfo.
pub const COMPLETED_PROPERTY_NAME: &str = "fits.completed";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_uses_the_public_custom_property_base() {
        assert_eq!(COMPLETED_PROPERTY, spa::sys::SPA_PROP_START_CUSTOM);
        assert_eq!(COMPLETED_PROPERTY_NAME, "fits.completed");
    }
}
