use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, mem, thread};

use _server_decoration::server::org_kde_kwin_server_decoration_manager::Mode as KdeDecorationsMode;
use anyhow::{bail, ensure, Context};
use calloop::futures::Scheduler;
use niri_config::debug::PreviewRender;
use niri_config::output::MaxBpc;
use niri_config::{
    Config, FloatOrInt, Key, Modifiers, OutputName, TrackLayout, WarpMouseToFocusMode,
    WorkspaceReference, Xkb,
};
use smithay::backend::allocator::Fourcc;
use smithay::backend::input::Keycode;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::utils::{
    select_dmabuf_feedback, CropRenderElement, Relocate, RelocateRenderElement,
    RescaleRenderElement,
};
use smithay::backend::renderer::element::{
    default_primary_scanout_output_compare, Element, Id, Kind, PrimaryScanoutOutput, RenderElement,
    RenderElementStates,
};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::sync::SyncPoint;
use smithay::backend::renderer::Color32F;
use smithay::desktop::utils::{
    bbox_from_surface_tree, output_update, send_dmabuf_feedback_surface_tree,
    send_frames_surface_tree, surface_presentation_feedback_flags_from_states,
    surface_primary_scanout_output, take_presentation_feedback_surface_tree,
    under_from_surface_tree, update_surface_primary_scanout_output, with_surfaces_surface_tree,
    OutputPresentationFeedback,
};
use smithay::desktop::{
    find_popup_root_surface, layer_map_for_output, LayerMap, LayerSurface, PopupGrab, PopupManager,
    PopupUngrabStrategy, Space, Window, WindowSurfaceType,
};
use smithay::input::keyboard::{Layout as KeyboardLayout, XkbConfig};
use smithay::input::pointer::{
    CursorIcon, CursorImageStatus, CursorImageSurfaceData, Focus,
    GrabStartData as PointerGrabStartData, MotionEvent,
};
use smithay::input::{Seat, SeatState};
use smithay::output::{self, Output, OutputModeSource, PhysicalProperties, Subpixel, WeakOutput};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{
    Interest, LoopHandle, LoopSignal, Mode, PostAction, RegistrationToken,
};
use smithay::reexports::wayland_protocols::ext::session_lock::v1::server::ext_session_lock_v1::ExtSessionLockV1;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::WmCapabilities;
use smithay::reexports::wayland_protocols_misc::server_decoration as _server_decoration;
use smithay::reexports::wayland_protocols_wlr::screencopy::v1::server::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;
use smithay::reexports::wayland_server::backend::{
    ClientData, ClientId, DisconnectReason, GlobalId,
};
use smithay::reexports::wayland_server::protocol::wl_shm;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, Display, DisplayHandle, Resource};
use smithay::utils::{
    ClockSource, IsAlive as _, Logical, Monotonic, Physical, Point, Rectangle, Scale, Size,
    Transform, SERIAL_COUNTER,
};
use smithay::wayland::background_effect::BackgroundEffectState;
use smithay::wayland::compositor::{
    with_states, with_surface_tree_downward, CompositorClientState, CompositorHandler,
    CompositorState, HookId, SurfaceData, TraversalAction,
};
use smithay::wayland::cursor_shape::CursorShapeManagerState;
use smithay::wayland::dmabuf::DmabufState;
use smithay::wayland::fractional_scale::FractionalScaleManagerState;
use smithay::wayland::idle_inhibit::IdleInhibitManagerState;
use smithay::wayland::idle_notify::IdleNotifierState;
use smithay::wayland::input_method::InputMethodManagerState;
use smithay::wayland::keyboard_shortcuts_inhibit::{
    KeyboardShortcutsInhibitState, KeyboardShortcutsInhibitor,
};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::pointer_constraints::{with_pointer_constraint, PointerConstraintsState};
use smithay::wayland::pointer_gestures::PointerGesturesState;
use smithay::wayland::presentation::PresentationState;
use smithay::wayland::relative_pointer::RelativePointerManagerState;
use smithay::wayland::security_context::SecurityContextState;
use smithay::wayland::selection::data_device::{set_data_device_selection, DataDeviceState};
use smithay::wayland::selection::ext_data_control::DataControlState as ExtDataControlState;
use smithay::wayland::selection::primary_selection::PrimarySelectionState;
use smithay::wayland::selection::wlr_data_control::DataControlState as WlrDataControlState;
use smithay::wayland::session_lock::{LockSurface, SessionLockManagerState, SessionLocker};
use smithay::wayland::shell::kde::decoration::KdeDecorationState;
use smithay::wayland::shell::wlr_layer::{self, Layer, WlrLayerShellState};
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
#[cfg(test)]
use smithay::wayland::single_pixel_buffer::SinglePixelBufferState;
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::tablet_manager::TabletManagerState;
use smithay::wayland::text_input::TextInputManagerState;
use smithay::wayland::viewporter::ViewporterState;
use smithay::wayland::virtual_keyboard::VirtualKeyboardManagerState;
use smithay::wayland::xdg_activation::XdgActivationState;
use smithay::wayland::xdg_foreign::XdgForeignState;
use wayland_server::protocol::wl_output::WlOutput;

#[cfg(feature = "dbus")]
use crate::a11y::A11y;
use crate::animation::Clock;
use crate::backend::tty::SurfaceDmabufFeedback;
use crate::backend::{Backend, Headless, RenderResult, Tty, Winit};
use crate::cursor::{CursorManager, CursorTextureCache, RenderCursor, XCursor};
#[cfg(feature = "dbus")]
use crate::dbus::freedesktop_locale1::Locale1ToNiri;
#[cfg(feature = "dbus")]
use crate::dbus::freedesktop_login1::Login1ToNiri;
#[cfg(feature = "dbus")]
use crate::dbus::gnome_shell_introspect::{self, IntrospectToNiri, NiriToIntrospect};
#[cfg(feature = "dbus")]
use crate::dbus::gnome_shell_screenshot::{NiriToScreenshot, ScreenshotToNiri};
use crate::frame_clock::FrameClock;
use crate::handlers::{configure_lock_surface, XDG_ACTIVATION_TOKEN_TIMEOUT};
use crate::input::pick_color_grab::PickColorGrab;
use crate::input::scroll_swipe_gesture::ScrollSwipeGesture;
use crate::input::scroll_tracker::ScrollTracker;
use crate::input::{
    apply_libinput_settings, mods_with_finger_scroll_binds, mods_with_mouse_binds,
    mods_with_tablet_stylus_binds, mods_with_wheel_binds, TabletData,
};
use crate::ipc::server::IpcServer;
use crate::layer::mapped::LayerSurfaceRenderElement;
use crate::layer::MappedLayer;
use crate::layout::tile::TileRenderElement;
use crate::layout::workspace::{Workspace, WorkspaceId};
use crate::layout::{
    HitType, Layout, LayoutElement as _, LayoutElementRenderElement, MonitorRenderElement,
};
use crate::niri_render_elements;
use crate::protocols::ext_workspace::{self, ExtWorkspaceManagerState};
use crate::protocols::foreign_toplevel::{self, ForeignToplevelManagerState};
use crate::protocols::gamma_control::GammaControlManagerState;
use crate::protocols::mutter_x11_interop::MutterX11InteropManagerState;
use crate::protocols::output_management::OutputManagementManagerState;
use crate::protocols::screencopy::{Screencopy, ScreencopyBuffer, ScreencopyManagerState};
use crate::protocols::virtual_pointer::VirtualPointerManagerState;
use crate::render_helpers::blur::BlurOptions;
use crate::render_helpers::debug::push_opaque_regions;
use crate::render_helpers::global_shader_element::{GlobalPassState, GlobalShaderElement};
use crate::render_helpers::offscreen::{OffscreenBuffer, OffscreenRenderElement};
use crate::render_helpers::panel::PanelRenderElement;
use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::scoped_shader_element::{ScopedShaderElement, ScopedSource};
use crate::render_helpers::shaders::{ProgramType, Shaders};
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use crate::render_helpers::surface::push_elements_from_surface_tree;
use crate::render_helpers::texture::TextureBuffer;
use crate::render_helpers::xray::{Xray, XrayPos};
use crate::render_helpers::{
    encompassing_geo, render_to_dmabuf, render_to_encompassing_texture, render_to_shm,
    render_to_texture, render_to_vec, shaders, RenderCtx, RenderTarget,
};
#[cfg(feature = "xdp-gnome-screencast")]
use crate::screencasting::Screencasting;
use crate::ui::config_error_notification::ConfigErrorNotification;
use crate::ui::exit_confirm_dialog::{ExitConfirmDialog, ExitConfirmDialogRenderElement};
use crate::ui::hotkey_overlay::HotkeyOverlay;
use crate::ui::mru::{MruCloseRequest, WindowMruUi, WindowMruUiRenderElement};
use crate::ui::screen_transition::{self, ScreenTransition};
use crate::ui::screenshot_ui::{OutputScreenshot, ScreenshotUi, ScreenshotUiRenderElement};
use crate::utils::scale::{closest_representable_scale, guess_monitor_scale};
use crate::utils::spawning::{CHILD_DISPLAY, CHILD_ENV};
use crate::utils::vblank_throttle::VBlankThrottle;
use crate::utils::watcher::Watcher;
use crate::utils::xwayland::satellite::Satellite;
use crate::utils::{
    center, center_f64, expand_home, get_monotonic_time, ipc_transform_to_smithay, is_mapped,
    logical_output, make_screenshot_path, output_matches_name, output_size, panel_orientation,
    send_scale_transform, write_png_rgba8, xwayland,
};
use crate::window::mapped::MappedId;
use crate::window::{InitialConfigureState, Mapped, ResolvedWindowRules, Unmapped, WindowRef};

const CLEAR_COLOR_LOCKED: [f32; 4] = [0.3, 0.1, 0.1, 1.];

// We'll try to send frame callbacks at least once a second. We'll make a timer that fires once a
// second, so with the worst timing the maximum interval between two frame callbacks for a surface
// should be ~1.995 seconds.
const FRAME_CALLBACK_THROTTLE: Option<Duration> = Some(Duration::from_millis(995));

pub struct Niri {
    pub config: Rc<RefCell<Config>>,

    /// Cached capability scan of the active global shader, invalidated on config reload.
    /// `None` = not yet computed this load.
    pub global_shader_caps: Cell<Option<niri_config::GlobalShaderCaps>>,

    /// Shared origin for per-window scoped shaders' `niri_time`. Set once at startup and never
    /// reset, so all window shaders animate in-phase off one monotonic real-seconds clock. Unlike
    /// the global shader's per-output `global_shader_start`, this is compositor-global because
    /// window shaders can exist without a global shader.
    pub window_shader_start: std::time::Instant,

    /// Output config from the config file.
    ///
    /// This does not include transient output config changes done via IPC. It is only used when
    /// reloading the config from disk to determine if the output configuration should be reloaded
    /// (and transient changes dropped).
    pub config_file_output_config: niri_config::Outputs,

    pub config_file_watcher: Option<Watcher>,

    pub event_loop: LoopHandle<'static, State>,
    pub scheduler: Scheduler<()>,
    pub stop_signal: LoopSignal,
    pub display_handle: DisplayHandle,

    /// Whether niri was run with `--session`
    pub is_session_instance: bool,

    /// Name of the Wayland socket.
    ///
    /// This is `None` when creating `Niri` without a Wayland socket.
    pub socket_name: Option<OsString>,

    pub start_time: Instant,

    /// Frame stamp bumped once per `redraw_queued_outputs` cycle, so `update_panel_sources` can
    /// tell whether a given content output's `PanelSource` was already refreshed this cycle (it
    /// may be reached more than once, e.g. via a screencopy re-render) and skip redundant work.
    panel_frame: Cell<u64>,

    /// Whether the at-startup=true window rules are active.
    pub is_at_startup: bool,

    /// Clock for driving animations.
    pub clock: Clock,

    // Each workspace corresponds to a Space. Each workspace generally has one Output mapped to it,
    // however it may have none (when there are no outputs connected) or multiple (when mirroring).
    pub layout: Layout<Mapped>,

    // This space does not actually contain any windows, but all outputs are mapped into it
    // according to their global position.
    pub global_space: Space<Window>,

    /// Mapped outputs, sorted by their name and position.
    pub sorted_outputs: Vec<Output>,

    // Windows which don't have a buffer attached yet.
    pub unmapped_windows: HashMap<WlSurface, Unmapped>,

    /// Layer surfaces which don't have a buffer attached yet.
    pub unmapped_layer_surfaces: HashSet<WlSurface>,

    /// Extra data for mapped layer surfaces.
    pub mapped_layer_surfaces: HashMap<LayerSurface, MappedLayer>,

    // Cached root surface for every surface, so that we can access it in destroyed() where the
    // normal get_parent() is cleared out.
    pub root_surface: HashMap<WlSurface, WlSurface>,

    // Dmabuf readiness pre-commit hook for a surface.
    pub dmabuf_pre_commit_hook: HashMap<WlSurface, HookId>,

    /// Clients to notify about their blockers being cleared.
    pub blocker_cleared_tx: Sender<Client>,
    pub blocker_cleared_rx: Receiver<Client>,

    pub output_state: HashMap<Output, OutputState>,

    // When false, we're idling with monitors powered off.
    pub monitors_active: bool,

    // When monitors are powered off (`monitors_active == false`) but the power-off requested
    // `skip-isolated`, outputs marked `isolated` stay powered on and keep rendering. Meaningless
    // while `monitors_active` is true.
    pub power_off_skip_isolated: bool,

    /// Whether the laptop lid is closed.
    ///
    /// Libinput guarantees that the lid switch starts in open state, and if it was closed during
    /// startup, libinput will immediately send a closed event.
    pub is_lid_closed: bool,

    /// Runtime toggle for touchpad disabled state (combined with config.touchpad.off).
    pub touchpad_disabled_by_toggle: bool,

    pub dwt_disabled_by_toggle: bool,

    pub devices: HashSet<input::Device>,
    pub tablets: HashMap<input::Device, TabletData>,
    pub touch: HashSet<input::Device>,

    // Smithay state.
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub kde_decoration_state: KdeDecorationState,
    pub layer_shell_state: WlrLayerShellState,
    pub session_lock_state: SessionLockManagerState,
    pub foreign_toplevel_state: ForeignToplevelManagerState,
    pub ext_workspace_state: ExtWorkspaceManagerState,
    pub screencopy_state: ScreencopyManagerState,
    pub output_management_state: OutputManagementManagerState,
    pub viewporter_state: ViewporterState,
    pub background_effect_state: BackgroundEffectState,
    pub xdg_foreign_state: XdgForeignState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub dmabuf_state: DmabufState,
    pub fractional_scale_manager_state: FractionalScaleManagerState,
    pub seat_state: SeatState<State>,
    pub tablet_state: TabletManagerState,
    pub text_input_state: TextInputManagerState,
    pub input_method_state: InputMethodManagerState,
    pub keyboard_shortcuts_inhibit_state: KeyboardShortcutsInhibitState,
    pub virtual_keyboard_state: VirtualKeyboardManagerState,
    pub virtual_pointer_state: VirtualPointerManagerState,
    pub pointer_gestures_state: PointerGesturesState,
    pub relative_pointer_state: RelativePointerManagerState,
    pub pointer_constraints_state: PointerConstraintsState,
    pub idle_notifier_state: IdleNotifierState<State>,
    pub idle_inhibit_manager_state: IdleInhibitManagerState,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state: PrimarySelectionState,
    pub wlr_data_control_state: WlrDataControlState,
    pub ext_data_control_state: ExtDataControlState,
    pub popups: PopupManager,
    pub popup_grab: Option<PopupGrabState>,
    pub presentation_state: PresentationState,
    pub security_context_state: SecurityContextState,
    pub gamma_control_manager_state: GammaControlManagerState,
    pub activation_state: XdgActivationState,
    pub mutter_x11_interop_state: MutterX11InteropManagerState,

    // This will not work as is outside of tests, so it is gated with #[cfg(test)] for now. In
    // particular, shaders will need to learn about the single pixel buffer. Also, it must be
    // verified that a black single-pixel-buffer background lets the foreground surface to be
    // unredirected.
    //
    // https://github.com/niri-wm/niri/issues/619
    #[cfg(test)]
    pub single_pixel_buffer_state: SinglePixelBufferState,

    pub seat: Seat<State>,
    /// Scancodes of the keys to suppress.
    pub suppressed_keys: HashSet<Keycode>,
    /// Button codes of the mouse buttons to suppress.
    pub suppressed_buttons: HashSet<u32>,
    pub bind_cooldown_timers: HashMap<Key, RegistrationToken>,
    pub bind_repeat_timer: Option<RegistrationToken>,
    pub keyboard_focus: KeyboardFocus,
    pub layer_shell_on_demand_focus: Option<LayerSurface>,
    pub idle_inhibiting_surfaces: HashSet<WlSurface>,
    pub is_fdo_idle_inhibited: Arc<AtomicBool>,
    pub keyboard_shortcuts_inhibiting_surfaces: HashMap<WlSurface, KeyboardShortcutsInhibitor>,

    /// Most recent XKB settings from org.freedesktop.locale1.
    pub xkb_from_locale1: Option<Xkb>,

    pub cursor_manager: CursorManager,
    pub cursor_texture_cache: CursorTextureCache,
    pub cursor_shape_manager_state: CursorShapeManagerState,
    pub dnd_icon: Option<DndIcon>,
    /// Contents under pointer.
    ///
    /// Periodically updated: on motion and other events and in the loop callback. If you require
    /// the real up-to-date contents somewhere, it's better to recompute on the spot.
    ///
    /// This is not pointer focus. I.e. during a click grab, the pointer focus remains on the
    /// client with the grab, but this field will keep updating to the latest contents as if no
    /// grab was active.
    ///
    /// This is primarily useful for emitting pointer motion events for surfaces that move
    /// underneath the cursor on their own (i.e. when the tiling layout moves). In this case, not
    /// taking grabs into account is expected, because we pass the information to pointer.motion()
    /// which passes it down through grabs, which decide what to do with it as they see fit.
    pub pointer_contents: PointContents,
    pub pointer_visibility: PointerVisibility,
    pub pointer_inactivity_timer: Option<RegistrationToken>,
    /// Whether the pointer inactivity timer got reset this event loop iteration.
    ///
    /// Used for limiting the reset to once per iteration, so that it's not spammed with high
    /// resolution mice.
    pub pointer_inactivity_timer_got_reset: bool,
    /// Whether the (idle notifier) activity was notified this event loop iteration.
    ///
    /// Used for limiting the notify to once per iteration, so that it's not spammed with high
    /// resolution mice.
    pub notified_activity_this_iteration: bool,
    pub pointer_inside_hot_corner: bool,
    pub tablet_cursor_location: Option<Point<f64, Logical>>,
    pub gesture_swipe_3f_cumulative: Option<(f64, f64)>,
    pub overview_scroll_swipe_gesture: ScrollSwipeGesture,
    pub vertical_wheel_tracker: ScrollTracker,
    pub horizontal_wheel_tracker: ScrollTracker,
    pub mods_with_mouse_binds: HashSet<Modifiers>,
    pub mods_with_wheel_binds: HashSet<Modifiers>,
    pub mods_with_tablet_stylus_binds: HashSet<Modifiers>,
    pub vertical_finger_scroll_tracker: ScrollTracker,
    pub horizontal_finger_scroll_tracker: ScrollTracker,
    pub mods_with_finger_scroll_binds: HashSet<Modifiers>,

    pub lock_state: LockState,

    // State that we last sent to the logind LockedHint.
    pub locked_hint: Option<bool>,

    pub screenshot_ui: ScreenshotUi,
    pub config_error_notification: ConfigErrorNotification,
    pub hotkey_overlay: HotkeyOverlay,
    pub exit_confirm_dialog: ExitConfirmDialog,

    pub window_mru_ui: WindowMruUi,
    pub pending_mru_commit: Option<PendingMruCommit>,

    pub pick_window: Option<async_channel::Sender<Option<MappedId>>>,
    pub pick_color: Option<async_channel::Sender<Option<niri_ipc::PickedColor>>>,

    pub debug_draw_opaque_regions: bool,
    pub debug_draw_damage: bool,

    #[cfg(feature = "dbus")]
    pub dbus: Option<crate::dbus::DBusServers>,
    #[cfg(feature = "dbus")]
    pub a11y_manager: Option<crate::dbus::freedesktop_a11y::Manager>,
    #[cfg(feature = "dbus")]
    pub a11y: A11y,
    #[cfg(feature = "dbus")]
    pub inhibit_power_key_fd: Option<zbus::zvariant::OwnedFd>,

    pub ipc_server: Option<IpcServer>,
    pub ipc_outputs_changed: bool,

    pub satellite: Option<Satellite>,

    #[cfg(feature = "xdp-gnome-screencast")]
    pub casting: Screencasting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerVisibility {
    /// The pointer is visible.
    Visible,
    /// The pointer is invisible, but retains its focus.
    ///
    /// This state is set temporarily after auto-hiding the pointer to keep tooltips open and grabs
    /// ongoing.
    Hidden,
    /// The pointer is invisible and cannot focus.
    ///
    /// Corresponds to a fully disabled pointer, for example after a touchscreen input, or after
    /// the pointer contents changed in a Hidden state.
    Disabled,
}

impl PointerVisibility {
    pub fn is_visible(&self) -> bool {
        matches!(self, Self::Visible)
    }
}

#[derive(Debug)]
pub struct DndIcon {
    pub surface: WlSurface,
    pub offset: Point<i32, Logical>,
}

/// Per-output, per-pass feedback + offscreen state for the multi-pass global shader. Each `Vec`
/// has one entry per pass in the active chain; they are ping-ponged across frames. Sized lazily by
/// [`GlobalShaderChain::resize`] from the (`&self`) render path via the enclosing `RefCell`.
#[derive(Default)]
pub struct GlobalShaderChain {
    /// Each pass's output last frame (its `niri_prev`).
    pub prev: Vec<Option<GlesTexture>>,
    /// Sink for each pass's output this frame; moved into `prev` after submit.
    pub result: Vec<Rc<RefCell<Option<GlesTexture>>>>,
    /// Each intermediate pass's display offscreen (the last pass renders to `dst`).
    pub pass_offscreen: Vec<Rc<crate::render_helpers::offscreen::OffscreenBuffer>>,
    /// Each pass's dedicated `global_buffer` offscreen.
    pub buffer: Vec<Rc<crate::render_helpers::offscreen::OffscreenBuffer>>,
    /// Each pass's dedicated buffer last frame (its `niri_buffer`).
    pub buffer_prev: Vec<Option<GlesTexture>>,
    /// Sink for each pass's dedicated buffer this frame; moved into `buffer_prev` after submit.
    pub buffer_result: Vec<Rc<RefCell<Option<GlesTexture>>>>,
}

impl GlobalShaderChain {
    /// Ensure all per-pass vecs are sized to `n`, preserving existing textures when the length is
    /// unchanged. A length change resets all per-pass feedback (the chain itself changed).
    pub fn resize(&mut self, n: usize) {
        use crate::render_helpers::offscreen::OffscreenBuffer;
        if self.prev.len() == n {
            return;
        }
        self.prev = (0..n).map(|_| None).collect();
        self.result = (0..n).map(|_| Rc::new(RefCell::new(None))).collect();
        self.pass_offscreen = (0..n)
            .map(|_| Rc::new(OffscreenBuffer::default()))
            .collect();
        self.buffer = (0..n)
            .map(|_| Rc::new(OffscreenBuffer::default()))
            .collect();
        self.buffer_prev = (0..n).map(|_| None).collect();
        self.buffer_result = (0..n).map(|_| Rc::new(RefCell::new(None))).collect();
    }
}

/// Decision for one frame of the `shader-animation-max-fps` throttle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShaderFrameDecision {
    /// This is the first shader frame, or a full interval has elapsed: render it as a shader
    /// frame and schedule the next wake one interval out.
    Due,
    /// Not yet due; the next shader frame is `remaining` away.
    Wait(Duration),
}

/// Given when the last shader frame rendered, the current time, and the capped frame interval,
/// decide whether this frame is due. Pure so it is unit-testable without a running compositor.
fn decide_shader_frame(
    last: Option<std::time::Instant>,
    now: std::time::Instant,
    interval: Duration,
) -> ShaderFrameDecision {
    match last {
        None => ShaderFrameDecision::Due,
        Some(last) => {
            let elapsed = now.saturating_duration_since(last);
            if elapsed >= interval {
                ShaderFrameDecision::Due
            } else {
                ShaderFrameDecision::Wait(interval - elapsed)
            }
        }
    }
}

/// What to do with the per-output shader-throttle timer after a redraw decision.
enum ThrottleAction {
    /// Leave the currently-armed timer as-is.
    Keep,
    /// Cancel any pending timer.
    Cancel,
    /// Arm (replacing any pending) a one-shot wake this far out.
    Arm(Duration),
}

thread_local! {
    /// Set for the duration of `Niri::render_inner`'s body. `update_panel_sources` asserts this
    /// is *not* set at entry: it takes `layer_map_for_output()` locks for panel content outputs,
    /// and running it while nested inside `render_inner` risks re-locking the same output's layer
    /// map (the exact recursive-lock deadlock the consolidated-carousel lens block hit on a
    /// single monitor). The prepass must always run before `render_inner` is entered.
    static IN_RENDER_INNER: Cell<bool> = const { Cell::new(false) };
}

/// RAII guard flipping [`IN_RENDER_INNER`] for the duration of `render_inner`'s body, so it reads
/// true for reentrant/nested calls too and is reliably cleared on early return or panic-unwind.
struct InRenderInnerGuard;

impl InRenderInnerGuard {
    fn enter() -> Self {
        let was_set = IN_RENDER_INNER.with(|c| c.replace(true));
        debug_assert!(!was_set, "render_inner should not be reentrant");
        Self
    }
}

impl Drop for InRenderInnerGuard {
    fn drop(&mut self) {
        IN_RENDER_INNER.with(|c| c.set(false));
    }
}

/// Retained damage-gated offscreen render of one output's panel content (its overview + Background
/// and Bottom layer-shell surfaces + workspace backgrounds, at whatever zoom the current host asks
/// for), stored on that CONTENT output's own [`OutputState`].
///
/// # Publish-on-damage over ping-pong buffers: the ownership invariant
///
/// The two backing [`OffscreenBuffer`]s live in [`OutputState::panel_offscreen`], OUTSIDE this
/// struct's `RefCell`, so the GPU render call is never made while a `panel_source` borrow is held
/// (`OffscreenBuffer::render` takes `&self`). `OffscreenBuffer::render` recreates its texture
/// whenever the previously-returned `GlesTexture` is not `is_unique_reference()` (a plain
/// `Arc::get_mut` check — see `render_helpers/offscreen.rs` and the identical reasoning at
/// `layout/tile.rs:1175` and `render_helpers/global_shader_element.rs:241`), so a buffer whose
/// texture clone is retained across frames gets a full recreate + full redraw on its next render,
/// defeating damage gating.
///
/// `flip` names the buffer the NEXT render targets (the scratch buffer). `elem` is replaced — and
/// `flip` toggled, and `generation` bumped — ONLY when a render actually produced damage
/// ([`crate::render_helpers::offscreen::OffscreenData::damaged`]). This maintains the invariant:
///
/// > The published `elem` (if any) only ever holds a texture clone of the buffer `flip` does NOT
/// > name. The scratch buffer named by `flip` has no live clone held by this struct, so across
/// > static frames it stays unique and its `OutputDamageTracker` diffs incrementally instead of
/// > recreating.
///
/// Two-frame trace (buffers A/B, `flip` = scratch):
///
/// * **Static frame** (`flip` = A, `elem` = Some(B-texture)): render into A diffs element states
///   against A's tracker → no damage → the fresh `OffscreenRenderElement` (holding a clone of
///   A's texture) is dropped immediately; `elem`, `generation` and `flip` are untouched. The
///   published texture identity is therefore stable across static frames, so a host-side
///   [`PanelRenderElement::update`] compare on `(source id, generation)` early-returns keeping
///   its old clone — which is exactly the still-published B texture, not a stale one. A remains
///   unique for the next render. Repeat indefinitely: zero publishes, zero host damage.
/// * **Damaged frame** (`flip` = A, `elem` = Some(B-texture)): render into A produces damage
///   (incremental, since A stayed unique) → publish `elem` = Some(A-texture), `generation` += 1,
///   `flip` → B; the old B clone held here drops. The next render targets B, whose tracker last
///   saw states from one publish earlier, so it produces catch-up damage and publishes again
///   (one redundant publish per content change; the render after that targets the now up-to-date
///   buffer and settles into the static case). During continuous animation this degenerates to
///   the plain alternate-every-frame ping-pong, which is optimal anyway.
///
/// A host's retained [`PanelRenderElement`] may still hold a clone of the previously published
/// texture until the host next redraws and `update()` swaps it (params differ via `generation`).
/// If the host has NOT redrawn by the time a render targets that buffer again, the buffer is
/// recreated (one full redraw) — sound, merely unoptimal; the Task 6 wiring should queue a host
/// redraw whenever a source it displays publishes, so retained clones are refreshed promptly.
///
/// The first-ever render and any buffer recreation (size growth, lost uniqueness, renderer
/// context change) report full damage and therefore always publish, so `generation` bumps
/// whenever texture identity or contents change — which is why the host-side compare keys on
/// `(source Id, generation)` and never on the per-buffer damage-bag `CommitCounter`s (those are
/// independent between A and B and reset to default on recreation).
pub struct PanelSource {
    /// Scratch buffer index in [`OutputState::panel_offscreen`] that the next render targets.
    /// Toggled only on publish, so it never names the buffer backing `elem`.
    flip: bool,
    /// The retained handle from the last published (damage-producing) render. Clones of its
    /// texture may be retained across frames (e.g. by a host's `PanelRenderElement`) provided
    /// they are refreshed whenever `generation` changes — see the invariant above.
    pub elem: Option<OffscreenRenderElement>,
    /// Bumped on every publish, i.e. exactly when `elem`'s texture identity/contents change.
    /// Together with the published elem's `Id`, this is the identity `PanelRenderElement`
    /// compares to decide whether to rebuild.
    ///
    /// Monotone across resets; identity must never repeat for a given buffer `Id`. `clear()`
    /// preserves this counter rather than zeroing it — see `clear()`'s doc for why.
    pub generation: u64,
    /// Frame stamp this source was last updated at (see `Niri::panel_frame`), making
    /// `update_panel_sources` idempotent when reached more than once in the same redraw cycle
    /// (e.g. via a screencopy re-render).
    frame: u64,
    /// The workspace-geo union (`target_mon.workspaces_with_render_geo_at_zoom(fill_zoom)`,
    /// reduced via `Rectangle::merge`) that produced the currently published `elem`, i.e. the
    /// same bbox `Layout::carousel_focus_jump` computes to map its uv. `update_panel_sources`
    /// compares each refresh's freshly-computed union against this (with a small epsilon — see
    /// its call site) and, on a real change, resets this source's offscreen buffers via
    /// `Self::clear`/`OffscreenBuffer::clear` BEFORE rendering, forcing recreation at the new
    /// extent. Needed because `OffscreenBuffer` only recreates on GROWTH: without this, a
    /// SHRINK (fewer/narrower workspaces, or window count changes at a fixed `fill_zoom`) would
    /// leave stale content in a sub-rect of an oversized allocation while the panel samples the
    /// full texture, i.e. a persistent (steady-state, window-content-driven) skew — not a
    /// transient sub-pixel artifact.
    last_extent: Option<Rectangle<f64, Logical>>,
}

impl Default for PanelSource {
    fn default() -> Self {
        Self {
            flip: false,
            elem: None,
            generation: 0,
            // Never equals a real stamp on first use: `Niri::panel_frame` starts at 0 and is
            // bumped to 1 before the first `redraw_queued_outputs` cycle even begins iterating.
            frame: 0,
            last_extent: None,
        }
    }
}

impl PanelSource {
    /// Resets to the fresh (never-rendered) state, dropping the retained published element (and
    /// thus releasing its texture clone) so an output that stopped participating in the carousel
    /// ring does not keep GPU memory pinned. Callers must also clear the backing
    /// `OutputState::panel_offscreen` buffers (see `OffscreenBuffer::clear`) — this only resets
    /// the published-element side of the pair.
    ///
    /// `generation` is deliberately carried over rather than zeroed by this reset. The backing
    /// `panel_offscreen` buffers (and their `OffscreenRenderElement`'s smithay `Id`s) live outside
    /// this struct on `OutputState` and survive a `clear()`, so an extent-change reset immediately
    /// followed by a re-render can republish under the SAME buffer `Id`. If `generation` also went
    /// back to 0 here, that republish would present `(source_id, generation)` == `(same Id, 0)` —
    /// identical to what a host's retained `PanelRenderElement` already cached from before the
    /// reset — and `PanelRenderElement::update`'s equality check would keep its stale texture
    /// clone instead of rebuilding. Preserving the counter across every reset means a post-reset
    /// publish always bumps to a value the host has never seen for that Id, so it can never alias.
    pub fn clear(&mut self) {
        let generation = self.generation;
        *self = Self::default();
        self.generation = generation;
    }
}

pub struct OutputState {
    pub global: GlobalId,
    pub frame_clock: FrameClock,
    pub redraw_state: RedrawState,
    pub on_demand_vrr_enabled: bool,
    // After the last redraw, some ongoing animations still remain.
    pub unfinished_animations_remain: bool,
    /// When we last allowed a shader-driven redraw on this output, for the
    /// `shader-animation-max-fps` throttle. `None` until the first shader frame.
    pub last_shader_frame: Option<std::time::Instant>,
    /// Pending one-shot timer that re-queues a throttled shader redraw at the next capped
    /// deadline. Stored so it can be cancelled/replaced; `None` when no throttle wake is armed.
    pub shader_throttle_timer: Option<RegistrationToken>,
    /// Last sequence received in a vblank event.
    pub last_drm_sequence: Option<u32>,
    pub vblank_throttle: VBlankThrottle,
    /// Sequence for frame callback throttling.
    ///
    /// We want to send frame callbacks for each surface at most once per monitor refresh cycle.
    ///
    /// Even if a surface commit resulted in empty damage to the monitor, we want to delay the next
    /// frame callback until roughly when a VBlank would occur, had the monitor been damaged. This
    /// is necessary to prevent clients busy-looping with frame callbacks that result in empty
    /// damage.
    ///
    /// This counter wrapping-increments by 1 every time we move into the next refresh cycle, as
    /// far as frame callback throttling is concerned. Specifically, it happens:
    ///
    /// 1. Upon a successful DRM frame submission. Notably, we don't wait for the VBlank here,
    ///    because the client buffers are already "latched" at the point of submission. Even if a
    ///    client submits a new buffer right away, we will wait for a VBlank to draw it, which
    ///    means that busy looping is avoided.
    /// 2. If a frame resulted in empty damage, a timer is queued to fire roughly when a VBlank
    ///    would occur, based on the last presentation time and output refresh interval. Sequence
    ///    is incremented in that timer, before attempting a redraw or sending frame callbacks.
    pub frame_callback_sequence: u32,
    /// Solid color buffer for the backdrop that we use instead of clearing to avoid damage
    /// tracking issues and make screenshots easier.
    pub backdrop_buffer: SolidColorBuffer,
    pub xray: Xray,
    pub lock_render_state: LockRenderState,
    pub lock_surface: Option<LockSurface>,
    pub lock_color_buffer: SolidColorBuffer,
    screen_transition: Option<ScreenTransition>,
    /// When the global shader became active; origin for `niri_time`. Interior-mutable because the
    /// time origin is lazily initialized from the (`&self`) `render_inner` path.
    pub global_shader_start: Cell<Option<std::time::Instant>>,
    /// Previous frame's `niri_screen` capture (screen only), exposed as `niri_screen_prev`. This
    /// is frame-level (the real screen), shared by all passes.
    pub global_shader_screen_prev: Option<GlesTexture>,
    /// Sink written by `GlobalShaderElement::draw` with this frame's screen capture; moved into
    /// `global_shader_screen_prev` after submit.
    pub global_shader_screen_result: Rc<RefCell<Option<GlesTexture>>>,
    /// Per-pass feedback + offscreen state for the multi-pass chain, sized to the active chain
    /// length and ping-ponged across frames. Wrapped in a `RefCell` so it can be (re)sized lazily
    /// from the `&self` render path (like the other interior-mutable shader sinks).
    pub global_shader_chain: RefCell<GlobalShaderChain>,
    /// Damage tracker used for the debug damage visualization.
    pub debug_damage_tracker: OutputDamageTracker,
    /// This output's own damage-gated panel content source (populated when this output
    /// participates in the carousel, as content for some host — possibly itself). See
    /// `PanelSource` and `Niri::update_panel_sources`.
    pub panel_source: RefCell<PanelSource>,
    /// Ping-pong offscreen buffers backing `panel_source`, indexed by `PanelSource::flip`. Kept
    /// OUTSIDE the `RefCell` (`OffscreenBuffer::render` takes `&self`) so the GPU render call in
    /// `update_panel_sources` never runs while a `panel_source` borrow is held.
    pub panel_offscreen: [OffscreenBuffer; 2],
    /// Retained per-ring-slot panel elements when this output is acting as a carousel HOST,
    /// keyed by ring index. Entries are dropped when the ring shrinks. Built from `panel_source`
    /// content (this output's own, or a sibling's) each frame; wired up in Gate B Task 6.
    pub panel_elems: RefCell<HashMap<usize, PanelRenderElement>>,
    /// Set (only ever from `update_panel_sources`, while this output is the active carousel HOST)
    /// when a panel content source published fresh pixels this render. Consumed in `redraw()`,
    /// right after `backend.render` returns and `&mut self` is available again, to queue another
    /// redraw so `panel_elems` picks up the new generation at content cadence rather than waiting
    /// for this host's next unrelated redraw. `update_panel_sources` runs under `&self` mid-render
    /// (see `IN_RENDER_INNER`), so it cannot call `Niri::queue_redraw` (`&mut self`) directly —
    /// this flag plus deferred consumption is the mirror of the `shader_throttle_timer` deferred
    /// re-borrow pattern used just below in `redraw()`.
    pub panel_redraw_pending: Cell<bool>,
    /// Click hit-test geometry for the carousel panel stack drawn on this output, refreshed by
    /// the panel-stack block in `render_inner` every frame panels are drawn (nearest/highest-z
    /// panel first, matching draw order and thus hit priority), and cleared whenever panels
    /// aren't drawn so a stale cache can't eat clicks after the carousel closes (see
    /// `clear_stale_panel_sources`). Each entry is `(ring_pos, corners)` for one drawn panel.
    pub panel_hits: RefCell<Vec<(f64, [[f64; 2]; 4])>>,
}

#[derive(Debug, Default)]
pub enum RedrawState {
    /// The compositor is idle.
    #[default]
    Idle,
    /// A redraw is queued.
    Queued,
    /// We submitted a frame to the KMS and waiting for it to be presented.
    WaitingForVBlank { redraw_needed: bool },
    /// We did not submit anything to KMS and made a timer to fire at the estimated VBlank.
    WaitingForEstimatedVBlank(RegistrationToken),
    /// A redraw is queued on top of the above.
    WaitingForEstimatedVBlankAndQueued(RegistrationToken),
}

pub struct PopupGrabState {
    pub root: WlSurface,
    pub grab: PopupGrab<State>,
    pub has_keyboard_grab: bool,
}

// The surfaces here are always toplevel surfaces focused as far as niri's logic is concerned, even
// when popup grabs are active (which means the real keyboard focus is on a popup descending from
// that toplevel surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyboardFocus {
    // Layout is focused by default if there's nothing else to focus.
    Layout { surface: Option<WlSurface> },
    LayerShell { surface: WlSurface },
    LockScreen { surface: Option<WlSurface> },
    ScreenshotUi,
    ExitConfirmDialog,
    Overview,
    Mru,
}

#[derive(Default, Clone, PartialEq)]
pub struct PointContents {
    // Output under point.
    pub output: Option<Output>,
    // Surface under point and its location in the global coordinate space.
    //
    // Can be `None` even when `window` is set, for example when the pointer is over the niri
    // border around the window.
    pub surface: Option<(WlSurface, Point<f64, Logical>)>,
    // If surface belongs to a window, this is that window.
    pub window: Option<(Window, HitType)>,
    // If surface belongs to a layer surface, this is that layer surface.
    pub layer: Option<LayerSurface>,
    // Pointer is over a hot corner.
    pub hot_corner: bool,
}

#[derive(Debug, Default)]
pub enum LockState {
    #[default]
    Unlocked,
    WaitingForSurfaces {
        confirmation: SessionLocker,
        deadline_token: RegistrationToken,
    },
    Locking(SessionLocker),
    Locked(ExtSessionLockV1),
}

#[derive(PartialEq, Eq)]
pub enum LockRenderState {
    /// The output displays a normal session frame.
    Unlocked,
    /// The output displays a locked frame.
    Locked,
}

// Not related to the one in Smithay.
//
// This state keeps track of when a surface last received a frame callback.
struct SurfaceFrameThrottlingState {
    /// Output and sequence that the frame callback was last sent at.
    last_sent_at: RefCell<Option<(Output, u32)>>,
}

pub enum CenterCoords {
    Separately,
    Both,
    // Force centering even if the cursor is already in the rectangle.
    BothAlways,
}

#[derive(Clone, PartialEq, Eq)]
pub enum CastTarget {
    // Dynamic cast before selecting anything.
    Nothing,
    Output {
        output: WeakOutput,
        /// Cached name of the output.
        name: String,
    },
    Window {
        id: u64,
    },
}

impl CastTarget {
    pub fn output(output: &Output) -> Self {
        Self::Output {
            output: output.downgrade(),
            name: output.name(),
        }
    }

    pub fn matches_output(&self, weak: &WeakOutput) -> bool {
        matches!(self, CastTarget::Output { output, .. } if output == weak)
    }

    pub fn matches(&self, ipc: &niri_ipc::CastTarget) -> bool {
        use CastTarget::*;
        match (self, ipc) {
            (Nothing, niri_ipc::CastTarget::Nothing {}) => true,
            (Output { name, .. }, niri_ipc::CastTarget::Output { name: ipc_name }) => {
                name == ipc_name
            }
            (Window { id }, niri_ipc::CastTarget::Window { id: ipc_id }) => id == ipc_id,
            _ => false,
        }
    }

    pub fn make_ipc(&self) -> niri_ipc::CastTarget {
        use CastTarget::*;
        match self {
            Nothing => niri_ipc::CastTarget::Nothing {},
            Output { name, .. } => niri_ipc::CastTarget::Output { name: name.clone() },
            Window { id } => niri_ipc::CastTarget::Window { id: *id },
        }
    }
}

/// Pending update to a window's focus timestamp.
#[derive(Debug)]
pub struct PendingMruCommit {
    id: MappedId,
    token: RegistrationToken,
    stamp: Duration,
}

impl RedrawState {
    fn queue_redraw(self) -> Self {
        match self {
            RedrawState::Idle => RedrawState::Queued,
            RedrawState::WaitingForEstimatedVBlank(token) => {
                RedrawState::WaitingForEstimatedVBlankAndQueued(token)
            }

            // A redraw is already queued.
            value @ (RedrawState::Queued | RedrawState::WaitingForEstimatedVBlankAndQueued(_)) => {
                value
            }

            // We're waiting for VBlank, request a redraw afterwards.
            RedrawState::WaitingForVBlank { .. } => RedrawState::WaitingForVBlank {
                redraw_needed: true,
            },
        }
    }
}

impl Default for SurfaceFrameThrottlingState {
    fn default() -> Self {
        Self {
            last_sent_at: RefCell::new(None),
        }
    }
}

impl KeyboardFocus {
    pub fn surface(&self) -> Option<&WlSurface> {
        match self {
            KeyboardFocus::Layout { surface } => surface.as_ref(),
            KeyboardFocus::LayerShell { surface } => Some(surface),
            KeyboardFocus::LockScreen { surface } => surface.as_ref(),
            KeyboardFocus::ScreenshotUi => None,
            KeyboardFocus::ExitConfirmDialog => None,
            KeyboardFocus::Overview => None,
            KeyboardFocus::Mru => None,
        }
    }

    pub fn into_surface(self) -> Option<WlSurface> {
        match self {
            KeyboardFocus::Layout { surface } => surface,
            KeyboardFocus::LayerShell { surface } => Some(surface),
            KeyboardFocus::LockScreen { surface } => surface,
            KeyboardFocus::ScreenshotUi => None,
            KeyboardFocus::ExitConfirmDialog => None,
            KeyboardFocus::Overview => None,
            KeyboardFocus::Mru => None,
        }
    }

    pub fn is_layout(&self) -> bool {
        matches!(self, KeyboardFocus::Layout { .. })
    }

    pub fn is_overview(&self) -> bool {
        matches!(self, KeyboardFocus::Overview)
    }
}

pub struct State {
    pub backend: Backend,
    pub niri: Niri,
}

impl State {
    pub fn new(
        config: Config,
        event_loop: LoopHandle<'static, State>,
        stop_signal: LoopSignal,
        display: Display<State>,
        headless: bool,
        create_wayland_socket: bool,
        is_session_instance: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let _span = tracy_client::span!("State::new");

        let config = Rc::new(RefCell::new(config));

        let has_display = env::var_os("WAYLAND_DISPLAY").is_some()
            || env::var_os("WAYLAND_SOCKET").is_some()
            || env::var_os("DISPLAY").is_some();

        let mut backend = if headless {
            let headless = Headless::new();
            Backend::Headless(headless)
        } else if has_display {
            let winit = Winit::new(config.clone(), event_loop.clone())?;
            Backend::Winit(winit)
        } else {
            let tty = Tty::new(config.clone(), event_loop.clone())
                .context("error initializing the TTY backend")?;
            Backend::Tty(tty)
        };

        let mut niri = Niri::new(
            config.clone(),
            event_loop,
            stop_signal,
            display,
            &backend,
            create_wayland_socket,
            is_session_instance,
        );
        backend.init(&mut niri);

        let mut state = Self { backend, niri };

        // Load the xkb_file config option if set by the user.
        state.load_xkb_file();
        // Initialize some IPC server state.
        state.ipc_keyboard_layouts_changed();
        // Focus the default monitor if set by the user.
        state.focus_default_monitor();

        Ok(state)
    }

    pub fn refresh_and_flush_clients(&mut self) {
        let _span = tracy_client::span!("State::refresh_and_flush_clients");

        self.refresh();

        // Advance animations to the current time (not target render time) before rendering outputs
        // in order to clear completed animations and render elements. Even if we're not rendering,
        // it's good to advance every now and then so the workspace clean-up and animations don't
        // build up (the 1 second frame callback timer will call this line).
        self.niri.advance_animations();

        self.niri.redraw_queued_outputs(&mut self.backend);

        {
            let _span = tracy_client::span!("flush_clients");
            self.niri.display_handle.flush_clients().unwrap();
        }

        #[cfg(feature = "dbus")]
        self.niri.update_locked_hint();

        // Clear the time so it's fetched afresh next iteration.
        self.niri.clock.clear();
        self.niri.pointer_inactivity_timer_got_reset = false;
        self.niri.notified_activity_this_iteration = false;
    }

    // We monitor both libinput and logind: libinput is always there (including without DBus), but
    // it misses some switch events (e.g. after unsuspend) on some systems.
    pub fn set_lid_closed(&mut self, is_closed: bool) {
        if self.niri.is_lid_closed == is_closed {
            return;
        }

        debug!("laptop lid {}", if is_closed { "closed" } else { "opened" });
        self.niri.is_lid_closed = is_closed;
        self.backend.on_output_config_changed(&mut self.niri);
    }

    fn refresh(&mut self) {
        let _span = tracy_client::span!("State::refresh");

        // Handle commits for surfaces whose blockers cleared this cycle. This should happen before
        // layout.refresh() since this is where these surfaces handle commits.
        self.notify_blocker_cleared();

        // These should be called periodically, before flushing the clients.
        self.niri.popups.cleanup();
        self.refresh_popup_grab();
        self.update_keyboard_focus();

        // Should be called before refresh_layout() because that one will refresh other window
        // states and then send a pending configure.
        self.niri.refresh_window_states();

        // Needs to be called after updating the keyboard focus.
        self.niri.refresh_layout();

        self.niri.cursor_manager.check_cursor_image_surface_alive();
        self.niri.refresh_pointer_outputs();
        self.niri.global_space.refresh();
        self.niri.refresh_idle_inhibit();
        self.refresh_pointer_contents();
        foreign_toplevel::refresh(self);
        ext_workspace::refresh(self);

        #[cfg(feature = "xdp-gnome-screencast")]
        self.niri.refresh_mapped_cast_outputs();
        // Should happen before refresh_window_rules(), but after anything that can start or stop
        // screencasts.
        #[cfg(feature = "xdp-gnome-screencast")]
        self.niri.refresh_mapped_cast_window_rules();
        self.ipc_refresh_casts();

        self.niri.refresh_window_rules();
        self.refresh_ipc_outputs();
        self.ipc_refresh_layout();
        self.ipc_refresh_keyboard_layout_index();

        // Needs to be called after updating the keyboard focus.
        #[cfg(feature = "dbus")]
        self.niri.refresh_a11y();
    }

    fn notify_blocker_cleared(&mut self) {
        let dh = self.niri.display_handle.clone();
        while let Ok(client) = self.niri.blocker_cleared_rx.try_recv() {
            trace!("calling blocker_cleared");
            self.client_compositor_state(&client)
                .blocker_cleared(self, &dh);
        }
    }

    pub fn move_cursor(&mut self, location: Point<f64, Logical>) {
        let mut under = match self.niri.pointer_visibility {
            PointerVisibility::Disabled => PointContents::default(),
            _ => self.niri.contents_under(location),
        };

        // Disable the hidden pointer if the contents underneath have changed.
        if !self.niri.pointer_visibility.is_visible() && self.niri.pointer_contents != under {
            self.niri.pointer_visibility = PointerVisibility::Disabled;

            // When setting PointerVisibility::Hidden together with pointer contents changing,
            // we can change straight to nothing to avoid one frame of hover. Notably, this can
            // be triggered through warp-mouse-to-focus combined with hide-when-typing.
            under = PointContents::default();
        }

        self.niri.pointer_contents.clone_from(&under);

        let pointer = &self.niri.seat.get_pointer().unwrap();
        pointer.motion(
            self,
            under.surface,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time: get_monotonic_time().as_millis() as u32,
            },
        );
        pointer.frame(self);

        self.niri.maybe_activate_pointer_constraint();

        // We do not show the pointer on programmatic or keyboard movement.

        // FIXME: granular
        self.niri.queue_redraw_all();
    }

    /// Moves cursor within the specified rectangle, only adjusting coordinates if needed.
    fn move_cursor_to_rect(&mut self, rect: Rectangle<f64, Logical>, mode: CenterCoords) -> bool {
        let pointer = &self.niri.seat.get_pointer().unwrap();
        let cur_loc = pointer.current_location();
        let x_in_bound = cur_loc.x >= rect.loc.x && cur_loc.x <= rect.loc.x + rect.size.w;
        let y_in_bound = cur_loc.y >= rect.loc.y && cur_loc.y <= rect.loc.y + rect.size.h;

        let p = match mode {
            CenterCoords::Separately => {
                if x_in_bound && y_in_bound {
                    return false;
                } else if y_in_bound {
                    // adjust x
                    Point::from((rect.loc.x + rect.size.w / 2.0, cur_loc.y))
                } else if x_in_bound {
                    // adjust y
                    Point::from((cur_loc.x, rect.loc.y + rect.size.h / 2.0))
                } else {
                    // adjust x and y
                    center_f64(rect)
                }
            }
            CenterCoords::Both => {
                if x_in_bound && y_in_bound {
                    return false;
                } else {
                    // adjust x and y
                    center_f64(rect)
                }
            }
            CenterCoords::BothAlways => center_f64(rect),
        };

        self.move_cursor(p);
        true
    }

    pub fn move_cursor_to_focused_tile(&mut self, mode: CenterCoords) -> bool {
        if !self.niri.keyboard_focus.is_layout() {
            return false;
        }

        if self.niri.tablet_cursor_location.is_some() {
            return false;
        }

        let Some(output) = self.niri.layout.active_output() else {
            return false;
        };
        let monitor = self.niri.layout.monitor_for_output(output).unwrap();

        let mut rv = false;
        let rect = monitor.active_window_visual_rectangle();

        if let Some(rect) = rect {
            let output_geo = self.niri.global_space.output_geometry(output).unwrap();
            let mut rect = rect;
            rect.loc += output_geo.loc.to_f64();
            rv = self.move_cursor_to_rect(rect, mode);
        }

        rv
    }

    pub fn focus_default_monitor(&mut self) {
        // Our default target is the first output in sorted order.
        let Some(mut target) = self.niri.sorted_outputs.first().cloned() else {
            // No outputs are connected.
            return;
        };

        let config = self.niri.config.borrow();
        for config in &config.outputs.0 {
            if !config.focus_at_startup {
                continue;
            }
            if let Some(output) = self.niri.output_by_name_match(&config.name) {
                target = output.clone();
                break;
            }
        }
        drop(config);

        self.niri.layout.focus_output(&target);
        self.move_cursor_to_output(&target);
    }

    /// Focus a specific window, taking care of a potential active output change and cursor
    /// warp.
    pub fn focus_window(&mut self, window: &Window) {
        let active_output = self.niri.layout.active_output().cloned();

        self.niri.layout.activate_window(window);

        let new_active = self.niri.layout.active_output().cloned();
        if new_active != active_output {
            if !self.maybe_warp_cursor_to_focus_centered() {
                self.move_cursor_to_output(&new_active.unwrap());
            }
        } else {
            self.maybe_warp_cursor_to_focus();
        }

        // FIXME: granular
        self.niri.queue_redraw_all();
    }

    pub fn confirm_mru(&mut self) {
        if let Some(window) = self.niri.close_mru(MruCloseRequest::Confirm) {
            // focus_window() will warp the cursor to the window only when the keyboard focus is on
            // the layout. However, right now the keyboard focus is still on the MRU (that we had
            // just closed) since it's only updated at the end of the event loop cycle. Force-update
            // the keyboard focus here to make cursor warping work.
            self.update_keyboard_focus();

            self.focus_window(&window);
        }
    }

    pub fn maybe_warp_cursor_to_focus(&mut self) -> bool {
        let focused = match self.niri.config.borrow().input.warp_mouse_to_focus {
            None => return false,
            Some(inner) => match inner.mode {
                None => CenterCoords::Separately,
                Some(WarpMouseToFocusMode::CenterXy) => CenterCoords::Both,
                Some(WarpMouseToFocusMode::CenterXyAlways) => CenterCoords::BothAlways,
            },
        };
        self.move_cursor_to_focused_tile(focused)
    }

    pub fn maybe_warp_cursor_to_focus_centered(&mut self) -> bool {
        let focused = match self.niri.config.borrow().input.warp_mouse_to_focus {
            None => return false,
            Some(inner) => match inner.mode {
                None => CenterCoords::Both,
                Some(WarpMouseToFocusMode::CenterXy) => CenterCoords::Both,
                Some(WarpMouseToFocusMode::CenterXyAlways) => CenterCoords::BothAlways,
            },
        };
        self.move_cursor_to_focused_tile(focused)
    }

    pub fn refresh_pointer_contents(&mut self) {
        let _span = tracy_client::span!("Niri::refresh_pointer_contents");

        let pointer = &self.niri.seat.get_pointer().unwrap();
        let location = pointer.current_location();

        if !self.niri.exit_confirm_dialog.is_open()
            && !self.niri.is_locked()
            && !self.niri.screenshot_ui.is_open()
        {
            // Don't refresh cursor focus during transitions.
            if let Some((output, _)) = self.niri.output_under(location) {
                let monitor = self.niri.layout.monitor_for_output(output).unwrap();
                if monitor.are_transitions_ongoing() {
                    return;
                }
            }
        }

        if !self.update_pointer_contents() {
            return;
        }

        pointer.frame(self);

        // Pointer motion from a surface to nothing triggers a cursor change to default, which
        // means we may need to redraw.

        // FIXME: granular
        self.niri.queue_redraw_all();
    }

    pub fn update_pointer_contents(&mut self) -> bool {
        let _span = tracy_client::span!("Niri::update_pointer_contents");

        let pointer = &self.niri.seat.get_pointer().unwrap();
        let location = pointer.current_location();
        let mut under = match self.niri.pointer_visibility {
            PointerVisibility::Disabled => PointContents::default(),
            _ => self.niri.contents_under(location),
        };

        // We're not changing the global cursor location here, so if the contents did not change,
        // then nothing changed.
        if self.niri.pointer_contents == under {
            return false;
        }

        // Disable the hidden pointer if the contents underneath have changed.
        if !self.niri.pointer_visibility.is_visible() {
            self.niri.pointer_visibility = PointerVisibility::Disabled;

            // When setting PointerVisibility::Hidden together with pointer contents changing,
            // we can change straight to nothing to avoid one frame of hover. Notably, this can
            // be triggered through warp-mouse-to-focus combined with hide-when-typing.
            under = PointContents::default();
            if self.niri.pointer_contents == under {
                return false;
            }
        }

        self.niri.pointer_contents.clone_from(&under);

        pointer.motion(
            self,
            under.surface,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time: get_monotonic_time().as_millis() as u32,
            },
        );

        self.niri.maybe_activate_pointer_constraint();

        true
    }

    pub fn move_cursor_to_output(&mut self, output: &Output) {
        let geo = self.niri.global_space.output_geometry(output).unwrap();
        self.move_cursor(center(geo).to_f64());
    }

    pub fn refresh_popup_grab(&mut self) {
        if let Some(grab) = &mut self.niri.popup_grab {
            if grab.grab.has_ended() {
                self.niri.popup_grab = None;
            }
        }
    }

    pub fn update_keyboard_focus(&mut self) {
        // Clean up on-demand layer surface focus if necessary.
        if let Some(surface) = &self.niri.layer_shell_on_demand_focus {
            // Still alive and has on-demand interactivity.
            let mut good = surface.alive()
                && surface.cached_state().keyboard_interactivity
                    == wlr_layer::KeyboardInteractivity::OnDemand;

            if let Some(mapped) = self.niri.mapped_layer_surfaces.get(surface) {
                // Check if it moved to the overview backdrop.
                if mapped.place_within_backdrop() {
                    good = false;
                }
            } else {
                // The layer surface is alive but it got unmapped.
                good = false;
            }

            if !good {
                self.niri.layer_shell_on_demand_focus = None;
            }
        }

        // Compute the current focus.
        let focus = if self.niri.exit_confirm_dialog.is_open() {
            KeyboardFocus::ExitConfirmDialog
        } else if self.niri.is_locked() {
            KeyboardFocus::LockScreen {
                surface: self.niri.lock_surface_focus(),
            }
        } else if self.niri.screenshot_ui.is_open() {
            KeyboardFocus::ScreenshotUi
        } else if self.niri.window_mru_ui.is_open() {
            KeyboardFocus::Mru
        } else if let Some(output) = self.niri.layout.active_output() {
            let mon = self.niri.layout.monitor_for_output(output).unwrap();
            let layers = layer_map_for_output(output);

            // Explicitly check for layer-shell popup grabs here, our keyboard focus will stay on
            // the root layer surface while it has grabs.
            let layer_grab = self.niri.popup_grab.as_ref().and_then(|g| {
                layers
                    .layer_for_surface(&g.root, WindowSurfaceType::TOPLEVEL)
                    .and_then(|l| l.can_receive_keyboard_focus().then(|| (&g.root, l.layer())))
            });
            let grab_on_layer = |layer: Layer| {
                layer_grab
                    .and_then(move |(s, l)| if l == layer { Some(s.clone()) } else { None })
                    .map(|surface| KeyboardFocus::LayerShell { surface })
            };

            let layout_focus = || {
                self.niri
                    .layout
                    .focus()
                    .map(|win| win.toplevel().wl_surface().clone())
                    .map(|surface| KeyboardFocus::Layout {
                        surface: Some(surface),
                    })
            };

            let excl_focus_on_layer = |layer| {
                layers.layers_on(layer).find_map(|surface| {
                    if surface.cached_state().keyboard_interactivity
                        != wlr_layer::KeyboardInteractivity::Exclusive
                    {
                        return None;
                    }

                    let mapped = self.niri.mapped_layer_surfaces.get(surface)?;
                    if mapped.place_within_backdrop() {
                        return None;
                    }

                    let surface = surface.wl_surface().clone();
                    Some(KeyboardFocus::LayerShell { surface })
                })
            };

            let on_d_focus_on_layer = |layer| {
                layers.layers_on(layer).find_map(|surface| {
                    let is_on_demand_surface =
                        Some(surface) == self.niri.layer_shell_on_demand_focus.as_ref();
                    is_on_demand_surface
                        .then(|| surface.wl_surface().clone())
                        .map(|surface| KeyboardFocus::LayerShell { surface })
                })
            };

            // Prefer exclusive focus on a layer, then check on-demand focus.
            let focus_on_layer =
                |layer| excl_focus_on_layer(layer).or_else(|| on_d_focus_on_layer(layer));

            let is_overview_open = self.niri.layout.is_overview_open();

            let mut surface = grab_on_layer(Layer::Overlay);
            // FIXME: we shouldn't prioritize the top layer grabs over regular overlay input or a
            // fullscreen layout window. This will need tracking in grab() to avoid handing it out
            // in the first place. Or a better way to structure this code.
            surface = surface.or_else(|| grab_on_layer(Layer::Top));

            if !is_overview_open {
                surface = surface.or_else(|| grab_on_layer(Layer::Bottom));
                surface = surface.or_else(|| grab_on_layer(Layer::Background));
            }

            surface = surface.or_else(|| focus_on_layer(Layer::Overlay));

            if mon.render_above_top_layer() {
                surface = surface.or_else(layout_focus);
                surface = surface.or_else(|| focus_on_layer(Layer::Top));
                surface = surface.or_else(|| focus_on_layer(Layer::Bottom));
                surface = surface.or_else(|| focus_on_layer(Layer::Background));
            } else {
                surface = surface.or_else(|| focus_on_layer(Layer::Top));

                if is_overview_open {
                    surface = Some(surface.unwrap_or(KeyboardFocus::Overview));
                }

                surface = surface.or_else(|| on_d_focus_on_layer(Layer::Bottom));
                surface = surface.or_else(|| on_d_focus_on_layer(Layer::Background));
                surface = surface.or_else(layout_focus);

                // Bottom and background layers can only receive exclusive focus when there are no
                // layout windows.
                surface = surface.or_else(|| excl_focus_on_layer(Layer::Bottom));
                surface = surface.or_else(|| excl_focus_on_layer(Layer::Background));
            }

            surface.unwrap_or(KeyboardFocus::Layout { surface: None })
        } else {
            KeyboardFocus::Layout { surface: None }
        };

        let keyboard = self.niri.seat.get_keyboard().unwrap();
        if self.niri.keyboard_focus != focus {
            trace!(
                "keyboard focus changed from {:?} to {:?}",
                self.niri.keyboard_focus,
                focus
            );

            // Tell the windows their new focus state for window rule purposes.
            if let KeyboardFocus::Layout {
                surface: Some(surface),
            } = &self.niri.keyboard_focus
            {
                if let Some((mapped, _)) = self.niri.layout.find_window_and_output_mut(surface) {
                    mapped.set_is_focused(false);
                }
            }
            if let KeyboardFocus::Layout {
                surface: Some(surface),
            } = &focus
            {
                if let Some((mapped, _)) = self.niri.layout.find_window_and_output_mut(surface) {
                    mapped.set_is_focused(true);

                    // If `mapped` does not have a focus timestamp, then the window is newly
                    // created/mapped and a timestamp is unconditionally created.
                    //
                    // If `mapped` already has a timestamp only update it after the focus lock-in
                    // period has gone by without the focus having elsewhere.
                    let stamp = get_monotonic_time();

                    let debounce = self.niri.config.borrow().recent_windows.debounce_ms;
                    let debounce = Duration::from_millis(u64::from(debounce));

                    if mapped.get_focus_timestamp().is_none() || debounce.is_zero() {
                        mapped.set_focus_timestamp(stamp);
                    } else {
                        let timer = Timer::from_duration(debounce);

                        let focus_token = self
                            .niri
                            .event_loop
                            .insert_source(timer, move |_, _, state| {
                                state.niri.mru_apply_keyboard_commit();
                                TimeoutAction::Drop
                            })
                            .unwrap();
                        if let Some(PendingMruCommit { token, .. }) =
                            self.niri.pending_mru_commit.replace(PendingMruCommit {
                                id: mapped.id(),
                                token: focus_token,
                                stamp,
                            })
                        {
                            self.niri.event_loop.remove(token);
                        }
                    }
                }
            }

            if let Some(grab) = self.niri.popup_grab.as_mut() {
                if grab.has_keyboard_grab && Some(&grab.root) != focus.surface() {
                    trace!(
                        "grab root {:?} is not the new focus {:?}, ungrabbing",
                        grab.root,
                        focus
                    );

                    grab.grab.ungrab(PopupUngrabStrategy::All);
                    keyboard.unset_grab(self);
                    self.niri.seat.get_pointer().unwrap().unset_grab(
                        self,
                        SERIAL_COUNTER.next_serial(),
                        get_monotonic_time().as_millis() as u32,
                    );
                    self.niri.popup_grab = None;
                }
            }

            if self.niri.config.borrow().input.keyboard.track_layout == TrackLayout::Window {
                let current_layout = keyboard.with_xkb_state(self, |context| {
                    let xkb = context.xkb().lock().unwrap();
                    xkb.active_layout()
                });

                let mut new_layout = current_layout;
                // Store the currently active layout for the surface.
                if let Some(current_focus) = self.niri.keyboard_focus.surface() {
                    with_states(current_focus, |data| {
                        let cell = data
                            .data_map
                            .get_or_insert::<Cell<KeyboardLayout>, _>(Cell::default);
                        cell.set(current_layout);
                    });
                }

                if let Some(focus) = focus.surface() {
                    new_layout = with_states(focus, |data| {
                        let cell = data.data_map.get_or_insert::<Cell<KeyboardLayout>, _>(|| {
                            // The default layout is effectively the first layout in the
                            // keymap, so use it for new windows.
                            Cell::new(KeyboardLayout::default())
                        });
                        cell.get()
                    });
                }
                if new_layout != current_layout && focus.surface().is_some() {
                    keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
                    keyboard.with_xkb_state(self, |mut context| {
                        context.set_layout(new_layout);
                    });
                }
            }

            self.niri.keyboard_focus.clone_from(&focus);
            keyboard.set_focus(self, focus.into_surface(), SERIAL_COUNTER.next_serial());

            // FIXME: can be more granular.
            self.niri.queue_redraw_all();
        }
    }

    /// Loads the xkb keymap from a file config setting.
    fn set_xkb_file(&mut self, xkb_file: String) -> anyhow::Result<()> {
        let xkb_file = PathBuf::from(xkb_file);
        let xkb_file = expand_home(&xkb_file)
            .context("failed to expand ~")?
            .unwrap_or(xkb_file);

        let keymap = std::fs::read_to_string(xkb_file).context("failed to read xkb_file")?;

        let keyboard = self.niri.seat.get_keyboard().unwrap();
        let num_lock = keyboard.modifier_state().num_lock;

        keyboard
            .set_keymap_from_string(self, keymap)
            .context("failed to set keymap")?;

        // Restore num lock to its previous value.
        let mut mods_state = keyboard.modifier_state();
        if mods_state.num_lock != num_lock {
            mods_state.num_lock = num_lock;
            keyboard.set_modifier_state(mods_state);
        }

        Ok(())
    }

    fn load_xkb_file(&mut self) {
        let xkb_file = self.niri.config.borrow().input.keyboard.xkb.file.clone();
        if let Some(xkb_file) = xkb_file {
            if let Err(err) = self.set_xkb_file(xkb_file) {
                warn!("error loading xkb_file: {err:?}");
            }
        }
    }

    pub fn set_xkb_config(&mut self, xkb: XkbConfig) {
        let keyboard = self.niri.seat.get_keyboard().unwrap();
        let num_lock = keyboard.modifier_state().num_lock;
        if let Err(err) = keyboard.set_xkb_config(self, xkb) {
            warn!("error updating xkb config: {err:?}");
            return;
        }

        // Restore num lock to its previous value.
        let mut mods_state = keyboard.modifier_state();
        if mods_state.num_lock != num_lock {
            mods_state.num_lock = num_lock;
            keyboard.set_modifier_state(mods_state);
        }
    }

    pub fn reload_config(&mut self, config: Result<Config, ()>) {
        let _span = tracy_client::span!("State::reload_config");

        let mut config = match config {
            Ok(config) => config,
            Err(()) => {
                self.niri.config_error_notification.show();
                self.niri.queue_redraw_all();

                #[cfg(feature = "dbus")]
                self.niri.a11y_announce_config_error();

                return;
            }
        };

        self.niri.config_error_notification.hide();

        // Find & orphan removed named workspaces.
        let mut removed_workspaces: Vec<String> = vec![];
        for ws in &self.niri.config.borrow().workspaces {
            if !config.workspaces.iter().any(|w| w.name == ws.name) {
                removed_workspaces.push(ws.name.0.clone());
            }
        }
        for name in removed_workspaces {
            self.niri.layout.unname_workspace(&name);
        }

        self.niri.layout.update_config(&config);
        for mapped in self.niri.mapped_layer_surfaces.values_mut() {
            mapped.update_config(&config);
        }

        // Create new named workspaces.
        for ws_config in &config.workspaces {
            self.niri.layout.ensure_named_workspace(ws_config);
        }

        let rate = 1.0 / config.animations.slowdown.max(0.001);
        self.niri.clock.set_rate(rate);
        self.niri
            .clock
            .set_complete_instantly(config.animations.off);

        *CHILD_ENV.write().unwrap() = mem::take(&mut config.environment);

        let mut reload_xkb = None;
        let mut libinput_config_changed = false;
        let mut output_config_changed = false;
        let mut preserved_output_config = None;
        let mut window_rules_changed = false;
        let mut layer_rules_changed = false;
        let mut shaders_changed = false;
        let mut cursor_inactivity_timeout_changed = false;
        let mut recent_windows_changed = false;
        let mut xwls_changed = false;
        let mut old_config = self.niri.config.borrow_mut();

        // Reload the cursor.
        if config.cursor != old_config.cursor {
            self.niri
                .cursor_manager
                .reload(&config.cursor.xcursor_theme, config.cursor.xcursor_size);
            self.niri.cursor_texture_cache.clear();
        }

        // We need &mut self to reload the xkb config, so just store it here.
        if config.input.keyboard.xkb != old_config.input.keyboard.xkb {
            reload_xkb = Some(config.input.keyboard.xkb.clone());
        }

        // Reload the repeat info.
        if config.input.keyboard.repeat_rate != old_config.input.keyboard.repeat_rate
            || config.input.keyboard.repeat_delay != old_config.input.keyboard.repeat_delay
        {
            let keyboard = self.niri.seat.get_keyboard().unwrap();
            keyboard.change_repeat_info(
                config.input.keyboard.repeat_rate.into(),
                config.input.keyboard.repeat_delay.into(),
            );
        }

        if config.input.touchpad != old_config.input.touchpad
            || config.input.mouse != old_config.input.mouse
            || config.input.trackball != old_config.input.trackball
            || config.input.trackpoint != old_config.input.trackpoint
            || config.input.tablet != old_config.input.tablet
            || config.input.touch != old_config.input.touch
        {
            libinput_config_changed = true;
        }

        let ignored_nodes_changed =
            config.debug.ignored_drm_devices != old_config.debug.ignored_drm_devices;

        if config.outputs != self.niri.config_file_output_config {
            output_config_changed = true;
            self.niri
                .config_file_output_config
                .clone_from(&config.outputs);
        } else {
            // Output config did not change from the last disk load, so we need to preserve the
            // transient changes.
            preserved_output_config = Some(mem::take(&mut old_config.outputs));
        }

        let binds_changed = config.binds != old_config.binds;
        let new_mod_key = self.backend.mod_key(&config);
        if new_mod_key != self.backend.mod_key(&old_config) || binds_changed {
            self.niri
                .hotkey_overlay
                .on_hotkey_config_updated(new_mod_key);
            self.niri.mods_with_mouse_binds = mods_with_mouse_binds(new_mod_key, &config.binds);
            self.niri.mods_with_wheel_binds = mods_with_wheel_binds(new_mod_key, &config.binds);
            self.niri.mods_with_tablet_stylus_binds =
                mods_with_tablet_stylus_binds(new_mod_key, &config.binds);
            self.niri.mods_with_finger_scroll_binds =
                mods_with_finger_scroll_binds(new_mod_key, &config.binds);
        }

        if config.window_rules != old_config.window_rules {
            window_rules_changed = true;
        }

        if config.layer_rules != old_config.layer_rules {
            layer_rules_changed = true;
        }

        if config.animations.window_resize.custom_shader
            != old_config.animations.window_resize.custom_shader
        {
            let src = config.animations.window_resize.custom_shader.as_deref();
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_custom_resize_program(renderer, src);
            });
            shaders_changed = true;
        }

        if config.animations.window_close.custom_shader
            != old_config.animations.window_close.custom_shader
        {
            let src = config.animations.window_close.custom_shader.as_deref();
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_custom_close_program(renderer, src);
            });
            shaders_changed = true;
        }

        if config.animations.window_open.custom_shader
            != old_config.animations.window_open.custom_shader
        {
            let src = config.animations.window_open.custom_shader.as_deref();
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_custom_open_program(renderer, src);
            });
            shaders_changed = true;
        }

        if config.global_shader.enable != old_config.global_shader.enable
            || config.global_shader.source != old_config.global_shader.source
            || config.global_shader.path != old_config.global_shader.path
            || config.global_shader.mode != old_config.global_shader.mode
            || config.global_shader.redraw != old_config.global_shader.redraw
            || config.global_shader.cursor_radius != old_config.global_shader.cursor_radius
            || config.global_shader.passes != old_config.global_shader.passes
        {
            let passes = global_shader_pass_sources(&config.global_shader);
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_custom_global_passes(renderer, &passes);
            });
            // Reset per-output time origin and ALL per-pass feedback state on (re)load. The chain
            // is re-sized next frame from the new chain length (GlobalShaderChain::resize).
            for state in self.niri.output_state.values_mut() {
                state.global_shader_start.set(None);
                *state.global_shader_chain.borrow_mut() = GlobalShaderChain::default();
                state.global_shader_screen_prev = None;
                *state.global_shader_screen_result.borrow_mut() = None;
            }
            // Recompute capability flags on next use.
            self.niri.global_shader_caps.set(None);
            shaders_changed = true;
        }

        if config.region_shaders != old_config.region_shaders
            || config.window_rules != old_config.window_rules
        {
            let chains = scoped_shader_chains(&config);
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_scoped_programs(renderer, &chains);
            });
            shaders_changed = true;
        }

        if config.cursor.hide_after_inactive_ms != old_config.cursor.hide_after_inactive_ms {
            cursor_inactivity_timeout_changed = true;
        }

        if config.debug.keep_laptop_panel_on_when_lid_is_closed
            != old_config.debug.keep_laptop_panel_on_when_lid_is_closed
        {
            output_config_changed = true;
        }

        if config.debug.ignored_drm_devices != old_config.debug.ignored_drm_devices {
            output_config_changed = true;
        }

        // FIXME: move backdrop rendering into layout::Monitor, then this will become unnecessary.
        if config.overview.backdrop_color != old_config.overview.backdrop_color {
            output_config_changed = true;
        }
        if config.layout.background_color != old_config.layout.background_color {
            output_config_changed = true;
        }

        if config.recent_windows != old_config.recent_windows {
            recent_windows_changed = true;
        }

        if config.xwayland_satellite != old_config.xwayland_satellite {
            xwls_changed = true;
        }

        *old_config = config;

        if let Some(outputs) = preserved_output_config {
            old_config.outputs = outputs;
        }

        // Release the borrow.
        drop(old_config);

        // Now with a &mut self we can reload the xkb config.
        if let Some(mut xkb) = reload_xkb {
            let mut set_xkb_config = true;

            // It's fine to .take() the xkb file, as this is a
            // clone and the file field is not used in the XkbConfig.
            if let Some(xkb_file) = xkb.file.take() {
                if let Err(err) = self.set_xkb_file(xkb_file) {
                    warn!("error reloading xkb_file: {err:?}");
                } else {
                    // We successfully set xkb file so we don't need to fallback to XkbConfig.
                    set_xkb_config = false;
                }
            }

            if set_xkb_config {
                // If xkb is unset in the niri config, use settings from locale1.
                if xkb == Xkb::default() {
                    trace!("using xkb from locale1");
                    xkb = self.niri.xkb_from_locale1.clone().unwrap_or_default();
                }

                self.set_xkb_config(xkb.to_xkb_config());
            }

            self.ipc_keyboard_layouts_changed();
        }

        if libinput_config_changed {
            let config = self.niri.config.borrow();
            for mut device in self.niri.devices.iter().cloned() {
                apply_libinput_settings(
                    &config.input,
                    &mut device,
                    self.niri.touchpad_disabled_by_toggle,
                    self.niri.dwt_disabled_by_toggle,
                );
            }
        }

        if ignored_nodes_changed {
            self.backend.update_ignored_nodes_config(&mut self.niri);
        }

        if output_config_changed {
            self.reload_output_config();
        }

        if window_rules_changed {
            self.niri.recompute_window_rules();
        }

        if layer_rules_changed {
            self.niri.recompute_layer_rules();
        }

        if shaders_changed {
            self.niri.update_shaders();
        }

        if cursor_inactivity_timeout_changed {
            // Force reset due to timeout change.
            self.niri.pointer_inactivity_timer_got_reset = false;
            self.niri.reset_pointer_inactivity_timer();
        }

        if binds_changed {
            self.niri.window_mru_ui.update_binds();
        }

        if recent_windows_changed {
            self.niri.window_mru_ui.update_config();
        }

        if xwls_changed {
            // If xwl-s was previously working and is now off, we don't try to kill it or stop
            // watching the sockets, for simplicity's sake.
            let was_working = self.niri.satellite.is_some();

            // Try to start, or restart in case the user corrected the path or something.
            xwayland::satellite::setup(self);

            let config = self.niri.config.borrow();
            let display_name = (!config.xwayland_satellite.off)
                .then_some(self.niri.satellite.as_ref())
                .flatten()
                .map(|satellite| satellite.display_name().to_owned());

            if let Some(name) = &display_name {
                if !was_working {
                    info!("listening on X11 socket: {name}");
                }
            }

            // This won't change the systemd environment, but oh well.
            *CHILD_DISPLAY.write().unwrap() = display_name;
        }

        // Can't really update xdg-decoration settings since we have to hide the globals for CSD
        // due to the SDL2 bug... I don't imagine clients are prepared for the xdg-decoration
        // global suddenly appearing? Either way, right now it's live-reloaded in a sense that new
        // clients will use the new xdg-decoration setting.

        self.niri.queue_redraw_all();
    }

    pub fn reload_output_config(&mut self) {
        let mut resized_outputs = vec![];
        let mut recolored_outputs = vec![];

        for output in self.niri.global_space.outputs() {
            let name = output.user_data().get::<OutputName>().unwrap();
            let full_config = self.niri.config.borrow_mut();
            let config = full_config.outputs.find(name);

            let scale = config
                .and_then(|c| c.scale)
                .map(|s| s.0)
                .unwrap_or_else(|| {
                    let size_mm = output.physical_properties().size;
                    let resolution = output.current_mode().unwrap().size;
                    guess_monitor_scale(size_mm, resolution)
                });
            let scale = closest_representable_scale(scale.clamp(0.1, 10.));

            let mut transform = panel_orientation(output)
                + config
                    .map(|c| ipc_transform_to_smithay(c.transform))
                    .unwrap_or(Transform::Normal);
            // FIXME: fix winit damage on other transforms.
            if name.connector == "winit" {
                transform = Transform::Flipped180;
            }

            if output.current_scale().fractional_scale() != scale
                || output.current_transform() != transform
            {
                output.change_current_state(
                    None,
                    Some(transform),
                    Some(output::Scale::Fractional(scale)),
                    None,
                );
                self.niri.ipc_outputs_changed = true;
                resized_outputs.push(output.clone());
            }

            let mut backdrop_color = config
                .and_then(|c| c.backdrop_color)
                .unwrap_or(full_config.overview.backdrop_color)
                .to_array_unpremul();
            backdrop_color[3] = 1.;
            let backdrop_color = Color32F::from(backdrop_color);

            if let Some(state) = self.niri.output_state.get_mut(output) {
                if state.backdrop_buffer.color() != backdrop_color {
                    state.backdrop_buffer.set_color(backdrop_color);
                    recolored_outputs.push(output.clone());
                }
            }

            for mon in self.niri.layout.monitors_mut() {
                if mon.output() != output {
                    continue;
                }

                let mut layout_config = config.and_then(|c| c.layout.clone());
                // Support the deprecated non-layout background-color key.
                if let Some(layout) = &mut layout_config {
                    if layout.background_color.is_none() {
                        layout.background_color = config.and_then(|c| c.background_color);
                    }
                }

                if mon.update_layout_config(layout_config) {
                    // Also redraw these; if anything, the background color could've changed.
                    recolored_outputs.push(output.clone());
                }
                break;
            }
        }

        for output in resized_outputs {
            self.niri.output_resized(&output);
        }

        for output in recolored_outputs {
            self.niri.queue_redraw(&output);
        }

        self.backend.on_output_config_changed(&mut self.niri);

        self.niri.reposition_outputs(None);

        if let Some(touch) = self.niri.seat.get_touch() {
            touch.cancel(self);
        }

        let config = self.niri.config.borrow().outputs.clone();
        self.niri.output_management_state.on_config_changed(config);
    }

    pub fn modify_output_config<F>(&mut self, name: &str, fun: F)
    where
        F: FnOnce(&mut niri_config::Output),
    {
        // Try hard to find the output config section corresponding to the output set by the
        // user. Since if we add a new section and some existing section also matches the
        // output, then our new section won't do anything.
        let temp;
        let match_name = if let Some(output) = self.niri.output_by_name_match(name) {
            output.user_data().get::<OutputName>().unwrap()
        } else if let Some(output_name) = self
            .backend
            .tty_checked()
            .and_then(|tty| tty.disconnected_connector_name_by_name_match(name))
        {
            temp = output_name;
            &temp
        } else {
            // Even if name is "make model serial", matching will work fine this way.
            temp = OutputName {
                connector: name.to_owned(),
                make: None,
                model: None,
                serial: None,
            };
            &temp
        };

        let mut config = self.niri.config.borrow_mut();
        let config = if let Some(config) = config.outputs.find_mut(match_name) {
            config
        } else {
            config.outputs.0.push(niri_config::Output {
                // Save name as set by the user.
                name: String::from(name),
                ..Default::default()
            });
            config.outputs.0.last_mut().unwrap()
        };

        fun(config);
    }

    pub fn apply_transient_output_config(&mut self, name: &str, action: niri_ipc::OutputAction) {
        self.modify_output_config(name, move |config| match action {
            niri_ipc::OutputAction::Off => config.off = true,
            niri_ipc::OutputAction::On => config.off = false,
            niri_ipc::OutputAction::Mode { mode } => {
                config.mode = match mode {
                    niri_ipc::ModeToSet::Automatic => None,
                    niri_ipc::ModeToSet::Specific(mode) => Some(niri_config::output::Mode {
                        custom: false,
                        mode,
                    }),
                };
                config.modeline = None;
            }
            niri_ipc::OutputAction::CustomMode { mode } => {
                config.mode = Some(niri_config::output::Mode { custom: true, mode });
                config.modeline = None;
            }
            niri_ipc::OutputAction::Modeline {
                clock,
                hdisplay,
                hsync_start,
                hsync_end,
                htotal,
                vdisplay,
                vsync_start,
                vsync_end,
                vtotal,
                hsync_polarity,
                vsync_polarity,
            } => {
                // Do not reset config.mode to None since it's used as a fallback.
                config.modeline = Some(niri_config::output::Modeline {
                    clock,
                    hdisplay,
                    hsync_start,
                    hsync_end,
                    htotal,
                    vdisplay,
                    vsync_start,
                    vsync_end,
                    vtotal,
                    hsync_polarity,
                    vsync_polarity,
                })
            }
            niri_ipc::OutputAction::Scale { scale } => {
                config.scale = match scale {
                    niri_ipc::ScaleToSet::Automatic => None,
                    niri_ipc::ScaleToSet::Specific(scale) => Some(FloatOrInt(scale)),
                }
            }
            niri_ipc::OutputAction::Transform { transform } => config.transform = transform,
            niri_ipc::OutputAction::Position { position } => {
                config.position = match position {
                    niri_ipc::PositionToSet::Automatic => None,
                    niri_ipc::PositionToSet::Specific(position) => Some(niri_config::Position {
                        x: position.x,
                        y: position.y,
                    }),
                }
            }
            niri_ipc::OutputAction::Vrr { vrr } => {
                config.variable_refresh_rate = if vrr.vrr {
                    Some(niri_config::Vrr {
                        on_demand: vrr.on_demand,
                    })
                } else {
                    None
                }
            }
            niri_ipc::OutputAction::MaxBpc { max_bpc } => config.max_bpc = Some(MaxBpc(max_bpc)),
        });

        self.reload_output_config();
    }

    pub fn refresh_ipc_outputs(&mut self) {
        if !self.niri.ipc_outputs_changed {
            return;
        }
        self.niri.ipc_outputs_changed = false;

        let _span = tracy_client::span!("State::refresh_ipc_outputs");

        for ipc_output in self.backend.ipc_outputs().lock().unwrap().values_mut() {
            let logical = self
                .niri
                .global_space
                .outputs()
                .find(|output| output.name() == ipc_output.name)
                .map(logical_output);
            ipc_output.logical = logical;
        }

        #[cfg(feature = "dbus")]
        self.niri.on_ipc_outputs_changed();

        let new_config = self.backend.ipc_outputs().lock().unwrap().clone();
        self.niri.output_management_state.notify_changes(new_config);
    }

    pub fn open_screenshot_ui(&mut self, show_pointer: bool, path: Option<String>) {
        if self.niri.is_locked() || self.niri.screenshot_ui.is_open() {
            return;
        }

        let default_output = self
            .niri
            .output_under_cursor()
            .or_else(|| self.niri.layout.active_output().cloned());
        let Some(default_output) = default_output else {
            return;
        };

        self.niri.update_render_elements(None);

        let Some(screenshots) = self
            .backend
            .with_primary_renderer(|renderer| self.niri.capture_screenshots(renderer).collect())
        else {
            return;
        };

        // Now that we captured the screenshots, clear grabs like drag-and-drop, etc.
        self.niri.seat.get_pointer().unwrap().unset_grab(
            self,
            SERIAL_COUNTER.next_serial(),
            get_monotonic_time().as_millis() as u32,
        );
        if let Some(touch) = self.niri.seat.get_touch() {
            touch.unset_grab(self);
        }

        self.backend.with_primary_renderer(|renderer| {
            self.niri
                .screenshot_ui
                .open(renderer, screenshots, default_output, show_pointer, path)
        });

        self.niri
            .cursor_manager
            .set_cursor_image(CursorImageStatus::Named(CursorIcon::Crosshair));
        self.niri.queue_redraw_all();
    }

    pub fn handle_pick_color(&mut self, tx: async_channel::Sender<Option<niri_ipc::PickedColor>>) {
        let pointer = self.niri.seat.get_pointer().unwrap();
        let start_data = PointerGrabStartData {
            focus: None,
            button: 0,
            location: pointer.current_location(),
        };
        let grab = PickColorGrab::new(start_data);
        pointer.set_grab(self, grab, SERIAL_COUNTER.next_serial(), Focus::Clear);
        self.niri.pick_color = Some(tx);
        self.niri
            .cursor_manager
            .set_cursor_image(CursorImageStatus::Named(CursorIcon::Crosshair));
        self.niri.queue_redraw_all();
    }

    pub fn confirm_screenshot(&mut self, write_to_disk: bool) {
        let ScreenshotUi::Open { path, .. } = &mut self.niri.screenshot_ui else {
            return;
        };
        let path = path.take();

        self.backend.with_primary_renderer(|renderer| {
            match self.niri.screenshot_ui.capture(renderer) {
                Ok((size, pixels)) => {
                    if let Err(err) = self.niri.save_screenshot(size, pixels, write_to_disk, path) {
                        warn!("error saving screenshot: {err:?}");
                    }
                }
                Err(err) => {
                    warn!("error capturing screenshot: {err:?}");
                }
            }
        });

        self.niri.screenshot_ui.close();
        self.niri
            .cursor_manager
            .set_cursor_image(CursorImageStatus::default_named());
        self.niri.queue_redraw_all();
    }

    pub fn store_unmap_snapshot(&mut self, window: &Window, output: Option<&Output>) {
        // The unmapping tile may have an xray background, in which case we will render xray
        // elements, so they need to be updated.
        self.niri.update_xray_render_elements(output);

        self.backend.with_primary_renderer(|renderer| {
            if let Some(output) = output {
                let mut ctx = RenderCtx {
                    target: RenderTarget::Output,
                    renderer,
                    xray: None,
                    // Feeds a static unmap snapshot (Tile::render_snapshot uses shader_time 0).
                    shader_time: 0.0,
                };

                self.niri.fill_xray_elements(ctx.r(), output);

                // If any background layer has block_out_from, also fill the Screencast xray
                // buffer so the unmap snapshot can render a buffer with blocked-out background.
                //
                // This will be used in Tile::render_snapshot().
                let has_blocked_out = self.niri.has_blocked_out_background_layers(output);
                if has_blocked_out {
                    let screencast_ctx = RenderCtx {
                        target: RenderTarget::Screencast,
                        ..ctx.r()
                    };
                    self.niri.fill_xray_elements(screencast_ctx, output);
                }

                let state = self.niri.output_state.get_mut(output).unwrap();
                self.niri.layout.store_unmap_snapshot(
                    renderer,
                    Some(&mut state.xray),
                    has_blocked_out,
                    window,
                );

                self.niri.clear_xray_elements(output);
            } else {
                self.niri
                    .layout
                    .store_unmap_snapshot(renderer, None, false, window);
            }
        });
    }

    #[cfg(not(feature = "xdp-gnome-screencast"))]
    pub fn set_dynamic_cast_target(&mut self, _target: CastTarget) {}

    #[cfg(feature = "dbus")]
    pub fn on_screen_shot_msg(
        &mut self,
        to_screenshot: &async_channel::Sender<NiriToScreenshot>,
        msg: ScreenshotToNiri,
    ) {
        match msg {
            ScreenshotToNiri::TakeScreenshot { include_cursor } => {
                self.handle_take_screenshot(to_screenshot, include_cursor);
            }
            ScreenshotToNiri::PickColor(tx) => {
                self.handle_pick_color(tx);
            }
        }
    }

    #[cfg(feature = "dbus")]
    fn handle_take_screenshot(
        &mut self,
        to_screenshot: &async_channel::Sender<NiriToScreenshot>,
        include_cursor: bool,
    ) {
        let _span = tracy_client::span!("TakeScreenshot");

        let rv = self.backend.with_primary_renderer(|renderer| {
            let on_done = {
                let to_screenshot = to_screenshot.clone();
                move |path| {
                    let msg = NiriToScreenshot::ScreenshotResult(Some(path));
                    if let Err(err) = to_screenshot.send_blocking(msg) {
                        warn!("error sending path to screenshot: {err:?}");
                    }
                }
            };

            let res = self
                .niri
                .screenshot_all_outputs(renderer, include_cursor, on_done);

            if let Err(err) = res {
                warn!("error taking a screenshot: {err:?}");

                let msg = NiriToScreenshot::ScreenshotResult(None);
                if let Err(err) = to_screenshot.send_blocking(msg) {
                    warn!("error sending None to screenshot: {err:?}");
                }
            }
        });

        if rv.is_none() {
            let msg = NiriToScreenshot::ScreenshotResult(None);
            if let Err(err) = to_screenshot.send_blocking(msg) {
                warn!("error sending None to screenshot: {err:?}");
            }
        }
    }

    #[cfg(feature = "dbus")]
    pub fn on_introspect_msg(
        &mut self,
        to_introspect: &async_channel::Sender<NiriToIntrospect>,
        msg: IntrospectToNiri,
    ) {
        use crate::utils::with_toplevel_role;

        let IntrospectToNiri::GetWindows = msg;
        let _span = tracy_client::span!("GetWindows");

        let mut windows = HashMap::new();

        #[cfg(feature = "xdp-gnome-screencast")]
        windows.insert(
            self.niri.casting.dynamic_cast_id_for_portal.get(),
            gnome_shell_introspect::WindowProperties {
                title: String::from("niri Dynamic Cast Target"),
                app_id: String::from("rs.bxt.niri.desktop"),
            },
        );

        self.niri.layout.with_windows(|mapped, _, _, _| {
            let id = mapped.id().get();
            let props = with_toplevel_role(mapped.toplevel(), |role| {
                gnome_shell_introspect::WindowProperties {
                    title: role.title.clone().unwrap_or_default(),
                    app_id: role
                        .app_id
                        .as_ref()
                        // We don't do proper .desktop file tracking (it's quite involved), and
                        // Wayland windows can set any app id they want. However, this seems to
                        // work well enough in practice.
                        .map(|app_id| format!("{app_id}.desktop"))
                        .unwrap_or_default(),
                }
            });

            windows.insert(id, props);
        });

        let msg = NiriToIntrospect::Windows(windows);
        if let Err(err) = to_introspect.send_blocking(msg) {
            warn!("error sending windows to introspect: {err:?}");
        }
    }

    #[cfg(feature = "dbus")]
    pub fn on_login1_msg(&mut self, msg: Login1ToNiri) {
        let Login1ToNiri::LidClosedChanged(is_closed) = msg;

        trace!("login1 lid {}", if is_closed { "closed" } else { "opened" });
        self.set_lid_closed(is_closed);
    }

    #[cfg(feature = "dbus")]
    pub fn on_locale1_msg(&mut self, msg: Locale1ToNiri) {
        let Locale1ToNiri::XkbChanged(xkb) = msg;

        trace!("locale1 xkb settings changed: {xkb:?}");
        let xkb = self.niri.xkb_from_locale1.insert(xkb);

        {
            let config = self.niri.config.borrow();
            if config.input.keyboard.xkb != Xkb::default() {
                trace!("ignoring locale1 xkb change because niri config has xkb settings");
                return;
            }
        }

        let xkb = xkb.clone();
        self.set_xkb_config(xkb.to_xkb_config());
        self.ipc_keyboard_layouts_changed();
    }
}

impl Niri {
    /// Capability flags for the active global shader, computed once per config load and cached.
    /// On a cache miss this resolves the source (reading the `path` file if used), so callers
    /// may hit one filesystem read after a reload; subsequent calls are free.
    pub fn global_shader_caps(&self) -> niri_config::GlobalShaderCaps {
        if let Some(caps) = self.global_shader_caps.get() {
            return caps;
        }
        let caps = {
            let cfg = self.config.borrow();
            let passes = global_shader_pass_sources(&cfg.global_shader);
            niri_config::GlobalShaderCaps::scan_chain(&passes)
        };
        self.global_shader_caps.set(Some(caps));
        caps
    }

    pub fn new(
        config: Rc<RefCell<Config>>,
        event_loop: LoopHandle<'static, State>,
        stop_signal: LoopSignal,
        display: Display<State>,
        backend: &Backend,
        create_wayland_socket: bool,
        is_session_instance: bool,
    ) -> Self {
        let _span = tracy_client::span!("Niri::new");

        let (executor, scheduler) = calloop::futures::executor().unwrap();
        event_loop.insert_source(executor, |_, _, _| ()).unwrap();

        let display_handle = display.handle();
        let config_ = config.borrow();
        let config_file_output_config = config_.outputs.clone();

        let mut animation_clock = Clock::default();

        let rate = 1.0 / config_.animations.slowdown.max(0.001);
        animation_clock.set_rate(rate);
        animation_clock.set_complete_instantly(config_.animations.off);

        let layout = Layout::new(animation_clock.clone(), &config_);

        let (blocker_cleared_tx, blocker_cleared_rx) = mpsc::channel();

        fn client_is_unrestricted(client: &Client) -> bool {
            !client.get_data::<ClientState>().unwrap().restricted
        }

        let compositor_state = CompositorState::new_v6::<State>(&display_handle);
        let xdg_shell_state = XdgShellState::new_with_capabilities::<State>(
            &display_handle,
            [WmCapabilities::Fullscreen, WmCapabilities::Maximize],
        );
        let xdg_decoration_state =
            XdgDecorationState::new_with_filter::<State, _>(&display_handle, |client| {
                client
                    .get_data::<ClientState>()
                    .unwrap()
                    .can_view_decoration_globals
            });
        let kde_decoration_state = KdeDecorationState::new_with_filter::<State, _>(
            &display_handle,
            // If we want CSD we will hide the global.
            KdeDecorationsMode::Server,
            |client| {
                client
                    .get_data::<ClientState>()
                    .unwrap()
                    .can_view_decoration_globals
            },
        );
        let layer_shell_state = WlrLayerShellState::new_with_filter::<State, _>(
            &display_handle,
            client_is_unrestricted,
        );
        let session_lock_state =
            SessionLockManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let shm_state = ShmState::new::<State>(
            &display_handle,
            vec![wl_shm::Format::Xbgr8888, wl_shm::Format::Abgr8888],
        );
        let output_manager_state =
            OutputManagerState::new_with_xdg_output::<State>(&display_handle);
        let dmabuf_state = DmabufState::new();
        let fractional_scale_manager_state =
            FractionalScaleManagerState::new::<State>(&display_handle);
        let mut seat_state = SeatState::new();
        let tablet_state = TabletManagerState::new::<State>(&display_handle);
        let pointer_gestures_state = PointerGesturesState::new::<State>(&display_handle);
        let relative_pointer_state = RelativePointerManagerState::new::<State>(&display_handle);
        let pointer_constraints_state = PointerConstraintsState::new::<State>(&display_handle);
        let idle_notifier_state = IdleNotifierState::new(&display_handle, event_loop.clone());
        let idle_inhibit_manager_state = IdleInhibitManagerState::new::<State>(&display_handle);
        let data_device_state = DataDeviceState::new::<State>(&display_handle);
        let primary_selection_state =
            PrimarySelectionState::new_with_filter::<State, _>(&display_handle, |client| {
                !client
                    .get_data::<ClientState>()
                    .unwrap()
                    .primary_selection_disabled
            });
        let wlr_data_control_state = WlrDataControlState::new::<State, _>(
            &display_handle,
            Some(&primary_selection_state),
            client_is_unrestricted,
        );
        let ext_data_control_state = ExtDataControlState::new::<State, _>(
            &display_handle,
            Some(&primary_selection_state),
            client_is_unrestricted,
        );
        let presentation_state =
            PresentationState::new::<State>(&display_handle, Monotonic::ID as u32);
        let security_context_state =
            SecurityContextState::new::<State, _>(&display_handle, client_is_unrestricted);

        let text_input_state = TextInputManagerState::new::<State>(&display_handle);
        let input_method_state =
            InputMethodManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let keyboard_shortcuts_inhibit_state =
            KeyboardShortcutsInhibitState::new::<State>(&display_handle);
        let virtual_keyboard_state =
            VirtualKeyboardManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let virtual_pointer_state =
            VirtualPointerManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let foreign_toplevel_state =
            ForeignToplevelManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let ext_workspace_state =
            ExtWorkspaceManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let mut output_management_state =
            OutputManagementManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        output_management_state.on_config_changed(config_.outputs.clone());
        let screencopy_state =
            ScreencopyManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let viewporter_state = ViewporterState::new::<State>(&display_handle);
        let background_effect_state = BackgroundEffectState::new::<State>(&display_handle);
        let xdg_foreign_state = XdgForeignState::new::<State>(&display_handle);

        let is_tty = matches!(backend, Backend::Tty(_));
        let gamma_control_manager_state =
            GammaControlManagerState::new::<State, _>(&display_handle, move |client| {
                is_tty && !client.get_data::<ClientState>().unwrap().restricted
            });
        let activation_state = XdgActivationState::new::<State>(&display_handle);
        event_loop
            .insert_source(
                Timer::from_duration(XDG_ACTIVATION_TOKEN_TIMEOUT),
                |_, _, state| {
                    state.niri.activation_state.retain_tokens(|_, token_data| {
                        token_data.timestamp.elapsed() < XDG_ACTIVATION_TOKEN_TIMEOUT
                    });
                    TimeoutAction::ToDuration(XDG_ACTIVATION_TOKEN_TIMEOUT)
                },
            )
            .unwrap();

        let mutter_x11_interop_state =
            MutterX11InteropManagerState::new::<State, _>(&display_handle, move |_| true);

        #[cfg(test)]
        let single_pixel_buffer_state = SinglePixelBufferState::new::<State>(&display_handle);

        let mut seat: Seat<State> = seat_state.new_wl_seat(&display_handle, backend.seat_name());
        let keyboard = match seat.add_keyboard(
            config_.input.keyboard.xkb.to_xkb_config(),
            config_.input.keyboard.repeat_delay.into(),
            config_.input.keyboard.repeat_rate.into(),
        ) {
            Err(err) => {
                if let smithay::input::keyboard::Error::BadKeymap = err {
                    warn!("error loading the configured xkb keymap, trying default");
                } else {
                    warn!("error adding keyboard: {err:?}");
                }
                seat.add_keyboard(
                    Default::default(),
                    config_.input.keyboard.repeat_delay.into(),
                    config_.input.keyboard.repeat_rate.into(),
                )
                .unwrap()
            }
            Ok(keyboard) => keyboard,
        };
        if config_.input.keyboard.numlock {
            let mut modifier_state = keyboard.modifier_state();
            modifier_state.num_lock = true;
            keyboard.set_modifier_state(modifier_state);
        }
        seat.add_pointer();

        let cursor_shape_manager_state = CursorShapeManagerState::new::<State>(&display_handle);
        let cursor_manager =
            CursorManager::new(&config_.cursor.xcursor_theme, config_.cursor.xcursor_size);

        let mod_key = backend.mod_key(&config.borrow());
        let mods_with_mouse_binds = mods_with_mouse_binds(mod_key, &config_.binds);
        let mods_with_wheel_binds = mods_with_wheel_binds(mod_key, &config_.binds);
        let mods_with_finger_scroll_binds = mods_with_finger_scroll_binds(mod_key, &config_.binds);
        let mods_with_tablet_stylus_binds = mods_with_tablet_stylus_binds(mod_key, &config_.binds);

        let screenshot_ui = ScreenshotUi::new(animation_clock.clone(), config.clone());
        let window_mru_ui = WindowMruUi::new(config.clone());
        let config_error_notification =
            ConfigErrorNotification::new(animation_clock.clone(), config.clone());

        let mut hotkey_overlay = HotkeyOverlay::new(config.clone(), mod_key);
        if !config_.hotkey_overlay.skip_at_startup {
            hotkey_overlay.show();
        }

        let exit_confirm_dialog = ExitConfirmDialog::new(animation_clock.clone(), config.clone());

        #[cfg(feature = "dbus")]
        let a11y = A11y::new(event_loop.clone());

        event_loop
            .insert_source(
                Timer::from_duration(Duration::from_secs(1)),
                |_, _, state| {
                    state.niri.send_frame_callbacks_on_fallback_timer();
                    TimeoutAction::ToDuration(Duration::from_secs(1))
                },
            )
            .unwrap();

        let socket_name = create_wayland_socket.then(|| {
            let socket_source = ListeningSocketSource::new_auto().unwrap();
            let socket_name = socket_source.socket_name().to_os_string();
            event_loop
                .insert_source(socket_source, move |client, _, state| {
                    state.niri.insert_client(NewClient {
                        client,
                        restricted: false,
                        credentials_unknown: false,
                    });
                })
                .unwrap();
            socket_name
        });

        let ipc_server = match IpcServer::start(&event_loop, socket_name.as_deref()) {
            Ok(server) => Some(server),
            Err(err) => {
                warn!("error starting IPC server: {err:?}");
                None
            }
        };

        #[cfg(feature = "xdp-gnome-screencast")]
        let screencasting = Screencasting::new(&event_loop);

        let display_source = Generic::new(display, Interest::READ, Mode::Level);
        event_loop
            .insert_source(display_source, |_, display, state| {
                // SAFETY: we don't drop the display.
                unsafe {
                    display.get_mut().dispatch_clients(state).unwrap();
                }
                Ok(PostAction::Continue)
            })
            .unwrap();

        event_loop
            .insert_source(
                Timer::from_duration(Duration::from_secs(60)),
                |_, _, state| {
                    let _span = tracy_client::span!("startup timeout");
                    state.niri.is_at_startup = false;
                    state.niri.recompute_window_rules();
                    state.niri.recompute_layer_rules();
                    TimeoutAction::Drop
                },
            )
            .unwrap();

        drop(config_);
        let mut niri = Self {
            config,
            global_shader_caps: Cell::new(None),
            window_shader_start: std::time::Instant::now(),
            config_file_output_config,
            config_file_watcher: None,

            event_loop,
            scheduler,
            stop_signal,
            socket_name,
            display_handle,
            is_session_instance,
            start_time: Instant::now(),
            panel_frame: Cell::new(0),
            is_at_startup: true,
            clock: animation_clock,

            layout,
            global_space: Space::default(),
            sorted_outputs: Vec::default(),
            output_state: HashMap::new(),
            unmapped_windows: HashMap::new(),
            unmapped_layer_surfaces: HashSet::new(),
            mapped_layer_surfaces: HashMap::new(),
            root_surface: HashMap::new(),
            dmabuf_pre_commit_hook: HashMap::new(),
            blocker_cleared_tx,
            blocker_cleared_rx,
            monitors_active: true,
            power_off_skip_isolated: false,
            is_lid_closed: false,
            touchpad_disabled_by_toggle: false,

            dwt_disabled_by_toggle: false,

            devices: HashSet::new(),
            tablets: HashMap::new(),
            touch: HashSet::new(),

            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            kde_decoration_state,
            layer_shell_state,
            session_lock_state,
            foreign_toplevel_state,
            ext_workspace_state,
            output_management_state,
            screencopy_state,
            viewporter_state,
            background_effect_state,
            xdg_foreign_state,
            text_input_state,
            input_method_state,
            keyboard_shortcuts_inhibit_state,
            virtual_keyboard_state,
            virtual_pointer_state,
            shm_state,
            output_manager_state,
            dmabuf_state,
            fractional_scale_manager_state,
            seat_state,
            tablet_state,
            pointer_gestures_state,
            relative_pointer_state,
            pointer_constraints_state,
            idle_notifier_state,
            idle_inhibit_manager_state,
            data_device_state,
            primary_selection_state,
            wlr_data_control_state,
            ext_data_control_state,
            popups: PopupManager::default(),
            popup_grab: None,
            suppressed_keys: HashSet::new(),
            suppressed_buttons: HashSet::new(),
            bind_cooldown_timers: HashMap::new(),
            bind_repeat_timer: Option::default(),
            presentation_state,
            security_context_state,
            gamma_control_manager_state,
            activation_state,
            mutter_x11_interop_state,
            #[cfg(test)]
            single_pixel_buffer_state,

            seat,
            keyboard_focus: KeyboardFocus::Layout { surface: None },
            layer_shell_on_demand_focus: None,
            idle_inhibiting_surfaces: HashSet::new(),
            is_fdo_idle_inhibited: Arc::new(AtomicBool::new(false)),
            keyboard_shortcuts_inhibiting_surfaces: HashMap::new(),
            xkb_from_locale1: None,
            cursor_manager,
            cursor_texture_cache: Default::default(),
            cursor_shape_manager_state,
            dnd_icon: None,
            pointer_contents: PointContents::default(),
            pointer_visibility: PointerVisibility::Visible,
            pointer_inactivity_timer: None,
            pointer_inactivity_timer_got_reset: false,
            notified_activity_this_iteration: false,
            pointer_inside_hot_corner: false,
            tablet_cursor_location: None,
            gesture_swipe_3f_cumulative: None,
            overview_scroll_swipe_gesture: ScrollSwipeGesture::new(),
            vertical_wheel_tracker: ScrollTracker::new(120),
            horizontal_wheel_tracker: ScrollTracker::new(120),
            mods_with_mouse_binds,
            mods_with_wheel_binds,
            mods_with_tablet_stylus_binds,

            // 10 is copied from Clutter: DISCRETE_SCROLL_STEP.
            vertical_finger_scroll_tracker: ScrollTracker::new(10),
            horizontal_finger_scroll_tracker: ScrollTracker::new(10),
            mods_with_finger_scroll_binds,

            lock_state: LockState::Unlocked,
            locked_hint: None,

            screenshot_ui,
            config_error_notification,
            hotkey_overlay,
            exit_confirm_dialog,

            window_mru_ui,
            pending_mru_commit: None,

            pick_window: None,
            pick_color: None,

            debug_draw_opaque_regions: false,
            debug_draw_damage: false,

            #[cfg(feature = "dbus")]
            dbus: None,
            #[cfg(feature = "dbus")]
            a11y_manager: None,
            #[cfg(feature = "dbus")]
            a11y,
            #[cfg(feature = "dbus")]
            inhibit_power_key_fd: None,

            ipc_server,
            ipc_outputs_changed: false,

            satellite: None,

            #[cfg(feature = "xdp-gnome-screencast")]
            casting: screencasting,
        };

        niri.reset_pointer_inactivity_timer();

        niri
    }

    pub fn insert_client(&mut self, client: NewClient) {
        let NewClient {
            client,
            restricted,
            credentials_unknown,
        } = client;

        let config = self.config.borrow();
        let data = Arc::new(ClientState {
            compositor_state: Default::default(),
            can_view_decoration_globals: config.prefer_no_csd,
            primary_selection_disabled: config.clipboard.disable_primary,
            restricted,
            credentials_unknown,
        });

        if let Err(err) = self.display_handle.insert_client(client, data) {
            warn!("error inserting client: {err}");
        }
    }

    #[cfg(feature = "dbus")]
    pub fn inhibit_power_key(&mut self) -> anyhow::Result<()> {
        use smithay::reexports::rustix::io::{fcntl_setfd, FdFlags};

        let conn = zbus::blocking::Connection::system()?;

        let message = conn.call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "Inhibit",
            &("handle-power-key", "niri", "Power key handling", "block"),
        )?;

        let fd: zbus::zvariant::OwnedFd = message.body().deserialize()?;

        // Don't leak the fd to child processes.
        if let Err(err) = fcntl_setfd(&fd, FdFlags::CLOEXEC) {
            warn!("error setting CLOEXEC on inhibit fd: {err:?}");
        };

        self.inhibit_power_key_fd = Some(fd);

        Ok(())
    }

    /// Repositions all outputs, optionally adding a new output.
    pub fn reposition_outputs(&mut self, new_output: Option<&Output>) {
        let _span = tracy_client::span!("Niri::reposition_outputs");

        #[derive(Debug)]
        struct Data {
            output: Output,
            name: OutputName,
            position: Option<Point<i32, Logical>>,
            config: Option<niri_config::Position>,
        }

        let config = self.config.borrow();
        let mut outputs = vec![];
        for output in self.global_space.outputs().chain(new_output) {
            let name = output.user_data().get::<OutputName>().unwrap();
            let position = self.global_space.output_geometry(output).map(|geo| geo.loc);
            let config = config.outputs.find(name).and_then(|c| c.position);

            outputs.push(Data {
                output: output.clone(),
                name: name.clone(),
                position,
                config,
            });
        }
        drop(config);

        for Data { output, .. } in &outputs {
            self.global_space.unmap_output(output);
        }

        // Connectors can appear in udev in any order. If we sort by name then we get output
        // positioning that does not depend on the order they appeared.
        //
        // This sorting first compares by make/model/serial so that it is stable regardless of the
        // connector name. However, if make/model/serial is equal or unknown, then it does fall
        // back to comparing the connector name, which should always be unique.
        outputs.sort_unstable_by(|a, b| a.name.compare(&b.name));

        // Place all outputs with explicitly configured position first, then the unconfigured ones.
        outputs.sort_by_key(|d| d.config.is_none());

        trace!(
            "placing outputs in order: {:?}",
            outputs.iter().map(|d| &d.name.connector)
        );

        self.sorted_outputs = outputs
            .iter()
            .map(|Data { output, .. }| output.clone())
            .collect();

        for data in outputs.into_iter() {
            let Data {
                output,
                name,
                position,
                config,
            } = data;

            let size = output_size(&output).to_i32_round();

            let new_position = config
                .map(|pos| Point::from((pos.x, pos.y)))
                .filter(|pos| {
                    // Ensure that the requested position does not overlap any existing output.
                    let target_geom = Rectangle::new(*pos, size);

                    let overlap = self
                        .global_space
                        .outputs()
                        .map(|output| self.global_space.output_geometry(output).unwrap())
                        .find(|geom| geom.overlaps(target_geom));

                    if let Some(overlap) = overlap {
                        warn!(
                            "output {} at x={} y={} sized {}x{} \
                             overlaps an existing output at x={} y={} sized {}x{}, \
                             falling back to automatic placement",
                            name.connector,
                            pos.x,
                            pos.y,
                            size.w,
                            size.h,
                            overlap.loc.x,
                            overlap.loc.y,
                            overlap.size.w,
                            overlap.size.h,
                        );

                        false
                    } else {
                        true
                    }
                })
                .unwrap_or_else(|| {
                    let x = self
                        .global_space
                        .outputs()
                        .map(|output| self.global_space.output_geometry(output).unwrap())
                        .map(|geom| geom.loc.x + geom.size.w)
                        .max()
                        .unwrap_or(0);

                    Point::from((x, 0))
                });

            self.global_space.map_output(&output, new_position);

            // By passing new_output as an Option, rather than mapping it into a bogus location
            // in global_space, we ensure that this branch always runs for it.
            if Some(new_position) != position {
                debug!(
                    "putting output {} at x={} y={}",
                    name.connector, new_position.x, new_position.y
                );
                output.change_current_state(None, None, None, Some(new_position));
                self.ipc_outputs_changed = true;
                self.queue_redraw(&output);
            }
        }
    }

    pub fn add_output(&mut self, output: Output, refresh_interval: Option<Duration>, vrr: bool) {
        let global = output.create_global::<State>(&self.display_handle);

        let name = output.user_data().get::<OutputName>().unwrap();

        let config = self.config.borrow();
        let c = config.outputs.find(name);
        let scale = c.and_then(|c| c.scale).map(|s| s.0).unwrap_or_else(|| {
            let size_mm = output.physical_properties().size;
            let resolution = output.current_mode().unwrap().size;
            guess_monitor_scale(size_mm, resolution)
        });
        let scale = closest_representable_scale(scale.clamp(0.1, 10.));

        let mut transform = panel_orientation(&output)
            + c.map(|c| ipc_transform_to_smithay(c.transform))
                .unwrap_or(Transform::Normal);

        let mut backdrop_color = c
            .and_then(|c| c.backdrop_color)
            .unwrap_or(config.overview.backdrop_color)
            .to_array_unpremul();
        backdrop_color[3] = 1.;

        // FIXME: fix winit damage on other transforms.
        if name.connector == "winit" {
            transform = Transform::Flipped180;
        }

        let mut layout_config = c.and_then(|c| c.layout.clone());
        // Support the deprecated non-layout background-color key.
        if let Some(layout) = &mut layout_config {
            if layout.background_color.is_none() {
                layout.background_color = c.and_then(|c| c.background_color);
            }
        }
        let isolated = c.is_some_and(|c| c.isolated);
        drop(config);

        // Set scale and transform before adding to the layout since that will read the output size.
        output.change_current_state(
            None,
            Some(transform),
            Some(output::Scale::Fractional(scale)),
            None,
        );

        self.layout.add_output(output.clone(), layout_config, isolated);

        let lock_render_state = if self.is_locked() {
            // We haven't rendered anything yet so it's as good as locked.
            LockRenderState::Locked
        } else {
            LockRenderState::Unlocked
        };

        let size = output_size(&output);
        let state = OutputState {
            global,
            redraw_state: RedrawState::Idle,
            on_demand_vrr_enabled: false,
            unfinished_animations_remain: false,
            last_shader_frame: None,
            shader_throttle_timer: None,
            frame_clock: FrameClock::new(refresh_interval, vrr),
            last_drm_sequence: None,
            vblank_throttle: VBlankThrottle::new(self.event_loop.clone(), name.connector.clone()),
            frame_callback_sequence: 0,
            backdrop_buffer: SolidColorBuffer::new(size, backdrop_color),
            xray: Xray::new(),
            lock_render_state,
            lock_surface: None,
            lock_color_buffer: SolidColorBuffer::new(size, CLEAR_COLOR_LOCKED),
            screen_transition: None,
            global_shader_start: Cell::new(None),
            global_shader_screen_prev: None,
            global_shader_screen_result: Rc::new(RefCell::new(None)),
            global_shader_chain: RefCell::new(GlobalShaderChain::default()),
            debug_damage_tracker: OutputDamageTracker::from_output(&output),
            panel_source: RefCell::new(PanelSource::default()),
            panel_offscreen: [OffscreenBuffer::default(), OffscreenBuffer::default()],
            panel_elems: RefCell::new(HashMap::new()),
            panel_redraw_pending: Cell::new(false),
            panel_hits: RefCell::new(Vec::new()),
        };
        let rv = self.output_state.insert(output.clone(), state);
        assert!(rv.is_none(), "output was already tracked");

        // Must be last since it will call queue_redraw(output) which needs things to be filled-in.
        self.reposition_outputs(Some(&output));
    }

    pub fn output_exists(&self, output: &Output) -> bool {
        self.output_state.contains_key(output)
    }

    /// Converts a `WlOutput` to a corresponding `Output` if it exists.
    ///
    /// Compared to raw `Output::from_resource`, this method also verifies that the output still
    /// exists in niri. Right after the output global is disabled, but before it is removed for
    /// good, `Output::from_resource` will succeed, but since niri already forgot the output,
    /// accessing it can cause logic bugs.
    pub fn output_from_resource(&self, wl_output: &WlOutput) -> Option<Output> {
        Output::from_resource(wl_output).filter(|output| self.output_exists(output))
    }

    pub fn remove_output(&mut self, output: &Output) {
        for layer in layer_map_for_output(output).layers() {
            layer.layer_surface().send_close();
        }

        self.layout.remove_output(output);
        self.global_space.unmap_output(output);
        self.reposition_outputs(None);
        self.gamma_control_manager_state.output_removed(output);

        let state = self.output_state.remove(output).unwrap();

        match state.redraw_state {
            RedrawState::Idle => (),
            RedrawState::Queued => (),
            RedrawState::WaitingForVBlank { .. } => (),
            RedrawState::WaitingForEstimatedVBlank(token) => self.event_loop.remove(token),
            RedrawState::WaitingForEstimatedVBlankAndQueued(token) => self.event_loop.remove(token),
        }

        self.stop_casts_for_target(CastTarget::output(output));
        self.screencopy_state.remove_output(output);

        // Disable the output global and remove some time later to give the clients some time to
        // process it.
        let global = state.global;
        self.display_handle.disable_global::<State>(global.clone());
        self.event_loop
            .insert_source(
                Timer::from_duration(Duration::from_secs(10)),
                move |_, _, state| {
                    state
                        .niri
                        .display_handle
                        .remove_global::<State>(global.clone());
                    TimeoutAction::Drop
                },
            )
            .unwrap();

        match mem::take(&mut self.lock_state) {
            LockState::Locking(confirmation) => {
                // We're locking and an output was removed, check if the requirements are now met.
                let all_locked = self
                    .output_state
                    .values()
                    .all(|state| state.lock_render_state == LockRenderState::Locked);

                if all_locked {
                    let lock = confirmation.ext_session_lock().clone();
                    confirmation.lock();
                    self.lock_state = LockState::Locked(lock);
                } else {
                    // Still waiting.
                    self.lock_state = LockState::Locking(confirmation);
                }
            }
            lock_state => {
                self.lock_state = lock_state;
                self.maybe_continue_to_locking();
            }
        }

        if self.screenshot_ui.close() {
            self.cursor_manager
                .set_cursor_image(CursorImageStatus::default_named());
            self.queue_redraw_all();
        }

        if self.window_mru_ui.output() == Some(output) {
            self.cancel_mru();
        }
    }

    pub fn output_resized(&mut self, output: &Output) {
        let output_size = output_size(output);
        let scale = output.current_scale();
        let transform = output.current_transform();

        {
            let mut layer_map = layer_map_for_output(output);
            for layer in layer_map.layers() {
                layer.with_surfaces(|surface, data| {
                    send_scale_transform(surface, data, scale, transform);
                });

                if let Some(mapped) = self.mapped_layer_surfaces.get_mut(layer) {
                    mapped.update_sizes(output_size, scale.fractional_scale());
                }
            }
            layer_map.arrange();
        }

        self.layout.update_output_size(output);

        if let Some(state) = self.output_state.get_mut(output) {
            state.backdrop_buffer.resize(output_size);

            state.lock_color_buffer.resize(output_size);
            if let Some(lock_surface) = &state.lock_surface {
                configure_lock_surface(lock_surface, output);
            }
        }

        // If the output size changed with an open screenshot UI, close the screenshot UI.
        if let Some((old_size, old_scale, old_transform)) = self.screenshot_ui.output_size(output) {
            let output_mode = output.current_mode().unwrap();
            let size = transform.transform_size(output_mode.size);
            let scale = output.current_scale().fractional_scale();
            // FIXME: scale changes and transform flips shouldn't matter but they currently do since
            // I haven't quite figured out how to draw the screenshot textures in
            // physical coordinates.
            if old_size != size || old_scale != scale || old_transform != transform {
                self.screenshot_ui.close();
                self.cursor_manager
                    .set_cursor_image(CursorImageStatus::default_named());
                self.queue_redraw_all();
                return;
            }
        }

        self.queue_redraw(output);
    }

    pub fn deactivate_monitors(&mut self, backend: &mut Backend, skip_isolated: bool) {
        if !self.monitors_active {
            return;
        }

        self.monitors_active = false;
        self.power_off_skip_isolated = skip_isolated;
        backend.set_monitors_active(false, skip_isolated);

        // Isolated outputs stay on when skipping them, so they must keep redrawing.
        if skip_isolated {
            self.queue_redraw_all();
        }
    }

    pub fn activate_monitors(&mut self, backend: &mut Backend) {
        if self.monitors_active {
            return;
        }

        self.monitors_active = true;
        self.power_off_skip_isolated = false;
        backend.set_monitors_active(true, false);

        self.queue_redraw_all();
    }

    pub fn output_under(&self, pos: Point<f64, Logical>) -> Option<(&Output, Point<f64, Logical>)> {
        let output = self.global_space.output_under(pos).next()?;
        let pos_within_output = pos
            - self
                .global_space
                .output_geometry(output)
                .unwrap()
                .loc
                .to_f64();

        Some((output, pos_within_output))
    }

    /// Whether the output is marked `isolated` in its config.
    ///
    /// Isolated outputs are intended for projection/signage/capture use, and are kept free of
    /// compositor chrome: the overview, the Alt-Tab switcher, the hotkey overlay, and the config
    /// error notification are not drawn on them. Windows and layer-shell surfaces still render.
    ///
    /// Note that the overview is additionally suppressed in the layout (see `Monitor::isolated`),
    /// since its zoom is driven by `overview_progress` rather than by this render-time check.
    ///
    /// Isolated outputs can also stay powered on during a `power-off-monitors skip-isolated=true`
    /// (see `deactivate_monitors` and `power_off_skip_isolated`). Not covered: layer-shell surfaces
    /// such as notification daemons.
    pub fn is_output_isolated(&self, output: &Output) -> bool {
        let config = self.config.borrow();
        output
            .user_data()
            .get::<OutputName>()
            .and_then(|name| config.outputs.find(name))
            .is_some_and(|c| c.isolated)
    }

    fn is_inside_hot_corner(&self, output: &Output, pos: Point<f64, Logical>) -> bool {
        let config = self.config.borrow();
        let hot_corners = output
            .user_data()
            .get::<OutputName>()
            .and_then(|name| config.outputs.find(name))
            .and_then(|c| c.hot_corners)
            .unwrap_or(config.gestures.hot_corners);

        if hot_corners.off {
            return false;
        }

        // Use size from the ceiled output geometry, since that's what we currently use for pointer
        // motion clamping.
        let geom = self.global_space.output_geometry(output).unwrap();
        let size = geom.size.to_f64();

        let contains = move |corner: Point<f64, Logical>| {
            Rectangle::new(corner, Size::new(1., 1.)).contains(pos)
        };

        if hot_corners.top_right && contains(Point::new(size.w - 1., 0.)) {
            return true;
        }
        if hot_corners.bottom_left && contains(Point::new(0., size.h - 1.)) {
            return true;
        }
        if hot_corners.bottom_right && contains(Point::new(size.w - 1., size.h - 1.)) {
            return true;
        }

        // If the user didn't explicitly set any corners, we default to top-left.
        if (hot_corners.top_left
            || !(hot_corners.top_right || hot_corners.bottom_right || hot_corners.bottom_left))
            && contains(Point::new(0., 0.))
        {
            return true;
        }

        false
    }

    pub fn is_sticky_obscured_under(
        &self,
        output: &Output,
        pos_within_output: Point<f64, Logical>,
    ) -> bool {
        // The ordering here must be consistent with the ordering in render() so that input is
        // consistent with the visuals.

        // Check if some layer-shell surface is on top.
        let layers = layer_map_for_output(output);
        let layer_surface_under = |layer, popup| {
            layers
                .layers_on(layer)
                .rev()
                .find_map(|layer| {
                    let mapped = self.mapped_layer_surfaces.get(layer)?;

                    let mut layer_pos_within_output =
                        layers.layer_geometry(layer).unwrap().loc.to_f64();
                    layer_pos_within_output += mapped.bob_offset();

                    let surface_type = if popup {
                        WindowSurfaceType::POPUP
                    } else {
                        WindowSurfaceType::TOPLEVEL
                    } | WindowSurfaceType::SUBSURFACE;
                    layer.surface_under(pos_within_output - layer_pos_within_output, surface_type)
                })
                .is_some()
        };

        let layer_toplevel_under = |layer| layer_surface_under(layer, false);
        let layer_popup_under = |layer| layer_surface_under(layer, true);

        if layer_popup_under(Layer::Overlay) || layer_toplevel_under(Layer::Overlay) {
            return true;
        }

        let mon = self.layout.monitor_for_output(output).unwrap();
        if mon.render_above_top_layer() {
            return false;
        }

        if self.is_inside_hot_corner(output, pos_within_output) {
            return true;
        }

        if layer_popup_under(Layer::Top) || layer_toplevel_under(Layer::Top) {
            return true;
        }

        false
    }

    pub fn is_layout_obscured_under(
        &self,
        output: &Output,
        pos_within_output: Point<f64, Logical>,
    ) -> bool {
        if self.layout.is_overview_open() {
            return false;
        }

        // Check if some layer-shell surface is on top.
        let layers = layer_map_for_output(output);
        let layer_popup_under = |layer| {
            layers
                .layers_on(layer)
                .rev()
                .find_map(|layer_surface| {
                    let mapped = self.mapped_layer_surfaces.get(layer_surface)?;
                    if mapped.place_within_backdrop() {
                        return None;
                    }

                    let mut layer_pos_within_output =
                        layers.layer_geometry(layer_surface).unwrap().loc.to_f64();
                    layer_pos_within_output += mapped.bob_offset();

                    // Background and bottom layers move together with the workspaces.
                    let mon = self.layout.monitor_for_output(output)?;
                    let (_, geo) = mon.workspace_under(pos_within_output)?;
                    layer_pos_within_output += geo.loc;

                    let surface_type = WindowSurfaceType::POPUP | WindowSurfaceType::SUBSURFACE;
                    layer_surface
                        .surface_under(pos_within_output - layer_pos_within_output, surface_type)
                })
                .is_some()
        };

        if layer_popup_under(Layer::Bottom) || layer_popup_under(Layer::Background) {
            return true;
        }

        false
    }

    /// Returns the workspace under the position to be activated.
    ///
    /// The return value is an output and a workspace index on it.
    pub fn workspace_under(
        &self,
        extended_bounds: bool,
        pos: Point<f64, Logical>,
    ) -> Option<(Output, &Workspace<Mapped>)> {
        if self.exit_confirm_dialog.is_open() || self.is_locked() || self.screenshot_ui.is_open() {
            return None;
        }

        let (output, pos_within_output) = self.output_under(pos)?;

        if self.is_sticky_obscured_under(output, pos_within_output) {
            return None;
        }

        if self.is_layout_obscured_under(output, pos_within_output) {
            return None;
        }

        let ws = self
            .layout
            .workspace_under(extended_bounds, output, pos_within_output)?;
        Some((output.clone(), ws))
    }

    pub fn workspace_under_cursor(
        &self,
        extended_bounds: bool,
    ) -> Option<(Output, &Workspace<Mapped>)> {
        let pos = self.seat.get_pointer().unwrap().current_location();
        self.workspace_under(extended_bounds, pos)
    }

    /// Returns the window under the position to be activated.
    ///
    /// The cursor may be inside the window's activation region, but not within the window's input
    /// region.
    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<&Mapped> {
        if self.exit_confirm_dialog.is_open()
            || self.is_locked()
            || self.screenshot_ui.is_open()
            || self.window_mru_ui.is_open()
        {
            return None;
        }

        let (output, pos_within_output) = self.output_under(pos)?;

        if self.is_sticky_obscured_under(output, pos_within_output) {
            return None;
        }

        if let Some((window, _loc)) = self
            .layout
            .interactive_moved_window_under(output, pos_within_output)
        {
            return Some(window);
        }

        if self.is_layout_obscured_under(output, pos_within_output) {
            return None;
        }

        let (window, _loc) = self.layout.window_under(output, pos_within_output)?;
        Some(window)
    }

    /// Returns the window under the cursor to be activated.
    ///
    /// The cursor may be inside the window's activation region, but not within the window's input
    /// region.
    pub fn window_under_cursor(&self) -> Option<&Mapped> {
        let pos = self.seat.get_pointer().unwrap().current_location();
        self.window_under(pos)
    }

    /// Returns contents under the given point.
    ///
    /// We don't have a proper global space for all windows, so this function converts window
    /// locations to global space according to where they are rendered.
    ///
    /// This function does not take pointer or touch grabs into account.
    pub fn contents_under(&self, pos: Point<f64, Logical>) -> PointContents {
        let mut rv = PointContents::default();

        let Some((output, pos_within_output)) = self.output_under(pos) else {
            return rv;
        };
        rv.output = Some(output.clone());
        let output_pos_in_global_space = self.global_space.output_geometry(output).unwrap().loc;

        // The ordering here must be consistent with the ordering in render() so that input is
        // consistent with the visuals.

        if self.exit_confirm_dialog.is_open() {
            return rv;
        } else if self.is_locked() {
            let Some(state) = self.output_state.get(output) else {
                return rv;
            };
            let Some(surface) = state.lock_surface.as_ref() else {
                return rv;
            };

            rv.surface = under_from_surface_tree(
                surface.wl_surface(),
                pos_within_output,
                // We put lock surfaces at (0, 0).
                (0, 0),
                WindowSurfaceType::ALL,
            )
            .map(|(surface, pos_within_output)| {
                (
                    surface,
                    (pos_within_output + output_pos_in_global_space).to_f64(),
                )
            });

            return rv;
        }

        if self.screenshot_ui.is_open() || self.window_mru_ui.is_open() {
            return rv;
        }

        let layers = layer_map_for_output(output);
        let layer_surface_under = |layer, popup| {
            layers
                .layers_on(layer)
                .rev()
                .find_map(|layer_surface| {
                    let mapped = self.mapped_layer_surfaces.get(layer_surface)?;
                    if mapped.place_within_backdrop() {
                        return None;
                    }

                    let mut layer_pos_within_output =
                        layers.layer_geometry(layer_surface).unwrap().loc.to_f64();
                    layer_pos_within_output += mapped.bob_offset();

                    // Background and bottom layers move together with the workspaces.
                    if matches!(layer, Layer::Background | Layer::Bottom) {
                        let mon = self.layout.monitor_for_output(output)?;
                        let (_, geo) = mon.workspace_under(pos_within_output)?;
                        layer_pos_within_output += geo.loc;
                        // Don't need to deal with zoom here because in the overview background and
                        // bottom layers don't receive input.
                    }

                    let surface_type = if popup {
                        WindowSurfaceType::POPUP
                    } else {
                        WindowSurfaceType::TOPLEVEL
                    } | WindowSurfaceType::SUBSURFACE;

                    layer_surface
                        .surface_under(pos_within_output - layer_pos_within_output, surface_type)
                        .map(|(surface, pos_within_layer)| {
                            (
                                (surface, pos_within_layer.to_f64() + layer_pos_within_output),
                                layer_surface,
                            )
                        })
                })
                .map(|(s, l)| (Some(s), (None, Some(l.clone()))))
        };

        let layer_toplevel_under = |layer| layer_surface_under(layer, false);
        let layer_popup_under = |layer| layer_surface_under(layer, true);

        let mapped_hit_data = |(mapped, hit): (&Mapped, HitType)| {
            let window = &mapped.window;
            let surface_and_pos = if let HitType::Input { win_pos } = hit {
                let win_pos_within_output = win_pos;
                window
                    .surface_under(
                        pos_within_output - win_pos_within_output,
                        WindowSurfaceType::ALL,
                    )
                    .map(|(s, pos_within_window)| {
                        (s, pos_within_window.to_f64() + win_pos_within_output)
                    })
            } else {
                None
            };
            (surface_and_pos, (Some((window.clone(), hit)), None))
        };

        let interactive_moved_window_under = || {
            self.layout
                .interactive_moved_window_under(output, pos_within_output)
                .map(mapped_hit_data)
        };
        let window_under = || {
            self.layout
                .window_under(output, pos_within_output)
                .map(mapped_hit_data)
        };

        let mon = self.layout.monitor_for_output(output).unwrap();

        let mut under =
            layer_popup_under(Layer::Overlay).or_else(|| layer_toplevel_under(Layer::Overlay));

        let is_overview_open = self.layout.is_overview_open();

        // When rendering above the top layer, we put the regular monitor elements first.
        // Otherwise, we will render all layer-shell pop-ups and the top layer on top.
        if mon.render_above_top_layer() {
            under = under
                .or_else(interactive_moved_window_under)
                .or_else(window_under)
                .or_else(|| layer_popup_under(Layer::Top))
                .or_else(|| layer_toplevel_under(Layer::Top))
                .or_else(|| layer_popup_under(Layer::Bottom))
                .or_else(|| layer_popup_under(Layer::Background))
                .or_else(|| layer_toplevel_under(Layer::Bottom))
                .or_else(|| layer_toplevel_under(Layer::Background));
        } else {
            if self.is_inside_hot_corner(output, pos_within_output) {
                rv.hot_corner = true;
                return rv;
            }

            under = under
                .or_else(|| layer_popup_under(Layer::Top))
                .or_else(|| layer_toplevel_under(Layer::Top));

            under = under.or_else(interactive_moved_window_under);

            if !is_overview_open {
                under = under
                    .or_else(|| layer_popup_under(Layer::Bottom))
                    .or_else(|| layer_popup_under(Layer::Background));
            }

            under = under.or_else(window_under);

            if !is_overview_open {
                under = under
                    .or_else(|| layer_toplevel_under(Layer::Bottom))
                    .or_else(|| layer_toplevel_under(Layer::Background));
            }
        }

        let Some((mut surface_and_pos, (window, layer))) = under else {
            return rv;
        };

        if let Some((_, surface_pos)) = &mut surface_and_pos {
            *surface_pos += output_pos_in_global_space.to_f64();
        }

        rv.surface = surface_and_pos;
        rv.window = window;
        rv.layer = layer;
        rv
    }

    pub fn output_under_cursor(&self) -> Option<Output> {
        let pos = self.seat.get_pointer().unwrap().current_location();
        self.global_space.output_under(pos).next().cloned()
    }

    pub fn output_left_of(&self, current: &Output) -> Option<Output> {
        let current_geo = self.global_space.output_geometry(current)?;
        let extended_geo = Rectangle::new(
            Point::from((i32::MIN / 2, current_geo.loc.y)),
            Size::from((i32::MAX, current_geo.size.h)),
        );

        self.global_space
            .outputs()
            .map(|output| (output, self.global_space.output_geometry(output).unwrap()))
            .filter(|(_, geo)| center(*geo).x < center(current_geo).x && geo.overlaps(extended_geo))
            .min_by_key(|(_, geo)| center(current_geo).x - center(*geo).x)
            .map(|(output, _)| output)
            .cloned()
    }

    pub fn output_right_of(&self, current: &Output) -> Option<Output> {
        let current_geo = self.global_space.output_geometry(current)?;
        let extended_geo = Rectangle::new(
            Point::from((i32::MIN / 2, current_geo.loc.y)),
            Size::from((i32::MAX, current_geo.size.h)),
        );

        self.global_space
            .outputs()
            .map(|output| (output, self.global_space.output_geometry(output).unwrap()))
            .filter(|(_, geo)| center(*geo).x > center(current_geo).x && geo.overlaps(extended_geo))
            .min_by_key(|(_, geo)| center(*geo).x - center(current_geo).x)
            .map(|(output, _)| output)
            .cloned()
    }

    pub fn output_up_of(&self, current: &Output) -> Option<Output> {
        let current_geo = self.global_space.output_geometry(current)?;
        let extended_geo = Rectangle::new(
            Point::from((current_geo.loc.x, i32::MIN / 2)),
            Size::from((current_geo.size.w, i32::MAX)),
        );

        self.global_space
            .outputs()
            .map(|output| (output, self.global_space.output_geometry(output).unwrap()))
            .filter(|(_, geo)| center(*geo).y < center(current_geo).y && geo.overlaps(extended_geo))
            .min_by_key(|(_, geo)| center(current_geo).y - center(*geo).y)
            .map(|(output, _)| output)
            .cloned()
    }

    pub fn output_down_of(&self, current: &Output) -> Option<Output> {
        let current_geo = self.global_space.output_geometry(current)?;
        let extended_geo = Rectangle::new(
            Point::from((current_geo.loc.x, i32::MIN / 2)),
            Size::from((current_geo.size.w, i32::MAX)),
        );

        self.global_space
            .outputs()
            .map(|output| (output, self.global_space.output_geometry(output).unwrap()))
            .filter(|(_, geo)| center(*geo).y > center(current_geo).y && geo.overlaps(extended_geo))
            .min_by_key(|(_, geo)| center(*geo).y - center(current_geo).y)
            .map(|(output, _)| output)
            .cloned()
    }

    pub fn output_previous_of(&self, current: &Output) -> Option<Output> {
        self.sorted_outputs
            .iter()
            .rev()
            .skip_while(|&output| output != current)
            .nth(1)
            .or(self.sorted_outputs.last())
            .filter(|&output| output != current)
            .cloned()
    }

    pub fn output_next_of(&self, current: &Output) -> Option<Output> {
        self.sorted_outputs
            .iter()
            .skip_while(|&output| output != current)
            .nth(1)
            .or(self.sorted_outputs.first())
            .filter(|&output| output != current)
            .cloned()
    }

    pub fn output_left(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_left_of(active)
    }

    pub fn output_right(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_right_of(active)
    }

    pub fn output_up(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_up_of(active)
    }

    pub fn output_down(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_down_of(active)
    }

    pub fn output_previous(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_previous_of(active)
    }

    pub fn output_next(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_next_of(active)
    }

    pub fn find_output_and_workspace_index(
        &self,
        workspace_reference: WorkspaceReference,
    ) -> Option<(Option<Output>, usize)> {
        let (target_workspace_index, target_workspace) = match workspace_reference {
            WorkspaceReference::Index(index) => {
                return Some((None, index.saturating_sub(1) as usize));
            }
            WorkspaceReference::Name(name) => self.layout.find_workspace_by_name(&name)?,
            WorkspaceReference::Id(id) => {
                let id = WorkspaceId::specific(id);
                self.layout.find_workspace_by_id(id)?
            }
        };

        let target_output = target_workspace.current_output();
        Some((target_output.cloned(), target_workspace_index))
    }

    pub fn find_window_by_id(&self, id: MappedId) -> Option<Window> {
        self.layout
            .windows()
            .find(|(_, m)| m.id() == id)
            .map(|(_, m)| m.window.clone())
    }

    pub fn output_for_tablet(&self) -> Option<&Output> {
        let config = self.config.borrow();
        if config.input.tablet.map_to_focused_output {
            self.layout.active_output()
        } else {
            let map_to_output = config.input.tablet.map_to_output.as_ref();
            map_to_output.and_then(|name| self.output_by_name_match(name))
        }
    }

    pub fn output_for_touch(&self) -> Option<&Output> {
        let config = self.config.borrow();
        let map_to_output = config.input.touch.map_to_output.as_ref();
        map_to_output
            .and_then(|name| self.output_by_name_match(name))
            .or_else(|| self.global_space.outputs().next())
    }

    pub fn output_by_name_match(&self, target: &str) -> Option<&Output> {
        self.global_space
            .outputs()
            .find(|output| output_matches_name(output, target))
    }

    pub fn output_for_root(&self, root: &WlSurface) -> Option<&Output> {
        // Check the main layout.
        let win_out = self.layout.find_window_and_output(root);
        let layout_output = win_out.map(|(_, output)| output);
        if let Some(output) = layout_output {
            return output;
        }

        // Check layer-shell.
        let has_layer_surface = |o: &&Output| {
            layer_map_for_output(o)
                .layer_for_surface(root, WindowSurfaceType::TOPLEVEL)
                .is_some()
        };
        self.layout.outputs().find(has_layer_surface)
    }

    pub fn lock_surface_focus(&self) -> Option<WlSurface> {
        let output_under_cursor = self.output_under_cursor();
        let output = output_under_cursor
            .as_ref()
            .or_else(|| self.layout.active_output())
            .or_else(|| self.global_space.outputs().next())?;

        let state = self.output_state.get(output)?;
        state.lock_surface.as_ref().map(|s| s.wl_surface()).cloned()
    }

    /// Schedules an immediate redraw on all outputs if one is not already scheduled.
    pub fn queue_redraw_all(&mut self) {
        for state in self.output_state.values_mut() {
            state.redraw_state = mem::take(&mut state.redraw_state).queue_redraw();
        }
    }

    /// Schedules an immediate redraw if one is not already scheduled.
    pub fn queue_redraw(&mut self, output: &Output) {
        let state = self.output_state.get_mut(output).unwrap();
        state.redraw_state = mem::take(&mut state.redraw_state).queue_redraw();
    }

    /// Cancel any pending `shader-animation-max-fps` throttle wake for `output`.
    fn cancel_shader_throttle_timer(&mut self, output: &Output) {
        let token = self
            .output_state
            .get_mut(output)
            .and_then(|state| state.shader_throttle_timer.take());
        if let Some(token) = token {
            self.event_loop.remove(token);
        }
    }

    /// Arm a one-shot wake `delay` from now that re-queues a throttled shader redraw for `output`,
    /// replacing any pending wake.
    fn arm_shader_throttle_timer(&mut self, output: &Output, delay: Duration) {
        self.cancel_shader_throttle_timer(output);
        let out = output.clone();
        let token =
            match self
                .event_loop
                .insert_source(Timer::from_duration(delay), move |_, _, state| {
                    match state.niri.output_state.get_mut(&out) {
                        Some(s) => s.shader_throttle_timer = None,
                        // Output disconnected before this wake fired; queue_redraw would unwrap a
                        // missing output and panic, so just drop the timer.
                        None => return TimeoutAction::Drop,
                    }
                    state.niri.queue_redraw(&out);
                    TimeoutAction::Drop
                }) {
                Ok(token) => token,
                Err(err) => {
                    warn!("error arming shader throttle timer: {err:?}");
                    return;
                }
            };
        if let Some(state) = self.output_state.get_mut(output) {
            state.shader_throttle_timer = Some(token);
        }
    }

    pub fn redraw_queued_outputs(&mut self, backend: &mut Backend) {
        let _span = tracy_client::span!("Niri::redraw_queued_outputs");

        // One stamp for this whole cycle: every output redrawn below shares it, so
        // `update_panel_sources` only refreshes each content output's `PanelSource` once even
        // though `render()` may run for it more than once (e.g. a screencopy re-render).
        self.panel_frame.set(self.panel_frame.get().wrapping_add(1));

        while let Some((output, _)) = self.output_state.iter().find(|(_, state)| {
            matches!(
                state.redraw_state,
                RedrawState::Queued | RedrawState::WaitingForEstimatedVBlankAndQueued(_)
            )
        }) {
            trace!("redrawing output");
            let output = output.clone();
            self.redraw(backend, &output);
        }
    }

    pub fn render_pointer<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        output: &Output,
        push: &mut dyn FnMut(PointerRenderElements<R>),
    ) {
        let _span = tracy_client::span!("Niri::render_pointer");
        let output_scale = output.current_scale();
        let output_pos = self.global_space.output_geometry(output).unwrap().loc;

        // Check whether we need to draw the tablet cursor or the regular cursor.
        let pointer_pos = self
            .tablet_cursor_location
            .unwrap_or_else(|| self.seat.get_pointer().unwrap().current_location());
        let pointer_pos = pointer_pos - output_pos.to_f64();

        // Get the render cursor to draw.
        let cursor_scale = output_scale.integer_scale();
        let render_cursor = self.cursor_manager.get_render_cursor(cursor_scale);

        let output_scale = Scale::from(output.current_scale().fractional_scale());

        match render_cursor {
            RenderCursor::Hidden => (),
            RenderCursor::Surface { surface, hotspot } => {
                let pointer_pos =
                    (pointer_pos - hotspot.to_f64()).to_physical_precise_round(output_scale);

                push_elements_from_surface_tree(
                    renderer,
                    &surface,
                    pointer_pos,
                    output_scale,
                    1.,
                    Kind::Cursor,
                    &mut |elem| push(elem.into()),
                );
            }
            RenderCursor::Named {
                icon,
                scale,
                cursor,
            } => {
                let (idx, frame) = cursor.frame(self.start_time.elapsed().as_millis() as u32);
                let hotspot = XCursor::hotspot(frame).to_logical(scale);
                let pointer_pos =
                    (pointer_pos - hotspot.to_f64()).to_physical_precise_round(output_scale);

                let texture = self.cursor_texture_cache.get(icon, scale, &cursor, idx);
                match MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    pointer_pos,
                    &texture,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                ) {
                    Ok(element) => push(element.into()),
                    Err(err) => {
                        warn!("error importing a cursor texture: {err:?}");
                    }
                }
            }
        }

        if let Some(dnd_icon) = self.dnd_icon.as_ref() {
            let pointer_pos =
                (pointer_pos + dnd_icon.offset.to_f64()).to_physical_precise_round(output_scale);
            push_elements_from_surface_tree(
                renderer,
                &dnd_icon.surface,
                pointer_pos,
                output_scale,
                1.,
                Kind::ScanoutCandidate,
                &mut |elem| push(elem.into()),
            );
        }
    }

    /// Checks if the pointer should be included on a window cast or screenshot.
    ///
    /// Returns `(cursor_global_pos, win_pos)` if the pointer should be included, or `None`
    /// otherwise.
    pub fn pointer_pos_for_window_cast(
        &self,
        mapped: &Mapped,
    ) -> Option<(Point<f64, Logical>, Point<f64, Logical>)> {
        // Tablet cursor.
        if let Some(tablet_pos) = self.tablet_cursor_location {
            let contents = self.contents_under(tablet_pos);
            if let Some((w, HitType::Input { win_pos })) = contents.window {
                if w == mapped.window {
                    // Tablet tools don't currently expose current focus, and don't currently
                    // have grabs. When those are implemented, this branch should be adjusted
                    // to look more similar to the branch below.
                    return Some((tablet_pos, win_pos));
                }
            }
        }
        // Regular cursor.
        else if let Some((w, HitType::Input { win_pos })) = &self.pointer_contents.window {
            if w == &mapped.window {
                // Grabs can modify the pointer focus, making it different from
                // pointer_contents. Notably, gestures like Mod+MMB will remove the pointer
                // focus, and ClickGrab will keep pointer focus on the clicked window even
                // while it's moving over a different window.
                //
                // So, double-check that current_focus() (after grabs) also matches the pointer
                // contents.
                let pointer = self.seat.get_pointer().unwrap();

                // The DnD grab is a bit special because it has its own focus (data device)
                // while the pointer focus is cleared. That focus is not currently exposed from
                // Smithay, and showing DnD icons on window screenshots seems useful, so let's
                // just allow it during DnD grabs.
                let is_dnd_grab = pointer
                    .with_grab(|_, grab| State::is_dnd_grab(grab.as_any()))
                    .unwrap_or(false);

                let current_focus_matches = is_dnd_grab
                    || pointer
                        .current_focus()
                        .map(|focused| self.find_root_shell_surface(&focused))
                        .is_some_and(|focused| mapped.is_wl_surface(&focused));
                if current_focus_matches {
                    // We don't check for pointer visibility because it can only be Visible or
                    // Hidden, and never Disabled (then it wouldn't have focus). Even when the
                    // pointer is Hidden, we want to render it, since the user explicitly
                    // requested show_pointer = true, and otherwise there's no easy way to
                    // screenshot a window with pointer with hide-when-typing because pressing
                    // the screenshot bind will hide the pointer.
                    return Some((pointer.current_location(), *win_pos));
                }
            }
        }

        None
    }

    pub fn refresh_pointer_outputs(&mut self) {
        if !self.pointer_visibility.is_visible() {
            return;
        }

        let _span = tracy_client::span!("Niri::refresh_pointer_outputs");

        // Check whether we need to draw the tablet cursor or the regular cursor.
        let pointer_pos = self
            .tablet_cursor_location
            .unwrap_or_else(|| self.seat.get_pointer().unwrap().current_location());

        match self.cursor_manager.cursor_image() {
            CursorImageStatus::Surface(ref surface) => {
                let hotspot = with_states(surface, |states| {
                    states
                        .data_map
                        .get::<CursorImageSurfaceData>()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .hotspot
                });

                let surface_pos = pointer_pos.to_i32_round() - hotspot;
                let bbox = bbox_from_surface_tree(surface, surface_pos);

                let dnd = self
                    .dnd_icon
                    .as_ref()
                    .map(|icon| &icon.surface)
                    .map(|surface| (surface, bbox_from_surface_tree(surface, surface_pos)));

                // FIXME we basically need to pick the largest scale factor across the overlapping
                // outputs, this is how it's usually done in clients as well.
                let mut cursor_scale = 1.;
                let mut cursor_transform = Transform::Normal;
                let mut dnd_scale = 1.;
                let mut dnd_transform = Transform::Normal;
                for output in self.global_space.outputs() {
                    let geo = self.global_space.output_geometry(output).unwrap();

                    // Compute pointer surface overlap.
                    if let Some(mut overlap) = geo.intersection(bbox) {
                        overlap.loc -= surface_pos;
                        cursor_scale =
                            f64::max(cursor_scale, output.current_scale().fractional_scale());
                        // FIXME: using the largest overlapping or "primary" output transform would
                        // make more sense here.
                        cursor_transform = output.current_transform();
                        output_update(output, Some(overlap), surface);
                    } else {
                        output_update(output, None, surface);
                    }

                    // Compute DnD icon surface overlap.
                    if let Some((surface, bbox)) = dnd {
                        if let Some(mut overlap) = geo.intersection(bbox) {
                            overlap.loc -= surface_pos;
                            dnd_scale =
                                f64::max(dnd_scale, output.current_scale().fractional_scale());
                            // FIXME: using the largest overlapping or "primary" output transform
                            // would make more sense here.
                            dnd_transform = output.current_transform();
                            output_update(output, Some(overlap), surface);
                        } else {
                            output_update(output, None, surface);
                        }
                    }
                }

                with_states(surface, |data| {
                    send_scale_transform(
                        surface,
                        data,
                        output::Scale::Fractional(cursor_scale),
                        cursor_transform,
                    )
                });
                if let Some((surface, _)) = dnd {
                    with_states(surface, |data| {
                        send_scale_transform(
                            surface,
                            data,
                            output::Scale::Fractional(dnd_scale),
                            dnd_transform,
                        );
                    });
                }
            }
            cursor_image => {
                // There's no cursor surface, but there might be a DnD icon.
                let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) else {
                    return;
                };

                let icon = if let CursorImageStatus::Named(icon) = cursor_image {
                    *icon
                } else {
                    Default::default()
                };

                let mut dnd_scale = 1.;
                let mut dnd_transform = Transform::Normal;
                for output in self.global_space.outputs() {
                    let geo = self.global_space.output_geometry(output).unwrap();

                    // The default cursor is rendered at the right scale for each output, which
                    // means that it may have a different hotspot for each output.
                    let output_scale = output.current_scale().integer_scale();
                    let cursor = self
                        .cursor_manager
                        .get_cursor_with_name(icon, output_scale)
                        .unwrap_or_else(|| self.cursor_manager.get_default_cursor(output_scale));

                    // For simplicity, we always use frame 0 for this computation. Let's hope the
                    // hotspot doesn't change between frames.
                    let hotspot = XCursor::hotspot(&cursor.frames()[0]).to_logical(output_scale);

                    let surface_pos = pointer_pos.to_i32_round() - hotspot;
                    let bbox = bbox_from_surface_tree(surface, surface_pos);

                    if let Some(mut overlap) = geo.intersection(bbox) {
                        overlap.loc -= surface_pos;
                        dnd_scale = f64::max(dnd_scale, output.current_scale().fractional_scale());
                        // FIXME: using the largest overlapping or "primary" output transform would
                        // make more sense here.
                        dnd_transform = output.current_transform();
                        output_update(output, Some(overlap), surface);
                    } else {
                        output_update(output, None, surface);
                    }
                }

                with_states(surface, |data| {
                    send_scale_transform(
                        surface,
                        data,
                        output::Scale::Fractional(dnd_scale),
                        dnd_transform,
                    );
                });
            }
        }
    }

    pub fn refresh_layout(&mut self) {
        let layout_is_active = match &self.keyboard_focus {
            KeyboardFocus::Layout { .. } => true,
            KeyboardFocus::LayerShell { .. } => false,

            // Draw layout as active in these cases to reduce unnecessary window animations.
            // There's no confusion because these are both fullscreen modes.
            //
            // FIXME: when going into the screenshot UI from a layer-shell focus, and then back to
            // layer-shell, the layout will briefly draw as active, despite never having focus.
            KeyboardFocus::LockScreen { .. } => true,
            KeyboardFocus::ScreenshotUi => true,
            KeyboardFocus::ExitConfirmDialog => true,
            KeyboardFocus::Overview => true,
            KeyboardFocus::Mru => true,
        };

        self.layout.refresh(layout_is_active);
    }

    pub fn refresh_idle_inhibit(&mut self) {
        let _span = tracy_client::span!("Niri::refresh_idle_inhibit");

        self.idle_inhibiting_surfaces.retain(|s| s.is_alive());

        let is_inhibited = self.is_fdo_idle_inhibited.load(Ordering::SeqCst)
            || self.idle_inhibiting_surfaces.iter().any(|surface| {
                with_states(surface, |states| {
                    surface_primary_scanout_output(surface, states).is_some()
                })
            });
        self.idle_notifier_state.set_is_inhibited(is_inhibited);
    }

    pub fn refresh_window_states(&mut self) {
        let _span = tracy_client::span!("Niri::refresh_window_states");

        let config = self.config.borrow();
        self.layout.with_windows_mut(|mapped, _output| {
            mapped.update_tiled_state(config.prefer_no_csd);
        });
        drop(config);
    }

    pub fn refresh_window_rules(&mut self) {
        let _span = tracy_client::span!("Niri::refresh_window_rules");

        let config = self.config.borrow();
        let window_rules = &config.window_rules;

        let mut windows = vec![];
        let mut outputs = HashSet::new();
        self.layout.with_windows_mut(|mapped, output| {
            if mapped.recompute_window_rules_if_needed(window_rules, self.is_at_startup) {
                windows.push(mapped.window.clone());

                if let Some(output) = output {
                    outputs.insert(output.clone());
                }

                // Since refresh_window_rules() is called after refresh_layout(), we need to update
                // the tiled state right here, so that it's picked up by the following
                // send_pending_configure().
                mapped.update_tiled_state(config.prefer_no_csd);
            }
        });
        drop(config);

        for win in windows {
            self.layout.update_window(&win, None);
            win.toplevel()
                .expect("no X11 support")
                .send_pending_configure();
        }
        for output in outputs {
            self.queue_redraw(&output);
        }
    }

    pub fn advance_animations(&mut self) {
        let _span = tracy_client::span!("Niri::advance_animations");

        self.layout.advance_animations();
        self.config_error_notification.advance_animations();
        self.exit_confirm_dialog.advance_animations();
        self.screenshot_ui.advance_animations();
        self.window_mru_ui.advance_animations();

        for state in self.output_state.values_mut() {
            if let Some(transition) = &mut state.screen_transition {
                if transition.is_done() {
                    state.screen_transition = None;
                }
            }
        }
    }

    pub fn update_render_elements(&mut self, output: Option<&Output>) {
        self.update_xray_render_elements(output);
        self.layout.update_render_elements(output);

        for (out, state) in self.output_state.iter_mut() {
            if output.is_none_or(|output| out == output) {
                let scale = Scale::from(out.current_scale().fractional_scale());
                let transform = out.current_transform();

                if let Some(transition) = &mut state.screen_transition {
                    transition.update_render_elements(scale, transform);
                }

                let layer_map = layer_map_for_output(out);
                for surface in layer_map.layers() {
                    let Some(mapped) = self.mapped_layer_surfaces.get_mut(surface) else {
                        continue;
                    };
                    let Some(geo) = layer_map.layer_geometry(surface) else {
                        continue;
                    };

                    mapped.update_render_elements(geo.size.to_f64());
                }
            }
        }
    }

    // Updates only those render elements that go in the xray buffer.
    pub fn update_xray_render_elements(&mut self, output: Option<&Output>) {
        for (out, state) in self.output_state.iter_mut() {
            if output.is_none_or(|output| out == output) {
                let scale = Scale::from(out.current_scale().fractional_scale());
                let mode = out.current_mode().unwrap();
                let transform = out.current_transform();
                let size = transform.transform_size(mode.size);

                state.xray.workspaces.clear();
                let mon = self.layout.monitor_for_output(out).unwrap();
                for (ws, geo) in mon.workspaces_with_render_geo() {
                    let bg_color = ws.render_background().color();
                    state.xray.workspaces.push((geo, bg_color));
                }
                state.xray.backdrop_color = state.backdrop_buffer.color();
                let blur_options = BlurOptions::from(self.config.borrow().blur);
                for buf in &state.xray.background {
                    let mut buffer = buf.borrow_mut();
                    buffer.update_size(size, scale);
                    buffer.update_blur_options(blur_options);
                }
                for buf in &state.xray.backdrop {
                    let mut buffer = buf.borrow_mut();
                    buffer.update_size(size, scale);
                    buffer.update_blur_options(blur_options);
                }

                let layer_map = layer_map_for_output(out);
                for surface in layer_map.layers_on(Layer::Background) {
                    let Some(mapped) = self.mapped_layer_surfaces.get_mut(surface) else {
                        continue;
                    };
                    let Some(geo) = layer_map.layer_geometry(surface) else {
                        continue;
                    };

                    mapped.update_render_elements(geo.size.to_f64());
                }
            }
        }
    }

    pub fn update_shaders(&mut self) {
        self.layout.update_shaders();

        for mapped in self.mapped_layer_surfaces.values_mut() {
            mapped.update_shaders();
        }
    }

    pub fn render_to_vec<R: NiriRenderer>(
        &self,
        ctx: RenderCtx<R>,
        output: &Output,
        include_pointer: bool,
    ) -> Vec<OutputRenderElements<R>> {
        let mut elements = Vec::new();
        self.render(ctx, output, include_pointer, &mut |elem| {
            elements.push(elem)
        });
        elements
    }

    /// Whether Task 6's cover-flow panel stack needs to draw anything this frame: the carousel is
    /// configured and open with reveal > 0, or the rotation hasn't settled back on the host. When
    /// false, the pre-carousel overview path draws with no prepass and no panel elements. See the
    /// drawing rule in the Gate B Task 6 brief.
    fn carousel_panels_needed(&self) -> bool {
        if !self.layout.is_overview_open() {
            return false;
        }
        let reveal = self.layout.carousel_reveal();
        let rotation = self.layout.carousel_rotation();
        let settled_on_host = rotation == rotation.round()
            && rotation.round() == 0.
            && !self.layout.carousel_rotating();
        !(reveal == 0. && settled_on_host)
    }

    /// Whether `output`'s own live workspace strip must stay suppressed because it is drawn (or
    /// will be, once settled) as a panel in the cover-flow stack instead: true while rotating, or
    /// once settled on some OTHER (non-host) ring position. False when panels aren't needed at
    /// all, or once settled back on the host itself (reveal > 0 but rotation == 0 and not
    /// animating) — in that case the host strip draws live, matching the drawing rule's "skip
    /// d == 0 when settled_on_host" panel-skip.
    fn carousel_host_strip_suppressed(&self, output: &Output) -> bool {
        if self.layout.active_output() != Some(output) || !self.carousel_panels_needed() {
            return false;
        }
        let rotation = self.layout.carousel_rotation();
        let settled_on_host = rotation == rotation.round() && rotation.round() == 0.;
        !settled_on_host || self.layout.carousel_rotating()
    }

    /// Queues a redraw of the carousel host when `content_output`'s own commit-driven redraw
    /// (queued by the caller, at its own call site) isn't enough to keep that content's panel
    /// fresh.
    ///
    /// Panel content is only ever published from inside the active host's own `Niri::render`
    /// (`update_panel_sources`, single-host gated). A sibling ring member's window commit queues
    /// a redraw of *that sibling's own* output only — if the host itself is otherwise idle (no
    /// unrelated damage of its own), a sibling animation resuming (e.g. a video unpausing) would
    /// leave its panel frozen on the host until the host's next unrelated redraw. Callers should
    /// invoke this alongside their existing `queue_redraw(content_output)` at every window/
    /// surface-commit site.
    ///
    /// Bails out in one branch in the common (carousel not active) case to keep this cheap on
    /// every commit.
    pub fn queue_carousel_host_redraw_for_sibling(&mut self, content_output: &Output) {
        if !self.carousel_panels_needed() {
            return;
        }
        let Some(host) = self.layout.active_output().cloned() else {
            return;
        };
        if &host == content_output {
            // The committing surface's own output IS the host: its own queue_redraw already
            // covers this (and the host's prepass re-publishes from its own render anyway).
            return;
        }
        let is_ring_member = self
            .layout
            .carousel_outputs()
            .into_iter()
            .any(|m| m.output() == content_output);
        if is_ring_member {
            self.queue_redraw(&host);
        }
    }

    pub fn render<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        output: &Output,
        include_pointer: bool,
        push: &mut dyn FnMut(OutputRenderElements<R>),
    ) {
        let _span = tracy_client::span!("Niri::render");

        if ctx.target == RenderTarget::Output {
            if let Some(preview) = self.config.borrow().debug.preview_render {
                // Note: this retargets away from RenderTarget::Output. The global post-process
                // shader is now gated on `target_renders_shaders`, so the preview is shaderless
                // unless `shaders-in-capture` is enabled — in which case it shows shaders via
                // the same capture path.
                ctx.target = match preview {
                    PreviewRender::Screencast => RenderTarget::Screencast,
                    PreviewRender::ScreenCapture => RenderTarget::ScreenCapture,
                };
            }
        }

        // Single-host gating: only the active output's own render ever refreshes panel sources
        // or reconciles ring membership. `update_panel_sources` bakes every source at THIS host's
        // `fill_zoom`, so calling it from a non-active output's render would fight over the same
        // sources' baked zoom with the real active host (the multi-host zoom-contamination
        // finding from the Task 5 review). Non-active-output renders (e.g. an isolated output, or
        // a screencopy of a non-focused monitor) touch no panel state at all; a stale ring member
        // is reconciled the next time the active host itself redraws.
        if self.layout.active_output() == Some(output) {
            if self.carousel_panels_needed() {
                self.update_panel_sources(&mut ctx.as_gles(), output, self.panel_frame.get());
                let ring: Vec<Output> = self
                    .layout
                    .carousel_outputs()
                    .into_iter()
                    .map(|m| m.output().clone())
                    .collect();
                self.clear_stale_panel_sources(&ring);
            } else {
                // The carousel isn't active on this host at all: nothing should still be
                // publishing panel content.
                self.clear_stale_panel_sources(&[]);
            }
        }

        // Skip the xray background re-render while the host's live workspace strip is
        // suppressed: xray elements are only consumed by that strip's per-workspace layer-shell
        // rendering in `render_inner`, which doesn't run in that case either.
        if !self.carousel_host_strip_suppressed(output) {
            self.fill_xray_elements(ctx.as_gles(), output);
        }

        // Reborrow to shorten lifetime to be able to put in xray.
        let mut ctx = ctx.r();
        let state = self.output_state.get(output).unwrap();
        ctx.xray = Some(&state.xray);

        self.render_inner(ctx, output, include_pointer, push);

        self.clear_xray_elements(output);
    }

    fn render_inner<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        output: &Output,
        include_pointer: bool,
        push: &mut dyn FnMut(OutputRenderElements<R>),
    ) {
        // See `IN_RENDER_INNER` / `update_panel_sources`: panel content prepasses must run before
        // this, never nested inside it.
        let _in_render_inner = InRenderInnerGuard::enter();

        let state = self.output_state.get(output).unwrap();
        let output_scale = Scale::from(output.current_scale().fractional_scale());

        let push = if self.debug_draw_opaque_regions {
            &mut move |elem| {
                push_opaque_regions(&elem, output_scale, push);
                push(elem);
            }
        } else {
            push
        };

        // Global post-process shader: always pushed for the real Output sink; pushed for capture
        // targets (Screencast/ScreenCapture) only when `shaders-in-capture` is enabled. Capture
        // renders use throwaway feedback sinks so the per-output ping-pong state is never
        // corrupted. A compiled program is also required; this gate has zero overhead when no
        // program is loaded.
        //
        // When reads_cursor is active, the element must render ABOVE the cursor (pushed before
        // render_pointer) so the cursor is captured into the screen texture. Otherwise it renders
        // BELOW the cursor (pushed after render_pointer) so the hardware cursor is unaffected.
        let reads_cursor = {
            let cfg = self.config.borrow();
            cfg.global_shader.enable && cfg.global_shader.reads_cursor
        };
        let capture_enabled = self.config.borrow().shaders_in_capture;
        let capture_render = ctx.target != RenderTarget::Output;
        let mut global_shader_elem: Option<GlobalShaderElement> =
            if crate::render_helpers::target_renders_shaders(ctx.target, capture_enabled)
                && Shaders::get(ctx.renderer)
                    .program(ProgramType::GlobalPass(0))
                    .is_some()
            {
                let start = state.global_shader_start.get().unwrap_or_else(|| {
                    let now = std::time::Instant::now();
                    state.global_shader_start.set(Some(now));
                    now
                });
                let time = start.elapsed().as_secs_f32();
                let scale = output.current_scale().fractional_scale() as f32;
                let area = Rectangle::from_size(output_size(output));

                // Cursor in output-local physical pixels (same source as render_pointer).
                let output_pos = self.global_space.output_geometry(output).unwrap().loc;
                let pointer_pos = self
                    .tablet_cursor_location
                    .unwrap_or_else(|| self.seat.get_pointer().unwrap().current_location());
                let pointer_local = pointer_pos - output_pos.to_f64();
                let cursor = (
                    (pointer_local.x * scale as f64) as f32,
                    (pointer_local.y * scale as f64) as f32,
                );

                let output_size_phys = (
                    (area.size.w * scale as f64) as f32,
                    (area.size.h * scale as f64) as f32,
                );

                // Region-local rendering: when cursor-radius is set on a cursor-only
                // (non-animating) shader, shrink the element to a box around the
                // cursor so damage + scanout are limited to that box. Otherwise
                // cover the whole output.
                let caps = self.global_shader_caps();
                let cursor_radius = self.config.borrow().global_shader.cursor_radius;
                // Number of passes in the active chain (registry length). Region mode is
                // whole-output only; multi-pass chains (N >= 2) always reshade the
                // full output.
                let n_passes = Shaders::get(ctx.renderer)
                    .custom_global_passes
                    .borrow()
                    .len();
                let (area, region_norm) = match cursor_radius {
                    Some(r)
                        if caps.uses_cursor && !caps.is_animating() && r > 0 && n_passes <= 1 =>
                    {
                        let r = r as f64; // logical px
                        let full = area;
                        let box_loc = (
                            (pointer_local.x - r).max(full.loc.x),
                            (pointer_local.y - r).max(full.loc.y),
                        );
                        let box_end = (
                            (pointer_local.x + r).min(full.loc.x + full.size.w),
                            (pointer_local.y + r).min(full.loc.y + full.size.h),
                        );
                        let box_rect = Rectangle::new(
                            (box_loc.0, box_loc.1).into(),
                            (
                                (box_end.0 - box_loc.0).max(0.0),
                                (box_end.1 - box_loc.1).max(0.0),
                            )
                                .into(),
                        );
                        if box_rect.size.w <= 0.0 || box_rect.size.h <= 0.0 {
                            // Cursor is off this output (e.g. on another monitor) so the clamped
                            // box is empty: render whole-output here. A zero-size region would
                            // otherwise divide by zero in the shader's tex2D_* remap.
                            (full, [0.0, 0.0, 1.0, 1.0])
                        } else {
                            let region_norm = [
                                ((box_rect.loc.x - full.loc.x) / full.size.w) as f32,
                                ((box_rect.loc.y - full.loc.y) / full.size.h) as f32,
                                (box_rect.size.w / full.size.w) as f32,
                                (box_rect.size.h / full.size.h) as f32,
                            ];
                            (box_rect, region_norm)
                        }
                    }
                    _ => (area, [0.0, 0.0, 1.0, 1.0]),
                };

                // A fresh Id every frame makes smithay damage both the new box and the vacated old
                // box (the removed element's last geometry), which is what gives region mode its
                // correct box(cursor) ∪ box(prev_cursor) damage. If this is ever changed to a
                // stable per-output Id for a damage optimization, region mode must damage
                // the vacated box explicitly or moving-cursor effects will leave trails. Size the
                // per-output chain state to the active chain length and snapshot
                // each pass's feedback handles for this frame. The borrow is
                // dropped before constructing the element (it only needs the cloned
                // handles, not the chain itself).
                let passes = {
                    let mut chain = state.global_shader_chain.borrow_mut();
                    chain.resize(n_passes);
                    (0..n_passes)
                        .map(|i| GlobalPassState {
                            // Read the live trail either way.
                            prev: chain.prev[i].clone(),
                            buffer_prev: chain.buffer_prev[i].clone(),
                            // Capture renders write to throwaway sinks/offscreens so the live
                            // output's ping-pong is never corrupted;
                            // the Output render uses the real shared ones.
                            result: if capture_render {
                                std::rc::Rc::new(std::cell::RefCell::new(None))
                            } else {
                                chain.result[i].clone()
                            },
                            buffer_result: if capture_render {
                                std::rc::Rc::new(std::cell::RefCell::new(None))
                            } else {
                                chain.buffer_result[i].clone()
                            },
                            pass_offscreen: if capture_render {
                                std::rc::Rc::new(
                                    crate::render_helpers::offscreen::OffscreenBuffer::default(),
                                )
                            } else {
                                chain.pass_offscreen[i].clone()
                            },
                            buffer: if capture_render {
                                std::rc::Rc::new(
                                    crate::render_helpers::offscreen::OffscreenBuffer::default(),
                                )
                            } else {
                                chain.buffer[i].clone()
                            },
                        })
                        .collect::<Vec<_>>()
                };
                // screen_result is the niri_screen_prev ping-pong sink; throwaway for capture
                // renders.
                let screen_result = if capture_render {
                    std::rc::Rc::new(std::cell::RefCell::new(None))
                } else {
                    state.global_shader_screen_result.clone()
                };

                Some(GlobalShaderElement::new(
                    Id::new(),
                    area,
                    scale,
                    time,
                    cursor,
                    region_norm,
                    output_size_phys,
                    state.global_shader_screen_prev.clone(),
                    screen_result,
                    passes,
                ))
            } else {
                None
            };

        // When reads_cursor is on, push the global shader element first so the cursor is
        // composited into niri_screen (the cursor becomes part of the captured screen texture).
        if reads_cursor {
            if let Some(elem) = global_shader_elem.take() {
                push(elem.into());
            }
        }

        // The pointer goes on the top (or below the global shader when reads_cursor is on).
        if include_pointer && self.pointer_visibility.is_visible() {
            self.render_pointer(ctx.renderer, output, &mut |elem| push(elem.into()));
        }

        // When reads_cursor is off (default), push the global shader element after the pointer
        // so the hardware cursor remains above the effect.
        if !reads_cursor {
            if let Some(elem) = global_shader_elem {
                push(elem.into());
            }
        }

        // Push region shader elements for regions matching this output.
        if crate::render_helpers::target_renders_shaders(ctx.target, capture_enabled) {
            let scale = output.current_scale().fractional_scale() as f32;
            let out_name = output.name();
            let full = Rectangle::from_size(output_size(output));
            let out_phys = (
                (full.size.w * scale as f64) as f32,
                (full.size.h * scale as f64) as f32,
            );
            let output_pos = self.global_space.output_geometry(output).unwrap().loc;
            let pointer_pos = self
                .tablet_cursor_location
                .unwrap_or_else(|| self.seat.get_pointer().unwrap().current_location());
            let pointer_local = pointer_pos - output_pos.to_f64();
            let cursor = (
                (pointer_local.x * scale as f64) as f32,
                (pointer_local.y * scale as f64) as f32,
            );
            let time = {
                let start = state.global_shader_start.get().unwrap_or_else(|| {
                    let now = std::time::Instant::now();
                    state.global_shader_start.set(Some(now));
                    now
                });
                start.elapsed().as_secs_f32()
            };

            // Clone region data out before any mutable borrows.
            let regions: Vec<_> = {
                let config = self.config.borrow();
                config
                    .region_shaders
                    .iter()
                    .filter(|r| r.output.as_deref().map_or(true, |want| out_name == want))
                    .map(|r| {
                        let chain = r.pass_sources(read_scoped_shader_path);
                        (r.geometry.clone(), chain)
                    })
                    .collect()
            };

            for (geom, chain) in regions {
                if chain.is_empty() {
                    continue;
                }
                let key = shaders::scoped_key(&chain);
                if Shaders::get(ctx.renderer)
                    .program(ProgramType::Scoped(key, 0))
                    .is_none()
                {
                    continue;
                }
                let area =
                    Rectangle::new((geom.x, geom.y).into(), (geom.width, geom.height).into());
                let region_norm = [
                    ((area.loc.x - full.loc.x as f64) / full.size.w) as f32,
                    ((area.loc.y - full.loc.y as f64) / full.size.h) as f32,
                    (area.size.w / full.size.w) as f32,
                    (area.size.h / full.size.h) as f32,
                ];
                let n_passes = chain.len();
                let offscreens = (0..n_passes.saturating_sub(1))
                    .map(|_| {
                        std::rc::Rc::new(
                            crate::render_helpers::offscreen::OffscreenBuffer::default(),
                        )
                    })
                    .collect::<Vec<_>>();
                debug_assert_eq!(offscreens.len(), n_passes.saturating_sub(1));
                let elem = ScopedShaderElement::new(
                    Id::new(),
                    area,
                    scale,
                    time,
                    cursor,
                    region_norm,
                    out_phys,
                    key,
                    n_passes,
                    ScopedSource::Capture,
                    offscreens,
                );
                push(elem.into());
            }
        }

        // Next, the screen transition texture.
        {
            if let Some(transition) = &state.screen_transition {
                push(transition.render(ctx.target).into());
            }
        }

        // Next, the exit confirm dialog.
        self.exit_confirm_dialog
            .render(ctx.renderer, output, &mut |elem| push(elem.into()));

        // Next, the config error notification too. Isolated outputs stay free of compositor chrome.
        if !self.is_output_isolated(output) {
            if let Some(element) = self.config_error_notification.render(ctx.renderer, output) {
                push(element.into());
            }
        }

        // If the session is locked, draw the lock surface.
        if self.is_locked() {
            if let Some(surface) = state.lock_surface.as_ref() {
                push_elements_from_surface_tree(
                    ctx.renderer,
                    surface.wl_surface(),
                    Point::new(0, 0),
                    output_scale,
                    1.,
                    Kind::ScanoutCandidate,
                    &mut |elem| push(elem.into()),
                );
            }

            // Draw the solid color background.
            push(
                SolidColorRenderElement::from_buffer(
                    &state.lock_color_buffer,
                    (0., 0.),
                    1.,
                    Kind::Unspecified,
                )
                .into(),
            );

            return;
        }

        // Prepare the background elements.
        let backdrop = SolidColorRenderElement::from_buffer(
            &state.backdrop_buffer,
            (0., 0.),
            1.,
            Kind::Unspecified,
        )
        .into();

        // If the screenshot UI is open, draw it.
        if self.screenshot_ui.is_open() {
            self.screenshot_ui
                .render_output(output, ctx.target, &mut |elem| push(elem.into()));

            // Add the backdrop for outputs that were connected while the screenshot UI was open.
            push(backdrop);

            return;
        }

        // The hotkey overlay and Alt-Tab switcher are compositor chrome, so keep them off isolated
        // outputs (projection/signage/capture); windows and layer-shell content still render below.
        if !self.is_output_isolated(output) {
            // Draw the hotkey overlay on top.
            if let Some(element) = self.hotkey_overlay.render(ctx.renderer, output) {
                push(element.into());
            }

            // Then, the Alt-Tab switcher.
            self.window_mru_ui
                .render_output(self, output, ctx.r(), &mut |elem| push(elem.into()));
        }

        // Don't draw the focus ring on the workspaces while interactively moving above those
        // workspaces, since the interactively-moved window already has a focus ring.
        let focus_ring = !self.layout.interactive_move_is_moving_above_output(output);

        // Get monitor elements.
        let mon = self.layout.monitor_for_output(output).unwrap();
        let zoom = mon.overview_zoom();

        // Get layer-shell elements.
        let layer_map = layer_map_for_output(output);

        // We use macros instead of closures to avoid borrowing issues (renderer and push() go
        // into different functions).
        macro_rules! push_popups_from_layer {
            ($layer:expr, $ns:expr, $xray_pos:expr, $backdrop:expr, $push:expr) => {{
                self.render_layer_popups(
                    ctx.r(),
                    $ns,
                    &layer_map,
                    $layer,
                    $xray_pos,
                    $backdrop,
                    $push,
                );
            }};
            ($layer:expr, true) => {{
                push_popups_from_layer!($layer, None, XrayPos::default(), true, &mut |elem| push(
                    elem.into()
                ));
            }};
            ($layer:expr, $ns:expr, $xray_pos:expr, $push:expr) => {{
                push_popups_from_layer!($layer, $ns, $xray_pos, false, $push);
            }};
            ($layer:expr) => {{
                push_popups_from_layer!($layer, None, XrayPos::default(), false, &mut |elem| push(
                    elem.into()
                ));
            }};
        }
        macro_rules! push_normal_from_layer {
            ($layer:expr, $ns:expr, $xray_pos:expr, $backdrop:expr, $push:expr) => {{
                self.render_layer_normal(
                    ctx.r(),
                    $ns,
                    &layer_map,
                    $layer,
                    $xray_pos,
                    $backdrop,
                    $push,
                );
            }};
            ($layer:expr, true) => {{
                push_normal_from_layer!($layer, None, XrayPos::default(), true, &mut |elem| {
                    push(elem.into())
                });
            }};
            ($layer:expr, $ns:expr, $xray_pos:expr, $push:expr) => {{
                push_normal_from_layer!($layer, $ns, $xray_pos, false, $push);
            }};
            ($layer:expr) => {{
                push_normal_from_layer!($layer, None, XrayPos::default(), false, &mut |elem| {
                    push(elem.into())
                });
            }};
        }

        // Whether this output's own live workspace strip must stay off because it is drawn (or
        // will be, once settled) as a panel in the cover-flow stack below instead. See
        // `carousel_host_strip_suppressed`'s doc for the exact rule.
        let host_strip_suppressed = self.carousel_host_strip_suppressed(output);

        // The overlay layer elements go next.
        push_popups_from_layer!(Layer::Overlay);
        push_normal_from_layer!(Layer::Overlay);

        // When rendering above the top layer, we put the regular monitor elements first.
        // Otherwise, we will render all layer-shell pop-ups and the top layer on top.
        if mon.render_above_top_layer() {
            self.layout
                .render_interactive_move_for_output(ctx.r(), output, &mut |elem| push(elem.into()));

            mon.render_insert_hint_between_workspaces(ctx.renderer, &mut |elem| push(elem.into()));

            mon.render_workspaces(ctx.r(), focus_ring, &mut |elem| push(elem.into()));

            push_popups_from_layer!(Layer::Top);
            push_normal_from_layer!(Layer::Top);

            push_popups_from_layer!(Layer::Bottom);
            push_popups_from_layer!(Layer::Background);
            push_normal_from_layer!(Layer::Bottom);
            push_normal_from_layer!(Layer::Background);

            // We don't expect more than one workspace when render_above_top_layer().
            if let Some((ws, _geo)) = mon.workspaces_with_render_geo().next() {
                push(ws.render_background().into());
            }
        } else {
            push_popups_from_layer!(Layer::Top);
            push_normal_from_layer!(Layer::Top);

            self.layout
                .render_interactive_move_for_output(ctx.r(), output, &mut |elem| push(elem.into()));

            mon.render_insert_hint_between_workspaces(ctx.renderer, &mut |elem| push(elem.into()));

            // Macro instead of closure to avoid borrowing push().
            macro_rules! process {
                ($geo:expr) => {{
                    &mut |elem| {
                        if let Some(elem) = scale_relocate_crop(elem, output_scale, zoom, $geo) {
                            push(elem.into());
                        }
                    }
                }};
            }

            // Suppressed while `host_strip_suppressed`: this output's own workspace strip
            // (and its per-workspace background/bottom layer-shell elements) is instead
            // drawn as a panel in the cover-flow stack below. Overlay/top popups above and
            // the backdrop/cursor elsewhere are unaffected.
            if !host_strip_suppressed {
                for (ws, geo) in mon.workspaces_with_render_geo() {
                    let ns = Some(ws.id().get() as usize);
                    let xray_pos = XrayPos::new(geo.loc, zoom);
                    push_popups_from_layer!(Layer::Bottom, ns, xray_pos, process!(geo));
                    push_popups_from_layer!(Layer::Background, ns, xray_pos, process!(geo));
                }

                mon.render_workspaces(ctx.r(), focus_ring, &mut |elem| push(elem.into()));

                for (ws, geo) in mon.workspaces_with_render_geo() {
                    // The render element namespace. This will be set to the workspace index for
                    // elements duplicated across workspaces (i.e. background and bottom layers) in
                    // order to have their non-xray framebuffer effects separated from each other.
                    //
                    // This doesn't have to correspond exactly to workspace id or idx, the only
                    // requirement is that there's only one framebuffer effect element with a given id +
                    // namespace on the frame at once. Id + namespace is used as the cache key in the
                    // damage tracker.
                    let ns = Some(ws.id().get() as usize);
                    let xray_pos = XrayPos::new(geo.loc, zoom);
                    push_normal_from_layer!(Layer::Bottom, ns, xray_pos, process!(geo));
                    push_normal_from_layer!(Layer::Background, ns, xray_pos, process!(geo));

                    process!(geo)(ws.render_background());
                }
            }
        }

        // ===== Consolidated carousel: cover-flow panel stack =====
        //
        // The drawing rule (Gate B Task 6 brief): for every ring entry (idx, ring_pos), let
        // d = ring_pos - rotation. d == 0 while settled_on_host is the live center, already
        // drawn above by the (unsuppressed) host workspace strip, so it's skipped here. Every
        // other visible d gets a panel: geometry from `carousel::panel_placement`, content from
        // that ring member's own `PanelSource` (baked by `update_panel_sources`, single-host
        // gated in `Niri::render`). This one rule also produces the settled-remote "lens" case
        // for free: rotation snaps to the target's ring position, so d == 0 there instead, and
        // it fills the center slot at CENTER_FRACTION while every other ring member (including
        // the host itself) recedes to the side as an ordinary panel.
        if self.layout.active_output() == Some(output) && self.carousel_panels_needed() {
            let view = mon.view_size();
            let scale = output.current_scale().fractional_scale();
            let rotation = self.layout.carousel_rotation();
            let reveal = self.layout.carousel_reveal();
            let settled_on_host = rotation == rotation.round()
                && rotation.round() == 0.
                && !self.layout.carousel_rotating();
            let outputs = self.layout.carousel_outputs();

            let mut visible: Vec<(usize, f64, crate::layout::carousel::PanelPlacement)> = self
                .layout
                .carousel_ring()
                .into_iter()
                .filter_map(|(idx, ring_pos)| {
                    let d = ring_pos - rotation;
                    if d == 0. && settled_on_host {
                        return None;
                    }
                    let placement =
                        crate::layout::carousel::panel_placement((view.w, view.h), d, reveal)?;
                    Some((idx, ring_pos, placement))
                })
                .collect();

            // Nearer panels first: z is highest (100.0) at the center and decreases with depth,
            // so sorting descending by z and pushing in that order puts nearer panels on top
            // (niri renders earlier-pushed elements in front). This is also hit-test priority
            // order for `panel_hits` below.
            visible.sort_by(|a, b| b.2.z.partial_cmp(&a.2.z).unwrap());

            let mut hits: Vec<(f64, [[f64; 2]; 4])> = Vec::with_capacity(visible.len());
            let mut panel_elems = state.panel_elems.borrow_mut();
            for (idx, ring_pos, placement) in &visible {
                let Some(m) = outputs.get(*idx) else {
                    continue;
                };
                let Some(source_state) = self.output_state.get(m.output()) else {
                    continue;
                };
                let source = source_state.panel_source.borrow();
                let Some(source_elem) = source.elem.as_ref() else {
                    continue;
                };
                let texture = source_elem.texture().clone();
                let source_id = source_elem.id().clone();
                let context_id = source_elem.context_id();
                let generation = source.generation;
                drop(source);

                let corners = crate::render_helpers::panel_quad::tilted_panel_corners(
                    placement.center,
                    placement.size,
                    placement.yaw,
                    view.w * crate::layout::carousel::FOCAL_FACTOR,
                );
                hits.push((*ring_pos, corners));

                match panel_elems.entry(*idx) {
                    std::collections::hash_map::Entry::Occupied(mut o) => {
                        o.get_mut().update(
                            corners,
                            texture,
                            source_id,
                            generation,
                            context_id,
                            placement.dim,
                            scale,
                            1.0,
                        );
                    }
                    std::collections::hash_map::Entry::Vacant(v) => {
                        if let Some(elem) = PanelRenderElement::new(
                            corners,
                            texture,
                            source_id,
                            generation,
                            context_id,
                            placement.dim,
                            scale,
                            1.0,
                        ) {
                            v.insert(elem);
                        }
                    }
                }
            }

            // Ring-shrink drop: entries whose ring index no longer has a placement this frame
            // (the ring got smaller, or that member scrolled beyond MAX_VISIBLE_DEPTH) don't
            // belong in the retained map anymore.
            let visible_idx: std::collections::HashSet<usize> =
                visible.iter().map(|(idx, _, _)| *idx).collect();
            panel_elems.retain(|idx, _| visible_idx.contains(idx));

            for (idx, _, _) in &visible {
                if let Some(elem) = panel_elems.get(idx) {
                    push(OutputRenderElements::Panel(elem.clone()));
                }
            }

            *state.panel_hits.borrow_mut() = hits;
        } else {
            state.panel_hits.borrow_mut().clear();
        }
        // ===== END panel stack =====

        mon.render_workspace_shadows(ctx.renderer, &mut |elem| push(elem.into()));

        // Then the backdrop.
        push_popups_from_layer!(Layer::Background, true);
        push_normal_from_layer!(Layer::Background, true);

        push(backdrop);
    }

    pub fn fill_xray_elements(&self, mut ctx: RenderCtx<GlesRenderer>, output: &Output) {
        let _span = tracy_client::span!("Niri::fill_xray_elements");

        // Make sure the xrayed elements themselves cannot use xray by mistake.
        ctx.xray = None;

        let state = self.output_state.get(output).unwrap();
        let xray = &state.xray;
        let layer_map = layer_map_for_output(output);

        // FIXME: it would be cool to call this code on-demand. It's even relatively simple to do:
        // move this function to after the render_inner() call, check if
        // Rc::strong_count(&xray.background) > 1, and only then construct the elements. This way,
        // only if something referenced the xray buffer will the elements get constructed.
        //
        // Unfortunately, currently this runs into an important limitation: offscreens are rendered
        // immediately deep inside render_inner(), and when they are, they already need the xray
        // elements filled.
        //
        // Perhaps in the future when offscreen rendering becomes on-demand, this optimization will
        // be possible.

        let mut buffer = xray.background[ctx.target as usize].borrow_mut();
        {
            let elements = buffer.elements();
            elements.clear();
            self.render_layer_normal(
                ctx.r(),
                None,
                &layer_map,
                Layer::Background,
                XrayPos::default(),
                false,
                &mut |elem| elements.push(elem.into()),
            );
            // Avoid unused capacity remaining forever.
            elements.shrink_to_fit();
        }

        let mut buffer = xray.backdrop[ctx.target as usize].borrow_mut();
        {
            let elements = buffer.elements();
            elements.clear();
            self.render_layer_normal(
                ctx.r(),
                None,
                &layer_map,
                Layer::Background,
                XrayPos::default(),
                true,
                &mut |elem| elements.push(elem.into()),
            );
            // Avoid unused capacity remaining forever.
            elements.shrink_to_fit();
        }
    }

    /// Damage-gated panel content prepass: for every output currently eligible for the carousel
    /// ring (`Layout::carousel_outputs`, which includes `host` itself), refreshes that output's
    /// own `PanelSource` if it hasn't already been refreshed this `frame` stamp.
    ///
    /// This MUST be called before `render_inner` is entered for `host` (see `IN_RENDER_INNER`):
    /// each content output's `layer_map_for_output` lock is taken here, outside of any host
    /// render scope, which is what avoids the recursive-lock deadlock the lens block hit when a
    /// single monitor was both host and (self-)target (see project memory
    /// `carousel-lens-layer-map-deadlock`).
    ///
    /// Content recipe (windows + Background/Bottom layer-shell + workspace backgrounds) is
    /// rendered at `fill_zoom`, computed against `host`'s own view — the panel stack's geometry
    /// in `render_inner` bakes every ring member's content at ITS host's `fill_zoom`. Elements are
    /// rendered at the content output's own origin, not shifted onto the host's view.
    ///
    /// Single-host gating: callers must only invoke this for the CURRENT active output (see
    /// `Niri::render`'s call site). Every source's pixels are baked against `host`'s `fill_zoom`;
    /// calling this for more than one host in the same cycle would have two hosts race to publish
    /// the same sibling sources at two different zooms, and whichever host rendered last would
    /// silently win for every host's panel stack (the multi-host zoom-contamination finding from
    /// the Task 5 review). There is deliberately no re-entrancy guard for this beyond the
    /// call-site gate: the fix belongs at the one call site, not scattered defensive checks here.
    fn update_panel_sources(&self, ctx: &mut RenderCtx<GlesRenderer>, host: &Output, frame: u64) {
        let _span = tracy_client::span!("Niri::update_panel_sources");
        debug_assert!(
            !IN_RENDER_INNER.with(Cell::get),
            "update_panel_sources must run before render_inner is entered for the host output"
        );

        let Some(host_mon) = self.layout.monitor_for_output(host) else {
            return;
        };
        let host_view = host_mon.view_size();
        let host_scale = host.current_scale().fractional_scale();
        let output_scale = Scale::from(host_scale);
        let host_state = self.output_state.get(host).unwrap();

        for m in self.layout.carousel_outputs() {
            let target = m.output().clone();
            let state = self.output_state.get(&target).unwrap();

            // Idempotent within one redraw cycle: the prepass may be reached more than once
            // (e.g. a screencopy re-render of the same or a different host).
            if state.panel_source.borrow().frame == frame {
                continue;
            }

            let target_view = m.view_size();
            let target_scale = m.scale().fractional_scale();
            // Physical space for the background/layer-shell crop below: the SAME convention
            // `render_overview_at_zoom` uses for `geo_phys` (windows/`PanelMonitor`) — the
            // target's own scale, not the host's. `geo` (from `workspaces_with_render_geo_at_zoom`
            // just below) is the target's unshifted logical render-geo; baking its physical crop
            // rect and relocate offset with `output_scale` (host) instead would only agree with
            // the windows path's baked physical space when host_scale == target_scale, producing
            // mixed-DPI misalignment between panel content types otherwise.
            let target_output_scale = Scale::from(target_scale);
            // Fit the target's overview into the host view (letterbox: min of the axis ratios),
            // corrected for the host/target output-scale difference. Same formula as the lens
            // block's `fill_zoom`.
            let fit = (host_view.w / target_view.w).min(host_view.h / target_view.h);
            let fill_zoom = fit * (host_scale / target_scale);

            // Same union `Layout::carousel_focus_jump` computes to map its uv: the workspace-geo
            // boxes at `fill_zoom`, reduced to their bounding box. With the per-workspace crop in
            // `render_overview_at_zoom` above (see its doc), this bbox equals the baked texture's
            // encompassing extent exactly — including on mixed-DPI host/target pairs, PROVIDED the
            // background/layer-shell crop below bakes its physical crop bounds with `target_scale`
            // (via `target_output_scale`), matching the windows path's convention. See
            // `render_overview_at_zoom`'s doc for why a scale mismatch between the two content
            // types would inflate this beyond the workspace-geo union.
            let bbox = m
                .workspaces_with_render_geo_at_zoom(fill_zoom)
                .map(|(_ws, geo)| geo)
                .reduce(|a, b| a.merge(b));

            // Extent-change reset. `OffscreenBuffer` only recreates its texture on GROWTH (see
            // its doc on `render`'s size check), so a SHRINK of this union (fewer/narrower
            // workspaces, or a window layout change, at an unchanged `fill_zoom`) would otherwise
            // leave stale content in a sub-rect of an oversized allocation while the panel
            // samples the full texture — a persistent, steady-state, window-content-driven skew,
            // not a transient sub-pixel artifact. Compare with a small epsilon so float jitter
            // across frames at an unchanged layout doesn't force a needless recreation (full
            // damage, one dropped frame of content) on every redraw.
            const EXTENT_EPSILON: f64 = 1e-3;
            let prev_extent = state.panel_source.borrow().last_extent;
            let extent_changed = match (prev_extent, bbox) {
                (Some(prev), Some(cur)) => {
                    (prev.loc.x - cur.loc.x).abs() > EXTENT_EPSILON
                        || (prev.loc.y - cur.loc.y).abs() > EXTENT_EPSILON
                        || (prev.size.w - cur.size.w).abs() > EXTENT_EPSILON
                        || (prev.size.h - cur.size.h).abs() > EXTENT_EPSILON
                }
                (None, None) => false,
                _ => true,
            };
            if extent_changed {
                let mut source = state.panel_source.borrow_mut();
                source.clear();
                source.last_extent = bbox;
                drop(source);
                for buf in &state.panel_offscreen {
                    buf.clear();
                }
            }

            let mut elements: Vec<OutputRenderElements<GlesRenderer>> = Vec::new();
            m.render_overview_at_zoom(ctx.r(), fill_zoom, false, &mut |elem| {
                elements.push(OutputRenderElements::PanelMonitor(elem));
            });

            // Background layer-shell surfaces + per-workspace solid background, drawn AFTER the
            // windows above so they land behind them (niri renders earlier-pushed elements on
            // top). No host-centering relocate: geo is the target's own, unshifted.
            let target_layer_map = layer_map_for_output(&target);
            for (ws, geo) in m.workspaces_with_render_geo_at_zoom(fill_zoom) {
                self.render_layer_normal(
                    ctx.r(),
                    None,
                    &target_layer_map,
                    Layer::Bottom,
                    XrayPos::default(),
                    false,
                    &mut |elem| {
                        if let Some(w) =
                            scale_relocate_crop(elem, target_output_scale, fill_zoom, geo)
                        {
                            elements.push(OutputRenderElements::RelocatedLayerSurface(w));
                        }
                    },
                );
                self.render_layer_normal(
                    ctx.r(),
                    None,
                    &target_layer_map,
                    Layer::Background,
                    XrayPos::default(),
                    false,
                    &mut |elem| {
                        if let Some(w) =
                            scale_relocate_crop(elem, target_output_scale, fill_zoom, geo)
                        {
                            elements.push(OutputRenderElements::RelocatedLayerSurface(w));
                        }
                    },
                );
                if let Some(w) = scale_relocate_crop(
                    ws.render_background(),
                    target_output_scale,
                    fill_zoom,
                    geo,
                ) {
                    elements.push(OutputRenderElements::RelocatedColor(w));
                }
            }
            drop(target_layer_map);

            // Scoped borrow: only read which ping-pong slot to render into. The GPU render below
            // must not run while a `panel_source` borrow is held — that is why the offscreen
            // buffers live outside the `RefCell`, directly on `OutputState`.
            let idx = usize::from(state.panel_source.borrow().flip);

            // Same GLES context as the eventual panel draw (single-GPU primary render path), and
            // command ordering within one context is guaranteed, so the `SyncPoint` this returns
            // is deliberately not threaded through. Revisit only if panels ever need to render
            // cross-context (e.g. multi-GPU screencast of panel content).
            let result = state.panel_offscreen[idx].render(ctx.renderer, output_scale, &elements);

            let mut source = state.panel_source.borrow_mut();
            source.frame = frame;
            match result {
                Ok((elem, _sync, data)) => {
                    // Publish-on-damage: replace the published elem (and toggle the scratch slot,
                    // and bump the content generation) only when this render actually changed the
                    // texture contents. `OffscreenData::damaged` reports exactly that — it is the
                    // inner `OutputDamageTracker`'s damage result, and buffer (re)creation always
                    // reports full damage, so recreation always publishes too. On a no-damage
                    // render the fresh `elem` (holding a clone of the scratch buffer's texture)
                    // is dropped right here, keeping the scratch buffer unique for the next call,
                    // while the previously published elem keeps a stable texture identity — which
                    // is what makes the host-side `PanelRenderElement` `(source id, generation)`
                    // compare sound. See `PanelSource`'s doc for the full ownership trace.
                    if data.damaged {
                        source.elem = Some(elem);
                        source.flip = !source.flip;
                        source.generation = source.generation.wrapping_add(1);
                        // Propagate at content cadence: some source (possibly a sibling's, not
                        // `host`'s own) just published fresh pixels, so queue another redraw of
                        // `host` to pick up the new generation promptly instead of waiting for
                        // `host`'s next unrelated redraw. Deferred via a flag — see the doc on
                        // `OutputState::panel_redraw_pending` for why this can't call
                        // `queue_redraw` directly from here.
                        host_state.panel_redraw_pending.set(true);
                    }
                }
                Err(err) => {
                    // Clear stale content rather than keep showing a frame that no longer
                    // reflects this output.
                    source.elem = None;
                    warn!("error rendering panel source for output: {err:?}");
                }
            }
        }
    }

    /// Clears `PanelSource` and drops the offscreen buffers for every output NOT in `keep` — the
    /// current carousel ring (or, when the carousel isn't active at all, an empty slice). Only
    /// the active host's own `Niri::render` call runs this (see the single-host gating on
    /// `update_panel_sources`), so this is the single authoritative place ring membership is
    /// reconciled; an output that stops participating gets its content dropped the next time the
    /// active host redraws.
    fn clear_stale_panel_sources(&self, keep: &[Output]) {
        for (output, state) in &self.output_state {
            if keep.contains(output) {
                continue;
            }

            // Early-out for the common (carousel-unconfigured) case: nothing has ever published
            // to this output's panel source, so there's no `elem`, no offscreen texture, and no
            // retained host-side panel elements to sweep. Skip the borrows entirely so an idle
            // compositor with the carousel never touched pays zero cost here.
            let already_clear = state.panel_source.borrow().elem.is_none()
                && state.panel_offscreen.iter().all(|buf| buf.is_empty())
                && state.panel_elems.borrow().is_empty()
                && state.panel_hits.borrow().is_empty();
            if already_clear {
                continue;
            }

            let mut source = state.panel_source.borrow_mut();
            if source.elem.is_some() {
                source.clear();
            }
            drop(source);
            for buf in &state.panel_offscreen {
                buf.clear();
            }

            // This output is no longer a carousel ring member (or the carousel closed
            // entirely, `keep` == `[]`): release any panel elements it retained while acting as
            // a HOST, so their `GlesTexture` clones don't stay pinned after close. See
            // `OutputState::panel_elems`'s doc.
            state.panel_elems.borrow_mut().clear();
            // Stale hit geometry would otherwise let a click land on where a panel used to be
            // after this output stops hosting the carousel ring — see `OutputState::panel_hits`.
            state.panel_hits.borrow_mut().clear();
        }
    }

    pub fn clear_xray_elements(&self, output: &Output) {
        let state = self.output_state.get(output).unwrap();
        let xray = &state.xray;

        // Clear the xray elements for all render targets after all rendering that could use them
        // did so.
        for buf in &xray.background {
            buf.borrow_mut().elements().clear();
        }
        for buf in &xray.backdrop {
            buf.borrow_mut().elements().clear();
        }
    }

    /// Checks if any background layer surface has `block_out_from` set.
    pub fn has_blocked_out_background_layers(&self, output: &Output) -> bool {
        let layer_map = layer_map_for_output(output);
        for for_backdrop in [false, true] {
            for (mapped, _geo) in
                self.layers_in_render_order(&layer_map, Layer::Background, for_backdrop)
            {
                if mapped.rules().block_out_from.is_some() {
                    return true;
                }
            }
        }
        false
    }

    fn layers_in_render_order<'a>(
        &'a self,
        layer_map: &'a LayerMap,
        layer: Layer,
        for_backdrop: bool,
    ) -> impl Iterator<Item = (&'a MappedLayer, Rectangle<i32, Logical>)> {
        // LayerMap returns layers in reverse stacking order.
        layer_map.layers_on(layer).rev().filter_map(move |surface| {
            let mapped = self.mapped_layer_surfaces.get(surface)?;

            if for_backdrop != mapped.place_within_backdrop() {
                return None;
            }

            let geo = layer_map.layer_geometry(surface)?;
            Some((mapped, geo))
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn render_layer_normal<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        ns: Option<usize>,
        layer_map: &LayerMap,
        layer: Layer,
        xray_pos: XrayPos,
        for_backdrop: bool,
        push: &mut dyn FnMut(LayerSurfaceRenderElement<R>),
    ) {
        for (mapped, geo) in self.layers_in_render_order(layer_map, layer, for_backdrop) {
            let loc = geo.loc.to_f64();
            let xray_pos = xray_pos.offset(loc);
            mapped.render_normal(ctx.r(), ns, loc, xray_pos, push);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_layer_popups<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        ns: Option<usize>,
        layer_map: &LayerMap,
        layer: Layer,
        xray_pos: XrayPos,
        for_backdrop: bool,
        push: &mut dyn FnMut(LayerSurfaceRenderElement<R>),
    ) {
        for (mapped, geo) in self.layers_in_render_order(layer_map, layer, for_backdrop) {
            let loc = geo.loc.to_f64();
            let xray_pos = xray_pos.offset(loc);
            mapped.render_popups(ctx.r(), ns, loc, xray_pos, push);
        }
    }

    fn redraw(&mut self, backend: &mut Backend, output: &Output) {
        let _span = tracy_client::span!("Niri::redraw");

        // Verify our invariant.
        let state = self.output_state.get_mut(output).unwrap();
        assert!(matches!(
            state.redraw_state,
            RedrawState::Queued | RedrawState::WaitingForEstimatedVBlankAndQueued(_)
        ));

        let target_presentation_time = state.frame_clock.next_presentation_time();

        // Freeze the clock at the target time.
        self.clock.set_unadjusted(target_presentation_time);

        self.update_render_elements(Some(output));

        // Isolated outputs keep rendering while the rest are powered off with skip-isolated.
        let output_active = self.monitors_active
            || (self.power_off_skip_isolated && self.is_output_isolated(output));

        let mut res = RenderResult::Skipped;
        if output_active {
            // Decide whether a time/feedback-driven global shader needs continuous redraws.
            // Computed before the `&mut state` borrow below, since this reads `&self`.
            let global_shader_animate = {
                let cfg = self.config.borrow();
                let enabled = cfg.global_shader.enable;
                let mode = niri_config::RedrawMode::parse(&cfg.global_shader.redraw);
                drop(cfg);
                enabled && mode.wants_continuous_redraw(self.global_shader_caps())
            };

            // Decide whether any region shader on this output needs continuous redraws.
            let out_name = output.name();
            let region_shader_animate = {
                let cfg = self.config.borrow();
                cfg.region_shaders.iter().any(|r| {
                    if r.output.as_deref().map_or(false, |want| out_name != want) {
                        return false;
                    }
                    let chain = r.pass_sources(read_scoped_shader_path);
                    niri_config::GlobalShaderCaps::scan_chain(&chain).is_animating()
                })
            };

            // Decide whether a window on this output runs a time-driven per-window shader that
            // must keep redrawing. Scans resolved shader sources like the region shaders above;
            // static shaders (no niri_time usage) impose no continuous-redraw cost.
            //
            // Only VISIBLE animating windows should force redraws — a shader on a background
            // workspace or a column scrolled off the viewport is invisible, so animating it just
            // pegs the GPU for nothing. In the overview, though, several workspaces are shown
            // zoomed out (and the per-tile visible flag reflects normal-view culling), so there we
            // fall back to scanning all windows to keep their shaders animating as before.
            let shader_animates = |win: &Mapped| {
                win.rules().shader.as_ref().map_or(false, |shader| {
                    niri_config::GlobalShaderCaps::scan_chain(&shader.passes).is_animating()
                })
            };
            let window_shader_animate = if self.layout.is_overview_open() {
                self.layout.windows_for_output(output).any(shader_animates)
            } else {
                self.layout
                    .visible_windows_for_output(output)
                    .any(shader_animates)
            };

            let state = self.output_state.get_mut(output).unwrap();
            state.unfinished_animations_remain = self.layout.are_animations_ongoing(Some(output));
            state.unfinished_animations_remain |=
                self.config_error_notification.are_animations_ongoing();
            state.unfinished_animations_remain |= self.exit_confirm_dialog.are_animations_ongoing();
            state.unfinished_animations_remain |= self.screenshot_ui.are_animations_ongoing();
            state.unfinished_animations_remain |= self.window_mru_ui.are_animations_ongoing();
            state.unfinished_animations_remain |= state.screen_transition.is_some();

            // Also keep redrawing if the current cursor is animated.
            state.unfinished_animations_remain |= self
                .cursor_manager
                .is_current_cursor_animated(output.current_scale().integer_scale());

            // Also check layer surfaces.
            if !state.unfinished_animations_remain {
                state.unfinished_animations_remain |= layer_map_for_output(output)
                    .layers()
                    .filter_map(|surface| self.mapped_layer_surfaces.get(surface))
                    .any(|mapped| mapped.are_animations_ongoing());
            }

            // Time/feedback-driven global, region and per-window shaders must keep redrawing to
            // animate when the desktop is otherwise idle. When `shader-animation-max-fps` is 0
            // (default) this is unconditional — a redraw every frame, as before. When a cap is set,
            // throttle the *shader-only* case to that rate: render one shader frame, then drive the
            // next via a one-shot timer instead of the vblank loop, so an idle animated shader
            // recomposites `cap_fps` times/sec instead of at the panel's native refresh. Shaders
            // riding along with a non-shader animation (drag, transition, overview) are never
            // throttled — those must stay smooth.
            let shader_animate =
                global_shader_animate || region_shader_animate || window_shader_animate;
            let cap_fps = self.config.borrow().shader_animation_max_fps;
            let now = std::time::Instant::now();

            let throttle = if cap_fps == 0 {
                state.unfinished_animations_remain |= shader_animate;
                ThrottleAction::Cancel
            } else if !shader_animate {
                ThrottleAction::Cancel
            } else if state.unfinished_animations_remain {
                // A non-shader animation already drives full-rate redraws; shaders ride along.
                // Keep the clock current so the throttle doesn't immediately fire afterwards.
                state.last_shader_frame = Some(now);
                ThrottleAction::Cancel
            } else {
                // Shaders are the sole reason to redraw → cap the rate.
                let interval = Duration::from_secs_f64(1.0 / f64::from(cap_fps));
                match decide_shader_frame(state.last_shader_frame, now, interval) {
                    ShaderFrameDecision::Due => {
                        // Render this as a shader frame; drive the next via the timer, not the
                        // vblank loop, so we get exactly cap_fps recomposites/sec.
                        state.last_shader_frame = Some(now);
                        ThrottleAction::Arm(interval)
                    }
                    ShaderFrameDecision::Wait(remaining) => {
                        // Other damage triggered this redraw between shader frames. Ensure a wake
                        // is pending for the next shader deadline without
                        // churning the timer.
                        if state.shader_throttle_timer.is_some() {
                            ThrottleAction::Keep
                        } else {
                            ThrottleAction::Arm(remaining)
                        }
                    }
                }
            };
            // `state`'s borrow ends here; the timer ops re-borrow `&mut self`.
            match throttle {
                ThrottleAction::Keep => {}
                ThrottleAction::Cancel => self.cancel_shader_throttle_timer(output),
                ThrottleAction::Arm(delay) => self.arm_shader_throttle_timer(output, delay),
            }

            // Render.
            res = backend.render(self, output, target_presentation_time);
        }

        // Consume `panel_redraw_pending` here, now that `&mut self` is available again: it was
        // set (if at all) via interior mutability from inside the just-finished render, by
        // `update_panel_sources` mid-`Niri::render`, which cannot call `queue_redraw` itself. See
        // `OutputState::panel_redraw_pending`'s doc.
        //
        // Deliberately only consumed when the render actually happened (`res != Skipped`): the
        // `Skipped` branch below unconditionally resets `redraw_state`, which would clobber the
        // `Queued`/`...AndQueued` state `queue_redraw` just set here, silently dropping the
        // deferred redraw. Leaving the flag SET on `Skipped` means it's picked up by this same
        // check the next time this output actually renders.
        if res != RenderResult::Skipped
            && self.output_state.get(output).unwrap().panel_redraw_pending.take()
        {
            self.queue_redraw(output);
        }

        let is_locked = self.is_locked();
        let state = self.output_state.get_mut(output).unwrap();

        if res == RenderResult::Skipped {
            // Update the redraw state on failed render.
            state.redraw_state = if let RedrawState::WaitingForEstimatedVBlank(token)
            | RedrawState::WaitingForEstimatedVBlankAndQueued(token) =
                state.redraw_state
            {
                RedrawState::WaitingForEstimatedVBlank(token)
            } else {
                RedrawState::Idle
            };
        }

        // Update the lock render state on successful render, or if monitors are inactive. When
        // monitors are inactive on a TTY, they have no framebuffer attached, so no sensitive data
        // from a last render will be visible.
        if res != RenderResult::Skipped || !self.monitors_active {
            state.lock_render_state = if is_locked {
                LockRenderState::Locked
            } else {
                LockRenderState::Unlocked
            };
        }

        // If we're in process of locking the session, check if the requirements were met.
        match mem::take(&mut self.lock_state) {
            LockState::Locking(confirmation) => {
                if state.lock_render_state == LockRenderState::Unlocked {
                    // We needed to render a locked frame on this output but failed.
                    self.unlock();
                } else {
                    // Check if all outputs are now locked.
                    let all_locked = self
                        .output_state
                        .values()
                        .all(|state| state.lock_render_state == LockRenderState::Locked);

                    if all_locked {
                        // All outputs are locked, report success.
                        let lock = confirmation.ext_session_lock().clone();
                        confirmation.lock();
                        self.lock_state = LockState::Locked(lock);
                    } else {
                        // Still waiting for other outputs.
                        self.lock_state = LockState::Locking(confirmation);
                    }
                }
            }
            lock_state => self.lock_state = lock_state,
        }

        self.refresh_on_demand_vrr(backend, output);

        // Send the frame callbacks.
        //
        // FIXME: The logic here could be a bit smarter. Currently, during an animation, the
        // surfaces that are visible for the very last frame (e.g. because the camera is moving
        // away) will receive frame callbacks, and the surfaces that are invisible but will become
        // visible next frame will not receive frame callbacks (so they will show stale contents for
        // one frame). We could advance the animations for the next frame and send frame callbacks
        // according to the expected new positions.
        //
        // However, this should probably be restricted to sending frame callbacks to more surfaces,
        // to err on the safe side.
        self.send_frame_callbacks(output);
        backend.with_primary_renderer(|renderer| {
            #[cfg(feature = "xdp-gnome-screencast")]
            {
                // Render and send to PipeWire screencast streams.
                self.render_for_screen_cast(renderer, output, target_presentation_time);

                // FIXME: when a window is hidden, it should probably still receive frame callbacks
                // and get rendered for screen cast. This is currently
                // unimplemented, but happens to work by chance, since output
                // redrawing is more eager than it should be.
                self.render_windows_for_screen_cast(renderer, output, target_presentation_time);
            }

            self.render_for_screencopy_with_damage(renderer, output);
        });
    }

    pub fn refresh_on_demand_vrr(&mut self, backend: &mut Backend, output: &Output) {
        let _span = tracy_client::span!("Niri::refresh_on_demand_vrr");

        let name = output.user_data().get::<OutputName>().unwrap();
        let on_demand = self
            .config
            .borrow()
            .outputs
            .find(name)
            .is_some_and(|output| output.is_vrr_on_demand());
        if !on_demand {
            return;
        }

        let current = self.layout.windows_for_output(output).any(|mapped| {
            mapped.rules().variable_refresh_rate == Some(true) && {
                let mut visible = false;
                mapped.window.with_surfaces(|surface, states| {
                    if !visible
                        && surface_primary_scanout_output(surface, states).as_ref() == Some(output)
                    {
                        visible = true;
                    }
                });
                visible
            }
        });

        backend.set_output_on_demand_vrr(self, output, current);
    }

    pub fn update_primary_scanout_output(
        &self,
        output: &Output,
        render_element_states: &RenderElementStates,
    ) {
        // FIXME: potentially tweak the compare function. The default one currently always prefers a
        // higher refresh-rate output, which is not always desirable (i.e. with a very small
        // overlap).
        //
        // While we only have cursors and DnD icons crossing output boundaries though, it doesn't
        // matter all that much.
        if let CursorImageStatus::Surface(surface) = &self.cursor_manager.cursor_image() {
            with_surface_tree_downward(
                surface,
                (),
                |_, _, _| TraversalAction::DoChildren(()),
                |surface, states, _| {
                    update_surface_primary_scanout_output(
                        surface,
                        output,
                        states,
                        None,
                        render_element_states,
                        default_primary_scanout_output_compare,
                    );
                },
                |_, _, _| true,
            );
        }

        if let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            with_surface_tree_downward(
                surface,
                (),
                |_, _, _| TraversalAction::DoChildren(()),
                |surface, states, _| {
                    update_surface_primary_scanout_output(
                        surface,
                        output,
                        states,
                        None,
                        render_element_states,
                        default_primary_scanout_output_compare,
                    );
                },
                |_, _, _| true,
            );
        }

        // We're only updating the current output's windows and layer surfaces. This should be fine
        // as in niri they can only be rendered on a single output at a time.
        //
        // The reason to do this at all is that it keeps track of whether the surface is visible or
        // not in a unified way with the pointer surfaces, which makes the logic elsewhere simpler.

        for mapped in self.layout.windows_for_output(output) {
            let win = &mapped.window;
            let offscreen_data = mapped.offscreen_data();
            let offscreen_data = offscreen_data.as_ref();

            win.with_surfaces(|surface, states| {
                let primary_scanout_output = states
                    .data_map
                    .get_or_insert_threadsafe(Mutex::<PrimaryScanoutOutput>::default);
                let mut primary_scanout_output = primary_scanout_output.lock().unwrap();

                let mut id = Id::from_wayland_resource(surface);

                if let Some(data) = offscreen_data {
                    // We have offscreen data; it's likely that all surfaces are on it.
                    if data.states.element_was_presented(id.clone()) {
                        // If the surface was presented to the offscreen, use the offscreen's id.
                        id = data.id.clone();
                    }

                    // If we the surface wasn't presented to the offscreen it can mean:
                    //
                    // - The surface was invisible. For example, it's obscured by another surface on
                    //   the offscreen, or simply isn't mapped.
                    // - The surface is rendered separately from the offscreen, for example: popups
                    //   during the window resize animation.
                    //
                    // In both of these cases, using the original surface element id and the
                    // original states is the correct thing to do. We may find the surface in the
                    // original states (in the second case). Either way, we definitely know it is
                    // *not* in the offscreen, and we won't miss it.
                    //
                    // There's one edge case: if the surface is both in the offscreen and separate,
                    // and the offscreen itself is invisible, while the separate surface is
                    // visible. In this case we'll currently mark the surface as invisible. We
                    // don't really use offscreens like that however, and if we start, it's easy
                    // enough to fix (need an extra check).
                }

                primary_scanout_output.update_from_render_element_states(
                    id,
                    output,
                    None,
                    render_element_states,
                    |_, _, output, _| output,
                );
            });
        }

        let xray = &self.output_state[output].xray;
        let xray_bg = xray.background[RenderTarget::Output as usize].borrow();
        let xray_bd = xray.backdrop[RenderTarget::Output as usize].borrow();

        for layer in layer_map_for_output(output).layers() {
            let surface = layer.wl_surface();
            let is_background = layer.layer() == Layer::Background;

            with_surfaces_surface_tree(surface, |surface, states| {
                let primary_scanout_output = states
                    .data_map
                    .get_or_insert_threadsafe(Mutex::<PrimaryScanoutOutput>::default);
                let mut primary_scanout_output = primary_scanout_output.lock().unwrap();
                let mut id = Id::from_wayland_resource(surface);

                // Background layers may be invisible normally but visible through an xray
                // background effect. Try to find it and use the xray element's id in this case.
                //
                // FIXME: this won't work if there's another layer of offscreen (e.g. window with
                // an xray background during its opening animation). But hopefully with the
                // refactor to draw background effects outside offscreens it won't be a problem.
                if is_background && !render_element_states.element_was_presented(id.clone()) {
                    // A layer may be present either in background or backdrop, never in both.
                    if xray_bg
                        .render_element_states()
                        .is_some_and(|s| s.element_was_presented(id.clone()))
                    {
                        id = xray_bg.id().clone();
                    } else if xray_bd
                        .render_element_states()
                        .is_some_and(|s| s.element_was_presented(id.clone()))
                    {
                        id = xray_bd.id().clone();
                    }
                }

                primary_scanout_output.update_from_render_element_states(
                    id,
                    output,
                    None,
                    render_element_states,
                    // Layer surfaces are shown only on one output at a time.
                    |_, _, output, _| output,
                );
            });

            // Popups never go into xray buffers.
            for (popup, _) in PopupManager::popups_for_surface(surface) {
                let surface = popup.wl_surface();
                with_surfaces_surface_tree(surface, |surface, states| {
                    update_surface_primary_scanout_output(
                        surface,
                        output,
                        states,
                        None,
                        render_element_states,
                        // Layer surfaces are shown only on one output at a time.
                        |_, _, output, _| output,
                    );
                });
            }
        }

        if let Some(surface) = &self.output_state[output].lock_surface {
            with_surface_tree_downward(
                surface.wl_surface(),
                (),
                |_, _, _| TraversalAction::DoChildren(()),
                |surface, states, _| {
                    update_surface_primary_scanout_output(
                        surface,
                        output,
                        states,
                        None,
                        render_element_states,
                        default_primary_scanout_output_compare,
                    );
                },
                |_, _, _| true,
            );
        }
    }

    pub fn send_dmabuf_feedbacks(
        &self,
        output: &Output,
        feedback: &SurfaceDmabufFeedback,
        render_element_states: &RenderElementStates,
    ) {
        let _span = tracy_client::span!("Niri::send_dmabuf_feedbacks");

        // We can unconditionally send the current output's feedback to regular and layer-shell
        // surfaces, as they can only be displayed on a single output at a time. Even if a surface
        // is currently invisible, this is the DMABUF feedback that it should know about.
        for mapped in self.layout.windows_for_output(output) {
            mapped.window.send_dmabuf_feedback(
                output,
                |_, _| Some(output.clone()),
                |surface, _| {
                    select_dmabuf_feedback(
                        surface,
                        render_element_states,
                        &feedback.render,
                        &feedback.scanout,
                    )
                },
            );
        }

        for surface in layer_map_for_output(output).layers() {
            surface.send_dmabuf_feedback(
                output,
                |_, _| Some(output.clone()),
                |surface, _| {
                    select_dmabuf_feedback(
                        surface,
                        render_element_states,
                        &feedback.render,
                        &feedback.scanout,
                    )
                },
            );
        }

        if let Some(surface) = &self.output_state[output].lock_surface {
            send_dmabuf_feedback_surface_tree(
                surface.wl_surface(),
                output,
                |_, _| Some(output.clone()),
                |surface, _| {
                    select_dmabuf_feedback(
                        surface,
                        render_element_states,
                        &feedback.render,
                        &feedback.scanout,
                    )
                },
            );
        }

        if let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            send_dmabuf_feedback_surface_tree(
                surface,
                output,
                surface_primary_scanout_output,
                |surface, _| {
                    select_dmabuf_feedback(
                        surface,
                        render_element_states,
                        &feedback.render,
                        &feedback.scanout,
                    )
                },
            );
        }

        if let CursorImageStatus::Surface(surface) = &self.cursor_manager.cursor_image() {
            send_dmabuf_feedback_surface_tree(
                surface,
                output,
                surface_primary_scanout_output,
                |surface, _| {
                    select_dmabuf_feedback(
                        surface,
                        render_element_states,
                        &feedback.render,
                        &feedback.scanout,
                    )
                },
            );
        }
    }

    pub fn send_frame_callbacks(&mut self, output: &Output) {
        let _span = tracy_client::span!("Niri::send_frame_callbacks");

        let state = self.output_state.get(output).unwrap();
        let sequence = state.frame_callback_sequence;

        let should_send = |surface: &WlSurface, states: &SurfaceData| {
            // Do the standard primary scanout output check. For pointer surfaces it deduplicates
            // the frame callbacks across potentially multiple outputs, and for regular windows and
            // layer-shell surfaces it avoids sending frame callbacks to invisible surfaces.
            let current_primary_output = surface_primary_scanout_output(surface, states);
            if current_primary_output.as_ref() != Some(output) {
                return None;
            }

            // Next, check the throttling status.
            let frame_throttling_state = states
                .data_map
                .get_or_insert(SurfaceFrameThrottlingState::default);
            let mut last_sent_at = frame_throttling_state.last_sent_at.borrow_mut();

            let mut send = true;

            // If we already sent a frame callback to this surface this output refresh
            // cycle, don't send one again to prevent empty-damage commit busy loops.
            if let Some((last_output, last_sequence)) = &*last_sent_at {
                if last_output == output && *last_sequence == sequence {
                    send = false;
                }
            }

            if send {
                *last_sent_at = Some((output.clone(), sequence));
                Some(output.clone())
            } else {
                None
            }
        };

        let frame_callback_time = get_monotonic_time();

        for mapped in self.layout.windows_for_output_mut(output) {
            mapped.send_frame(
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                should_send,
            );
        }

        for surface in layer_map_for_output(output).layers() {
            surface.send_frame(
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                should_send,
            );
        }

        if let Some(surface) = &self.output_state[output].lock_surface {
            send_frames_surface_tree(
                surface.wl_surface(),
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                should_send,
            );
        }

        if let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            send_frames_surface_tree(
                surface,
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                should_send,
            );
        }

        if let CursorImageStatus::Surface(surface) = self.cursor_manager.cursor_image() {
            send_frames_surface_tree(
                surface,
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                should_send,
            );
        }
    }

    pub fn send_frame_callbacks_on_fallback_timer(&mut self) {
        let _span = tracy_client::span!("Niri::send_frame_callbacks_on_fallback_timer");

        // Make up a bogus output; we don't care about it here anyway, just the throttling timer.
        let output = Output::new(
            String::new(),
            PhysicalProperties {
                size: Size::from((0, 0)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        let output = &output;

        let frame_callback_time = get_monotonic_time();

        self.layout.with_windows_mut(|mapped, _| {
            mapped.send_frame(
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                |_, _| None,
            );
        });

        for (output, state) in self.output_state.iter() {
            for surface in layer_map_for_output(output).layers() {
                surface.send_frame(
                    output,
                    frame_callback_time,
                    FRAME_CALLBACK_THROTTLE,
                    |_, _| None,
                );
            }

            if let Some(surface) = &state.lock_surface {
                send_frames_surface_tree(
                    surface.wl_surface(),
                    output,
                    frame_callback_time,
                    FRAME_CALLBACK_THROTTLE,
                    |_, _| None,
                );
            }
        }

        if let Some(surface) = &self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            send_frames_surface_tree(
                surface,
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                |_, _| None,
            );
        }

        if let CursorImageStatus::Surface(surface) = self.cursor_manager.cursor_image() {
            send_frames_surface_tree(
                surface,
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                |_, _| None,
            );
        }
    }

    pub fn take_presentation_feedbacks(
        &mut self,
        output: &Output,
        render_element_states: &RenderElementStates,
    ) -> OutputPresentationFeedback {
        let mut feedback = OutputPresentationFeedback::new(output);

        if let CursorImageStatus::Surface(surface) = &self.cursor_manager.cursor_image() {
            take_presentation_feedback_surface_tree(
                surface,
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(
                        surface,
                        None,
                        render_element_states,
                    )
                },
            );
        }

        if let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            take_presentation_feedback_surface_tree(
                surface,
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(
                        surface,
                        None,
                        render_element_states,
                    )
                },
            );
        }

        for mapped in self.layout.windows_for_output(output) {
            mapped.window.take_presentation_feedback(
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(
                        surface,
                        None,
                        render_element_states,
                    )
                },
            )
        }

        for surface in layer_map_for_output(output).layers() {
            surface.take_presentation_feedback(
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(
                        surface,
                        None,
                        render_element_states,
                    )
                },
            );
        }

        if let Some(surface) = &self.output_state[output].lock_surface {
            take_presentation_feedback_surface_tree(
                surface.wl_surface(),
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(
                        surface,
                        None,
                        render_element_states,
                    )
                },
            );
        }

        feedback
    }

    pub fn render_for_screencopy_with_damage(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
    ) {
        let _span = tracy_client::span!("Niri::render_for_screencopy_with_damage");

        let mut screencopy_state = mem::take(&mut self.screencopy_state);

        screencopy_state.with_queues_mut(|queue| {
            let (damage_tracker, screencopy) = queue.split();
            if let Some(screencopy) = screencopy {
                if screencopy.output() == output {
                    let ctx = RenderCtx {
                        renderer,
                        target: RenderTarget::ScreenCapture,
                        xray: None,
                        shader_time: self.window_shader_start.elapsed().as_secs_f32(),
                    };
                    let offset = screencopy.region_loc().upscale(-1);
                    let mut elements = Vec::new();
                    self.render(ctx, output, screencopy.overlay_cursor(), &mut |elem| {
                        let elem =
                            RelocateRenderElement::from_element(elem, offset, Relocate::Relative);
                        elements.push(elem);
                    });

                    let (damages, states) = Self::damage_screencopy_internal(
                        output,
                        &elements,
                        damage_tracker,
                        screencopy,
                    );
                    if let Some(damages) = damages {
                        // Convert from Physical coordinates back to Buffer coordinates.
                        let transform = output.current_transform();
                        let physical_size = transform.transform_size(screencopy.buffer_size());
                        let damages = damages.iter().map(|dmg| {
                            dmg.to_logical(1).to_buffer(
                                1,
                                transform.invert(),
                                &physical_size.to_logical(1),
                            )
                        });

                        screencopy.damage(damages);

                        let render_result = Self::render_for_screencopy_internal(
                            renderer,
                            damage_tracker,
                            &elements,
                            states,
                            screencopy,
                        );
                        match render_result {
                            Ok(sync) => {
                                queue.pop().submit_after_sync(false, sync, &self.event_loop);
                            }
                            Err(err) => {
                                // Recreate damage tracker to report full damage next check.
                                *damage_tracker =
                                    OutputDamageTracker::new((0, 0), 1.0, Transform::Normal);
                                queue.pop();
                                warn!("error rendering for screencopy: {err:?}");
                            }
                        }
                    } else {
                        trace!("no damage found, waiting till next redraw");
                    }
                };
            }
        });

        self.screencopy_state = screencopy_state;
    }

    pub fn render_for_screencopy_without_damage(
        &mut self,
        renderer: &mut GlesRenderer,
        manager: &ZwlrScreencopyManagerV1,
        screencopy: Screencopy,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("Niri::render_for_screencopy");

        let output = screencopy.output();
        ensure!(
            self.output_state.contains_key(output),
            "screencopy output missing"
        );

        self.update_render_elements(Some(output));

        let ctx = RenderCtx {
            renderer,
            target: RenderTarget::ScreenCapture,
            xray: None,
            shader_time: self.window_shader_start.elapsed().as_secs_f32(),
        };
        let offset = screencopy.region_loc().upscale(-1);
        let mut elements = Vec::new();
        self.render(ctx, output, screencopy.overlay_cursor(), &mut |elem| {
            let elem = RelocateRenderElement::from_element(elem, offset, Relocate::Relative);
            elements.push(elem);
        });

        let Some(damage_tracker) = self.screencopy_state.damage_tracker(manager) else {
            error!("screencopy queue must not be deleted as long as frames exist");
            bail!("screencopy queue missing");
        };

        let (_damages, states) =
            Self::damage_screencopy_internal(output, &elements, damage_tracker, &screencopy);
        let res = Self::render_for_screencopy_internal(
            renderer,
            damage_tracker,
            &elements,
            states,
            &screencopy,
        );
        let res = res.map(|sync| screencopy.submit_after_sync(false, sync, &self.event_loop));

        if res.is_err() {
            // Recreate damage tracker to report full damage next check.
            *damage_tracker = OutputDamageTracker::new((0, 0), 1.0, Transform::Normal);
        }

        res
    }

    fn damage_screencopy_internal<'a>(
        output: &Output,
        elements: &[impl Element],
        damage_tracker: &'a mut OutputDamageTracker,
        screencopy: &Screencopy,
    ) -> (
        Option<&'a Vec<Rectangle<i32, Physical>>>,
        RenderElementStates,
    ) {
        let OutputModeSource::Static {
            size: last_size,
            scale: last_scale,
            transform: last_transform,
        } = damage_tracker.mode().clone()
        else {
            unreachable!("damage tracker must have static mode");
        };

        let size = screencopy.buffer_size();
        let scale: Scale<f64> = output.current_scale().fractional_scale().into();
        let transform = output.current_transform();

        if size != last_size || scale != last_scale || transform != last_transform {
            *damage_tracker = OutputDamageTracker::new(size, scale, transform);
        }

        // Just checked damage tracker has static mode
        damage_tracker.damage_output(1, elements).unwrap()
    }

    #[allow(clippy::type_complexity)]
    fn render_for_screencopy_internal(
        renderer: &mut GlesRenderer,
        damage_tracker: &mut OutputDamageTracker,
        elements: &[impl RenderElement<GlesRenderer>],
        states: RenderElementStates,
        screencopy: &Screencopy,
    ) -> anyhow::Result<Option<SyncPoint>> {
        let sync = match screencopy.buffer() {
            ScreencopyBuffer::Dmabuf(dmabuf) => {
                let sync =
                    render_to_dmabuf(renderer, damage_tracker, dmabuf.clone(), elements, states)
                        .context("error rendering to screencopy dmabuf")?;
                Some(sync)
            }
            ScreencopyBuffer::Shm(wl_buffer) => {
                render_to_shm(renderer, damage_tracker, wl_buffer, elements, states)
                    .context("error rendering to screencopy shm buffer")?;
                None
            }
        };

        Ok(sync)
    }

    #[cfg(not(feature = "xdp-gnome-screencast"))]
    pub fn stop_casts_for_target(&mut self, _target: CastTarget) {}

    #[cfg(not(feature = "xdp-gnome-screencast"))]
    pub fn stop_cast(&mut self, _session_id: crate::utils::CastSessionId) {}

    pub fn debug_toggle_damage(&mut self) {
        self.debug_draw_damage = !self.debug_draw_damage;

        if self.debug_draw_damage {
            for (output, state) in &mut self.output_state {
                state.debug_damage_tracker = OutputDamageTracker::from_output(output);
            }
        }

        self.queue_redraw_all();
    }

    pub fn capture_screenshots<'a>(
        &'a self,
        renderer: &'a mut GlesRenderer,
    ) -> impl Iterator<Item = (Output, [OutputScreenshot; 3])> + 'a {
        self.global_space.outputs().cloned().filter_map(|output| {
            let size = output.current_mode().unwrap().size;
            let transform = output.current_transform();
            let size = transform.transform_size(size);

            let scale = Scale::from(output.current_scale().fractional_scale());
            let targets = [
                RenderTarget::Output,
                RenderTarget::Screencast,
                RenderTarget::ScreenCapture,
            ];
            let screenshot = targets.map(|target| {
                let ctx = RenderCtx {
                    renderer,
                    target,
                    xray: None,
                    shader_time: self.window_shader_start.elapsed().as_secs_f32(),
                };
                let elements = self.render_to_vec(ctx, &output, false);
                let elements = elements.iter().rev();

                let res = render_to_texture(
                    renderer,
                    size,
                    scale,
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    elements,
                );
                if let Err(err) = &res {
                    warn!("error rendering output {}: {err:?}", output.name());
                }
                let res_output = res.ok();

                let mut pointer = Vec::new();

                // We check the pointer visibility for Disabled (and not .is_visible()) in order to
                // show the pointer even when it's hidden through cursor {} options. The user can
                // then toggle it in the screenshot UI as needed.
                if self.pointer_visibility != PointerVisibility::Disabled {
                    self.render_pointer(renderer, &output, &mut |elem| pointer.push(elem));
                }

                let res_pointer = if pointer.is_empty() {
                    None
                } else {
                    let res = render_to_encompassing_texture(
                        renderer,
                        scale,
                        Transform::Normal,
                        Fourcc::Abgr8888,
                        &pointer,
                    );
                    if let Err(err) = &res {
                        warn!("error rendering pointer for {}: {err:?}", output.name());
                    }
                    res.ok()
                };

                res_output.map(|(texture, _)| {
                    OutputScreenshot::from_textures(
                        renderer,
                        scale,
                        texture,
                        res_pointer.map(|(texture, _, geo)| (texture, geo)),
                    )
                })
            });

            if screenshot.iter().any(|res| res.is_none()) {
                return None;
            }

            let screenshot = screenshot.map(|res| res.unwrap());
            Some((output, screenshot))
        })
    }

    pub fn screenshot(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        write_to_disk: bool,
        include_pointer: bool,
        path: Option<String>,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("Niri::screenshot");

        self.update_render_elements(Some(output));

        let size = output.current_mode().unwrap().size;
        let transform = output.current_transform();
        let size = transform.transform_size(size);

        let scale = Scale::from(output.current_scale().fractional_scale());
        let ctx = RenderCtx {
            renderer,
            target: RenderTarget::ScreenCapture,
            xray: None,
            shader_time: self.window_shader_start.elapsed().as_secs_f32(),
        };
        let elements = self.render_to_vec(ctx, output, include_pointer);
        let elements = elements.iter().rev();
        let pixels = render_to_vec(
            renderer,
            size,
            scale,
            Transform::Normal,
            Fourcc::Abgr8888,
            elements,
        )?;

        self.save_screenshot(size, pixels, write_to_disk, path)
            .context("error saving screenshot")
    }

    pub fn screenshot_window(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        mapped: &Mapped,
        write_to_disk: bool,
        show_pointer: bool,
        path: Option<String>,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("Niri::screenshot_window");

        let scale = Scale::from(output.current_scale().fractional_scale());
        let alpha =
            if mapped.sizing_mode().is_fullscreen() || mapped.is_ignoring_opacity_window_rule() {
                1.
            } else {
                mapped.rules().opacity.unwrap_or(1.).clamp(0., 1.)
            };

        let mut elements: Vec<WindowScreenshotRenderElement<GlesRenderer>> = Vec::new();

        // Add pointer if requested and it's over this window.
        if show_pointer {
            if let Some((_, win_pos)) = self.pointer_pos_for_window_cast(mapped) {
                // Pointer elements are at output-local physical coords.
                // Relocate by -win_pos to make them window-relative.
                let pos = win_pos.to_physical_precise_round(scale).upscale(-1);
                self.render_pointer(renderer, output, &mut |elem| {
                    let elem = RelocateRenderElement::from_element(elem, pos, Relocate::Relative);
                    elements.push(elem.into());
                });
            }
        }
        let pointer_count = elements.len();

        let ctx = RenderCtx {
            renderer,
            target: RenderTarget::ScreenCapture,
            xray: None,
            shader_time: self.window_shader_start.elapsed().as_secs_f32(),
        };
        mapped.render(
            ctx,
            mapped.window.geometry().loc.to_f64(),
            scale,
            alpha,
            XrayPos::default(),
            &mut |elem| elements.push(elem.into()),
        );

        // The pointer is not included in encompassing_geo because we don't want it to expand the
        // screenshot size.
        let geo = encompassing_geo(scale, elements.iter().skip(pointer_count));
        let elements = elements.iter().rev().map(|elem| {
            RelocateRenderElement::from_element(elem, geo.loc.upscale(-1), Relocate::Relative)
        });
        let pixels = render_to_vec(
            renderer,
            geo.size,
            scale,
            Transform::Normal,
            Fourcc::Abgr8888,
            elements,
        )?;

        self.save_screenshot(geo.size, pixels, write_to_disk, path)
            .context("error saving screenshot")
    }

    pub fn save_screenshot(
        &self,
        size: Size<i32, Physical>,
        pixels: Vec<u8>,
        write_to_disk: bool,
        path_arg: Option<String>,
    ) -> anyhow::Result<()> {
        let path = write_to_disk
            .then(|| {
                // When given an explicit path, don't try to strftime it or create parents.
                path_arg.map(|p| (PathBuf::from(p), false)).or_else(|| {
                    match make_screenshot_path(&self.config.borrow()) {
                        Ok(path) => path.map(|p| (p, true)),
                        Err(err) => {
                            warn!("error making screenshot path: {err:?}");
                            None
                        }
                    }
                })
            })
            .flatten();

        // Prepare to set the encoded image as our clipboard selection. This must be done from the
        // main thread.
        let (tx, rx) = calloop::channel::sync_channel::<Arc<[u8]>>(1);
        self.event_loop
            .insert_source(rx, move |event, _, state| match event {
                calloop::channel::Event::Msg(buf) => {
                    set_data_device_selection(
                        &state.niri.display_handle,
                        &state.niri.seat,
                        vec![String::from("image/png")],
                        buf.clone(),
                    );
                }
                calloop::channel::Event::Closed => (),
            })
            .unwrap();

        // Prepare to send screenshot completion event back to main thread.
        let (event_tx, event_rx) = calloop::channel::sync_channel::<Option<String>>(1);
        self.event_loop
            .insert_source(event_rx, move |event, _, state| match event {
                calloop::channel::Event::Msg(path) => {
                    state.ipc_screenshot_taken(path);
                }
                calloop::channel::Event::Closed => (),
            })
            .unwrap();

        // Encode and save the image in a thread as it's slow.
        thread::spawn(move || {
            let mut buf = vec![];

            let w = std::io::Cursor::new(&mut buf);
            if let Err(err) = write_png_rgba8(w, size.w as u32, size.h as u32, &pixels) {
                warn!("error encoding screenshot image: {err:?}");
                return;
            }

            let buf: Arc<[u8]> = Arc::from(buf.into_boxed_slice());
            let _ = tx.send(buf.clone());

            let mut image_path = None;

            if let Some((path, create_parent)) = path {
                debug!("saving screenshot to {path:?}");

                if create_parent {
                    if let Some(parent) = path.parent() {
                        // Relative paths with one component, i.e. "test.png", have Some("") parent.
                        if !parent.as_os_str().is_empty() {
                            if let Err(err) = std::fs::create_dir_all(parent) {
                                if err.kind() != std::io::ErrorKind::AlreadyExists {
                                    warn!("error creating screenshot directory: {err:?}");
                                }
                            }
                        }
                    }
                }

                match std::fs::write(&path, buf) {
                    Ok(()) => image_path = Some(path),
                    Err(err) => {
                        warn!("error saving screenshot image: {err:?}");
                    }
                }
            } else {
                debug!("not saving screenshot to disk");
            }

            #[cfg(feature = "dbus")]
            if let Err(err) = crate::utils::show_screenshot_notification(image_path.as_deref()) {
                warn!("error showing screenshot notification: {err:?}");
            }

            // Send screenshot completion event.
            let path_string = image_path
                .as_ref()
                .and_then(|p| p.to_str())
                .map(|s| s.to_owned());
            let _ = event_tx.send(path_string);
        });

        Ok(())
    }

    #[cfg(feature = "dbus")]
    pub fn screenshot_all_outputs(
        &mut self,
        renderer: &mut GlesRenderer,
        include_pointer: bool,
        on_done: impl FnOnce(PathBuf) + Send + 'static,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("Niri::screenshot_all_outputs");

        self.update_render_elements(None);

        let outputs: Vec<_> = self.global_space.outputs().cloned().collect();

        // FIXME: support multiple outputs, needs fixing multi-scale handling and cropping.
        anyhow::ensure!(outputs.len() == 1);

        let output = outputs.into_iter().next().unwrap();
        let geom = self.global_space.output_geometry(&output).unwrap();

        let output_scale = output.current_scale().integer_scale();
        let geom = geom.to_physical(output_scale);

        let size = geom.size;
        let transform = output.current_transform();
        let size = transform.transform_size(size);

        let ctx = RenderCtx {
            renderer,
            target: RenderTarget::ScreenCapture,
            xray: None,
            shader_time: self.window_shader_start.elapsed().as_secs_f32(),
        };
        let elements = self.render_to_vec(ctx, &output, include_pointer);
        let elements = elements.iter().rev();
        let pixels = render_to_vec(
            renderer,
            size,
            Scale::from(f64::from(output_scale)),
            Transform::Normal,
            Fourcc::Abgr8888,
            elements,
        )?;

        let path = make_screenshot_path(&self.config.borrow())
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                let mut path = env::temp_dir();
                path.push("screenshot.png");
                path
            });
        debug!("saving screenshot to {path:?}");

        thread::spawn(move || {
            let file = match std::fs::File::create(&path) {
                Ok(file) => file,
                Err(err) => {
                    warn!("error creating file: {err:?}");
                    return;
                }
            };

            let w = std::io::BufWriter::new(file);
            if let Err(err) = write_png_rgba8(w, size.w as u32, size.h as u32, &pixels) {
                warn!("error encoding screenshot image: {err:?}");
                return;
            }

            on_done(path);
        });

        Ok(())
    }

    pub fn is_locked(&self) -> bool {
        match self.lock_state {
            LockState::Unlocked | LockState::WaitingForSurfaces { .. } => false,
            LockState::Locking(_) | LockState::Locked(_) => true,
        }
    }

    pub fn lock(&mut self, confirmation: SessionLocker) {
        // Check if another client is in the process of locking.
        if matches!(
            self.lock_state,
            LockState::WaitingForSurfaces { .. } | LockState::Locking(_)
        ) {
            info!("refusing lock as another client is currently locking");
            return;
        }

        // Check if we're already locked with an active client.
        if let LockState::Locked(lock) = &self.lock_state {
            if lock.is_alive() {
                info!("refusing lock as already locked with an active client");
                return;
            }

            // If the client had died, continue with the new lock.
            info!("locking session (replacing existing dead lock)");

            // Since the session was already locked, we know that the outputs are blanked, and
            // can lock right away.
            let lock = confirmation.ext_session_lock().clone();
            confirmation.lock();
            self.lock_state = LockState::Locked(lock);

            return;
        }

        info!("locking session");

        if self.output_state.is_empty() {
            // There are no outputs, lock the session right away.
            self.screenshot_ui.close();
            self.cursor_manager
                .set_cursor_image(CursorImageStatus::default_named());

            let lock = confirmation.ext_session_lock().clone();
            confirmation.lock();
            self.lock_state = LockState::Locked(lock);
        } else {
            // There are outputs which we need to redraw before locking. But before we do that,
            // let's wait for the lock surfaces.
            //
            // Give them a second; swaylock can take its time to paint a big enough image.
            let timer = Timer::from_duration(Duration::from_millis(1000));
            let deadline_token = self
                .event_loop
                .insert_source(timer, |_, _, state| {
                    trace!("lock deadline expired, continuing");
                    state.niri.continue_to_locking();
                    TimeoutAction::Drop
                })
                .unwrap();

            self.lock_state = LockState::WaitingForSurfaces {
                confirmation,
                deadline_token,
            };
        }
    }

    pub fn maybe_continue_to_locking(&mut self) {
        if !matches!(self.lock_state, LockState::WaitingForSurfaces { .. }) {
            // Not waiting.
            return;
        }

        // Check if there are any outputs whose lock surfaces had not had a commit yet.
        for state in self.output_state.values() {
            let Some(surface) = &state.lock_surface else {
                // Surface not created yet.
                return;
            };

            if !is_mapped(surface.wl_surface()) {
                return;
            }
        }

        // All good.
        trace!("lock surfaces are ready, continuing");
        self.continue_to_locking();
    }

    fn continue_to_locking(&mut self) {
        match mem::take(&mut self.lock_state) {
            LockState::WaitingForSurfaces {
                confirmation,
                deadline_token,
            } => {
                self.event_loop.remove(deadline_token);

                self.screenshot_ui.close();
                self.cursor_manager
                    .set_cursor_image(CursorImageStatus::default_named());
                self.cancel_mru();

                if self.output_state.is_empty() {
                    // There are no outputs, lock the session right away.
                    let lock = confirmation.ext_session_lock().clone();
                    confirmation.lock();
                    self.lock_state = LockState::Locked(lock);
                } else {
                    // There are outputs which we need to redraw before locking.
                    self.lock_state = LockState::Locking(confirmation);
                    self.queue_redraw_all();
                }
            }
            other => {
                error!("continue_to_locking() called with wrong lock state: {other:?}",);
                self.lock_state = other;
            }
        }
    }

    pub fn unlock(&mut self) {
        info!("unlocking session");

        let prev = mem::take(&mut self.lock_state);
        if let LockState::WaitingForSurfaces { deadline_token, .. } = prev {
            self.event_loop.remove(deadline_token);
        }

        for output_state in self.output_state.values_mut() {
            output_state.lock_surface = None;
        }
        self.queue_redraw_all();
    }

    #[cfg(feature = "dbus")]
    fn update_locked_hint(&mut self) {
        use std::sync::LazyLock;

        if !self.is_session_instance {
            return;
        }

        static XDG_SESSION_ID: LazyLock<Option<String>> = LazyLock::new(|| {
            let id = std::env::var("XDG_SESSION_ID").ok();
            if id.is_none() {
                warn!(
                    "env var 'XDG_SESSION_ID' is unset or invalid; logind LockedHint won't be set"
                );
            }
            id
        });

        let Some(session_id) = &*XDG_SESSION_ID else {
            return;
        };

        fn call(session_id: &str, locked: bool) -> anyhow::Result<()> {
            let conn = zbus::blocking::Connection::system()
                .context("error connecting to the system bus")?;

            let message = conn
                .call_method(
                    Some("org.freedesktop.login1"),
                    "/org/freedesktop/login1",
                    Some("org.freedesktop.login1.Manager"),
                    "GetSession",
                    &(session_id),
                )
                .context("failed to call GetSession")?;

            let message_body = message.body();
            let session_path: zbus::zvariant::ObjectPath = message_body
                .deserialize()
                .context("failed to deserialize GetSession reply")?;

            conn.call_method(
                Some("org.freedesktop.login1"),
                session_path,
                Some("org.freedesktop.login1.Session"),
                "SetLockedHint",
                &(locked),
            )
            .context("failed to call SetLockedHint")?;

            Ok(())
        }

        // Consider only the fully locked state here. When using the locked hint with sleep
        // inhibitor tools, we want to allow sleep only after the screens are fully cleared with
        // the lock screen, which corresponds to the Locked state.
        let locked = matches!(self.lock_state, LockState::Locked(_));

        if self.locked_hint.is_some_and(|h| h == locked) {
            return;
        }

        self.locked_hint = Some(locked);

        let res = thread::Builder::new()
            .name("Logind LockedHint Updater".to_owned())
            .spawn(move || {
                let _span = tracy_client::span!("LockedHint");

                if let Err(err) = call(session_id, locked) {
                    warn!("failed to set logind LockedHint: {err:?}");
                }
            });

        if let Err(err) = res {
            warn!("error spawning a thread to set logind LockedHint: {err:?}");
        }
    }

    pub fn new_lock_surface(&mut self, surface: LockSurface, output: &Output) {
        let lock = match &self.lock_state {
            LockState::Unlocked => {
                error!("tried to add a lock surface on an unlocked session");
                return;
            }
            LockState::WaitingForSurfaces { confirmation, .. } => confirmation.ext_session_lock(),
            LockState::Locking(confirmation) => confirmation.ext_session_lock(),
            LockState::Locked(lock) => lock,
        };

        if lock.client() != surface.wl_surface().client() {
            debug!("ignoring lock surface from an unrelated client");
            return;
        }

        let Some(output_state) = self.output_state.get_mut(output) else {
            error!("missing output state");
            return;
        };

        output_state.lock_surface = Some(surface);
    }

    /// Activates the pointer constraint if necessary according to the current pointer contents.
    ///
    /// Make sure the pointer location and contents are up to date before calling this.
    pub fn maybe_activate_pointer_constraint(&self) {
        let Some((surface, surface_loc)) = &self.pointer_contents.surface else {
            return;
        };

        let pointer = self.seat.get_pointer().unwrap();
        if Some(surface) != pointer.current_focus().as_ref() {
            return;
        }

        with_pointer_constraint(surface, &pointer, |constraint| {
            let Some(constraint) = constraint else { return };

            if constraint.is_active() {
                return;
            }

            // Constraint does not apply if not within region.
            if let Some(region) = constraint.region() {
                let pointer_pos = pointer.current_location();
                let pos_within_surface = pointer_pos - *surface_loc;
                if !region.contains(pos_within_surface.to_i32_round()) {
                    return;
                }
            }

            constraint.activate();
        });
    }

    pub fn focus_layer_surface_if_on_demand(&mut self, surface: Option<LayerSurface>) {
        if let Some(surface) = surface {
            if surface.cached_state().keyboard_interactivity
                == wlr_layer::KeyboardInteractivity::OnDemand
            {
                if self.layer_shell_on_demand_focus.as_ref() != Some(&surface) {
                    self.layer_shell_on_demand_focus = Some(surface);

                    // FIXME: granular.
                    self.queue_redraw_all();
                }

                return;
            }
        }

        // Something else got clicked, clear on-demand layer-shell focus.
        if self.layer_shell_on_demand_focus.is_some() {
            self.layer_shell_on_demand_focus = None;

            // FIXME: granular.
            self.queue_redraw_all();
        }
    }

    /// Tries to find and return the root shell surface for a given surface.
    ///
    /// I.e. for popups, this function will try to find the parent toplevel or layer surface. For
    /// regular subsurfaces, it will find the root surface.
    pub fn find_root_shell_surface(&self, surface: &WlSurface) -> WlSurface {
        let Some(root) = self.root_surface.get(surface) else {
            return surface.clone();
        };

        if let Some(popup) = self.popups.find_popup(root) {
            return find_popup_root_surface(&popup).unwrap_or_else(|_| root.clone());
        }

        root.clone()
    }

    #[cfg(feature = "dbus")]
    pub fn on_ipc_outputs_changed(&self) {
        let _span = tracy_client::span!("Niri::on_ipc_outputs_changed");

        let Some(dbus) = &self.dbus else { return };
        let Some(conn_display_config) = dbus.conn_display_config.clone() else {
            return;
        };

        let res = thread::Builder::new()
            .name("DisplayConfig MonitorsChanged Emitter".to_owned())
            .spawn(move || {
                use crate::dbus::mutter_display_config::DisplayConfig;
                let _span = tracy_client::span!("MonitorsChanged");
                let iface = match conn_display_config
                    .object_server()
                    .interface::<_, DisplayConfig>("/org/gnome/Mutter/DisplayConfig")
                {
                    Ok(iface) => iface,
                    Err(err) => {
                        warn!("error getting DisplayConfig interface: {err:?}");
                        return;
                    }
                };

                async_io::block_on(async move {
                    if let Err(err) = DisplayConfig::monitors_changed(iface.signal_emitter()).await
                    {
                        warn!("error emitting MonitorsChanged: {err:?}");
                    }
                });
            });

        if let Err(err) = res {
            warn!("error spawning a thread to send MonitorsChanged: {err:?}");
        }
    }

    pub fn handle_focus_follows_mouse(&mut self, new_focus: &PointContents) {
        let Some(ffm) = self.config.borrow().input.focus_follows_mouse else {
            return;
        };

        let pointer = &self.seat.get_pointer().unwrap();
        if pointer.is_grabbed() {
            return;
        }

        if self.window_mru_ui.is_open() {
            return;
        }

        // Recompute the current pointer focus because we don't update it during animations.
        let current_focus = self.contents_under(pointer.current_location());

        if let Some(output) = &new_focus.output {
            if current_focus.output.as_ref() != Some(output) {
                self.layout.focus_output(output);
            }
        }

        if let Some(window) = &new_focus.window {
            if !self.layout.is_overview_open() && current_focus.window.as_ref() != Some(window) {
                let (window, hit) = window;

                // Don't trigger focus-follows-mouse over the tab indicator.
                if matches!(
                    hit,
                    HitType::Activate {
                        is_tab_indicator: true
                    }
                ) {
                    return;
                }

                if !self.layout.should_trigger_focus_follows_mouse_on(window) {
                    return;
                }

                if let Some(threshold) = ffm.max_scroll_amount {
                    if self.layout.scroll_amount_to_activate(window) > threshold.0 {
                        return;
                    }
                }

                self.layout.activate_window_without_raising(window);
                self.layer_shell_on_demand_focus = None;
            }
        }

        if let Some(layer) = &new_focus.layer {
            if current_focus.layer.as_ref() != Some(layer) {
                self.layer_shell_on_demand_focus = Some(layer.clone());
            }
        }
    }

    pub fn do_screen_transition(&mut self, renderer: &mut GlesRenderer, delay_ms: Option<u16>) {
        let _span = tracy_client::span!("Niri::do_screen_transition");

        self.update_render_elements(None);

        let textures: Vec<_> = self
            .output_state
            .keys()
            .cloned()
            .filter_map(|output| {
                let size = output.current_mode().unwrap().size;
                let transform = output.current_transform();

                let scale = Scale::from(output.current_scale().fractional_scale());
                let targets = [
                    RenderTarget::Output,
                    RenderTarget::Screencast,
                    RenderTarget::ScreenCapture,
                ];
                let textures = targets.map(|target| {
                    let ctx = RenderCtx {
                        renderer,
                        target,
                        xray: None,
                        shader_time: self.window_shader_start.elapsed().as_secs_f32(),
                    };
                    let elements = self.render_to_vec(ctx, &output, false);
                    let elements = elements.iter().rev();

                    let res = render_to_texture(
                        renderer,
                        size,
                        scale,
                        transform,
                        Fourcc::Abgr8888,
                        elements,
                    );

                    if let Err(err) = &res {
                        warn!("error rendering output {}: {err:?}", output.name());
                    }

                    res
                });

                if textures.iter().any(|res| res.is_err()) {
                    return None;
                }

                let textures = textures.map(|res| {
                    let texture = res.unwrap().0;
                    TextureBuffer::from_texture(
                        renderer,
                        texture,
                        scale,
                        transform,
                        Vec::new(), // We want windows below to get frame callbacks.
                    )
                });

                Some((output, textures))
            })
            .collect();

        let delay = delay_ms.map_or(screen_transition::DELAY, |d| {
            Duration::from_millis(u64::from(d))
        });

        for (output, from_texture) in textures {
            let state = self.output_state.get_mut(&output).unwrap();
            state.screen_transition = Some(ScreenTransition::new(
                from_texture,
                delay,
                self.clock.clone(),
            ));
        }

        // We don't actually need to queue a redraw because the point is to freeze the screen for a
        // bit, and even if the delay was zero, we're drawing the same contents anyway.
    }

    pub fn recompute_window_rules(&mut self) {
        let _span = tracy_client::span!("Niri::recompute_window_rules");

        let changed = {
            let window_rules = &self.config.borrow().window_rules;

            for unmapped in self.unmapped_windows.values_mut() {
                let new_rules = ResolvedWindowRules::compute(
                    window_rules,
                    WindowRef::Unmapped(unmapped),
                    self.is_at_startup,
                );
                if let InitialConfigureState::Configured { rules, .. } = &mut unmapped.state {
                    *rules = new_rules;
                }
            }

            let mut windows = vec![];
            self.layout.with_windows_mut(|mapped, _| {
                if mapped.recompute_window_rules(window_rules, self.is_at_startup) {
                    windows.push(mapped.window.clone());
                }
            });
            let changed = !windows.is_empty();
            for win in windows {
                self.layout.update_window(&win, None);
            }
            changed
        };

        if changed {
            // FIXME: granular.
            self.queue_redraw_all();
        }
    }

    pub fn recompute_layer_rules(&mut self) {
        let _span = tracy_client::span!("Niri::recompute_layer_rules");

        let mut changed = false;
        {
            let config = self.config.borrow();
            let rules = &config.layer_rules;

            for mapped in self.mapped_layer_surfaces.values_mut() {
                if mapped.recompute_layer_rules(rules, self.is_at_startup) {
                    changed = true;
                    mapped.update_config(&config);
                }
            }
        }

        if changed {
            // FIXME: granular.
            self.queue_redraw_all();
        }
    }

    pub fn reset_pointer_inactivity_timer(&mut self) {
        if self.pointer_inactivity_timer_got_reset {
            return;
        }

        let _span = tracy_client::span!("Niri::reset_pointer_inactivity_timer");

        if let Some(token) = self.pointer_inactivity_timer.take() {
            self.event_loop.remove(token);
        }

        let Some(timeout_ms) = self.config.borrow().cursor.hide_after_inactive_ms else {
            return;
        };

        let duration = Duration::from_millis(timeout_ms as u64);
        let timer = Timer::from_duration(duration);
        let token = self
            .event_loop
            .insert_source(timer, move |_, _, state| {
                state.niri.pointer_inactivity_timer = None;

                // If the pointer is already invisible, don't reset it back to Hidden causing one
                // frame of hover.
                if state.niri.pointer_visibility.is_visible() {
                    state.niri.pointer_visibility = PointerVisibility::Hidden;
                    state.niri.queue_redraw_all();
                }

                TimeoutAction::Drop
            })
            .unwrap();
        self.pointer_inactivity_timer = Some(token);

        self.pointer_inactivity_timer_got_reset = true;
    }

    pub fn notify_activity(&mut self) {
        if self.notified_activity_this_iteration {
            return;
        }

        let _span = tracy_client::span!("Niri::notify_activity");

        self.idle_notifier_state.notify_activity(&self.seat);

        self.notified_activity_this_iteration = true;
    }

    pub fn close_mru(&mut self, close_request: MruCloseRequest) -> Option<Window> {
        if !self.window_mru_ui.is_open() {
            return None;
        }
        self.queue_redraw_all();

        let id = self.window_mru_ui.close(close_request)?;
        self.find_window_by_id(id)
    }

    pub fn cancel_mru(&mut self) {
        self.close_mru(MruCloseRequest::Cancel);
    }

    /// Apply a pending MRU commit immediately.
    ///
    /// Called for example on keyboard events that reach the active window, which immediately adds
    /// it to the MRU.
    pub fn mru_apply_keyboard_commit(&mut self) {
        let Some(pending) = self.pending_mru_commit.take() else {
            return;
        };
        self.event_loop.remove(pending.token);

        if let Some(window) = self
            .layout
            .workspaces_mut()
            .flat_map(|ws| ws.windows_mut())
            .find(|w| w.id() == pending.id)
        {
            window.set_focus_timestamp(pending.stamp);
        }
    }

    pub fn queue_redraw_mru_output(&mut self) {
        if let Some(output) = self.window_mru_ui.output().cloned() {
            self.queue_redraw(&output);
        }
    }
}

pub struct NewClient {
    pub client: UnixStream,
    pub restricted: bool,
    pub credentials_unknown: bool,
}

pub struct ClientState {
    pub compositor_state: CompositorClientState,
    pub can_view_decoration_globals: bool,
    pub primary_selection_disabled: bool,
    /// Whether this client is denied from the restricted protocols such as security-context.
    pub restricted: bool,
    /// We cannot retrieve this client's socket credentials.
    pub credentials_unknown: bool,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

/// Resolves the global post-process shader source from config: inline `source` vs `path`.
///
/// Returns `None` (and warns) when disabled, when neither/both of `source`/`path` are set, when an
/// inline source is empty, or when a path can't be read.
pub(crate) fn global_shader_source(cfg: &niri_config::GlobalShader) -> Option<String> {
    if !cfg.enable {
        return None;
    }
    match (&cfg.source, &cfg.path) {
        (Some(s), None) => {
            if s.trim().is_empty() {
                warn!("global-shader: empty source, disabling");
                None
            } else {
                Some(s.clone())
            }
        }
        (None, Some(p)) => {
            let path = match expand_home(std::path::Path::new(p)) {
                Ok(Some(expanded)) => expanded,
                Ok(None) => std::path::PathBuf::from(p),
                Err(err) => {
                    warn!("global-shader: cannot expand path {p:?}: {err}; disabling");
                    return None;
                }
            };
            match std::fs::read_to_string(&path) {
                Ok(s) => Some(s),
                Err(err) => {
                    warn!("global-shader: cannot read path {p:?}: {err}; disabling");
                    None
                }
            }
        }
        (Some(_), Some(_)) => {
            warn!("global-shader: both source and path set, disabling");
            None
        }
        (None, None) => {
            warn!("global-shader: enabled but no source or path, disabling");
            None
        }
    }
}

/// Read a scoped/region shader `path` to its file contents (expanding `~`). Returns `None` if the
/// path cannot be expanded or read. This is the single source of truth for resolving a scoped
/// shader path: the `scoped_key` computed at compile time (`scoped_shader_chains`) must match the
/// one looked up at render time, so all `pass_sources` call sites resolve paths through this fn.
pub(crate) fn read_scoped_shader_path(p: &str) -> Option<String> {
    let path = match expand_home(std::path::Path::new(p)) {
        Ok(Some(e)) => e,
        Ok(None) => std::path::PathBuf::from(p),
        Err(_) => return None,
    };
    std::fs::read_to_string(&path).ok()
}

/// Resolve every configured scoped shader (region shaders + window-rule shaders) into its
/// `(source, hyprland)` pass list, reading any `path` files. The list is the full set compiled
/// into `Shaders.scoped` (dedupe is by key inside `set_scoped_programs`).
pub(crate) fn scoped_shader_chains(config: &niri_config::Config) -> Vec<Vec<(String, bool)>> {
    let mut chains: Vec<Vec<(String, bool)>> = config
        .region_shaders
        .iter()
        .map(|r| r.pass_sources(read_scoped_shader_path))
        .collect();
    for rule in &config.window_rules {
        if let Some(shader) = &rule.shader {
            let chain = shader.pass_sources(read_scoped_shader_path);
            if !chain.is_empty() {
                chains.push(chain);
            }
        }
    }
    chains
}

/// Resolve a global-shader config into an ordered list of `(source, hyprland)` passes. Empty
/// `passes` => the legacy single shader becomes a length-1 chain. If any pass cannot be resolved
/// (missing/unreadable/ambiguous), the whole chain is dropped (returns empty) so we never run a
/// partial chain.
pub(crate) fn global_shader_pass_sources(cfg: &niri_config::GlobalShader) -> Vec<(String, bool)> {
    if !cfg.enable {
        return Vec::new();
    }
    if cfg.passes.is_empty() {
        return match global_shader_source(cfg) {
            Some(src) => vec![(src, cfg.mode == "hyprland")],
            None => Vec::new(),
        };
    }
    if cfg.source.is_some() || cfg.path.is_some() {
        warn!("global-shader: both top-level source/path and pass{{}} blocks set; using passes");
    }
    let mut out = Vec::with_capacity(cfg.passes.len());
    for pass in &cfg.passes {
        let resolved = match (&pass.source, &pass.path) {
            (Some(s), None) if !s.trim().is_empty() => Some(s.clone()),
            (Some(_), None) => {
                warn!("global-shader: empty pass source, disabling chain");
                None
            }
            (None, Some(p)) => {
                let path = match expand_home(std::path::Path::new(p)) {
                    Ok(Some(e)) => e,
                    Ok(None) => std::path::PathBuf::from(p),
                    Err(err) => {
                        warn!(
                            "global-shader: cannot expand pass path {p:?}: {err}; disabling chain"
                        );
                        return Vec::new();
                    }
                };
                match std::fs::read_to_string(&path) {
                    Ok(s) => Some(s),
                    Err(err) => {
                        warn!("global-shader: cannot read pass path {p:?}: {err}; disabling chain");
                        None
                    }
                }
            }
            (Some(_), Some(_)) => {
                warn!("global-shader: pass has both source and path; disabling chain");
                None
            }
            (None, None) => {
                warn!("global-shader: pass has neither source nor path; disabling chain");
                None
            }
        };
        match resolved {
            Some(src) => out.push((src, pass.mode == "hyprland")),
            None => return Vec::new(),
        }
    }
    out
}

fn scale_relocate_crop<E: Element>(
    elem: E,
    output_scale: Scale<f64>,
    zoom: f64,
    ws_geo: Rectangle<f64, Logical>,
) -> Option<CropRenderElement<RelocateRenderElement<RescaleRenderElement<E>>>> {
    let ws_geo = ws_geo.to_physical_precise_round(output_scale);
    let elem = RescaleRenderElement::from_element(elem, Point::from((0, 0)), zoom);
    let elem = RelocateRenderElement::from_element(elem, ws_geo.loc, Relocate::Relative);
    CropRenderElement::from_element(elem, output_scale, ws_geo)
}

niri_render_elements! {
    PointerRenderElements<R> => {
        Wayland = WaylandSurfaceRenderElement<R>,
        NamedPointer = MemoryRenderBufferRenderElement<R>,
    }
}

niri_render_elements! {
    WindowScreenshotRenderElement<R> => {
        Layout = LayoutElementRenderElement<R>,
        Pointer = RelocateRenderElement<PointerRenderElements<R>>,
    }
}

niri_render_elements! {
    OutputRenderElements<R> => {
        Monitor = MonitorRenderElement<R>,
        // Cropped per-workspace to its render-geo box by `render_overview_at_zoom` (its only
        // caller, the carousel panel prepass below) — see that method's doc. A distinct variant
        // from `Monitor` (rather than changing `Monitor`'s payload type) because `Monitor` is
        // also reached generically via `.into()` from `render_workspaces` et al. (the REAL
        // overview render), which must keep emitting plain, uncropped `MonitorRenderElement<R>`.
        PanelMonitor = CropRenderElement<MonitorRenderElement<R>>,
        RescaledTile = RescaleRenderElement<TileRenderElement<R>>,
        LayerSurface = LayerSurfaceRenderElement<R>,
        RelocatedLayerSurface = CropRenderElement<RelocateRenderElement<RescaleRenderElement<
            LayerSurfaceRenderElement<R>
        >>>,
        RelocatedColor = CropRenderElement<RelocateRenderElement<RescaleRenderElement<
            SolidColorRenderElement
        >>>,
        Pointer = PointerRenderElements<R>,
        Wayland = WaylandSurfaceRenderElement<R>,
        SolidColor = SolidColorRenderElement,
        ScreenshotUi = ScreenshotUiRenderElement,
        WindowMruUi = WindowMruUiRenderElement<R>,
        ExitConfirmDialog = ExitConfirmDialogRenderElement,
        Texture = PrimaryGpuTextureRenderElement,
        // Used for the CPU-rendered panels.
        RelocatedMemoryBuffer = RelocateRenderElement<MemoryRenderBufferRenderElement<R>>,
        GlobalShader = GlobalShaderElement,
        ScopedShader = ScopedShaderElement,
        // Consolidated carousel: perspective-tilted cover-flow panel.
        Panel = PanelRenderElement,
    }
}

#[cfg(test)]
mod shader_throttle_tests {
    use std::time::{Duration, Instant};

    use super::{decide_shader_frame, ShaderFrameDecision};

    #[test]
    fn first_frame_is_due() {
        let now = Instant::now();
        assert_eq!(
            decide_shader_frame(None, now, Duration::from_millis(33)),
            ShaderFrameDecision::Due
        );
    }

    #[test]
    fn within_interval_waits_for_remaining() {
        let now = Instant::now();
        let interval = Duration::from_millis(33);
        let last = now - Duration::from_millis(10);
        match decide_shader_frame(Some(last), now, interval) {
            ShaderFrameDecision::Wait(remaining) => {
                // ~23ms left; allow a little slack for clock resolution.
                assert!(remaining <= Duration::from_millis(23));
                assert!(remaining >= Duration::from_millis(22));
            }
            ShaderFrameDecision::Due => panic!("should not be due yet"),
        }
    }

    #[test]
    fn at_or_past_interval_is_due() {
        let now = Instant::now();
        let interval = Duration::from_millis(33);
        // Exactly one interval elapsed.
        assert_eq!(
            decide_shader_frame(Some(now - interval), now, interval),
            ShaderFrameDecision::Due
        );
        // Well past the interval.
        assert_eq!(
            decide_shader_frame(Some(now - Duration::from_millis(100)), now, interval),
            ShaderFrameDecision::Due
        );
    }

    #[test]
    fn clock_going_backwards_does_not_panic() {
        let now = Instant::now();
        // last is in the "future" relative to now (saturating_duration_since → 0 elapsed).
        let last = now + Duration::from_millis(50);
        match decide_shader_frame(Some(last), now, Duration::from_millis(33)) {
            ShaderFrameDecision::Wait(remaining) => {
                assert_eq!(remaining, Duration::from_millis(33));
            }
            ShaderFrameDecision::Due => panic!("elapsed clamped to 0, so not due"),
        }
    }
}

#[cfg(test)]
mod panel_source_tests {
    use super::PanelSource;

    #[test]
    fn clear_preserves_generation() {
        let mut source = PanelSource::default();
        assert_eq!(source.generation, 0);

        // Simulate a few publishes bumping the counter, the way `update_panel_sources` does on
        // a damaged render.
        source.generation = 3;

        source.clear();

        // The whole point of this test: `clear()` must NOT reset `generation` to 0. Doing so
        // would let a post-reset publish republish under `(source_id, 0, context_id)` — a value
        // the host's retained `PanelRenderElement` may have already cached from before the
        // reset (aliasing, since the ping-pong buffers and their `Id`s outlive `clear()`).
        assert_eq!(source.generation, 3);

        // Everything else resets to the fresh (never-rendered) state.
        assert!(source.elem.is_none());
        assert_eq!(source.frame, 0);
        assert_eq!(source.last_extent, None);
    }

    #[test]
    fn clear_is_idempotent_on_generation() {
        let mut source = PanelSource::default();
        source.generation = 5;

        source.clear();
        source.clear();
        source.clear();

        assert_eq!(source.generation, 5);
    }
}
