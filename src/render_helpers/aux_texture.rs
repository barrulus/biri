//! Binding extra sampler textures next to the primary texture of a GLES draw.
//!
//! Smithay's texture draws bind their texture on unit 0. Shaders that read additional textures
//! (alpha masks, feedback buffers, captured screen regions) bind them on the units after it and
//! point their sampler uniforms at those units.

use smithay::backend::renderer::gles::ffi;
use smithay::backend::renderer::gles::ffi::types::{GLenum, GLuint};

/// Binds `tex_id` as a 2D texture on texture unit `unit` with linear filtering and the given
/// wrap mode, then makes unit 0 active again.
///
/// # Safety
///
/// Must be called with a current GL context, from inside a renderer `with_context` callback.
pub unsafe fn bind(gl: &ffi::Gles2, unit: u32, tex_id: GLuint, wrap: GLenum) {
    gl.ActiveTexture(ffi::TEXTURE0 + unit);
    gl.BindTexture(ffi::TEXTURE_2D, tex_id);
    gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MIN_FILTER, ffi::LINEAR as i32);
    gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MAG_FILTER, ffi::LINEAR as i32);
    gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_WRAP_S, wrap as i32);
    gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_WRAP_T, wrap as i32);
    gl.ActiveTexture(ffi::TEXTURE0);
}

/// Unbinds the 2D texture on unit `unit`, then makes unit 0 active again.
///
/// # Safety
///
/// Same requirements as [`bind`].
pub unsafe fn unbind(gl: &ffi::Gles2, unit: u32) {
    gl.ActiveTexture(ffi::TEXTURE0 + unit);
    gl.BindTexture(ffi::TEXTURE_2D, 0);
    gl.ActiveTexture(ffi::TEXTURE0);
}
