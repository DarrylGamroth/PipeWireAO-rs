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

## Independent buffer metadata

Native PipeWireAO 3.0.0 owns the layouts in `spa/buffer/meta.h`. Cargo's
bindgen build regenerates sys bindings and inline helper wrappers from those
headers. Retired Acquisition and BufferProperties metadata are unsupported.

`Buffer::metadata_records(requirements)` validates all bounded descriptors and
returns read-only `MetadataRecords` tied to the buffer lease. Typed accessors
return only valid facts. Iteration also exposes unavailable slots and opaque
optional bytes without allocating. Required descriptors use exact type and size;
unknown optional records remain skippable. Bounds are 64 records and 4096 aligned
bytes. `Parameters::with_metadata_record(type, size, required)` declares the cold
port capability without reducing custom IDs to a 32-bit mask.

Header publication counters remain distinct from physical acquisition identity.
Exposure timing, absolute REALTIME/TAI events, optical chopping and electrical
trigger state are independent records. Clock labels alone do not establish
synchronization. No PTP provider is required.

Focused software verification uses `cargo test -p libspa-ao --lib` with the native
prefix's absolute `PKG_CONFIG_PATH` and `LD_LIBRARY_PATH`; this does not establish
physical clock accuracy or deadline performance.

## License
PipeWireAO-rs is distributed under the terms of the MIT license.

See [LICENSE](LICENSE) for more information.
