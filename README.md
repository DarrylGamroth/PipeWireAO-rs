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

## License
PipeWireAO-rs is distributed under the terms of the MIT license.

See [LICENSE](LICENSE) for more information.
