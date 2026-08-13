// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(unpredictable_function_pointer_comparisons)]
#[allow(clippy::all)]
/// bindgen-generated definitions
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}
pub use bindings::*;

// `SPA_ID_INVALID` is a cast-style macro in PipeWire's public headers. Newer
// Clang versions omit it from bindgen output, so define its stable ABI value.
pub const SPA_ID_INVALID: u32 = u32::MAX;

// Manually defined symbols that are manually compiled into a C object file, as they need to be present at link-time.
//
// As SPA is a header-only library, global variables and functions are `static` / `static inline`
// and we need to compile them into a C object ourselves.
//
// For functions, this is handled by bindgens "wrap_static_fns" feature.
//
// The rest is added in modules here.
mod type_info;
pub use type_info::*;

mod command;
pub use command::*;
mod meta;
pub use meta::*;

mod node_command;
pub use node_command::*;
