// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(unpredictable_function_pointer_comparisons)]
#[allow(unnecessary_transmutes)]
#[allow(clippy::all)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}
pub use bindings::*;

// `PW_ID_ANY` is a cast-style macro in PipeWire's public headers. Newer
// Clang versions omit it from bindgen output, so define its stable ABI value.
pub const PW_ID_ANY: u32 = u32::MAX;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init() {
        unsafe {
            pw_init(std::ptr::null_mut(), std::ptr::null_mut());
            pw_deinit();
        }
    }
}
