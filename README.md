# PipeWireAO-rs

Rust bindings for the collision-free PipeWireAO and SPA libraries. This fork
adds ndarray formats, acquisition metadata, and owned filter helpers used by
the AO runtime. The published package identities are `pipewire-ao`,
`pipewire-ao-sys`, `libspa-ao`, and `libspa-ao-sys`; they do not replace the
upstream Rust packages.

The library targets retain the `pipewire`, `pipewire_sys`, `libspa`, and
`libspa_sys` source names so an existing consumer can select the AO package
explicitly without rewriting imports:

```toml
pipewire = { package = "pipewire-ao", version = "0.10" }
```

## Requirements

- Rust 1.80 or newer
- PipeWireAO development files providing `libpipewire-ao-0.3` and
  `libspa-ao-0.2` through pkg-config
- Clang (see [bindgen requirements](https://rust-lang.github.io/rust-bindgen/requirements.html))

The build probes only the AO pkg-config names and intentionally fails when only
upstream PipeWire is installed. For an uninstalled sibling PipeWireAO build,
run Cargo through its Meson development environment.

## Per-buffer properties

`libspa::buffer::meta::BufferProperties::from_bytes` borrows the buffer's aligned
Properties allocation and uses the native bounded validator before exposing typed
POD values. `BufferPropertyRequirement::new(key, type)` makes missing or mistyped
keys fail validation. Unknown optional typed keys may be skipped; unknown mandatory
keys require a matching consumer requirement. Borrowed values cannot outlive the
storage borrow. The native contract limits allocations to 4096 bytes and 64 fields.

`optical_classification()` returns absent, unfringed, fringed or mixed content,
independently of Acquisition v3 electrical trigger state. `None` means the property
is missing, distinct from explicit absent/mixed. `trigger_classification()` uses
the existing native Acquisition trigger enum
(absent/low/high/mixed), without inferring any optical content. The safe
`MetaAcquisition::trigger_classification()` accessor exposes that same enum for
v3 and returns no classification for v1/v2. `acquisition_is_consistent()` rejects
a disagreement between a present typed trigger and Acquisition v3, including
records without identity; v1/v2 permit a separately declared typed trigger.
Header and Acquisition retain their existing ABI and APIs. This extension requires
native PipeWireAO BufferProperties V1 headers;
Cargo's existing bindgen build regenerates native bindings automatically.

Focused verification against the source build uses
`cargo test -p libspa-ao --lib buffer::meta` with the native build's absolute
`PKG_CONFIG_PATH` and `LD_LIBRARY_PATH`. This checks software metadata handling,
not physical acquisition or deadline performance.

## License
PipeWireAO-rs is distributed under the terms of the MIT license.

See [LICENSE](LICENSE) for more information.
