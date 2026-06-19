use smithay::backend::renderer::gles::{ffi, GlesError, GlesFrame, GlesTexture};
use smithay::backend::renderer::Texture as _;
use smithay::utils::{Physical, Rectangle};

/// Blit the `dst` region of the frame's current DRAW framebuffer into `into`.
///
/// Binds a temporary FBO with `into` as `COLOR_ATTACHMENT0`, disables the scissor test,
/// blits the `dst` region into `(0, 0, size)` of `into`, then restores the previous state
/// and deletes the temporary FBO.
pub fn capture_framebuffer_region(
    frame: &mut GlesFrame<'_, '_>,
    dst: Rectangle<i32, Physical>,
    into: &GlesTexture,
) -> Result<(), GlesError> {
    let size = into.size();
    frame.with_context(|gl| unsafe {
        while gl.GetError() != ffi::NO_ERROR {}

        let mut current_fbo = 0i32;
        gl.GetIntegerv(ffi::DRAW_FRAMEBUFFER_BINDING, &mut current_fbo as *mut _);

        // BlitFramebuffer is affected by the scissor test, we don't want that.
        gl.Disable(ffi::SCISSOR_TEST);

        let mut fbo = 0;
        gl.GenFramebuffers(1, &mut fbo as *mut _);
        gl.BindFramebuffer(ffi::DRAW_FRAMEBUFFER, fbo);

        gl.FramebufferTexture2D(
            ffi::DRAW_FRAMEBUFFER,
            ffi::COLOR_ATTACHMENT0,
            ffi::TEXTURE_2D,
            into.tex_id(),
            0,
        );

        gl.BlitFramebuffer(
            dst.loc.x,
            dst.loc.y,
            dst.loc.x + dst.size.w,
            dst.loc.y + dst.size.h,
            0,
            0,
            size.w,
            size.h,
            ffi::COLOR_BUFFER_BIT,
            ffi::LINEAR,
        );

        // Restore state set by GlesFrame that we just modified.
        gl.BindFramebuffer(ffi::DRAW_FRAMEBUFFER, current_fbo as u32);
        gl.Enable(ffi::SCISSOR_TEST);

        gl.DeleteFramebuffers(1, &mut fbo as *mut _);

        if gl.GetError() != ffi::NO_ERROR {
            Err(GlesError::BlitError)
        } else {
            Ok(())
        }
    })?
}
