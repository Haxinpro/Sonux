# Vendored PipeWire binding patches

`libspa-sys` and `pipewire-sys` are copied from the pinned `pipewire-rs`
v0.10.0 source (`5dd5dfad`) and remain under the upstream `LICENSE` (MIT).

Sonux carries these two small crates because Clang 22 omits the cast-style
`SPA_ID_INVALID` and `PW_ID_ANY` macros from bindgen output. The local changes
blocklist those macros and define their stable `u32::MAX` ABI values in Rust.
The higher-level `libspa` and `pipewire` crates still come directly from the
pinned upstream revision.
