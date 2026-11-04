use std::cell::{Cell, OnceCell, RefCell};

use niri_config::utils::Flag;
use niri_config::workspace::WorkspaceName;
use niri_config::{
    CenterFocusedColumn, FloatOrInt, MainAxis, OutputName, Struts, TabIndicatorLength,
    TabIndicatorPosition, WorkspaceReference,
};
use proptest::prelude::*;
use proptest_derive::Arbitrary;
use smithay::output::{Mode, PhysicalProperties, Subpixel};
use smithay::utils::Rectangle;

use super::*;

mod animations;
mod fullscreen;

impl<W: LayoutElement> Default for Layout<W> {
    fn default() -> Self {
        Self::with_options(Clock::with_time(Duration::ZERO), Default::default())
    }
}

#[derive(Debug)]
struct TestWindowInner {
    id: usize,
    parent_id: Cell<Option<usize>>,
    bbox: Cell<Rectangle<i32, Logical>>,
    initial_bbox: Rectangle<i32, Logical>,
    requested_size: Cell<Option<Size<i32, Logical>>>,
    // Emulates the window ignoring the compositor-provided size.
    forced_size: Cell<Option<Size<i32, Logical>>>,
    min_size: Size<i32, Logical>,
    max_size: Size<i32, Logical>,
    pending_sizing_mode: Cell<SizingMode>,
    pending_activated: Cell<bool>,
    sizing_mode: Cell<SizingMode>,
    is_windowed_fullscreen: Cell<bool>,
    is_pending_windowed_fullscreen: Cell<bool>,
    animate_next_configure: Cell<bool>,
    animation_snapshot: RefCell<Option<LayoutElementRenderSnapshot>>,
    rules: ResolvedWindowRules,
}

#[derive(Debug, Clone)]
struct TestWindow(Rc<TestWindowInner>);

#[derive(Debug, Clone, Arbitrary)]
struct TestWindowParams {
    #[proptest(strategy = "1..=5usize")]
    id: usize,
    #[proptest(strategy = "arbitrary_parent_id()")]
    parent_id: Option<usize>,
    is_floating: bool,
    #[proptest(strategy = "arbitrary_bbox()")]
    bbox: Rectangle<i32, Logical>,
    #[proptest(strategy = "arbitrary_min_max_size()")]
    min_max_size: (Size<i32, Logical>, Size<i32, Logical>),
    #[proptest(strategy = "prop::option::of(arbitrary_rules())")]
    rules: Option<ResolvedWindowRules>,
}

impl TestWindowParams {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            parent_id: None,
            is_floating: false,
            bbox: Rectangle::from_size(Size::from((100, 200))),
            min_max_size: Default::default(),
            rules: None,
        }
    }
}

impl TestWindow {
    fn new(params: TestWindowParams) -> Self {
        Self(Rc::new(TestWindowInner {
            id: params.id,
            parent_id: Cell::new(params.parent_id),
            bbox: Cell::new(params.bbox),
            initial_bbox: params.bbox,
            requested_size: Cell::new(None),
            forced_size: Cell::new(None),
            min_size: params.min_max_size.0,
            max_size: params.min_max_size.1,
            pending_sizing_mode: Cell::new(SizingMode::Normal),
            pending_activated: Cell::new(false),
            sizing_mode: Cell::new(SizingMode::Normal),
            is_windowed_fullscreen: Cell::new(false),
            is_pending_windowed_fullscreen: Cell::new(false),
            animate_next_configure: Cell::new(false),
            animation_snapshot: RefCell::new(None),
            rules: params.rules.unwrap_or_default(),
        }))
    }

    fn communicate(&self) -> bool {
        let mut changed = false;

        let size = self.0.forced_size.get().or(self.0.requested_size.get());
        if let Some(size) = size {
            assert!(size.w >= 0);
            assert!(size.h >= 0);

            let mut new_bbox = self.0.initial_bbox;
            if size.w != 0 {
                new_bbox.size.w = size.w;
            }
            if size.h != 0 {
                new_bbox.size.h = size.h;
            }

            if self.0.bbox.get() != new_bbox {
                if self.0.animate_next_configure.get() {
                    self.0.animation_snapshot.replace(Some(RenderSnapshot {
                        contents: Vec::new(),
                        contents_with_blocked_out_bg: None,
                        blocked_out_contents: Vec::new(),
                        block_out_from: None,
                        size: self.0.bbox.get().size.to_f64(),
                        texture: OnceCell::new(),
                        texture_with_blocked_out_bg: Default::default(),
                        blocked_out_texture: OnceCell::new(),
                    }));
                }

                self.0.bbox.set(new_bbox);
                changed = true;
            }
        }

        self.0.animate_next_configure.set(false);

        if self.0.sizing_mode.get() != self.0.pending_sizing_mode.get() {
            self.0.sizing_mode.set(self.0.pending_sizing_mode.get());
            changed = true;
        }

        if self.0.is_windowed_fullscreen.get() != self.0.is_pending_windowed_fullscreen.get() {
            self.0
                .is_windowed_fullscreen
                .set(self.0.is_pending_windowed_fullscreen.get());
            changed = true;
        }

        changed
    }
}

impl LayoutElement for TestWindow {
    type Id = usize;

    fn id(&self) -> &Self::Id {
        &self.0.id
    }

    fn size(&self) -> Size<i32, Logical> {
        self.0.bbox.get().size
    }

    fn buf_loc(&self) -> Point<i32, Logical> {
        (0, 0).into()
    }

    fn is_in_input_region(&self, _point: Point<f64, Logical>) -> bool {
        false
    }

    fn request_size(
        &mut self,
        size: Size<i32, Logical>,
        mode: SizingMode,
        _animate: bool,
        _transaction: Option<Transaction>,
    ) {
        if self.0.requested_size.get() != Some(size) {
            self.0.requested_size.set(Some(size));
            self.0.animate_next_configure.set(true);
        }

        self.0.pending_sizing_mode.set(mode);

        if mode.is_fullscreen() {
            self.0.is_pending_windowed_fullscreen.set(false);
        }
    }

    fn min_size(&self) -> Size<i32, Logical> {
        self.0.min_size
    }

    fn max_size(&self) -> Size<i32, Logical> {
        self.0.max_size
    }

    fn is_wl_surface(&self, _wl_surface: &WlSurface) -> bool {
        false
    }

    fn set_preferred_scale_transform(&self, _scale: output::Scale, _transform: Transform) {}

    fn has_ssd(&self) -> bool {
        false
    }

    fn output_enter(&self, _output: &Output) {}

    fn output_leave(&self, _output: &Output) {}

    fn set_offscreen_data(&self, _data: Option<OffscreenData>) {}

    fn set_activated(&mut self, active: bool) {
        self.0.pending_activated.set(active);
    }

    fn set_bounds(&self, _bounds: Size<i32, Logical>) {}

    fn is_ignoring_opacity_window_rule(&self) -> bool {
        false
    }

    fn configure_intent(&self) -> ConfigureIntent {
        ConfigureIntent::CanSend
    }

    fn send_pending_configure(&mut self) {}

    fn set_active_in_column(&mut self, _active: bool) {}

    fn set_floating(&mut self, _floating: bool) {}

    fn set_sticky(&mut self, _sticky: bool) {}

    fn sizing_mode(&self) -> SizingMode {
        self.0.sizing_mode.get()
    }

    fn pending_sizing_mode(&self) -> SizingMode {
        self.0.pending_sizing_mode.get()
    }

    fn requested_size(&self) -> Option<Size<i32, Logical>> {
        self.0.requested_size.get()
    }

    fn is_windowed_fullscreen(&self) -> bool {
        self.0.is_windowed_fullscreen.get()
    }

    fn is_pending_windowed_fullscreen(&self) -> bool {
        self.0.is_pending_windowed_fullscreen.get()
    }

    fn request_windowed_fullscreen(&mut self, value: bool) {
        self.0.is_pending_windowed_fullscreen.set(value);
    }

    fn is_child_of(&self, parent: &Self) -> bool {
        self.0.parent_id.get() == Some(parent.0.id)
    }

    fn refresh(&self) {}

    fn rules(&self) -> &ResolvedWindowRules {
        &self.0.rules
    }

    fn take_animation_snapshot(&mut self) -> Option<LayoutElementRenderSnapshot> {
        self.0.animation_snapshot.take()
    }

    fn set_interactive_resize(&mut self, _data: Option<InteractiveResizeData>) {}

    fn cancel_interactive_resize(&mut self) {}

    fn on_commit(&mut self, _serial: Serial) {}

    fn interactive_resize_data(&self) -> Option<InteractiveResizeData> {
        None
    }

    fn is_urgent(&self) -> bool {
        false
    }
}

fn arbitrary_size() -> impl Strategy<Value = Size<i32, Logical>> {
    any::<(u16, u16)>().prop_map(|(w, h)| Size::from((w.max(1).into(), h.max(1).into())))
}

fn arbitrary_bbox() -> impl Strategy<Value = Rectangle<i32, Logical>> {
    any::<(i16, i16, u16, u16)>().prop_map(|(x, y, w, h)| {
        let loc: Point<i32, _> = Point::from((x.into(), y.into()));
        let size: Size<i32, _> = Size::from((w.max(1).into(), h.max(1).into()));
        Rectangle::new(loc, size)
    })
}

fn arbitrary_size_change() -> impl Strategy<Value = SizeChange> {
    prop_oneof![
        (0..).prop_map(SizeChange::SetFixed),
        (0f64..).prop_map(SizeChange::SetProportion),
        any::<i32>().prop_map(SizeChange::AdjustFixed),
        any::<f64>().prop_map(SizeChange::AdjustProportion),
        // Interactive resize can have negative values here.
        Just(SizeChange::SetFixed(-100)),
    ]
}

fn arbitrary_position_change() -> impl Strategy<Value = PositionChange> {
    prop_oneof![
        (-1000f64..1000f64).prop_map(PositionChange::SetFixed),
        any::<f64>().prop_map(PositionChange::SetProportion),
        (-1000f64..1000f64).prop_map(PositionChange::AdjustFixed),
        any::<f64>().prop_map(PositionChange::AdjustProportion),
        any::<f64>().prop_map(PositionChange::SetFixed),
        any::<f64>().prop_map(PositionChange::AdjustFixed),
    ]
}

fn arbitrary_min_max() -> impl Strategy<Value = (i32, i32)> {
    prop_oneof![
        Just((0, 0)),
        (1..65536).prop_map(|n| (n, n)),
        (1..65536).prop_map(|min| (min, 0)),
        (1..).prop_map(|max| (0, max)),
        (1..65536, 1..).prop_map(|(min, max): (i32, i32)| (min, max.max(min))),
    ]
}

fn arbitrary_min_max_size() -> impl Strategy<Value = (Size<i32, Logical>, Size<i32, Logical>)> {
    prop_oneof![
        5 => (arbitrary_min_max(), arbitrary_min_max()).prop_map(
            |((min_w, max_w), (min_h, max_h))| {
                let min_size = Size::from((min_w, min_h));
                let max_size = Size::from((max_w, max_h));
                (min_size, max_size)
            },
        ),
        1 => arbitrary_min_max().prop_map(|(w, h)| {
            let size = Size::from((w, h));
            (size, size)
        }),
    ]
}

prop_compose! {
    fn arbitrary_rules()(
        focus_ring in arbitrary_focus_ring(),
        border in arbitrary_border(),
    ) -> ResolvedWindowRules {
        ResolvedWindowRules {
            focus_ring,
            border,
            ..ResolvedWindowRules::default()
        }
    }
}

fn arbitrary_view_offset_gesture_delta() -> impl Strategy<Value = f64> {
    prop_oneof![(-10f64..10f64), (-50000f64..50000f64),]
}

fn arbitrary_resize_edge() -> impl Strategy<Value = ResizeEdge> {
    prop_oneof![
        Just(ResizeEdge::RIGHT),
        Just(ResizeEdge::BOTTOM),
        Just(ResizeEdge::LEFT),
        Just(ResizeEdge::TOP),
        Just(ResizeEdge::BOTTOM_RIGHT),
        Just(ResizeEdge::BOTTOM_LEFT),
        Just(ResizeEdge::TOP_RIGHT),
        Just(ResizeEdge::TOP_LEFT),
        Just(ResizeEdge::empty()),
    ]
}

fn arbitrary_scale() -> impl Strategy<Value = f64> {
    prop_oneof![Just(1.), Just(1.5), Just(2.),]
}

fn arbitrary_msec_delta() -> impl Strategy<Value = i32> {
    prop_oneof![
        1 => Just(-1000),
        2 => Just(-10),
        1 => Just(0),
        2 => Just(10),
        6 => Just(1000),
    ]
}

fn arbitrary_parent_id() -> impl Strategy<Value = Option<usize>> {
    prop_oneof![
        5 => Just(None),
        1 => prop::option::of(1..=5usize),
    ]
}

fn arbitrary_scroll_direction() -> impl Strategy<Value = ScrollDirection> {
    prop_oneof![Just(ScrollDirection::Left), Just(ScrollDirection::Right)]
}

fn arbitrary_column_display() -> impl Strategy<Value = ColumnDisplay> {
    prop_oneof![Just(ColumnDisplay::Normal), Just(ColumnDisplay::Tabbed)]
}

#[derive(Debug, Clone, Arbitrary)]
enum Op {
    AddOutput(#[proptest(strategy = "1..=5usize")] usize),
    AddScaledOutput {
        #[proptest(strategy = "1..=5usize")]
        id: usize,
        #[proptest(strategy = "arbitrary_scale()")]
        scale: f64,
        #[proptest(strategy = "prop::option::of(arbitrary_layout_part().prop_map(Box::new))")]
        layout_config: Option<Box<niri_config::LayoutPart>>,
    },
    RemoveOutput(#[proptest(strategy = "1..=5usize")] usize),
    FocusOutput(#[proptest(strategy = "1..=5usize")] usize),
    UpdateOutputLayoutConfig {
        #[proptest(strategy = "1..=5usize")]
        id: usize,
        #[proptest(strategy = "prop::option::of(arbitrary_layout_part().prop_map(Box::new))")]
        layout_config: Option<Box<niri_config::LayoutPart>>,
    },
    AddNamedWorkspace {
        #[proptest(strategy = "1..=5usize")]
        ws_name: usize,
        #[proptest(strategy = "proptest::collection::vec(1..=5usize, 0..=3)")]
        output_names: Vec<usize>,
        #[proptest(strategy = "prop::option::of(arbitrary_layout_part().prop_map(Box::new))")]
        layout_config: Option<Box<niri_config::LayoutPart>>,
    },
    UnnameWorkspace {
        #[proptest(strategy = "1..=5usize")]
        ws_name: usize,
    },
    UpdateWorkspaceLayoutConfig {
        #[proptest(strategy = "1..=5usize")]
        ws_name: usize,
        #[proptest(strategy = "prop::option::of(arbitrary_layout_part().prop_map(Box::new))")]
        layout_config: Option<Box<niri_config::LayoutPart>>,
    },
    AddWindow {
        params: TestWindowParams,
    },
    AddWindowNextTo {
        params: TestWindowParams,
        #[proptest(strategy = "1..=5usize")]
        next_to_id: usize,
    },
    AddWindowToNamedWorkspace {
        params: TestWindowParams,
        #[proptest(strategy = "1..=5usize")]
        ws_name: usize,
    },
    CloseWindow(#[proptest(strategy = "1..=5usize")] usize),
    FullscreenWindow(#[proptest(strategy = "1..=5usize")] usize),
    SetFullscreenWindow {
        #[proptest(strategy = "1..=5usize")]
        window: usize,
        is_fullscreen: bool,
    },
    ToggleWindowedFullscreen(#[proptest(strategy = "1..=5usize")] usize),
    FocusColumnLeft,
    FocusColumnRight,
    FocusColumnFirst,
    FocusColumnLast,
    FocusColumnRightOrFirst,
    FocusColumnLeftOrLast,
    FocusColumn(#[proptest(strategy = "1..=5usize")] usize),
    FocusWindowOrMonitorUp(#[proptest(strategy = "1..=2u8")] u8),
    FocusWindowOrMonitorDown(#[proptest(strategy = "1..=2u8")] u8),
    FocusColumnOrMonitorLeft(#[proptest(strategy = "1..=2u8")] u8),
    FocusColumnOrMonitorRight(#[proptest(strategy = "1..=2u8")] u8),
    FocusWindowDown,
    FocusWindowUp,
    FocusWindowDownOrColumnLeft,
    FocusWindowDownOrColumnRight,
    FocusWindowUpOrColumnLeft,
    FocusWindowUpOrColumnRight,
    FocusWindowOrWorkspaceDown,
    FocusWindowOrWorkspaceUp,
    FocusWindow(#[proptest(strategy = "1..=5usize")] usize),
    FocusWindowInColumn(#[proptest(strategy = "1..=5u8")] u8),
    FocusWindowTop,
    FocusWindowBottom,
    FocusWindowDownOrTop,
    FocusWindowUpOrBottom,
    MoveColumnLeft,
    MoveColumnRight,
    MoveColumnToFirst,
    MoveColumnToLast,
    MoveColumnLeftOrToMonitorLeft(#[proptest(strategy = "1..=2u8")] u8),
    MoveColumnRightOrToMonitorRight(#[proptest(strategy = "1..=2u8")] u8),
    MoveColumnToIndex(#[proptest(strategy = "1..=5usize")] usize),
    MoveWindowDown,
    MoveWindowUp,
    MoveWindowDownOrToWorkspaceDown,
    MoveWindowUpOrToWorkspaceUp,
    ConsumeOrExpelWindowLeft {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
    },
    ConsumeOrExpelWindowRight {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
    },
    ConsumeWindowIntoColumn,
    ExpelWindowFromColumn,
    SwapWindowInDirection(#[proptest(strategy = "arbitrary_scroll_direction()")] ScrollDirection),
    ToggleColumnTabbedDisplay,
    SetColumnDisplay(#[proptest(strategy = "arbitrary_column_display()")] ColumnDisplay),
    CenterColumn,
    CenterWindow {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
    },
    CenterVisibleColumns,
    FocusWorkspaceDown,
    FocusWorkspaceUp,
    FocusWorkspace(#[proptest(strategy = "0..=4usize")] usize),
    FocusWorkspaceAutoBackAndForth(#[proptest(strategy = "0..=4usize")] usize),
    FocusWorkspacePrevious,
    MoveWindowToWorkspaceDown(bool),
    MoveWindowToWorkspaceUp(bool),
    MoveWindowToWorkspace {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        window_id: Option<usize>,
        #[proptest(strategy = "0..=4usize")]
        workspace_idx: usize,
        allow_hidden: bool,
    },
    MoveColumnToWorkspaceDown(bool),
    MoveColumnToWorkspaceUp(bool),
    MoveColumnToWorkspace(#[proptest(strategy = "0..=4usize")] usize, bool, bool),
    MoveWorkspaceDown,
    MoveWorkspaceUp,
    MoveWorkspaceToIndex {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        ws_name: Option<usize>,
        #[proptest(strategy = "0..=4usize")]
        target_idx: usize,
    },
    MoveWorkspaceToMonitor {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        ws_name: Option<usize>,
        #[proptest(strategy = "0..=5usize")]
        output_id: usize,
    },
    SetWorkspaceName {
        #[proptest(strategy = "1..=5usize")]
        new_ws_name: usize,
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        ws_name: Option<usize>,
    },
    UnsetWorkspaceName {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        ws_name: Option<usize>,
    },
    ToggleWorkspaceVisibility(#[proptest(strategy = "1..=5usize")] usize),
    HideWorkspace(#[proptest(strategy = "1..=5usize")] usize),
    UnhideWorkspace(#[proptest(strategy = "1..=5usize")] usize),
    MoveWindowToOutput {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        window_id: Option<usize>,
        #[proptest(strategy = "1..=5usize")]
        output_id: usize,
        #[proptest(strategy = "proptest::option::of(0..=4usize)")]
        target_ws_idx: Option<usize>,
    },
    MoveColumnToOutput {
        #[proptest(strategy = "1..=5usize")]
        output_id: usize,
        #[proptest(strategy = "proptest::option::of(0..=4usize)")]
        target_ws_idx: Option<usize>,
        activate: bool,
    },
    SwitchPresetColumnWidth,
    SwitchPresetColumnWidthBack,
    SwitchPresetWindowWidth {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
    },
    SwitchPresetWindowWidthBack {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
    },
    SwitchPresetWindowHeight {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
    },
    SwitchPresetWindowHeightBack {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
    },
    MaximizeColumn,
    MaximizeWindowToEdges {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
    },
    SetColumnWidth(#[proptest(strategy = "arbitrary_size_change()")] SizeChange),
    SetWindowWidth {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
        #[proptest(strategy = "arbitrary_size_change()")]
        change: SizeChange,
    },
    SetWindowHeight {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
        #[proptest(strategy = "arbitrary_size_change()")]
        change: SizeChange,
    },
    ResetWindowHeight {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
    },
    ExpandColumnToAvailableWidth,
    ToggleWindowFloating {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
    },
    SetWindowFloating {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
        floating: bool,
    },
    FocusFloating,
    FocusTiling,
    SwitchFocusFloatingTiling,
    MoveFloatingWindow {
        #[proptest(strategy = "proptest::option::of(1..=5usize)")]
        id: Option<usize>,
        #[proptest(strategy = "arbitrary_position_change()")]
        x: PositionChange,
        #[proptest(strategy = "arbitrary_position_change()")]
        y: PositionChange,
        animate: bool,
    },
    SetParent {
        #[proptest(strategy = "1..=5usize")]
        id: usize,
        #[proptest(strategy = "prop::option::of(1..=5usize)")]
        new_parent_id: Option<usize>,
    },
    SetForcedSize {
        #[proptest(strategy = "1..=5usize")]
        id: usize,
        #[proptest(strategy = "proptest::option::of(arbitrary_size())")]
        size: Option<Size<i32, Logical>>,
    },
    Communicate(#[proptest(strategy = "1..=5usize")] usize),
    Refresh {
        is_active: bool,
    },
    AdvanceAnimations {
        #[proptest(strategy = "arbitrary_msec_delta()")]
        msec_delta: i32,
    },
    CompleteAnimations,
    MoveWorkspaceToOutput(#[proptest(strategy = "1..=5usize")] usize),
    ViewOffsetGestureBegin {
        #[proptest(strategy = "1..=5usize")]
        output_idx: usize,
        #[proptest(strategy = "proptest::option::of(0..=4usize)")]
        workspace_idx: Option<usize>,
        is_touchpad: bool,
    },
    ViewOffsetGestureUpdate {
        #[proptest(strategy = "arbitrary_view_offset_gesture_delta()")]
        delta: f64,
        timestamp: Duration,
        is_touchpad: bool,
    },
    ViewOffsetGestureEnd {
        is_touchpad: Option<bool>,
    },
    WorkspaceSwitchGestureBegin {
        #[proptest(strategy = "1..=5usize")]
        output_idx: usize,
        is_touchpad: bool,
    },
    WorkspaceSwitchGestureUpdate {
        #[proptest(strategy = "-400f64..400f64")]
        delta: f64,
        timestamp: Duration,
        is_touchpad: bool,
    },
    WorkspaceSwitchGestureEnd {
        is_touchpad: Option<bool>,
    },
    OverviewGestureBegin,
    OverviewGestureUpdate {
        #[proptest(strategy = "-400f64..400f64")]
        delta: f64,
        timestamp: Duration,
    },
    OverviewGestureEnd,
    InteractiveMoveBegin {
        #[proptest(strategy = "1..=5usize")]
        window: usize,
        #[proptest(strategy = "1..=5usize")]
        output_idx: usize,
        #[proptest(strategy = "-20000f64..20000f64")]
        px: f64,
        #[proptest(strategy = "-20000f64..20000f64")]
        py: f64,
    },
    InteractiveMoveUpdate {
        #[proptest(strategy = "1..=5usize")]
        window: usize,
        #[proptest(strategy = "-20000f64..20000f64")]
        dx: f64,
        #[proptest(strategy = "-20000f64..20000f64")]
        dy: f64,
        #[proptest(strategy = "1..=5usize")]
        output_idx: usize,
        #[proptest(strategy = "-20000f64..20000f64")]
        px: f64,
        #[proptest(strategy = "-20000f64..20000f64")]
        py: f64,
    },
    InteractiveMoveEnd {
        #[proptest(strategy = "1..=5usize")]
        window: usize,
    },
    DndUpdate {
        #[proptest(strategy = "1..=5usize")]
        output_idx: usize,
        #[proptest(strategy = "-20000f64..20000f64")]
        px: f64,
        #[proptest(strategy = "-20000f64..20000f64")]
        py: f64,
    },
    DndEnd,
    InteractiveResizeBegin {
        #[proptest(strategy = "1..=5usize")]
        window: usize,
        #[proptest(strategy = "arbitrary_resize_edge()")]
        edges: ResizeEdge,
    },
    InteractiveResizeUpdate {
        #[proptest(strategy = "1..=5usize")]
        window: usize,
        #[proptest(strategy = "-20000f64..20000f64")]
        dx: f64,
        #[proptest(strategy = "-20000f64..20000f64")]
        dy: f64,
    },
    InteractiveResizeEnd {
        #[proptest(strategy = "1..=5usize")]
        window: usize,
    },
    ToggleOverview,
    UpdateConfig {
        #[proptest(strategy = "arbitrary_layout_part().prop_map(Box::new)")]
        layout_config: Box<niri_config::LayoutPart>,
    },
}

impl Op {
    fn apply(self, layout: &mut Layout<TestWindow>) {
        match self {
            Op::AddOutput(id) => {
                let name = format!("output{id}");
                if layout.outputs().any(|o| o.name() == name) {
                    return;
                }

                let output = Output::new(
                    name.clone(),
                    PhysicalProperties {
                        size: Size::from((1280, 720)),
                        subpixel: Subpixel::Unknown,
                        make: String::new(),
                        model: String::new(),
                        serial_number: String::new(),
                    },
                );
                output.change_current_state(
                    Some(Mode {
                        size: Size::from((1280, 720)),
                        refresh: 60000,
                    }),
                    None,
                    None,
                    None,
                );
                output.user_data().insert_if_missing(|| OutputName {
                    connector: name,
                    make: None,
                    model: None,
                    serial: None,
                });
                layout.add_output(output.clone(), None, false);
            }
            Op::AddScaledOutput {
                id,
                scale,
                layout_config,
            } => {
                let name = format!("output{id}");
                if layout.outputs().any(|o| o.name() == name) {
                    return;
                }

                let output = Output::new(
                    name.clone(),
                    PhysicalProperties {
                        size: Size::from((1280, 720)),
                        subpixel: Subpixel::Unknown,
                        make: String::new(),
                        model: String::new(),
                        serial_number: String::new(),
                    },
                );
                output.change_current_state(
                    Some(Mode {
                        size: Size::from((1280, 720)),
                        refresh: 60000,
                    }),
                    None,
                    Some(smithay::output::Scale::Fractional(scale)),
                    None,
                );
                output.user_data().insert_if_missing(|| OutputName {
                    connector: name,
                    make: None,
                    model: None,
                    serial: None,
                });
                layout.add_output(output.clone(), layout_config.map(|x| *x), false);
            }
            Op::RemoveOutput(id) => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.remove_output(&output);
            }
            Op::FocusOutput(id) => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.focus_output(&output);
            }
            Op::UpdateOutputLayoutConfig { id, layout_config } => {
                let name = format!("output{id}");
                let Some(mon) = layout.monitors_mut().find(|m| m.output_name() == &name) else {
                    return;
                };

                mon.update_layout_config(layout_config.map(|x| *x));
            }
            Op::AddNamedWorkspace {
                ws_name,
                output_names,
                layout_config,
            } => {
                layout.ensure_named_workspace(&WorkspaceConfig {
                    hidden: None,
                    name: WorkspaceName(format!("ws{ws_name}")),
                    open_on_output: output_names
                        .into_iter()
                        .map(|name| format!("output{name}"))
                        .collect(),
                    layout: layout_config.map(|x| niri_config::WorkspaceLayoutPart(*x)),
                });
            }
            Op::UnnameWorkspace { ws_name } => {
                layout.unname_workspace(&format!("ws{ws_name}"));
            }
            Op::UpdateWorkspaceLayoutConfig {
                ws_name,
                layout_config,
            } => {
                let ws_name = format!("ws{ws_name}");
                let Some(ws) = layout
                    .workspaces_mut()
                    .find(|ws| ws.name() == Some(&ws_name))
                else {
                    return;
                };

                ws.update_layout_config(layout_config.map(|x| *x));
            }
            Op::SetWorkspaceName {
                new_ws_name,
                ws_name,
            } => {
                let ws_ref =
                    ws_name.map(|ws_name| WorkspaceReference::Name(format!("ws{ws_name}")));
                layout.set_workspace_name(format!("ws{new_ws_name}"), ws_ref);
            }
            Op::UnsetWorkspaceName { ws_name } => {
                let ws_ref =
                    ws_name.map(|ws_name| WorkspaceReference::Name(format!("ws{ws_name}")));
                layout.unset_workspace_name(ws_ref);
            }
            Op::ToggleWorkspaceVisibility(ws_name) => {
                layout.toggle_workspace_visibility(format!("ws{ws_name}"));
            }
            Op::HideWorkspace(ws_name) => {
                layout.hide_workspace(&format!("ws{ws_name}"));
            }
            Op::UnhideWorkspace(ws_name) => {
                layout.unhide_workspace(&format!("ws{ws_name}"));
            }
            Op::AddWindow { mut params } => {
                if layout.has_window(&params.id) {
                    return;
                }
                if let Some(parent_id) = params.parent_id {
                    if parent_id_causes_loop(layout, params.id, parent_id) {
                        params.parent_id = None;
                    }
                }

                let is_floating = params.is_floating;
                let win = TestWindow::new(params);
                layout.add_window(
                    win,
                    AddWindowTarget::Auto,
                    None,
                    None,
                    false,
                    is_floating,
                    false,
                    ActivateWindow::default(),
                    false,
                );
            }
            Op::AddWindowNextTo {
                mut params,
                next_to_id,
            } => {
                let mut found_next_to = false;

                if let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move {
                    let win_id = move_.tile.window().0.id;
                    if win_id == params.id {
                        return;
                    }
                    if win_id == next_to_id {
                        found_next_to = true;
                    }
                }

                match &mut layout.monitor_set {
                    MonitorSet::Normal { monitors, .. } => {
                        for mon in monitors {
                            for ws in &mut mon.workspaces {
                                for win in ws.windows() {
                                    if win.0.id == params.id {
                                        return;
                                    }

                                    if win.0.id == next_to_id {
                                        found_next_to = true;
                                    }
                                }
                            }
                        }
                    }
                    MonitorSet::NoOutputs { workspaces, .. } => {
                        for ws in workspaces {
                            for win in ws.windows() {
                                if win.0.id == params.id {
                                    return;
                                }

                                if win.0.id == next_to_id {
                                    found_next_to = true;
                                }
                            }
                        }
                    }
                }

                if !found_next_to {
                    return;
                }

                if let Some(parent_id) = params.parent_id {
                    if parent_id_causes_loop(layout, params.id, parent_id) {
                        params.parent_id = None;
                    }
                }

                let is_floating = params.is_floating;
                let win = TestWindow::new(params);
                layout.add_window(
                    win,
                    AddWindowTarget::NextTo(&next_to_id),
                    None,
                    None,
                    false,
                    is_floating,
                    false,
                    ActivateWindow::default(),
                    false,
                );
            }
            Op::AddWindowToNamedWorkspace {
                mut params,
                ws_name,
            } => {
                let ws_name = format!("ws{ws_name}");
                let mut ws_id = None;

                if let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move {
                    if move_.tile.window().0.id == params.id {
                        return;
                    }
                }

                match &mut layout.monitor_set {
                    MonitorSet::Normal { monitors, .. } => {
                        for mon in monitors {
                            for ws in &mut mon.workspaces {
                                for win in ws.windows() {
                                    if win.0.id == params.id {
                                        return;
                                    }
                                }

                                if ws
                                    .name
                                    .as_ref()
                                    .is_some_and(|name| name.eq_ignore_ascii_case(&ws_name))
                                {
                                    ws_id = Some(ws.id());
                                }
                            }
                        }
                    }
                    MonitorSet::NoOutputs { workspaces, .. } => {
                        for ws in workspaces {
                            for win in ws.windows() {
                                if win.0.id == params.id {
                                    return;
                                }
                            }

                            if ws
                                .name
                                .as_ref()
                                .is_some_and(|name| name.eq_ignore_ascii_case(&ws_name))
                            {
                                ws_id = Some(ws.id());
                            }
                        }
                    }
                }

                let Some(ws_id) = ws_id else {
                    return;
                };

                if let Some(parent_id) = params.parent_id {
                    if parent_id_causes_loop(layout, params.id, parent_id) {
                        params.parent_id = None;
                    }
                }

                let is_floating = params.is_floating;
                let win = TestWindow::new(params);
                layout.add_window(
                    win,
                    AddWindowTarget::Workspace(ws_id),
                    None,
                    None,
                    false,
                    is_floating,
                    false,
                    ActivateWindow::default(),
                    false,
                );
            }
            Op::CloseWindow(id) => {
                layout.remove_window(&id, Transaction::new());
            }
            Op::FullscreenWindow(id) => {
                if !layout.has_window(&id) {
                    return;
                }
                layout.toggle_fullscreen(&id);
            }
            Op::SetFullscreenWindow {
                window,
                is_fullscreen,
            } => {
                if !layout.has_window(&window) {
                    return;
                }
                layout.set_fullscreen(&window, is_fullscreen);
            }
            Op::ToggleWindowedFullscreen(id) => {
                if !layout.has_window(&id) {
                    return;
                }
                layout.toggle_windowed_fullscreen(&id);
            }
            Op::FocusColumnLeft => layout.focus_left(),
            Op::FocusColumnRight => layout.focus_right(),
            Op::FocusColumnFirst => layout.focus_column_first(),
            Op::FocusColumnLast => layout.focus_column_last(),
            Op::FocusColumnRightOrFirst => layout.focus_column_right_or_first(),
            Op::FocusColumnLeftOrLast => layout.focus_column_left_or_last(),
            Op::FocusColumn(index) => layout.focus_column(index),
            Op::FocusWindowOrMonitorUp(id) => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.focus_window_up_or_output(&output);
            }
            Op::FocusWindowOrMonitorDown(id) => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.focus_window_down_or_output(&output);
            }
            Op::FocusColumnOrMonitorLeft(id) => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.focus_column_left_or_output(&output);
            }
            Op::FocusColumnOrMonitorRight(id) => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.focus_column_right_or_output(&output);
            }
            Op::FocusWindowDown => layout.focus_down(),
            Op::FocusWindowUp => layout.focus_up(),
            Op::FocusWindowDownOrColumnLeft => layout.focus_down_or_left(),
            Op::FocusWindowDownOrColumnRight => layout.focus_down_or_right(),
            Op::FocusWindowUpOrColumnLeft => layout.focus_up_or_left(),
            Op::FocusWindowUpOrColumnRight => layout.focus_up_or_right(),
            Op::FocusWindowOrWorkspaceDown => layout.focus_window_or_workspace_down(),
            Op::FocusWindowOrWorkspaceUp => layout.focus_window_or_workspace_up(),
            Op::FocusWindow(id) => layout.activate_window(&id),
            Op::FocusWindowInColumn(index) => layout.focus_window_in_column(index),
            Op::FocusWindowTop => layout.focus_window_top(),
            Op::FocusWindowBottom => layout.focus_window_bottom(),
            Op::FocusWindowDownOrTop => layout.focus_window_down_or_top(),
            Op::FocusWindowUpOrBottom => layout.focus_window_up_or_bottom(),
            Op::MoveColumnLeft => layout.move_left(),
            Op::MoveColumnRight => layout.move_right(),
            Op::MoveColumnToFirst => layout.move_column_to_first(),
            Op::MoveColumnToLast => layout.move_column_to_last(),
            Op::MoveColumnLeftOrToMonitorLeft(id) => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.move_column_left_or_to_output(&output);
            }
            Op::MoveColumnRightOrToMonitorRight(id) => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.move_column_right_or_to_output(&output);
            }
            Op::MoveColumnToIndex(index) => layout.move_column_to_index(index),
            Op::MoveWindowDown => layout.move_down(),
            Op::MoveWindowUp => layout.move_up(),
            Op::MoveWindowDownOrToWorkspaceDown => layout.move_down_or_to_workspace_down(),
            Op::MoveWindowUpOrToWorkspaceUp => layout.move_up_or_to_workspace_up(),
            Op::ConsumeOrExpelWindowLeft { id } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.consume_or_expel_window_left(id.as_ref());
            }
            Op::ConsumeOrExpelWindowRight { id } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.consume_or_expel_window_right(id.as_ref());
            }
            Op::ConsumeWindowIntoColumn => layout.consume_into_column(),
            Op::ExpelWindowFromColumn => layout.expel_from_column(),
            Op::SwapWindowInDirection(direction) => layout.swap_window_in_direction(direction),
            Op::ToggleColumnTabbedDisplay => layout.toggle_column_tabbed_display(),
            Op::SetColumnDisplay(display) => layout.set_column_display(display),
            Op::CenterColumn => layout.center_column(),
            Op::CenterWindow { id } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.center_window(id.as_ref());
            }
            Op::CenterVisibleColumns => layout.center_visible_columns(),
            Op::FocusWorkspaceDown => layout.switch_workspace_down(),
            Op::FocusWorkspaceUp => layout.switch_workspace_up(),
            Op::FocusWorkspace(idx) => layout.switch_workspace(idx, false),
            Op::FocusWorkspaceAutoBackAndForth(idx) => {
                layout.switch_workspace_auto_back_and_forth(idx)
            }
            Op::FocusWorkspacePrevious => layout.switch_workspace_previous(),
            Op::MoveWindowToWorkspaceDown(focus) => layout.move_to_workspace_down(focus),
            Op::MoveWindowToWorkspaceUp(focus) => layout.move_to_workspace_up(focus),
            Op::MoveWindowToWorkspace {
                window_id,
                workspace_idx,
                allow_hidden,
            } => {
                let window_id = window_id.filter(|id| layout.has_window(id));
                layout.move_to_workspace(
                    window_id.as_ref(),
                    workspace_idx,
                    ActivateWindow::Smart,
                    allow_hidden,
                );
            }
            Op::MoveColumnToWorkspaceDown(focus) => layout.move_column_to_workspace_down(focus),
            Op::MoveColumnToWorkspaceUp(focus) => layout.move_column_to_workspace_up(focus),
            Op::MoveColumnToWorkspace(idx, focus, allow_hidden) => {
                layout.move_column_to_workspace(idx, focus, allow_hidden)
            }
            Op::MoveWindowToOutput {
                window_id,
                output_id: id,
                target_ws_idx,
            } => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };
                let mon = layout.monitor_for_output(&output).unwrap();

                let window_id = window_id.filter(|id| layout.has_window(id));
                let target_ws_idx = target_ws_idx.filter(|idx| mon.workspaces.len() > *idx);
                layout.move_to_output(
                    window_id.as_ref(),
                    &output,
                    target_ws_idx,
                    ActivateWindow::Smart,
                );
            }
            Op::MoveColumnToOutput {
                output_id: id,
                target_ws_idx,
                activate,
            } => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.move_column_to_output(&output, target_ws_idx, activate);
            }
            Op::MoveWorkspaceDown => layout.move_workspace_down(),
            Op::MoveWorkspaceUp => layout.move_workspace_up(),
            Op::MoveWorkspaceToIndex {
                ws_name: Some(ws_name),
                target_idx,
            } => {
                let MonitorSet::Normal { monitors, .. } = &mut layout.monitor_set else {
                    return;
                };

                let Some((old_idx, old_output)) = monitors.iter().find_map(|monitor| {
                    monitor
                        .workspaces
                        .iter()
                        .enumerate()
                        .find_map(|(i, ws)| {
                            if ws.name == Some(format!("ws{ws_name}")) {
                                Some(i)
                            } else {
                                None
                            }
                        })
                        .map(|i| (i, monitor.output.clone()))
                }) else {
                    return;
                };

                layout.move_workspace_to_idx(Some((Some(old_output), old_idx)), target_idx)
            }
            Op::MoveWorkspaceToIndex {
                ws_name: None,
                target_idx,
            } => layout.move_workspace_to_idx(None, target_idx),
            Op::MoveWorkspaceToMonitor {
                ws_name: None,
                output_id: id,
            } => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };
                layout.move_workspace_to_output(&output);
            }
            Op::MoveWorkspaceToMonitor {
                ws_name: Some(ws_name),
                output_id: id,
            } => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };
                let MonitorSet::Normal { monitors, .. } = &mut layout.monitor_set else {
                    return;
                };

                let Some((old_idx, old_output)) = monitors.iter().find_map(|monitor| {
                    monitor
                        .workspaces
                        .iter()
                        .enumerate()
                        .find_map(|(i, ws)| {
                            if ws.name == Some(format!("ws{ws_name}")) {
                                Some(i)
                            } else {
                                None
                            }
                        })
                        .map(|i| (i, monitor.output.clone()))
                }) else {
                    return;
                };

                layout.move_workspace_to_output_by_id(old_idx, Some(old_output), &output);
            }
            Op::SwitchPresetColumnWidth => layout.toggle_width(true),
            Op::SwitchPresetColumnWidthBack => layout.toggle_width(false),
            Op::SwitchPresetWindowWidth { id } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.toggle_window_width(id.as_ref(), true);
            }
            Op::SwitchPresetWindowWidthBack { id } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.toggle_window_width(id.as_ref(), false);
            }
            Op::SwitchPresetWindowHeight { id } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.toggle_window_height(id.as_ref(), true);
            }
            Op::SwitchPresetWindowHeightBack { id } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.toggle_window_height(id.as_ref(), false);
            }
            Op::MaximizeColumn => layout.toggle_full_width(),
            Op::MaximizeWindowToEdges { id } => {
                let id = id.or_else(|| layout.focus().map(|win| *win.id()));
                let Some(id) = id else {
                    return;
                };
                if !layout.has_window(&id) {
                    return;
                }
                layout.toggle_maximized(&id);
            }
            Op::SetColumnWidth(change) => layout.set_column_width(change),
            Op::SetWindowWidth { id, change } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.set_window_width(id.as_ref(), change);
            }
            Op::SetWindowHeight { id, change } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.set_window_height(id.as_ref(), change);
            }
            Op::ResetWindowHeight { id } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.reset_window_height(id.as_ref());
            }
            Op::ExpandColumnToAvailableWidth => layout.expand_column_to_available_width(),
            Op::ToggleWindowFloating { id } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.toggle_window_floating(id.as_ref());
            }
            Op::SetWindowFloating { id, floating } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.set_window_floating(id.as_ref(), floating);
            }
            Op::FocusFloating => {
                layout.focus_floating();
            }
            Op::FocusTiling => {
                layout.focus_tiling();
            }
            Op::SwitchFocusFloatingTiling => {
                layout.switch_focus_floating_tiling();
            }
            Op::MoveFloatingWindow { id, x, y, animate } => {
                let id = id.filter(|id| layout.has_window(id));
                layout.move_floating_window(id.as_ref(), x, y, animate);
            }
            Op::SetParent {
                id,
                mut new_parent_id,
            } => {
                if !layout.has_window(&id) {
                    return;
                }

                if let Some(parent_id) = new_parent_id {
                    if parent_id_causes_loop(layout, id, parent_id) {
                        new_parent_id = None;
                    }
                }

                let mut update = false;

                if let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move {
                    if move_.tile.window().0.id == id {
                        move_.tile.window().0.parent_id.set(new_parent_id);
                        update = true;
                    }
                }

                match &mut layout.monitor_set {
                    MonitorSet::Normal { monitors, .. } => {
                        'outer: for mon in monitors {
                            for ws in &mut mon.workspaces {
                                for win in ws.windows() {
                                    if win.0.id == id {
                                        win.0.parent_id.set(new_parent_id);
                                        update = true;
                                        break 'outer;
                                    }
                                }
                            }
                        }
                    }
                    MonitorSet::NoOutputs { workspaces, .. } => {
                        'outer: for ws in workspaces {
                            for win in ws.windows() {
                                if win.0.id == id {
                                    win.0.parent_id.set(new_parent_id);
                                    update = true;
                                    break 'outer;
                                }
                            }
                        }
                    }
                }

                if update {
                    if let Some(new_parent_id) = new_parent_id {
                        layout.descendants_added(&new_parent_id);
                    }
                }
            }
            Op::SetForcedSize { id, size } => {
                for (_mon, win) in layout.windows() {
                    if win.0.id == id {
                        win.0.forced_size.set(size);
                        return;
                    }
                }
            }
            Op::Communicate(id) => {
                let mut update = false;

                if let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move {
                    if move_.tile.window().0.id == id {
                        if move_.tile.window().communicate() {
                            update = true;
                        }

                        if update {
                            // FIXME: serial.
                            layout.update_window(&id, None);
                        }
                        return;
                    }
                }

                match &mut layout.monitor_set {
                    MonitorSet::Normal { monitors, .. } => {
                        'outer: for mon in monitors {
                            for ws in &mut mon.workspaces {
                                for win in ws.windows() {
                                    if win.0.id == id {
                                        if win.communicate() {
                                            update = true;
                                        }
                                        break 'outer;
                                    }
                                }
                            }
                        }
                    }
                    MonitorSet::NoOutputs { workspaces, .. } => {
                        'outer: for ws in workspaces {
                            for win in ws.windows() {
                                if win.0.id == id {
                                    if win.communicate() {
                                        update = true;
                                    }
                                    break 'outer;
                                }
                            }
                        }
                    }
                }

                if update {
                    // FIXME: serial.
                    layout.update_window(&id, None);
                }
            }
            Op::Refresh { is_active } => {
                layout.refresh(is_active);
            }
            Op::AdvanceAnimations { msec_delta } => {
                let mut now = layout.clock.now_unadjusted();
                if msec_delta >= 0 {
                    now = now.saturating_add(Duration::from_millis(msec_delta as u64));
                } else {
                    now = now.saturating_sub(Duration::from_millis(-msec_delta as u64));
                }
                layout.clock.set_unadjusted(now);
                layout.advance_animations();
            }
            Op::CompleteAnimations => {
                layout.clock.set_complete_instantly(true);
                layout.advance_animations();
                layout.clock.set_complete_instantly(false);
            }
            Op::MoveWorkspaceToOutput(id) => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.move_workspace_to_output(&output);
            }
            Op::ViewOffsetGestureBegin {
                output_idx: id,
                workspace_idx,
                is_touchpad: normalize,
            } => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.view_offset_gesture_begin(&output, workspace_idx, normalize);
            }
            Op::ViewOffsetGestureUpdate {
                delta,
                timestamp,
                is_touchpad,
            } => {
                layout.view_offset_gesture_update(delta, timestamp, is_touchpad);
            }
            Op::ViewOffsetGestureEnd { is_touchpad } => {
                layout.view_offset_gesture_end(is_touchpad);
            }
            Op::WorkspaceSwitchGestureBegin {
                output_idx: id,
                is_touchpad,
            } => {
                let name = format!("output{id}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };

                layout.workspace_switch_gesture_begin(&output, is_touchpad);
            }
            Op::WorkspaceSwitchGestureUpdate {
                delta,
                timestamp,
                is_touchpad,
            } => {
                layout.workspace_switch_gesture_update(delta, timestamp, is_touchpad);
            }
            Op::WorkspaceSwitchGestureEnd { is_touchpad } => {
                layout.workspace_switch_gesture_end(is_touchpad);
            }
            Op::OverviewGestureBegin => {
                layout.overview_gesture_begin();
            }
            Op::OverviewGestureUpdate { delta, timestamp } => {
                layout.overview_gesture_update(delta, timestamp);
            }
            Op::OverviewGestureEnd => {
                layout.overview_gesture_end();
            }
            Op::InteractiveMoveBegin {
                window,
                output_idx,
                px,
                py,
            } => {
                let name = format!("output{output_idx}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };
                layout.interactive_move_begin(window, &output, Point::from((px, py)));
            }
            Op::InteractiveMoveUpdate {
                window,
                dx,
                dy,
                output_idx,
                px,
                py,
            } => {
                let name = format!("output{output_idx}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };
                layout.interactive_move_update(
                    &window,
                    Point::from((dx, dy)),
                    output,
                    Point::from((px, py)),
                );
            }
            Op::InteractiveMoveEnd { window } => {
                layout.interactive_move_end(&window);
            }
            Op::DndUpdate { output_idx, px, py } => {
                let name = format!("output{output_idx}");
                let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                    return;
                };
                layout.dnd_update(output, Point::from((px, py)));
            }
            Op::DndEnd => {
                layout.dnd_end();
            }
            Op::InteractiveResizeBegin { window, edges } => {
                layout.interactive_resize_begin(window, edges);
            }
            Op::InteractiveResizeUpdate { window, dx, dy } => {
                layout.interactive_resize_update(&window, Point::from((dx, dy)));
            }
            Op::InteractiveResizeEnd { window } => {
                layout.interactive_resize_end(&window);
            }
            Op::ToggleOverview => {
                layout.toggle_overview();
            }
            Op::UpdateConfig { layout_config } => {
                let options = Options {
                    layout: niri_config::Layout::from_part(&layout_config),
                    ..Default::default()
                };

                layout.update_options(options);
            }
        }
    }
}

#[track_caller]
fn check_ops_on_layout(layout: &mut Layout<TestWindow>, ops: impl IntoIterator<Item = Op>) {
    for op in ops {
        op.apply(layout);
        layout.verify_invariants();
    }
}

#[track_caller]
fn check_ops(ops: impl IntoIterator<Item = Op>) -> Layout<TestWindow> {
    let mut layout = Layout::default();
    check_ops_on_layout(&mut layout, ops);
    layout
}

fn workspace_output_name(layout: &Layout<TestWindow>, ws_name: usize) -> &str {
    layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.name() == Some(&format!("ws{ws_name}")))
        .and_then(|(monitor, _, _)| monitor)
        .unwrap()
        .output_name()
}

#[test]
fn workspace_output_fallback_promotes_preferred_in_reverse_connection_order() {
    // output3 is not a candidate, so it stays the primary monitor throughout; every assertion
    // below can only hold because of the candidate list.
    let mut layout = Layout::default();
    check_ops_on_layout(
        &mut layout,
        [
            Op::AddOutput(3),
            Op::AddNamedWorkspace {
                ws_name: 1,
                output_names: vec![1, 2],
                layout_config: None,
            },
        ],
    );
    assert_eq!(workspace_output_name(&layout, 1), "output3");

    // The second candidate connects first; the workspace must leave the primary monitor.
    check_ops_on_layout(&mut layout, [Op::AddOutput(2)]);
    assert_eq!(workspace_output_name(&layout, 1), "output2");

    // The first candidate connects last; it still wins.
    check_ops_on_layout(&mut layout, [Op::AddOutput(1)]);
    assert_eq!(workspace_output_name(&layout, 1), "output1");
}

#[test]
fn workspace_output_preferred_removal_uses_fallback() {
    let mut layout = Layout::default();
    check_ops_on_layout(
        &mut layout,
        [
            Op::AddOutput(3),
            Op::AddNamedWorkspace {
                ws_name: 1,
                output_names: vec![1, 2],
                layout_config: None,
            },
            Op::AddOutput(1),
            Op::AddOutput(2),
        ],
    );
    assert_eq!(workspace_output_name(&layout, 1), "output1");

    // The preferred candidate disconnects: the next candidate wins over the primary monitor.
    check_ops_on_layout(&mut layout, [Op::RemoveOutput(1)]);
    assert_eq!(workspace_output_name(&layout, 1), "output2");

    // No candidate left: fall back to the primary monitor.
    check_ops_on_layout(&mut layout, [Op::RemoveOutput(2)]);
    assert_eq!(workspace_output_name(&layout, 1), "output3");
}

#[test]
fn workspace_output_without_match_stays_on_default_output() {
    let mut layout = Layout::default();
    check_ops_on_layout(
        &mut layout,
        [
            Op::AddOutput(3),
            Op::AddNamedWorkspace {
                ws_name: 1,
                output_names: vec![1, 2],
                layout_config: None,
            },
        ],
    );
    assert_eq!(workspace_output_name(&layout, 1), "output3");

    // An output that is not in the candidate list must not take the workspace.
    check_ops_on_layout(&mut layout, [Op::AddOutput(4)]);
    assert_eq!(workspace_output_name(&layout, 1), "output3");
}

#[test]
fn explicit_workspace_move_overrides_output_candidates() {
    let mut layout = Layout::default();
    check_ops_on_layout(
        &mut layout,
        [
            Op::AddOutput(3),
            Op::AddNamedWorkspace {
                ws_name: 1,
                output_names: vec![1, 2],
                layout_config: None,
            },
            Op::AddOutput(2),
        ],
    );
    assert_eq!(workspace_output_name(&layout, 1), "output2");

    // After an explicit move, the candidate list no longer applies, even when the preferred
    // candidate connects afterwards.
    check_ops_on_layout(
        &mut layout,
        [
            Op::MoveWorkspaceToMonitor {
                ws_name: Some(1),
                output_id: 3,
            },
            Op::AddOutput(1),
        ],
    );
    assert_eq!(workspace_output_name(&layout, 1), "output3");
}

#[track_caller]
fn check_ops_with_options(
    options: Options,
    ops: impl IntoIterator<Item = Op>,
) -> Layout<TestWindow> {
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    check_ops_on_layout(&mut layout, ops);
    layout
}

fn scrolling_position_for_window(ws: &Workspace<TestWindow>, id: usize) -> Option<(usize, usize)> {
    ws.scrolling()
        .columns()
        .enumerate()
        .find_map(|(col_idx, col)| {
            col.tiles().enumerate().find_map(|(tile_idx, (tile, _))| {
                (*tile.window().id() == id).then_some((col_idx, tile_idx))
            })
        })
}

#[test]
fn sticky_focus_switches_between_tiling_and_sticky() {
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
    ]);
    layout.toggle_window_sticky(Some(&2));
    assert_eq!(
        layout
            .active_monitor()
            .unwrap()
            .active_window()
            .unwrap()
            .id(),
        &2
    );

    layout.switch_focus_floating_tiling();
    assert_eq!(
        layout
            .active_monitor()
            .unwrap()
            .active_window()
            .unwrap()
            .id(),
        &1
    );
    layout.switch_focus_floating_tiling();
    assert_eq!(
        layout
            .active_monitor()
            .unwrap()
            .active_window()
            .unwrap()
            .id(),
        &2
    );

    layout.refresh(true);
    let monitor = layout.active_monitor().unwrap();
    assert!(monitor
        .sticky
        .active_window()
        .unwrap()
        .0
        .pending_activated
        .get());
    assert!(!monitor
        .active_workspace_ref()
        .active_window()
        .unwrap()
        .0
        .pending_activated
        .get());

    layout.focus_tiling();
    assert_eq!(
        layout
            .active_monitor()
            .unwrap()
            .active_window()
            .unwrap()
            .id(),
        &1
    );
    layout.focus_floating();
    assert_eq!(
        layout
            .active_monitor()
            .unwrap()
            .active_window()
            .unwrap()
            .id(),
        &2
    );
    layout.verify_invariants();
}

#[test]
fn sticky_center_actions_target_sticky_window() {
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ]);
    layout.toggle_window_sticky(Some(&1));
    for action in 0..3 {
        layout.move_floating_window(
            None,
            PositionChange::SetFixed(0.),
            PositionChange::SetFixed(0.),
            false,
        );
        let before = layout
            .active_monitor()
            .unwrap()
            .sticky
            .tiles_with_render_positions()
            .next()
            .unwrap()
            .1;
        match action {
            0 => layout.center_column(),
            1 => layout.center_window(None),
            _ => layout.center_window(Some(&1)),
        }
        layout.clock.set_complete_instantly(true);
        layout.advance_animations();
        layout.clock.set_complete_instantly(false);
        let after = layout
            .active_monitor()
            .unwrap()
            .sticky
            .tiles_with_render_positions()
            .next()
            .unwrap()
            .1;
        assert_ne!(before, after);
        layout.verify_invariants();
    }
}

#[test]
fn sticky_focus_stays_when_no_tiled_window_exists() {
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ]);
    layout.toggle_window_sticky(Some(&1));
    layout.focus_tiling();
    layout.switch_focus_floating_tiling();
    assert_eq!(
        layout
            .active_monitor()
            .unwrap()
            .active_window()
            .unwrap()
            .id(),
        &1
    );
    layout.verify_invariants();
}

#[test]
fn toggle_sticky_restores_tiling_window_to_column() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ConsumeOrExpelWindowLeft { id: None },
    ];

    let mut layout = check_ops(ops);

    let original_ws_id = layout.active_workspace().unwrap().id();
    let original_pos =
        scrolling_position_for_window(layout.active_workspace().unwrap(), 2).unwrap();

    layout.toggle_window_sticky(Some(&2));
    layout.verify_invariants();
    assert!(layout.is_sticky_window(&2));

    layout.toggle_window_sticky(Some(&2));
    layout.verify_invariants();
    assert!(!layout.is_sticky_window(&2));

    let (ws_id, is_floating, pos) = match &layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => monitors
            .iter()
            .flat_map(|mon| mon.workspaces.iter())
            .find_map(|ws| {
                ws.has_window(&2).then(|| {
                    (
                        ws.id(),
                        ws.is_floating(&2),
                        scrolling_position_for_window(ws, 2),
                    )
                })
            })
            .unwrap(),
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };

    assert_eq!(ws_id, original_ws_id);
    assert!(!is_floating);
    assert_eq!(pos, Some(original_pos));
}

#[test]
fn toggle_sticky_restores_window_to_original_workspace() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ];

    let mut layout = check_ops(ops);
    let original_ws_id = layout.active_workspace().unwrap().id();

    layout.toggle_window_sticky(Some(&1));
    layout.verify_invariants();
    assert!(layout.is_sticky_window(&1));

    layout.switch_workspace_down();
    layout.verify_invariants();
    assert_ne!(layout.active_workspace().unwrap().id(), original_ws_id);

    layout.toggle_window_sticky(Some(&1));
    layout.verify_invariants();
    assert!(!layout.is_sticky_window(&1));
    assert_eq!(layout.active_workspace().unwrap().id(), original_ws_id);
}

#[test]
fn workspace_switch_gesture_clamps_to_visible_region() {
    // Minimized from a CI proptest run: hide the active (named) workspace, then a touchpad
    // workspace-switch gesture with a large delta — the gesture must clamp to the visible
    // region, not scroll into the hidden block.
    check_ops([
        Op::AddOutput(3),
        Op::SetWorkspaceName {
            new_ws_name: 5,
            ws_name: None,
        },
        Op::ToggleWorkspaceVisibility(5),
        Op::WorkspaceSwitchGestureBegin {
            output_idx: 3,
            is_touchpad: true,
        },
        Op::WorkspaceSwitchGestureUpdate {
            delta: 331.1446917951993,
            timestamp: Duration::ZERO,
            is_touchpad: true,
        },
        Op::WorkspaceSwitchGestureEnd { is_touchpad: None },
    ]);
}

#[test]
fn move_window_to_hidden_workspace_with_empty_above_first() {
    // Minimized from proptest: with empty-workspace-above-first, moving the active window
    // into the hidden workspace and focusing down must not leave the active workspace inside
    // the hidden block.
    check_ops([
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![],
            layout_config: None,
        },
        Op::UpdateConfig {
            layout_config: Box::new(niri_config::LayoutPart {
                empty_workspace_above_first: Some(niri_config::utils::Flag(true)),
                ..Default::default()
            }),
        },
        Op::AddWindowToNamedWorkspace {
            params: TestWindowParams::new(1),
            ws_name: 1,
        },
        Op::AddOutput(1),
        Op::MoveWindowToWorkspaceUp(false),
        Op::ToggleWorkspaceVisibility(1),
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 3,
            allow_hidden: true,
        },
        Op::FocusWorkspaceDown,
    ]);
}

#[test]
fn dnd_scroll_workspace_down_skips_hidden() {
    // Minimized from proptest: during a DnD workspace scroll, switching down clamps to the
    // last workspace in the list — which must be the last visible one, not the hidden block.
    check_ops([
        Op::OverviewGestureBegin,
        Op::AddOutput(5),
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![],
            layout_config: None,
        },
        Op::ToggleWorkspaceVisibility(1),
        Op::DndUpdate {
            output_idx: 5,
            px: 0.0,
            py: 0.0,
        },
        Op::FocusWorkspaceDown,
    ]);
}

#[test]
fn focus_after_column_move_unhides_workspace() {
    // Minimized from proptest: moving a column into a hidden workspace force-unhides it and
    // starts a switch animation; focusing again must not leave the switch animation pointing
    // past the end of the workspace list.
    check_ops([
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![],
            layout_config: None,
        },
        Op::AddOutput(1),
        Op::ToggleWorkspaceVisibility(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MoveColumnToWorkspace(2, true, true),
        Op::FocusWorkspace(2),
    ]);
}

#[test]
fn hide_unhide_workspace_by_name() {
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
        Op::FocusWorkspaceDown,
    ]);

    fn ws1_state(layout: &Layout<TestWindow>) -> Option<(bool, bool)> {
        layout.workspaces().find_map(|(_, _, ws)| {
            (ws.name() == Some(&"ws1".to_string())).then_some((ws.hidden, ws.needs_hidden))
        })
    }

    assert_eq!(layout.hide_workspace("nope"), None);
    assert_eq!(layout.unhide_workspace("nope"), None);
    assert_eq!(layout.toggle_workspace_visibility("nope".to_string()), None);

    assert_eq!(layout.hide_workspace("ws1"), Some(true));
    layout.verify_invariants();
    assert_eq!(ws1_state(&layout), Some((true, false)));

    // Hiding again is a no-op.
    assert_eq!(layout.hide_workspace("ws1"), Some(false));
    layout.verify_invariants();
    assert_eq!(ws1_state(&layout), Some((true, false)));

    assert_eq!(layout.unhide_workspace("ws1"), Some(true));
    layout.verify_invariants();
    assert_eq!(ws1_state(&layout), Some((false, false)));

    assert_eq!(layout.unhide_workspace("ws1"), Some(false));
    layout.verify_invariants();

    // Toggle reports the new visibility.
    assert_eq!(
        layout.toggle_workspace_visibility("ws1".to_string()),
        Some(false)
    );
    layout.verify_invariants();
    assert_eq!(ws1_state(&layout), Some((true, false)));
    assert_eq!(
        layout.toggle_workspace_visibility("ws1".to_string()),
        Some(true)
    );
    layout.verify_invariants();
    assert_eq!(ws1_state(&layout), Some((false, false)));

    // Focusing a hidden workspace by name shows it temporarily; an explicit unhide makes
    // that permanent.
    assert_eq!(layout.hide_workspace("ws1"), Some(true));
    let idx = layout
        .workspaces()
        .position(|(_, _, ws)| ws.name() == Some(&"ws1".to_string()))
        .unwrap();
    layout.switch_workspace(idx, true);
    layout.verify_invariants();
    assert_eq!(ws1_state(&layout), Some((false, true)));
    assert_eq!(layout.unhide_workspace("ws1"), Some(true));
    layout.verify_invariants();
    assert_eq!(ws1_state(&layout), Some((false, false)));
}

#[test]
fn unsticky_restores_into_hidden_origin_workspace() {
    // A sticky window whose origin workspace was hidden in the meantime: unsticky restores
    // it there, which must not activate the hidden workspace in place.
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
    ]);

    layout.toggle_window_sticky(Some(&1));
    layout.verify_invariants();
    assert!(layout.is_sticky_window(&1));

    layout.toggle_workspace_visibility("ws1".to_string());
    layout.verify_invariants();

    layout.toggle_window_sticky(Some(&1));
    layout.verify_invariants();
    assert!(!layout.is_sticky_window(&1));
}

#[test]
fn verify_invariants_accepts_hidden_workspaces() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::SetWorkspaceName {
            new_ws_name: 2,
            ws_name: None,
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::SetWorkspaceName {
            new_ws_name: 3,
            ws_name: None,
        },
    ];
    let mut layout = check_ops(ops);

    layout.toggle_workspace_visibility("ws3".to_string());
    layout.verify_invariants();
    layout.toggle_workspace_visibility("ws2".to_string());
    layout.verify_invariants();
    layout.toggle_workspace_visibility("ws3".to_string());
    layout.verify_invariants();
}

#[test]
fn move_column_into_hidden_workspace() {
    // Minimized from proptest: move-column-to-workspace targeting a hidden workspace index
    // must not activate the hidden workspace in place.
    check_ops([
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddNamedWorkspace {
            ws_name: 4,
            output_names: vec![],
            layout_config: None,
        },
        Op::AddOutput(1),
        Op::ToggleWorkspaceVisibility(4),
        Op::MoveColumnToWorkspace(2, true, true),
    ]);
}

#[test]
fn move_column_to_workspace_index_past_visible_end_stays_visible() {
    // https://github.com/barrulus/biri/issues/31: a workspace index past the end of the
    // visible region must clamp to the last visible workspace. Clamping against the whole
    // workspace vec would land the column on the first hidden workspace instead (and
    // force-unhide it, since focus follows).
    let layout = check_ops([
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![],
            layout_config: None,
        },
        Op::AddOutput(1),
        Op::ToggleWorkspaceVisibility(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MoveColumnToWorkspace(4, true, false),
    ]);

    assert_window_stayed_visible(&layout);
}

#[test]
fn move_window_to_workspace_index_past_visible_end_stays_visible() {
    // Same as above for move-window-to-workspace.
    let layout = check_ops([
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![],
            layout_config: None,
        },
        Op::AddOutput(1),
        Op::ToggleWorkspaceVisibility(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MoveWindowToWorkspace {
            window_id: Some(1),
            workspace_idx: 4,
            allow_hidden: false,
        },
    ]);

    assert_window_stayed_visible(&layout);
}

fn assert_window_stayed_visible(layout: &Layout<TestWindow>) {
    let (_, _, ws) = layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(&1))
        .expect("window 1 must still exist");
    assert!(
        !ws.hidden,
        "window must not land in the hidden block: index-based moves address \
         the visible region"
    );

    let hidden = layout
        .workspaces()
        .find_map(|(_, _, ws)| (ws.name() == Some(&"ws1".to_string())).then_some(ws.hidden));
    assert_eq!(hidden, Some(true), "ws1 must remain hidden");
}

#[test]
fn move_window_down_from_last_visible_workspace_stays_visible() {
    // Relative moves clamp to the visible region: moving the window down from the last
    // visible workspace must not target the hidden block (not even by unhiding it).
    let layout = check_ops([
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![],
            layout_config: None,
        },
        Op::AddOutput(1),
        Op::ToggleWorkspaceVisibility(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MoveWindowToWorkspaceDown(true),
        Op::MoveWindowToWorkspaceDown(true),
        Op::MoveColumnToWorkspaceDown(true),
    ]);

    let hidden = layout
        .workspaces()
        .find_map(|(_, _, ws)| (ws.name() == Some(&"ws1".to_string())).then_some(ws.hidden));
    assert_eq!(hidden, Some(true), "ws1 must remain hidden");
}

#[test]
fn interactive_move_drop_below_workspaces_with_one_hidden() {
    // Dropping an interactively-moved window below all visible workspaces resolves to "reuse
    // the bottom empty workspace" — which must be the last visible one, not a hidden one at
    // the end of the list.
    check_ops([
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![],
            layout_config: None,
        },
        Op::AddOutput(1),
        Op::ToggleWorkspaceVisibility(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleOverview,
        Op::InteractiveMoveBegin {
            window: 1,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::InteractiveMoveUpdate {
            window: 1,
            dx: 0.,
            dy: 5000.,
            output_idx: 1,
            px: 0.,
            py: 5000.,
        },
        Op::InteractiveMoveEnd { window: 1 },
    ]);
}

#[test]
fn move_window_to_output_into_hidden_workspace() {
    // Minimized from proptest: move-window-to-output targeting a hidden workspace index on
    // the same output must not activate the hidden workspace in place.
    check_ops([
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![],
            layout_config: None,
        },
        Op::AddOutput(5),
        Op::ToggleWorkspaceVisibility(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MoveWindowToOutput {
            window_id: None,
            output_id: 5,
            target_ws_idx: Some(2),
        },
    ]);
}

#[test]
fn move_window_into_hidden_workspace_then_move_workspace_down() {
    // Minimized from proptest: moving a window into a hidden workspace by index and then
    // moving the active workspace down must keep the hidden block contiguous at the end.
    check_ops([
        Op::AddNamedWorkspace {
            ws_name: 2,
            output_names: vec![],
            layout_config: None,
        },
        Op::AddOutput(1),
        Op::ToggleWorkspaceVisibility(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 2,
            allow_hidden: true,
        },
        Op::MoveWorkspaceDown,
    ]);
}

#[test]
fn move_last_visible_workspace_away_from_hidden_block() {
    // Minimized from proptest: moving the active workspace to another monitor when the
    // source monitor's only other workspace is hidden must not underflow in
    // clean_up_workspaces (first hidden index 0) or leave the hidden block mid-list.
    check_ops([
        Op::AddNamedWorkspace {
            ws_name: 4,
            output_names: vec![],
            layout_config: None,
        },
        Op::AddOutput(1),
        Op::AddOutput(3),
        Op::ToggleWorkspaceVisibility(4),
        Op::MoveWorkspaceToMonitor {
            ws_name: None,
            output_id: 3,
        },
    ]);
}

#[test]
fn add_output_during_overview_gesture_with_hidden_workspace() {
    // Minimized from proptest: adding an output while an overview gesture (and its DnD
    // workspace switch) is active must not trip clean_up_workspaces' workspace_switch assert.
    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(
        options,
        [
            Op::AddNamedWorkspace {
                ws_name: 2,
                output_names: vec![],
                layout_config: None,
            },
            Op::AddOutput(1),
            Op::OverviewGestureBegin,
            Op::ToggleWorkspaceVisibility(2),
            Op::DndUpdate {
                output_idx: 1,
                px: 0.0,
                py: 0.0,
            },
            Op::AddOutput(2),
        ],
    );
}

#[test]
fn clean_up_workspaces_skips_two_workspace_collapse_with_hidden() {
    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
    ];
    // [emptyTop, "ws1"(win1), empty]
    let mut layout = check_ops_with_options(options, ops);

    // Hiding "ws1" collapses the two remaining empties and re-adds the hidden
    // one at the end: [emptyTop, "ws1"(hidden)].
    layout.toggle_workspace_visibility("ws1".to_string());
    layout.verify_invariants();

    // A later cleanup (e.g. after a workspace-switch gesture ends) must not
    // assert on the legal [empty, hidden-named] state.
    let monitor = match &mut layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => &mut monitors[0],
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    monitor.clean_up_workspaces();
    layout.verify_invariants();
}

#[test]
fn close_window_in_hidden_workspace_with_empty_workspace_above_first() {
    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
    ];
    // [emptyTop, "ws1"(win1), empty]
    let mut layout = check_ops_with_options(options, ops);

    // [emptyTop, "ws1"(hidden, win1)]
    layout.toggle_workspace_visibility("ws1".to_string());
    layout.verify_invariants();

    // Closing the hidden workspace's window reaches remove_window's own
    // two-workspace collapse; it must not assert on [empty, hidden-named].
    Op::CloseWindow(1).apply(&mut layout);
    layout.verify_invariants();
}

#[test]
fn move_hidden_workspace_to_other_output_keeps_it_hidden() {
    let ops = [
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
    ];
    let mut layout = check_ops(ops);

    layout.toggle_workspace_visibility("ws1".to_string());
    layout.verify_invariants();

    // Move the hidden workspace to output 2.
    let (old_idx, old_output, new_output) = match &layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => {
            let old_idx = monitors[0]
                .workspaces
                .iter()
                .position(|ws| ws.name.as_deref() == Some("ws1"))
                .unwrap();
            (
                old_idx,
                monitors[0].output.clone(),
                monitors[1].output.clone(),
            )
        }
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    layout.move_workspace_to_output_by_id(old_idx, Some(old_output), &new_output);
    layout.verify_invariants();

    // The workspace lives on monitor 1 now, still hidden, still reachable by name.
    let monitor = match &layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => &monitors[1],
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    let ws = monitor
        .workspaces
        .iter()
        .find(|ws| ws.name.as_deref() == Some("ws1"))
        .expect("moved workspace must be on the target monitor");
    assert!(ws.hidden, "moved workspace must stay hidden");

    layout.toggle_workspace_visibility("ws1".to_string());
    layout.verify_invariants();
}

#[test]
fn output_disconnect_preserves_hidden_blocks() {
    let ops = [
        Op::AddOutput(1),
        Op::AddOutput(2),
        // Window + named workspace on output 1 (primary).
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
        // Window + named workspace on output 2.
        Op::FocusOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::SetWorkspaceName {
            new_ws_name: 2,
            ws_name: None,
        },
        // A visible workspace on output 2 too, so the incoming set mixes
        // visible and hidden workspaces.
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
    ];
    let mut layout = check_ops(ops);
    layout.toggle_workspace_visibility("ws1".to_string());
    layout.toggle_workspace_visibility("ws2".to_string());
    layout.verify_invariants();

    // Disconnect output 2; its workspaces (incl. the hidden one) land on output 1.
    Op::RemoveOutput(2).apply(&mut layout);
    layout.verify_invariants();

    // Both hidden workspaces must be reachable again.
    layout.toggle_workspace_visibility("ws1".to_string());
    layout.verify_invariants();
    layout.toggle_workspace_visibility("ws2".to_string());
    layout.verify_invariants();
}

#[test]
fn naming_workspace_on_non_active_monitor_compensates_there() {
    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let ops = [Op::AddOutput(1), Op::AddOutput(2)];
    // Focus stays on output 1 (the active monitor).
    let mut layout = check_ops_with_options(options, ops);

    // Name the FIRST workspace of the non-active monitor by id. The compensating
    // empty workspace must be added on that monitor, not the active one.
    let wsid = match &layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => monitors[1].workspaces[0].id(),
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    layout.set_workspace_name("ws1".to_string(), Some(WorkspaceReference::Id(wsid.get())));
    layout.verify_invariants();
}

#[test]
fn hide_workspace_at_index_zero_hides_the_right_workspace() {
    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ];
    let mut layout = check_ops_with_options(options, ops);

    let monitor = match &mut layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => &mut monitors[0],
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    // Manufacture a named workspace at idx 0 (as a release build with the old
    // set_workspace_name defect could).
    monitor.workspaces[0].name = Some("ws1".to_string());

    monitor.hide_workspace_by_idx(0);

    // The named workspace must be the hidden one — not the freshly added empty.
    let hidden: Vec<_> = monitor.workspaces.iter().filter(|ws| ws.hidden).collect();
    assert_eq!(hidden.len(), 1);
    assert_eq!(hidden[0].name.as_deref(), Some("ws1"));
}

#[test]
fn move_workspace_down_stops_at_hidden_block() {
    // Minimized from proptest: moving the active workspace down must not swap
    // it into the hidden block.
    check_ops([
        Op::AddOutput(1),
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![],
            layout_config: None,
        },
        Op::ToggleWorkspaceVisibility(1),
        Op::MoveWorkspaceDown,
    ]);
}

#[test]
fn remove_last_output_without_windows_leaves_no_workspaces() {
    // check_ops runs verify_invariants after each op; removing the last output
    // with nothing on it must not leave an empty unnamed workspace in NoOutputs.
    check_ops([Op::AddOutput(1), Op::RemoveOutput(1)]);
}

#[test]
fn unhide_placement_survives_workspace_cleanup() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::SetWorkspaceName {
            new_ws_name: 3,
            ws_name: None,
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(4),
        },
    ];
    // [win1, win2, "ws3"(win3), win4, empty]
    let mut layout = check_ops(ops);

    // Hide "ws3" (records original_idx 2): [win1, win2, win4, empty, H].
    layout.toggle_workspace_visibility("ws3".to_string());
    layout.verify_invariants();

    // Close win1; its workspace empties out and gets cleaned up:
    // [win2, win4, empty, H]. The stored original_idx must follow (2 -> 1).
    Op::CloseWindow(1).apply(&mut layout);
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    layout.verify_invariants();

    // Unhide: "ws3" must reappear between win2 and win4, its original relative
    // position — not dumped at the end of the visible region.
    layout.toggle_workspace_visibility("ws3".to_string());
    layout.verify_invariants();

    let monitor = match &layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => &monitors[0],
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };
    let pos_ws3 = monitor
        .workspaces
        .iter()
        .position(|ws| ws.has_window(&3))
        .unwrap();
    let pos_win2 = monitor
        .workspaces
        .iter()
        .position(|ws| ws.has_window(&2))
        .unwrap();
    let pos_win4 = monitor
        .workspaces
        .iter()
        .position(|ws| ws.has_window(&4))
        .unwrap();
    assert!(
        pos_win2 < pos_ws3 && pos_ws3 < pos_win4,
        "unhidden workspace must return to its original relative position \
         (got win2={pos_win2}, ws3={pos_ws3}, win4={pos_win4})"
    );
}

#[test]
fn unhide_next_to_remaining_hidden_block_keeps_empty_workspace_before_it() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::SetWorkspaceName {
            new_ws_name: 2,
            ws_name: None,
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::SetWorkspaceName {
            new_ws_name: 3,
            ws_name: None,
        },
    ];

    // [win1, "ws2"(win2), "ws3"(win3), empty]
    let mut layout = check_ops(ops);

    // Hide "ws3" (records original_idx 2), then "ws2"; the hidden block now sits at
    // the end with an empty workspace right before it.
    layout.toggle_workspace_visibility("ws3".to_string());
    layout.toggle_workspace_visibility("ws2".to_string());

    // Unhiding "ws3" reinserts at original_idx 2, which is exactly where the hidden
    // block starts. It must land before the empty workspace guarding the block, not
    // between the empty workspace and the block.
    layout.toggle_workspace_visibility("ws3".to_string());

    let monitor = match &layout.monitor_set {
        MonitorSet::Normal { monitors, .. } => &monitors[0],
        MonitorSet::NoOutputs { .. } => unreachable!(),
    };

    assert!(
        monitor.workspaces.iter().any(|ws| ws.has_window(&3)),
        "unhidden workspace must still exist"
    );
    let first_hidden = monitor
        .workspaces
        .iter()
        .position(|ws| ws.hidden)
        .expect("\"ws2\" must still be hidden");
    assert!(first_hidden > 0);
    assert!(
        !monitor.workspaces[first_hidden - 1].has_windows_or_name(),
        "workspace before the hidden block must stay empty and unnamed"
    );
}

#[test]
fn vertical_main_axis_places_columns_vertically() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;

    let layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::AddWindow {
                params: TestWindowParams::new(2),
            },
        ],
    );

    let ws = layout.active_workspace().unwrap();
    let positions: Vec<_> = ws
        .tiles_with_render_positions()
        .map(|(_, pos, _)| pos)
        .collect();

    assert_eq!(positions.len(), 2);

    let dx = (positions[0].x - positions[1].x).abs();
    let dy = (positions[0].y - positions[1].y).abs();
    assert!(
        dy > dx,
        "expected vertical separation, got dx={dx}, dy={dy}"
    );
}

#[test]
fn vertical_main_axis_insert_position_follows_y() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;

    let layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::AddWindow {
                params: TestWindowParams::new(2),
            },
        ],
    );

    let ws = layout.active_workspace().unwrap();
    let mut centers: Vec<_> = ws
        .tiles_with_render_positions()
        .map(|(tile, pos, _)| {
            let size = tile.window().size().to_f64();
            Point::from((pos.x + size.w / 2., pos.y + size.h / 2.))
        })
        .collect();
    centers.sort_by(|a, b| a.y.total_cmp(&b.y));

    assert_eq!(centers.len(), 2);

    let insert_col_idx = |center| match ws.scrolling_insert_position(center) {
        super::monitor::InsertPosition::NewColumn(idx)
        | super::monitor::InsertPosition::InColumn(idx, _) => idx,
        super::monitor::InsertPosition::Floating => unreachable!(),
    };

    let upper_idx = insert_col_idx(centers[0]);
    let lower_idx = insert_col_idx(centers[1]);

    assert!(
        lower_idx > upper_idx,
        "expected insert position to progress with y, got upper={upper_idx}, lower={lower_idx}"
    );
}

#[test]
fn vertical_main_axis_dnd_edge_scroll_uses_vertical_edges() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;

    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::AddWindow {
                params: TestWindowParams::new(2),
            },
        ],
    );

    let ws = layout.active_workspace_mut().unwrap();
    let area = ws.working_area();

    ws.dnd_scroll_gesture_begin();

    let center =
        Point::<f64, Logical>::from((area.loc.x + area.size.w / 2., area.loc.y + area.size.h / 2.));
    let left = Point::from((area.loc.x + 1., center.y));
    let top = Point::from((center.x, area.loc.y + 1.));

    assert!(!ws.dnd_scroll_gesture_scroll(left, 1.));
    assert!(ws.dnd_scroll_gesture_scroll(top, 1.));
}

#[test]
fn vertical_main_axis_overview_places_workspaces_horizontally() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;

    let layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::FocusWorkspaceDown,
            Op::AddWindow {
                params: TestWindowParams::new(2),
            },
            Op::ToggleOverview,
        ],
    );

    let output = layout
        .outputs()
        .find(|output| output.name() == "output1")
        .cloned()
        .unwrap();
    let monitor = layout.monitor_for_output(&output).unwrap();

    let geos: Vec<_> = monitor.workspaces_render_geo().take(2).collect();
    assert_eq!(geos.len(), 2);

    let dx = (geos[0].loc.x - geos[1].loc.x).abs();
    let dy = (geos[0].loc.y - geos[1].loc.y).abs();
    assert!(
        dx > dy,
        "expected overview workspaces to be arranged horizontally, got dx={dx}, dy={dy}"
    );
}

#[test]
fn vertical_main_axis_set_column_width_changes_tile_height() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;

    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
        ],
    );

    let (_, win) = layout.windows().next().unwrap();
    let before = win.requested_size().unwrap();

    check_ops_on_layout(
        &mut layout,
        [Op::SetColumnWidth(SizeChange::AdjustProportion(5.))],
    );

    let (_, win) = layout.windows().next().unwrap();
    let after = win.requested_size().unwrap();

    assert_eq!(before.w, after.w);
    assert!(
        after.h > before.h,
        "expected height to grow: {before:?} -> {after:?}"
    );
}

#[test]
fn vertical_main_axis_interactive_resize_bottom_changes_tile_height() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;

    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
        ],
    );

    let (_, win) = layout.windows().next().unwrap();
    let before = win.requested_size().unwrap();

    check_ops_on_layout(
        &mut layout,
        [
            Op::InteractiveResizeBegin {
                window: 1,
                edges: ResizeEdge::BOTTOM,
            },
            Op::InteractiveResizeUpdate {
                window: 1,
                dx: 0.,
                dy: 120.,
            },
            Op::InteractiveResizeEnd { window: 1 },
        ],
    );

    let (_, win) = layout.windows().next().unwrap();
    let after = win.requested_size().unwrap();

    assert_eq!(before.w, after.w);
    assert!(
        after.h > before.h,
        "expected interactive resize to grow height: {before:?} -> {after:?}"
    );
}

#[test]
fn vertical_main_axis_interactive_move_tracks_pointer_along_y() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;

    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
        ],
    );

    let output = layout
        .outputs()
        .find(|o| o.name() == "output1")
        .cloned()
        .unwrap();

    let (tile_pos, start) = {
        let ws = layout.active_workspace().unwrap();
        let (tile, tile_pos, _) = ws
            .tiles_with_render_positions()
            .find(|(tile, _, _)| *tile.window().id() == 1)
            .unwrap();

        let start = tile_pos + tile.window_loc() + Point::from((10., 10.));
        (tile_pos, start)
    };

    assert!(layout.interactive_move_begin(1, &output, start));

    let delta = Point::from((0., 220.));
    let pointer_pos = start + delta;
    assert!(layout.interactive_move_update(&1, delta, output.clone(), pointer_pos));

    let tile_pos_after = {
        let ws = layout.active_workspace().unwrap();
        ws.tiles_with_render_positions()
            .find(|(tile, _, _)| *tile.window().id() == 1)
            .unwrap()
            .1
    };

    let moved_x = (tile_pos_after.x - tile_pos.x).abs();
    let moved_y = (tile_pos_after.y - tile_pos.y).abs();
    assert!(
        moved_y > moved_x,
        "expected move gesture to follow y in vertical mode, got dx={moved_x}, dy={moved_y}"
    );
}

#[test]
fn vertical_main_axis_floating_move_column_right_moves_window_right() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;

    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::ToggleWindowFloating { id: None },
        ],
    );

    let mut before = None;
    layout.with_windows(|win, _, _, layout| {
        if *win.id() == 1 {
            before = layout.tile_pos_in_workspace_view;
        }
    });
    let before = before.unwrap();

    check_ops_on_layout(&mut layout, [Op::MoveColumnRight]);

    let mut after = None;
    layout.with_windows(|win, _, _, layout| {
        if *win.id() == 1 {
            after = layout.tile_pos_in_workspace_view;
        }
    });
    let after = after.unwrap();

    let moved_x = after.0 - before.0;
    let moved_y = after.1 - before.1;
    assert!(
        moved_x > moved_y.abs(),
        "expected move-column-right to move floating window right in vertical mode, got dx={moved_x}, dy={moved_y}"
    );
}

#[test]
fn vertical_main_axis_floating_move_window_down_moves_window_down() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;

    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::ToggleWindowFloating { id: None },
        ],
    );

    let mut before = None;
    layout.with_windows(|win, _, _, layout| {
        if *win.id() == 1 {
            before = layout.tile_pos_in_workspace_view;
        }
    });
    let before = before.unwrap();

    check_ops_on_layout(&mut layout, [Op::MoveWindowDown]);

    let mut after = None;
    layout.with_windows(|win, _, _, layout| {
        if *win.id() == 1 {
            after = layout.tile_pos_in_workspace_view;
        }
    });
    let after = after.unwrap();

    let moved_x = after.0 - before.0;
    let moved_y = after.1 - before.1;
    assert!(
        moved_y > moved_x.abs(),
        "expected move-window-down to move floating window down in vertical mode, got dx={moved_x}, dy={moved_y}"
    );
}

#[test]
fn vertical_main_axis_floating_set_column_width_changes_window_height() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;

    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::ToggleWindowFloating { id: None },
        ],
    );

    let (_, win) = layout.windows().next().unwrap();
    let before = win.expected_size().unwrap();

    check_ops_on_layout(
        &mut layout,
        [Op::SetColumnWidth(SizeChange::AdjustProportion(5.))],
    );

    let (_, win) = layout.windows().next().unwrap();
    let after = win.expected_size().unwrap();

    assert_eq!(before.w, after.w);
    assert!(
        after.h > before.h,
        "expected floating column width to grow height in vertical mode: {before:?} -> {after:?}"
    );
}

#[test]
fn vertical_main_axis_floating_set_window_height_changes_window_width() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;

    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::ToggleWindowFloating { id: None },
        ],
    );

    let (_, win) = layout.windows().next().unwrap();
    let before = win.expected_size().unwrap();

    check_ops_on_layout(
        &mut layout,
        [Op::SetWindowHeight {
            id: None,
            change: SizeChange::AdjustProportion(5.),
        }],
    );

    let (_, win) = layout.windows().next().unwrap();
    let after = win.expected_size().unwrap();

    assert_eq!(before.h, after.h);
    assert!(
        after.w > before.w,
        "expected floating window height to grow width in vertical mode: {before:?} -> {after:?}"
    );
}

#[test]
fn scrolling_windows_have_ipc_tile_positions() {
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ]);

    let mut tile_pos = None;
    layout.with_windows(|win, _, _, layout| {
        if *win.id() == 1 {
            tile_pos = layout.tile_pos_in_workspace_view;
        }
    });

    assert!(tile_pos.is_some());
}

#[test]
fn operations_dont_panic() {
    if std::env::var_os("RUN_SLOW_TESTS").is_none() {
        eprintln!("ignoring slow test");
        return;
    }

    let every_op = [
        Op::AddOutput(0),
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::RemoveOutput(0),
        Op::RemoveOutput(1),
        Op::RemoveOutput(2),
        Op::FocusOutput(0),
        Op::FocusOutput(1),
        Op::FocusOutput(2),
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![1],
            layout_config: None,
        },
        Op::UnnameWorkspace { ws_name: 1 },
        Op::ToggleWorkspaceVisibility(1),
        Op::ToggleWorkspaceVisibility(2),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindowNextTo {
            params: TestWindowParams::new(2),
            next_to_id: 1,
        },
        Op::AddWindowToNamedWorkspace {
            params: TestWindowParams::new(3),
            ws_name: 1,
        },
        Op::CloseWindow(0),
        Op::CloseWindow(1),
        Op::CloseWindow(2),
        Op::FullscreenWindow(1),
        Op::FullscreenWindow(2),
        Op::FullscreenWindow(3),
        Op::MaximizeWindowToEdges { id: Some(1) },
        Op::MaximizeWindowToEdges { id: Some(2) },
        Op::MaximizeWindowToEdges { id: Some(3) },
        Op::FocusColumnLeft,
        Op::FocusColumnRight,
        Op::FocusColumnRightOrFirst,
        Op::FocusColumnLeftOrLast,
        Op::FocusWindowOrMonitorUp(0),
        Op::FocusWindowOrMonitorDown(1),
        Op::FocusColumnOrMonitorLeft(0),
        Op::FocusColumnOrMonitorRight(1),
        Op::FocusWindowUp,
        Op::FocusWindowUpOrColumnLeft,
        Op::FocusWindowUpOrColumnRight,
        Op::FocusWindowOrWorkspaceUp,
        Op::FocusWindowDown,
        Op::FocusWindowDownOrColumnLeft,
        Op::FocusWindowDownOrColumnRight,
        Op::FocusWindowOrWorkspaceDown,
        Op::MoveColumnLeft,
        Op::MoveColumnRight,
        Op::MoveColumnLeftOrToMonitorLeft(0),
        Op::MoveColumnRightOrToMonitorRight(1),
        Op::ConsumeWindowIntoColumn,
        Op::ExpelWindowFromColumn,
        Op::CenterColumn,
        Op::FocusWorkspaceDown,
        Op::FocusWorkspaceUp,
        Op::FocusWorkspace(1),
        Op::FocusWorkspace(2),
        Op::MoveWindowToWorkspaceDown(true),
        Op::MoveWindowToWorkspaceUp(true),
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 1,
            allow_hidden: true,
        },
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 2,
            allow_hidden: true,
        },
        Op::MoveColumnToWorkspaceDown(true),
        Op::MoveColumnToWorkspaceUp(true),
        Op::MoveColumnToWorkspace(1, true, true),
        Op::MoveColumnToWorkspace(2, true, true),
        Op::MoveWindowDown,
        Op::MoveWindowDownOrToWorkspaceDown,
        Op::MoveWindowUp,
        Op::MoveWindowUpOrToWorkspaceUp,
        Op::ConsumeOrExpelWindowLeft { id: None },
        Op::ConsumeOrExpelWindowRight { id: None },
        Op::MoveWorkspaceToOutput(1),
        Op::ToggleColumnTabbedDisplay,
    ];

    for third in &every_op {
        for second in &every_op {
            for first in &every_op {
                // eprintln!("{first:?}, {second:?}, {third:?}");

                let mut layout = Layout::default();
                first.clone().apply(&mut layout);
                layout.verify_invariants();
                second.clone().apply(&mut layout);
                layout.verify_invariants();
                third.clone().apply(&mut layout);
                layout.verify_invariants();
            }
        }
    }
}

#[test]
fn operations_from_starting_state_dont_panic() {
    if std::env::var_os("RUN_SLOW_TESTS").is_none() {
        eprintln!("ignoring slow test");
        return;
    }

    // Running every op from an empty state doesn't get us to all the interesting states. So,
    // also run it from a manually-created starting state with more things going on to exercise
    // more code paths.
    let setup_ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MoveWindowToWorkspaceDown(true),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::FocusColumnLeft,
        Op::ConsumeWindowIntoColumn,
        Op::AddWindow {
            params: TestWindowParams::new(4),
        },
        Op::AddOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(5),
        },
        Op::MoveWindowToOutput {
            window_id: None,
            output_id: 2,
            target_ws_idx: None,
        },
        Op::FocusOutput(1),
        Op::Communicate(1),
        Op::Communicate(2),
        Op::Communicate(3),
        Op::Communicate(4),
        Op::Communicate(5),
    ];

    let every_op = [
        Op::AddOutput(0),
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::RemoveOutput(0),
        Op::RemoveOutput(1),
        Op::RemoveOutput(2),
        Op::FocusOutput(0),
        Op::FocusOutput(1),
        Op::FocusOutput(2),
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![1],
            layout_config: None,
        },
        Op::UnnameWorkspace { ws_name: 1 },
        Op::ToggleWorkspaceVisibility(1),
        Op::ToggleWorkspaceVisibility(2),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddWindowNextTo {
            params: TestWindowParams::new(6),
            next_to_id: 0,
        },
        Op::AddWindowNextTo {
            params: TestWindowParams::new(7),
            next_to_id: 1,
        },
        Op::AddWindowToNamedWorkspace {
            params: TestWindowParams::new(5),
            ws_name: 1,
        },
        Op::CloseWindow(0),
        Op::CloseWindow(1),
        Op::CloseWindow(2),
        Op::FullscreenWindow(1),
        Op::FullscreenWindow(2),
        Op::FullscreenWindow(3),
        Op::MaximizeWindowToEdges { id: Some(1) },
        Op::MaximizeWindowToEdges { id: Some(2) },
        Op::MaximizeWindowToEdges { id: Some(3) },
        Op::SetFullscreenWindow {
            window: 1,
            is_fullscreen: false,
        },
        Op::SetFullscreenWindow {
            window: 1,
            is_fullscreen: true,
        },
        Op::SetFullscreenWindow {
            window: 2,
            is_fullscreen: false,
        },
        Op::SetFullscreenWindow {
            window: 2,
            is_fullscreen: true,
        },
        Op::FocusColumnLeft,
        Op::FocusColumnRight,
        Op::FocusColumnRightOrFirst,
        Op::FocusColumnLeftOrLast,
        Op::FocusWindowOrMonitorUp(0),
        Op::FocusWindowOrMonitorDown(1),
        Op::FocusColumnOrMonitorLeft(0),
        Op::FocusColumnOrMonitorRight(1),
        Op::FocusWindowUp,
        Op::FocusWindowUpOrColumnLeft,
        Op::FocusWindowUpOrColumnRight,
        Op::FocusWindowOrWorkspaceUp,
        Op::FocusWindowDown,
        Op::FocusWindowDownOrColumnLeft,
        Op::FocusWindowDownOrColumnRight,
        Op::FocusWindowOrWorkspaceDown,
        Op::MoveColumnLeft,
        Op::MoveColumnRight,
        Op::MoveColumnLeftOrToMonitorLeft(0),
        Op::MoveColumnRightOrToMonitorRight(1),
        Op::ConsumeWindowIntoColumn,
        Op::ExpelWindowFromColumn,
        Op::CenterColumn,
        Op::FocusWorkspaceDown,
        Op::FocusWorkspaceUp,
        Op::FocusWorkspace(1),
        Op::FocusWorkspace(2),
        Op::FocusWorkspace(3),
        Op::MoveWindowToWorkspaceDown(true),
        Op::MoveWindowToWorkspaceUp(true),
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 1,
            allow_hidden: true,
        },
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 2,
            allow_hidden: true,
        },
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 3,
            allow_hidden: true,
        },
        Op::MoveColumnToWorkspaceDown(true),
        Op::MoveColumnToWorkspaceUp(true),
        Op::MoveColumnToWorkspace(1, true, true),
        Op::MoveColumnToWorkspace(2, true, true),
        Op::MoveColumnToWorkspace(3, true, true),
        Op::MoveWindowDown,
        Op::MoveWindowDownOrToWorkspaceDown,
        Op::MoveWindowUp,
        Op::MoveWindowUpOrToWorkspaceUp,
        Op::ConsumeOrExpelWindowLeft { id: None },
        Op::ConsumeOrExpelWindowRight { id: None },
        Op::ToggleColumnTabbedDisplay,
    ];

    for third in &every_op {
        for second in &every_op {
            for first in &every_op {
                // eprintln!("{first:?}, {second:?}, {third:?}");

                let mut layout = Layout::default();
                for op in &setup_ops {
                    op.clone().apply(&mut layout);
                }

                let mut layout = Layout::default();
                first.clone().apply(&mut layout);
                layout.verify_invariants();
                second.clone().apply(&mut layout);
                layout.verify_invariants();
                third.clone().apply(&mut layout);
                layout.verify_invariants();
            }
        }
    }
}

#[test]
fn primary_active_workspace_idx_not_updated_on_output_add() {
    let ops = [
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::FocusOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::FocusOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::RemoveOutput(2),
        Op::FocusWorkspace(3),
        Op::AddOutput(2),
    ];

    check_ops(ops);
}

#[test]
fn window_closed_on_previous_workspace() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::FocusWorkspaceDown,
        Op::CloseWindow(0),
    ];

    check_ops(ops);
}

#[test]
fn removing_output_must_keep_empty_focus_on_primary() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    // The workspace from the removed output was inserted at position 0, so the active workspace
    // must change to 1 to keep the focus on the empty workspace.
    assert_eq!(monitors[0].active_workspace_idx, 1);
}

#[test]
fn move_to_workspace_by_idx_does_not_leave_empty_workspaces() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddOutput(2),
        Op::FocusOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::RemoveOutput(1),
        Op::MoveWindowToWorkspace {
            window_id: Some(0),
            workspace_idx: 2,
            allow_hidden: true,
        },
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert!(monitors[0].workspaces[1].has_windows());
}

#[test]
fn empty_workspaces_dont_move_back_to_original_output() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
        Op::FocusWorkspace(1),
        Op::CloseWindow(1),
        Op::AddOutput(1),
    ];

    check_ops(ops);
}

#[test]
fn named_workspaces_dont_update_original_output_on_adding_window() {
    let ops = [
        Op::AddOutput(1),
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
        Op::FocusWorkspaceUp,
        // Adding a window updates the original output for unnamed workspaces.
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        // Connecting the previous output should move the named workspace back since its
        // original output wasn't updated.
        Op::AddOutput(1),
    ];

    let layout = check_ops(ops);
    let (mon, _, ws) = layout
        .workspaces()
        .find(|(_, _, ws)| ws.name().is_some())
        .unwrap();
    assert!(ws.name().is_some()); // Sanity check.
    let mon = mon.unwrap();
    assert_eq!(mon.output_name(), "output1");
}

#[test]
fn workspaces_update_original_output_on_moving_to_same_output() {
    let ops = [
        Op::AddOutput(1),
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
        Op::FocusWorkspaceUp,
        Op::MoveWorkspaceToOutput(2),
        Op::AddOutput(1),
    ];

    let layout = check_ops(ops);
    let (mon, _, ws) = layout
        .workspaces()
        .find(|(_, _, ws)| ws.name().is_some())
        .unwrap();
    assert!(ws.name().is_some()); // Sanity check.
    let mon = mon.unwrap();
    assert_eq!(mon.output_name(), "output2");
}

#[test]
fn workspaces_update_original_output_on_moving_to_same_monitor() {
    let ops = [
        Op::AddOutput(1),
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
        Op::FocusWorkspaceUp,
        Op::MoveWorkspaceToMonitor {
            ws_name: Some(1),
            output_id: 2,
        },
        Op::AddOutput(1),
    ];

    let layout = check_ops(ops);
    let (mon, _, ws) = layout
        .workspaces()
        .find(|(_, _, ws)| ws.name().is_some())
        .unwrap();
    assert!(ws.name().is_some()); // Sanity check.
    let mon = mon.unwrap();
    assert_eq!(mon.output_name(), "output2");
}

#[test]
fn large_negative_height_change() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::AdjustProportion(-1e129),
        },
    ];

    let mut options = Options::default();
    options.layout.border.off = false;
    options.layout.border.width = 1.;

    check_ops_with_options(options, ops);
}

#[test]
fn large_max_size() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams {
                min_max_size: (Size::from((0, 0)), Size::from((i32::MAX, i32::MAX))),
                ..TestWindowParams::new(1)
            },
        },
    ];

    let mut options = Options::default();
    options.layout.border.off = false;
    options.layout.border.width = 1.;

    check_ops_with_options(options, ops);
}

#[test]
fn workspace_cleanup_during_switch() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::CloseWindow(1),
    ];

    check_ops(ops);
}

#[test]
fn workspace_transfer_during_switch() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(2),
        Op::FocusOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::RemoveOutput(1),
        Op::FocusWorkspaceDown,
        Op::FocusWorkspaceDown,
        Op::AddOutput(1),
    ];

    check_ops(ops);
}

#[test]
fn workspace_transfer_during_switch_from_last() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
        Op::FocusWorkspaceUp,
        Op::AddOutput(1),
    ];

    check_ops(ops);
}

#[test]
fn workspace_transfer_during_switch_gets_cleaned_up() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::RemoveOutput(1),
        Op::AddOutput(2),
        Op::MoveColumnToWorkspaceDown(true),
        Op::MoveColumnToWorkspaceDown(true),
        Op::AddOutput(1),
    ];

    check_ops(ops);
}

#[test]
fn move_workspace_to_output() {
    let ops = [
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::FocusOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::MoveWorkspaceToOutput(2),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal {
        monitors,
        active_monitor_idx,
        ..
    } = layout.monitor_set
    else {
        unreachable!()
    };

    assert_eq!(active_monitor_idx, 1);
    assert_eq!(monitors[0].workspaces.len(), 1);
    assert!(!monitors[0].workspaces[0].has_windows());
    assert_eq!(monitors[1].active_workspace_idx, 0);
    assert_eq!(monitors[1].workspaces.len(), 2);
    assert!(monitors[1].workspaces[0].has_windows());
}

#[test]
fn open_right_of_on_different_workspace() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddWindowNextTo {
            params: TestWindowParams::new(3),
            next_to_id: 1,
        },
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    let mon = monitors.into_iter().next().unwrap();
    assert_eq!(
        mon.active_workspace_idx, 1,
        "the second workspace must remain active"
    );
    assert_eq!(
        mon.workspaces[0].scrolling().active_column_idx(),
        1,
        "the new window must become active"
    );
}

#[test]
// empty_workspace_above_first = true
fn open_right_of_on_different_workspace_ewaf() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddWindowNextTo {
            params: TestWindowParams::new(3),
            next_to_id: 1,
        },
    ];

    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let layout = check_ops_with_options(options, ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    let mon = monitors.into_iter().next().unwrap();
    assert_eq!(
        mon.active_workspace_idx, 2,
        "the second workspace must remain active"
    );
    assert_eq!(
        mon.workspaces[1].scrolling().active_column_idx(),
        1,
        "the new window must become active"
    );
}

#[test]
fn removing_all_outputs_preserves_empty_named_workspaces() {
    let ops = [
        Op::AddOutput(1),
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![],
            layout_config: None,
        },
        Op::AddNamedWorkspace {
            ws_name: 2,
            output_names: vec![],
            layout_config: None,
        },
        Op::RemoveOutput(1),
    ];

    let layout = check_ops(ops);

    let MonitorSet::NoOutputs { workspaces } = layout.monitor_set else {
        unreachable!()
    };

    assert_eq!(workspaces.len(), 2);
}

#[test]
fn config_change_updates_cached_sizes() {
    let mut config = Config::default();
    let border = &mut config.layout.border;
    border.off = false;
    border.width = 2.;

    let mut layout = Layout::new(Clock::default(), &config);

    Op::AddWindow {
        params: TestWindowParams {
            bbox: Rectangle::from_size(Size::from((1280, 200))),
            ..TestWindowParams::new(1)
        },
    }
    .apply(&mut layout);

    config.layout.border.width = 4.;
    layout.update_config(&config);

    layout.verify_invariants();
}

#[test]
fn preset_height_change_removes_preset() {
    let mut config = Config::default();
    config.layout.preset_window_heights = vec![PresetSize::Fixed(1), PresetSize::Fixed(2)];

    let mut layout = Layout::new(Clock::default(), &config);

    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ConsumeOrExpelWindowLeft { id: None },
        Op::SwitchPresetWindowHeight { id: None },
        Op::SwitchPresetWindowHeight { id: None },
    ];
    for op in ops {
        op.apply(&mut layout);
    }

    // Leave only one.
    config.layout.preset_window_heights = vec![PresetSize::Fixed(1)];

    layout.update_config(&config);

    layout.verify_invariants();
}

#[test]
fn set_window_height_recomputes_to_auto() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ConsumeOrExpelWindowLeft { id: None },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ConsumeOrExpelWindowLeft { id: None },
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::SetFixed(100),
        },
        Op::FocusWindowUp,
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::SetFixed(200),
        },
    ];

    check_ops(ops);
}

#[test]
fn one_window_in_column_becomes_weight_1() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ConsumeOrExpelWindowLeft { id: None },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ConsumeOrExpelWindowLeft { id: None },
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::SetFixed(100),
        },
        Op::Communicate(2),
        Op::FocusWindowUp,
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::SetFixed(200),
        },
        Op::Communicate(1),
        Op::CloseWindow(0),
        Op::CloseWindow(1),
    ];

    check_ops(ops);
}

#[test]
fn fixed_height_takes_max_non_auto_into_account() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::SetWindowHeight {
            id: Some(0),
            change: SizeChange::SetFixed(704),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ConsumeOrExpelWindowLeft { id: None },
    ];

    let options = Options {
        layout: niri_config::Layout {
            border: niri_config::Border {
                off: false,
                width: 4.,
                ..Default::default()
            },
            gaps: 0.,
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn start_interactive_move_then_remove_window() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::InteractiveMoveBegin {
            window: 0,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::CloseWindow(0),
    ];

    check_ops(ops);
}

#[test]
fn interactive_move_onto_empty_output() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::InteractiveMoveBegin {
            window: 0,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::AddOutput(2),
        Op::InteractiveMoveUpdate {
            window: 0,
            dx: 1000.,
            dy: 0.,
            output_idx: 2,
            px: 0.,
            py: 0.,
        },
        Op::InteractiveMoveEnd { window: 0 },
    ];

    check_ops(ops);
}

#[test]
fn interactive_move_onto_empty_output_ewaf() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::InteractiveMoveBegin {
            window: 0,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::AddOutput(2),
        Op::InteractiveMoveUpdate {
            window: 0,
            dx: 1000.,
            dy: 0.,
            output_idx: 2,
            px: 0.,
            py: 0.,
        },
        Op::InteractiveMoveEnd { window: 0 },
    ];

    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn interactive_move_onto_last_workspace() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::InteractiveMoveBegin {
            window: 0,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::InteractiveMoveUpdate {
            window: 0,
            dx: 1000.,
            dy: 0.,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::FocusWorkspaceDown,
        Op::AdvanceAnimations { msec_delta: 1000 },
        Op::InteractiveMoveEnd { window: 0 },
    ];

    check_ops(ops);
}

#[test]
fn interactive_move_onto_first_empty_workspace() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::InteractiveMoveBegin {
            window: 1,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::InteractiveMoveUpdate {
            window: 1,
            dx: 1000.,
            dy: 0.,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::FocusWorkspaceUp,
        Op::AdvanceAnimations { msec_delta: 1000 },
        Op::InteractiveMoveEnd { window: 1 },
    ];
    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn output_active_workspace_is_preserved() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::RemoveOutput(1),
        Op::AddOutput(1),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert_eq!(monitors[0].active_workspace_idx, 1);
}

#[test]
fn output_active_workspace_is_preserved_with_other_outputs() {
    let ops = [
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::RemoveOutput(1),
        Op::AddOutput(1),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert_eq!(monitors[1].active_workspace_idx, 1);
}

#[test]
fn named_workspace_to_output() {
    let ops = [
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![],
            layout_config: None,
        },
        Op::AddOutput(1),
        Op::MoveWorkspaceToOutput(1),
        Op::FocusWorkspaceUp,
    ];
    check_ops(ops);
}

#[test]
// empty_workspace_above_first = true
fn named_workspace_to_output_ewaf() {
    let ops = [
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![2],
            layout_config: None,
        },
        Op::AddOutput(1),
        Op::AddOutput(2),
    ];
    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn move_window_to_empty_workspace_above_first() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MoveWorkspaceUp,
        Op::MoveWorkspaceDown,
        Op::FocusWorkspaceUp,
        Op::MoveWorkspaceDown,
    ];
    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn move_window_to_different_output() {
    let ops = [
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::MoveWorkspaceToOutput(2),
    ];
    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn close_window_empty_ws_above_first() {
    let ops = [
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(1),
        Op::CloseWindow(1),
    ];
    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn add_and_remove_output() {
    let ops = [
        Op::AddOutput(2),
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::RemoveOutput(2),
    ];
    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn switch_ewaf_on() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ];

    let mut layout = check_ops(ops);
    layout.update_options(Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    });
    layout.verify_invariants();
}

#[test]
fn switch_ewaf_off() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ];

    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut layout = check_ops_with_options(options, ops);
    layout.update_options(Options::default());
    layout.verify_invariants();
}

#[test]
fn interactive_move_drop_on_other_output_during_animation() {
    let ops = [
        Op::AddOutput(3),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::InteractiveMoveBegin {
            window: 3,
            output_idx: 3,
            px: 0.0,
            py: 0.0,
        },
        Op::FocusWorkspaceDown,
        Op::AddOutput(4),
        Op::InteractiveMoveUpdate {
            window: 3,
            dx: 0.0,
            dy: 8300.68619826683,
            output_idx: 4,
            px: 0.0,
            py: 0.0,
        },
        Op::RemoveOutput(4),
        Op::InteractiveMoveEnd { window: 3 },
    ];
    check_ops(ops);
}

#[test]
fn add_window_next_to_only_interactively_moved_without_outputs() {
    let ops = [
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(1),
        Op::InteractiveMoveBegin {
            window: 2,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        Op::InteractiveMoveUpdate {
            window: 2,
            dx: 0.0,
            dy: 3586.692842955048,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        Op::RemoveOutput(1),
        // We have no outputs, and the only existing window is interactively moved, meaning there
        // are no workspaces either.
        Op::AddWindowNextTo {
            params: TestWindowParams::new(3),
            next_to_id: 2,
        },
    ];

    check_ops(ops);
}

#[test]
fn interactive_move_toggle_floating_ends_dnd_gesture() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::InteractiveMoveBegin {
            window: 2,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        Op::InteractiveMoveUpdate {
            window: 2,
            dx: 0.0,
            dy: 3586.692842955048,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        Op::Refresh { is_active: false },
        Op::ToggleWindowFloating { id: None },
        Op::InteractiveMoveEnd { window: 2 },
    ];

    check_ops(ops);
}

#[test]
fn interactive_move_from_workspace_with_layout_config() {
    let ops = [
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![2],
            layout_config: Some(Box::new(niri_config::LayoutPart {
                border: Some(niri_config::BorderRule {
                    on: true,
                    ..Default::default()
                }),
                ..Default::default()
            })),
        },
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::InteractiveMoveBegin {
            window: 2,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        Op::InteractiveMoveUpdate {
            window: 2,
            dx: 0.0,
            dy: 3586.692842955048,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        // Now remove and add the output. It will have the same workspace.
        Op::RemoveOutput(1),
        Op::AddOutput(1),
        Op::InteractiveMoveUpdate {
            window: 2,
            dx: 0.0,
            dy: 0.0,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        // Now move onto a different workspace.
        Op::FocusWorkspaceDown,
        Op::CompleteAnimations,
        Op::InteractiveMoveUpdate {
            window: 2,
            dx: 0.0,
            dy: 0.0,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
    ];

    check_ops(ops);
}

#[test]
fn set_width_fixed_negative() {
    let ops = [
        Op::AddOutput(3),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::ToggleWindowFloating { id: Some(3) },
        Op::SetColumnWidth(SizeChange::SetFixed(-100)),
    ];
    check_ops(ops);
}

#[test]
fn set_height_fixed_negative() {
    let ops = [
        Op::AddOutput(3),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::ToggleWindowFloating { id: Some(3) },
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::SetFixed(-100),
        },
    ];
    check_ops(ops);
}

#[test]
fn interactive_resize_to_negative() {
    let ops = [
        Op::AddOutput(3),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::ToggleWindowFloating { id: Some(3) },
        Op::InteractiveResizeBegin {
            window: 3,
            edges: ResizeEdge::BOTTOM_RIGHT,
        },
        Op::InteractiveResizeUpdate {
            window: 3,
            dx: -10000.,
            dy: -10000.,
        },
    ];
    check_ops(ops);
}

#[test]
fn windows_on_other_workspaces_remain_activated() {
    let ops = [
        Op::AddOutput(3),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::FocusWorkspaceDown,
        Op::Refresh { is_active: true },
    ];

    let layout = check_ops(ops);
    let (_, win) = layout.windows().next().unwrap();
    assert!(win.0.pending_activated.get());
}

#[test]
fn stacking_add_parent_brings_up_child() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                parent_id: Some(1),
                ..TestWindowParams::new(0)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(1)
            },
        },
    ];

    check_ops(ops);
}

#[test]
fn stacking_add_parent_brings_up_descendants() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                parent_id: Some(2),
                ..TestWindowParams::new(0)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                parent_id: Some(0),
                ..TestWindowParams::new(1)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(2)
            },
        },
    ];

    check_ops(ops);
}

#[test]
fn stacking_activate_brings_up_descendants() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(0)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                parent_id: Some(0),
                ..TestWindowParams::new(1)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                parent_id: Some(1),
                ..TestWindowParams::new(2)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(3)
            },
        },
        Op::FocusWindow(0),
    ];

    check_ops(ops);
}

#[test]
fn stacking_set_parent_brings_up_child() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(0)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(1)
            },
        },
        Op::SetParent {
            id: 0,
            new_parent_id: Some(1),
        },
    ];

    check_ops(ops);
}

#[test]
fn move_window_to_workspace_with_different_active_output() {
    let ops = [
        Op::AddOutput(0),
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::FocusOutput(1),
        Op::MoveWindowToWorkspace {
            window_id: Some(0),
            workspace_idx: 2,
            allow_hidden: true,
        },
    ];

    check_ops(ops);
}

#[test]
fn set_first_workspace_name() {
    let ops = [
        Op::AddOutput(0),
        Op::SetWorkspaceName {
            new_ws_name: 0,
            ws_name: None,
        },
    ];

    check_ops(ops);
}

#[test]
fn set_first_workspace_name_ewaf() {
    let ops = [
        Op::AddOutput(0),
        Op::SetWorkspaceName {
            new_ws_name: 0,
            ws_name: None,
        },
    ];

    let options = Options {
        layout: niri_config::Layout {
            empty_workspace_above_first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn set_last_workspace_name() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::FocusWorkspaceDown,
        Op::SetWorkspaceName {
            new_ws_name: 0,
            ws_name: None,
        },
    ];

    check_ops(ops);
}

#[test]
fn move_workspace_to_same_monitor_doesnt_reorder() {
    let ops = [
        Op::AddOutput(0),
        Op::SetWorkspaceName {
            new_ws_name: 0,
            ws_name: None,
        },
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::MoveWorkspaceToMonitor {
            ws_name: Some(0),
            output_id: 0,
        },
    ];

    let layout = check_ops(ops);
    let counts: Vec<_> = layout
        .workspaces()
        .map(|(_, _, ws)| ws.windows().count())
        .collect();
    assert_eq!(counts, &[1, 2, 0]);
}

#[test]
fn removing_window_above_preserves_focused_window() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::FocusColumnFirst,
        Op::ConsumeWindowIntoColumn,
        Op::ConsumeWindowIntoColumn,
        Op::FocusWindowDown,
        Op::CloseWindow(0),
    ];

    let layout = check_ops(ops);
    let win = layout.focus().unwrap();
    assert_eq!(win.0.id, 1);
}

#[test]
fn preset_column_width_fixed_correct_with_border() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::SwitchPresetColumnWidth,
    ];

    let options = Options {
        layout: niri_config::Layout {
            preset_column_widths: vec![PresetSize::Fixed(500)],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut layout = check_ops_with_options(options, ops);

    let win = layout.windows().next().unwrap().1;
    assert_eq!(win.requested_size().unwrap().w, 500);

    // Add border.
    let options = Options {
        layout: niri_config::Layout {
            preset_column_widths: vec![PresetSize::Fixed(500)],
            border: niri_config::Border {
                off: false,
                width: 5.,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    layout.update_options(options);

    // With border, the window gets less size.
    let win = layout.windows().next().unwrap().1;
    assert_eq!(win.requested_size().unwrap().w, 490);

    // However, preset fixed width will still work correctly.
    layout.toggle_width(true);
    let win = layout.windows().next().unwrap().1;
    assert_eq!(win.requested_size().unwrap().w, 500);
}

#[test]
fn preset_column_width_reset_after_set_width() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::SwitchPresetColumnWidth,
        Op::SetWindowWidth {
            id: None,
            change: SizeChange::AdjustFixed(-10),
        },
        Op::SwitchPresetColumnWidth,
    ];

    let options = Options {
        layout: niri_config::Layout {
            preset_column_widths: vec![PresetSize::Fixed(500), PresetSize::Fixed(1000)],
            ..Default::default()
        },
        ..Default::default()
    };
    let layout = check_ops_with_options(options, ops);
    let win = layout.windows().next().unwrap().1;
    assert_eq!(win.requested_size().unwrap().w, 500);
}

#[test]
fn move_column_to_workspace_unfocused_with_multiple_monitors() {
    let ops = [
        Op::AddOutput(1),
        Op::SetWorkspaceName {
            new_ws_name: 101,
            ws_name: None,
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::SetWorkspaceName {
            new_ws_name: 102,
            ws_name: None,
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(2),
        Op::FocusOutput(2),
        Op::SetWorkspaceName {
            new_ws_name: 201,
            ws_name: None,
        },
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::AddWindow {
            params: TestWindowParams::new(4),
        },
        Op::MoveColumnToOutput {
            output_id: 1,
            target_ws_idx: Some(0),
            activate: false,
        },
        Op::FocusOutput(1),
    ];

    let layout = check_ops(ops);

    assert_eq!(layout.active_workspace().unwrap().name().unwrap(), "ws102");

    for (mon, win) in layout.windows() {
        let mon = mon.unwrap();
        let ws = mon
            .workspaces
            .iter()
            .find(|w| w.has_window(win.id()))
            .unwrap();

        assert_eq!(
            ws.name().unwrap(),
            match win.id() {
                1 | 4 => "ws101",
                2 => "ws102",
                3 => "ws201",
                _ => unreachable!(),
            }
        );
    }
}

#[test]
fn move_column_to_workspace_down_focus_false_on_floating_window() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ToggleWindowFloating { id: None },
        Op::MoveColumnToWorkspaceDown(false),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert_eq!(monitors[0].active_workspace_idx, 0);
}

#[test]
fn move_column_to_workspace_focus_false_on_floating_window() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ToggleWindowFloating { id: None },
        Op::MoveColumnToWorkspace(1, false, true),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert_eq!(monitors[0].active_workspace_idx, 0);
}

#[test]
fn restore_to_floating_persists_across_fullscreen_maximize() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleWindowFloating { id: None },
        // Maximize then fullscreen.
        Op::MaximizeWindowToEdges { id: None },
        Op::FullscreenWindow(1),
        // Unfullscreen.
        Op::FullscreenWindow(1),
    ];

    let mut layout = check_ops(ops);

    // Unfullscreening should return the window to the maximized state.
    let scrolling = layout.active_workspace().unwrap().scrolling();
    assert!(scrolling.tiles().next().is_some());

    let ops = [
        // Unmaximize.
        Op::MaximizeWindowToEdges { id: None },
    ];
    check_ops_on_layout(&mut layout, ops);

    // Unmaximize should return the window back to floating.
    let scrolling = layout.active_workspace().unwrap().scrolling();
    assert!(scrolling.tiles().next().is_none());
}

#[test]
fn unmaximize_during_fullscreen_does_not_float() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleWindowFloating { id: None },
        // Maximize then fullscreen.
        Op::MaximizeWindowToEdges { id: None },
        Op::FullscreenWindow(1),
        // Unmaximize.
        Op::MaximizeWindowToEdges { id: None },
    ];

    let mut layout = check_ops(ops);

    // Unmaximize shouldn't have changed the window state since it's fullscreen.
    let scrolling = layout.active_workspace().unwrap().scrolling();
    assert!(scrolling.tiles().next().is_some());

    let ops = [
        // Unfullscreen.
        Op::FullscreenWindow(1),
    ];
    check_ops_on_layout(&mut layout, ops);

    // Unfullscreen should return the window back to floating.
    let scrolling = layout.active_workspace().unwrap().scrolling();
    assert!(scrolling.tiles().next().is_none());
}

#[test]
fn move_column_to_workspace_maximize_and_fullscreen() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MaximizeWindowToEdges { id: None },
        Op::FullscreenWindow(1),
        Op::MoveColumnToWorkspaceDown(true),
        Op::FullscreenWindow(1),
    ];

    let layout = check_ops(ops);
    let (_, win) = layout.windows().next().unwrap();

    // Unfullscreening should return to maximized because the window was maximized before.
    assert_eq!(win.pending_sizing_mode(), SizingMode::Maximized);
}

#[test]
fn move_window_to_workspace_maximize_and_fullscreen() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MaximizeWindowToEdges { id: None },
        Op::FullscreenWindow(1),
        Op::MoveWindowToWorkspaceDown(true),
        Op::FullscreenWindow(1),
    ];

    let layout = check_ops(ops);
    let (_, win) = layout.windows().next().unwrap();

    // Unfullscreening should return to maximized because the window was maximized before.
    //
    // FIXME: it currently doesn't because windows themselves can only be either fullscreen or
    // maximized. So when a window is fullscreen, whether it is also maximized or not is stored in
    // the column. MoveWindowToWorkspace removes the window from the column and this information is
    // forgotten.
    assert_eq!(win.pending_sizing_mode(), SizingMode::Normal);
}

#[test]
fn tabs_with_different_border() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams {
                rules: Some(ResolvedWindowRules {
                    border: niri_config::BorderRule {
                        on: true,
                        ..Default::default()
                    },
                    ..ResolvedWindowRules::default()
                }),
                ..TestWindowParams::new(2)
            },
        },
        Op::SwitchPresetWindowHeight { id: None },
        Op::ToggleColumnTabbedDisplay,
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::ConsumeOrExpelWindowLeft { id: None },
    ];

    let options = Options {
        layout: niri_config::Layout {
            struts: Struts {
                left: FloatOrInt(0.),
                right: FloatOrInt(0.),
                top: FloatOrInt(20000.),
                bottom: FloatOrInt(0.),
            },
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn expel_pending_left_from_fullscreen_tabbed_column() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FullscreenWindow(1),
        Op::Communicate(1),
        // 1 is now fullscreen, view_offset_to_restore is set.
        Op::ToggleColumnTabbedDisplay,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ConsumeOrExpelWindowLeft { id: Some(2) },
        // 2 is consumed into a fullscreen column, fullscreen is requested but not applied.
        //
        // Now, get it back out while keeping it focused.
        //
        // Importantly, we expel it *left*, which results in adding a new column with the exact
        // same active_column_idx.
        Op::FocusWindow(2),
        Op::ConsumeOrExpelWindowLeft { id: None },
    ];

    check_ops(ops);
}

#[test]
fn workspace_render_geo_at_fractional_scale() {
    let ops = [
        Op::AddScaledOutput {
            id: 1,
            scale: 1.1,
            layout_config: None,
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::CompleteAnimations,
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = &layout.monitor_set else {
        unreachable!()
    };

    let mon = &monitors[0];
    let mut iter = mon.workspaces_with_render_geo();
    let (_ws, geo) = iter.next().unwrap();
    assert!(
        iter.next().is_none(),
        "animations are completed, only one workspace should be visible"
    );
    assert_eq!(
        geo.loc.y, 0.,
        "active workspace must be at y = 0 exactly, \
         otherwise a pointer against the screen edge at y = 0 won't hit it"
    );
}

fn parent_id_causes_loop(layout: &Layout<TestWindow>, id: usize, mut parent_id: usize) -> bool {
    if parent_id == id {
        return true;
    }

    'outer: loop {
        for (_, win) in layout.windows() {
            if win.0.id == parent_id {
                match win.0.parent_id.get() {
                    Some(new_parent_id) => {
                        if new_parent_id == id {
                            // Found a loop.
                            return true;
                        }

                        parent_id = new_parent_id;
                        continue 'outer;
                    }
                    // Reached window with no parent.
                    None => return false,
                }
            }
        }

        // Parent is not in the layout.
        return false;
    }
}

fn arbitrary_spacing() -> impl Strategy<Value = f64> {
    // Give equal weight to:
    // - 0: the element is disabled
    // - 4: some reasonable value
    // - random value, likely unreasonably big
    prop_oneof![Just(0.), Just(4.), ((1.)..=65535.)]
}

fn arbitrary_spacing_neg() -> impl Strategy<Value = f64> {
    // Give equal weight to:
    // - 0: the element is disabled
    // - 4: some reasonable value
    // - -4: some reasonable negative value
    // - random value, likely unreasonably big
    prop_oneof![Just(0.), Just(4.), Just(-4.), ((1.)..=65535.)]
}

fn arbitrary_struts() -> impl Strategy<Value = Struts> {
    (
        arbitrary_spacing_neg(),
        arbitrary_spacing_neg(),
        arbitrary_spacing_neg(),
        arbitrary_spacing_neg(),
    )
        .prop_map(|(left, right, top, bottom)| Struts {
            left: FloatOrInt(left),
            right: FloatOrInt(right),
            top: FloatOrInt(top),
            bottom: FloatOrInt(bottom),
        })
}

fn arbitrary_center_focused_column() -> impl Strategy<Value = CenterFocusedColumn> {
    prop_oneof![
        Just(CenterFocusedColumn::Never),
        Just(CenterFocusedColumn::OnOverflow),
        Just(CenterFocusedColumn::Always),
    ]
}

fn arbitrary_tab_indicator_position() -> impl Strategy<Value = TabIndicatorPosition> {
    prop_oneof![
        Just(TabIndicatorPosition::Left),
        Just(TabIndicatorPosition::Right),
        Just(TabIndicatorPosition::Top),
        Just(TabIndicatorPosition::Bottom),
    ]
}

prop_compose! {
    fn arbitrary_focus_ring()(
        off in any::<bool>(),
        width in prop::option::of(arbitrary_spacing().prop_map(FloatOrInt)),
    ) -> niri_config::BorderRule {
        niri_config::BorderRule {
            off,
            on: !off,
            width,
            ..Default::default()
        }
    }
}

prop_compose! {
    fn arbitrary_border()(
        off in any::<bool>(),
        width in prop::option::of(arbitrary_spacing().prop_map(FloatOrInt)),
    ) -> niri_config::BorderRule {
        niri_config::BorderRule {
            off,
            on: !off,
            width,
            ..Default::default()
        }
    }
}

prop_compose! {
    fn arbitrary_shadow()(
        off in any::<bool>(),
        softness in prop::option::of(arbitrary_spacing().prop_map(FloatOrInt)),
    ) -> niri_config::ShadowRule {
        niri_config::ShadowRule {
            off,
            on: !off,
            softness,
            ..Default::default()
        }
    }
}

prop_compose! {
    fn arbitrary_tab_indicator()(
        off in any::<bool>(),
        hide_when_single_tab in prop::option::of(any::<bool>().prop_map(Flag)),
        place_within_column in prop::option::of(any::<bool>().prop_map(Flag)),
        width in prop::option::of(arbitrary_spacing().prop_map(FloatOrInt)),
        gap in prop::option::of(arbitrary_spacing_neg().prop_map(FloatOrInt)),
        length in prop::option::of((0f64..2f64)
            .prop_map(|x| TabIndicatorLength { total_proportion: Some(x) })),
        position in prop::option::of(arbitrary_tab_indicator_position()),
    ) -> niri_config::TabIndicatorPart {
        niri_config::TabIndicatorPart {
            off,
            on: !off,
            hide_when_single_tab,
            place_within_column,
            width,
            gap,
            length,
            position,
            ..Default::default()
        }
    }
}

prop_compose! {
    fn arbitrary_layout_part()(
        main_axis in prop::option::of(prop_oneof![Just(MainAxis::Horizontal), Just(MainAxis::Vertical)]),
        gaps in prop::option::of(arbitrary_spacing().prop_map(FloatOrInt)),
        struts in prop::option::of(arbitrary_struts()),
        focus_ring in prop::option::of(arbitrary_focus_ring()),
        border in prop::option::of(arbitrary_border()),
        shadow in prop::option::of(arbitrary_shadow()),
        tab_indicator in prop::option::of(arbitrary_tab_indicator()),
        center_focused_column in prop::option::of(arbitrary_center_focused_column()),
        always_center_single_column in prop::option::of(any::<bool>().prop_map(Flag)),
        empty_workspace_above_first in prop::option::of(any::<bool>().prop_map(Flag)),
    ) -> niri_config::LayoutPart {
        niri_config::LayoutPart {
            main_axis,
            gaps,
            struts,
            center_focused_column,
            always_center_single_column,
            empty_workspace_above_first,
            focus_ring,
            border,
            shadow,
            tab_indicator,
            ..Default::default()
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: if std::env::var_os("RUN_SLOW_TESTS").is_none() {
            eprintln!("ignoring slow test");
            0
        } else {
            ProptestConfig::default().cases
        },
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_operations_dont_panic(
        ops: Vec<Op>,
        layout_config in arbitrary_layout_part(),
    ) {
        // eprintln!("{ops:?}");
        let options = Options {
            layout: niri_config::Layout::from_part(&layout_config),
            ..Default::default()
        };

        check_ops_with_options(options, ops);
    }
}

#[test]
fn isolated_output_stays_out_of_overview() {
    fn make_output(name: &str) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            None,
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    let mut layout = Layout::<TestWindow>::default();

    let normal = make_output("normal");
    let isolated = make_output("isolated");
    layout.add_output(normal.clone(), None, false);
    layout.add_output(isolated.clone(), None, true);

    layout.toggle_overview();

    let normal_mon = layout.monitor_for_output(&normal).unwrap();
    let isolated_mon = layout.monitor_for_output(&isolated).unwrap();

    // The regular output enters the overview and receives zoom progress.
    assert!(
        normal_mon.overview_progress_value().is_some(),
        "regular output should receive overview progress"
    );

    // The isolated output must not. The visual zoom is driven by overview_progress (not
    // overview_open), so leaving progress set here is what makes the overview still show up.
    assert_eq!(
        isolated_mon.overview_progress_value(),
        None,
        "isolated output must not receive overview progress"
    );
    assert_eq!(
        isolated_mon.overview_zoom(),
        1.,
        "isolated output must not zoom out"
    );
}

#[test]
fn consolidated_mode_scopes_overview_to_active_output() {
    fn make_output(name: &str) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            None,
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    let mut options = Options::default();
    options.overview.consolidated_carousel = Some(niri_config::ConsolidatedCarousel {
        reveal_zoom: 0.25,
        assembled_zoom: 0.1,
    });

    let mut layout = Layout::<TestWindow>::with_options(Clock::with_time(Duration::ZERO), options);

    let first = make_output("first");
    let second = make_output("second");
    layout.add_output(first.clone(), None, false);
    layout.add_output(second.clone(), None, false);

    layout.toggle_overview();

    let mons = layout.monitors().collect::<Vec<_>>();
    assert!(mons[0].overview_open, "active output enters overview");
    assert!(!mons[1].overview_open, "non-active output stays live");

    // The visual zoom is driven by `overview_progress`, not `overview_open`:
    // the sibling must also be denied progress or it still zooms into the
    // overview on screen (the 2026-08-04 hardware finding).
    let mons = layout.monitors().collect::<Vec<_>>();
    assert!(
        mons[0].overview_progress_value().is_some(),
        "active output gets overview progress"
    );
    assert_eq!(
        None,
        mons[1].overview_progress_value(),
        "non-active output gets no overview progress"
    );

    // `advance_animations` re-pushes progress every frame — it must keep
    // withholding it from the sibling too.
    layout.advance_animations();
    let mons = layout.monitors().collect::<Vec<_>>();
    assert!(mons[0].overview_progress_value().is_some());
    assert_eq!(None, mons[1].overview_progress_value());
}

#[test]
fn removing_active_output_refreshes_overview_state() {
    fn make_output(name: &str) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            None,
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    let mut options = Options::default();
    options.overview.consolidated_carousel = Some(niri_config::ConsolidatedCarousel {
        reveal_zoom: 0.25,
        assembled_zoom: 0.1,
    });

    let mut layout = Layout::<TestWindow>::with_options(Clock::with_time(Duration::ZERO), options);

    let first = make_output("first");
    let second = make_output("second");
    let third = make_output("third");
    layout.add_output(first.clone(), None, false);
    layout.add_output(second.clone(), None, false);
    layout.add_output(third.clone(), None, false);

    // Explicitly make the middle output the active one, so removing it
    // exercises the index-shift path in `remove_output`.
    layout.focus_output(&second);
    assert_eq!(layout.active_output(), Some(&second));

    layout.toggle_overview(); // open

    let mons = layout.monitors().collect::<Vec<_>>();
    assert!(!mons[0].overview_open, "first (inactive) stays live");
    assert!(mons[1].overview_open, "second (active) enters overview");
    assert!(!mons[2].overview_open, "third (inactive) stays live");

    layout.remove_output(&second);
    layout.verify_invariants();

    assert_eq!(layout.carousel_rotation(), 0.0);

    // Active monitor shifted (index-shift in `remove_output`); overview
    // participation must have been recomputed for whichever monitor is
    // active now, not left stale from before the removal.
    let active = layout
        .active_output()
        .cloned()
        .expect("an output remains active");
    for mon in layout.monitors() {
        let should_be_open = mon.output() == &active;
        assert_eq!(
            mon.overview_open,
            should_be_open,
            "overview_open for {:?} should match whether it is now active",
            mon.output_name()
        );
    }
}

#[test]
fn carousel_reveal_tracks_zoom_threshold() {
    fn make_output(name: &str) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            None,
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    let mut options = Options::default();
    options.overview.consolidated_carousel = Some(niri_config::ConsolidatedCarousel {
        reveal_zoom: 0.4,
        assembled_zoom: 0.15,
    });

    let mut layout = Layout::<TestWindow>::with_options(Clock::with_time(Duration::ZERO), options);

    let first = make_output("first");
    let second = make_output("second");
    layout.add_output(first.clone(), None, false);
    layout.add_output(second.clone(), None, false);

    layout.toggle_overview();

    // Finish the overview-open animation so overview_zoom() reflects the target zoom.
    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Above reveal-zoom: no reveal yet.
    layout.set_overview_zoom_for_test(0.5);
    assert_eq!(layout.carousel_reveal(), 0.);

    // Halfway between reveal_zoom (0.4) and assembled_zoom (0.15).
    layout.set_overview_zoom_for_test(0.275);
    assert!((layout.carousel_reveal() - 0.5).abs() < 1e-9);

    // At/below assembled_zoom: fully revealed.
    layout.set_overview_zoom_for_test(0.15);
    assert_eq!(layout.carousel_reveal(), 1.);

    layout.set_overview_zoom_for_test(0.05);
    assert_eq!(layout.carousel_reveal(), 1.);

    // The assembled gallery is the terminal state of the zoom-out gesture:
    // the zoom target itself must clamp at assembled_zoom, so the live host
    // strip can't keep shrinking under the frozen gallery (the 2026-08-04
    // hardware finding, screenshot -5).
    assert_eq!(
        layout.active_monitor_ref().unwrap().overview_zoom_target(),
        0.15,
        "zoom target clamps at assembled_zoom in consolidated mode"
    );
}

#[test]
fn overview_zoom_persists_across_close() {
    // PR #2 port: the zoom target survives close (the close animation scales
    // from it, and the next open resumes at the parked zoom); only the preset
    // cycling index resets.
    let mut layout = Layout::<TestWindow>::default();

    let output = Output::new(
        "out".to_owned(),
        PhysicalProperties {
            size: Size::from((1280, 720)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    output.change_current_state(
        Some(Mode {
            size: Size::from((1280, 720)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    output.user_data().insert_if_missing(|| OutputName {
        connector: "out".to_owned(),
        make: None,
        model: None,
        serial: None,
    });
    layout.add_output(output, None, false);

    layout.toggle_overview();
    layout.set_overview_zoom_for_test(0.3);
    layout.toggle_overview(); // close
    layout.toggle_overview(); // reopen

    assert_eq!(
        layout.active_monitor_ref().unwrap().overview_zoom_target(),
        0.3,
        "parked zoom survives close/reopen"
    );
}

#[test]
fn zoom_target_unclamped_without_consolidated_carousel() {
    let mut layout = Layout::<TestWindow>::default();

    let output = Output::new(
        "out".to_owned(),
        PhysicalProperties {
            size: Size::from((1280, 720)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    output.change_current_state(
        Some(Mode {
            size: Size::from((1280, 720)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    output.user_data().insert_if_missing(|| OutputName {
        connector: "out".to_owned(),
        make: None,
        model: None,
        serial: None,
    });
    layout.add_output(output, None, false);

    layout.toggle_overview();
    layout.set_overview_zoom_for_test(0.05);
    assert_eq!(
        layout.active_monitor_ref().unwrap().overview_zoom_target(),
        0.05,
        "no clamp when the carousel isn't configured"
    );
}

#[test]
fn carousel_lens_false_while_rotation_is_zero() {
    fn make_output(name: &str) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            None,
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    let mut options = Options::default();
    options.overview.consolidated_carousel = Some(niri_config::ConsolidatedCarousel {
        reveal_zoom: 0.4,
        assembled_zoom: 0.15,
    });

    let mut layout = Layout::<TestWindow>::with_options(Clock::with_time(Duration::ZERO), options);

    let first = make_output("first");
    let second = make_output("second");
    layout.add_output(first.clone(), None, false);
    layout.add_output(second.clone(), None, false);

    layout.toggle_overview();

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Rotation starts at 0.0 (host) on open -- never in lens regardless of zoom.
    layout.set_overview_zoom_for_test(0.5);
    assert!(!layout.in_carousel_lens());

    layout.set_overview_zoom_for_test(0.2);
    assert!(!layout.in_carousel_lens());

    layout.set_overview_zoom_for_test(0.05);
    assert!(!layout.in_carousel_lens());
}

#[test]
fn carousel_outputs_includes_host_and_reset_centers_on_active() {
    fn make_output(name: &str) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            None,
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    // Outputs in order: "A" (active/host, window), "B" (window), "C" (window).
    let mut layout = Layout::<TestWindow>::default();

    let a = make_output("A");
    let b = make_output("B");
    let c = make_output("C");

    layout.add_output(a.clone(), None, false);
    layout.add_output(b.clone(), None, false);
    layout.add_output(c.clone(), None, false);

    layout.add_window(
        TestWindow::new(TestWindowParams::new(0)),
        AddWindowTarget::Output(&a),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );
    layout.add_window(
        TestWindow::new(TestWindowParams::new(1)),
        AddWindowTarget::Output(&b),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );
    layout.add_window(
        TestWindow::new(TestWindowParams::new(2)),
        AddWindowTarget::Output(&c),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );

    let names: Vec<String> = layout
        .carousel_outputs()
        .iter()
        .map(|m| m.output_name().clone())
        .collect();
    assert_eq!(
        names,
        vec!["A".to_string(), "B".to_string(), "C".to_string()]
    ); // host included

    layout.reset_carousel_center();
    assert_eq!(layout.carousel_rotation_target(), 0.0); // centered (rotation snapped to host) on
                                                        // active host "A"
}

#[test]
fn rotate_carousel_wraps_over_ring_positions() {
    // Outputs positioned left-to-right by `x`, same order as added, so ring
    // position equals the old monitor-list index (host "A" is at x=0 -> ring 0).
    fn make_output(name: &str, x: i32) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            Some(Point::from((x, 0))),
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    // 3 outputs, rotation starts at 0.0 (host).
    let mut layout = Layout::<TestWindow>::default();

    let a = make_output("A", 0);
    let b = make_output("B", 1280);
    let c = make_output("C", 2560);

    layout.add_output(a.clone(), None, false);
    layout.add_output(b.clone(), None, false);
    layout.add_output(c.clone(), None, false);

    layout.add_window(
        TestWindow::new(TestWindowParams::new(0)),
        AddWindowTarget::Output(&a),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );
    layout.add_window(
        TestWindow::new(TestWindowParams::new(1)),
        AddWindowTarget::Output(&b),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );
    layout.add_window(
        TestWindow::new(TestWindowParams::new(2)),
        AddWindowTarget::Output(&c),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );

    layout.reset_carousel_center();

    // Settle each retarget instantly so `carousel_rotation_target()` reflects it
    // right away (animation config aside, the target field updates synchronously).
    layout.rotate_carousel(1);
    assert_eq!(layout.carousel_rotation_target(), 1.0);
    layout.rotate_carousel(1);
    assert_eq!(layout.carousel_rotation_target(), 2.0);
    layout.rotate_carousel(1);
    assert_eq!(layout.carousel_rotation_target(), 0.0); // wrapped to leftmost
    layout.rotate_carousel(-1);
    assert_eq!(layout.carousel_rotation_target(), 2.0); // wrapped backward to rightmost
}

#[test]
fn lens_routes_workspace_switch_to_remote_monitor() {
    // Scrolling workspaces while the lens is settled on a remote output must
    // switch workspaces on THAT monitor, not the host (2026-08-04 hardware
    // finding, screenshot -4).
    fn make_output(name: &str, x: i32) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            Some(Point::from((x, 0))),
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    let mut options = Options::default();
    options.overview.consolidated_carousel = Some(niri_config::ConsolidatedCarousel {
        reveal_zoom: 0.4,
        assembled_zoom: 0.15,
    });
    let mut layout = Layout::<TestWindow>::with_options(Clock::with_time(Duration::ZERO), options);

    let a = make_output("A", 0);
    let b = make_output("B", 1280);
    layout.add_output(a.clone(), None, false);
    layout.add_output(b.clone(), None, false);

    layout.add_window(
        TestWindow::new(TestWindowParams::new(0)),
        AddWindowTarget::Output(&a),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );
    layout.add_window(
        TestWindow::new(TestWindowParams::new(1)),
        AddWindowTarget::Output(&b),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );
    layout.focus_output(&a);

    layout.toggle_overview();
    layout.clock.set_complete_instantly(true);
    layout.advance_animations();

    // Not lens'd (settled on host): falls back to normal routing.
    assert!(!layout.carousel_lens_switch_workspace(false));

    // Rotate to the remote and settle.
    layout.rotate_carousel(1);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);
    assert!(layout.in_carousel_lens());

    let ws_idx = |layout: &Layout<TestWindow>, out: &Output| {
        layout
            .monitors()
            .find(|m| m.output() == out)
            .unwrap()
            .active_workspace_idx()
    };
    assert_eq!(ws_idx(&layout, &a), 0);
    assert_eq!(ws_idx(&layout, &b), 0);

    // Lens'd on B: the switch lands on B, host A stays put.
    assert!(layout.carousel_lens_switch_workspace(false));
    assert_eq!(ws_idx(&layout, &a), 0, "host workspaces unmoved");
    assert_eq!(ws_idx(&layout, &b), 1, "remote workspace switched down");

    assert!(layout.carousel_lens_switch_workspace(true));
    assert_eq!(ws_idx(&layout, &b), 0, "remote workspace switched back up");

    // Column navigation routes to the lens'd remote too (2026-08-04 round 2:
    // side-scrolling was still hitting the workspace under the cursor).
    layout.add_window(
        TestWindow::new(TestWindowParams::new(2)),
        AddWindowTarget::Output(&b),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::No,
        false,
    );
    let col_idx = |layout: &Layout<TestWindow>, out: &Output| {
        layout
            .monitors()
            .find(|m| m.output() == out)
            .unwrap()
            .active_workspace_ref()
            .scrolling()
            .active_column_idx()
    };
    assert_eq!(col_idx(&layout, &b), 0);
    assert!(layout.carousel_lens_focus_column(true));
    assert_eq!(col_idx(&layout, &b), 1, "remote column focused right");
    assert!(layout.carousel_lens_focus_column(false));
    assert_eq!(col_idx(&layout, &b), 0, "remote column focused back left");
}

#[test]
fn overview_close_snaps_carousel_rotation_home() {
    // Regression test: closing the overview while the carousel is rotated
    // onto a remote output must instantly snap rotation back to the host
    // (ring position 0.0). Otherwise the compositor keeps painting that
    // output's lens panel over the host desktop after the overview closes,
    // with no way back (Gate B hardware checkpoint finding).
    fn make_output(name: &str, x: i32) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            Some(Point::from((x, 0))),
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    let mut options = Options::default();
    options.overview.consolidated_carousel = Some(niri_config::ConsolidatedCarousel {
        reveal_zoom: 0.4,
        assembled_zoom: 0.15,
    });

    let mut layout = Layout::<TestWindow>::with_options(Clock::with_time(Duration::ZERO), options);

    let a = make_output("A", 0);
    let b = make_output("B", 1280);

    layout.add_output(a.clone(), None, false);
    layout.add_output(b.clone(), None, false);

    layout.add_window(
        TestWindow::new(TestWindowParams::new(0)),
        AddWindowTarget::Output(&a),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );
    layout.add_window(
        TestWindow::new(TestWindowParams::new(1)),
        AddWindowTarget::Output(&b),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );

    layout.toggle_overview(); // open

    // Finish the overview-open animation.
    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Zoom into the revealed-carousel band.
    layout.set_overview_zoom_for_test(0.15);

    layout.rotate_carousel(1);

    // Settle the rotation animation.
    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    assert_eq!(layout.carousel_rotation_target(), 1.0);
    assert_eq!(layout.carousel_rotation(), 1.0);

    layout.toggle_overview(); // close

    assert_eq!(layout.carousel_rotation(), 0.0);
    assert_eq!(layout.carousel_rotation_target(), 0.0);
    assert!(!layout.carousel_rotating());
}

fn pullback_test_layout() -> (Layout<TestWindow>, Output, Output) {
    fn make_output(name: &str, x: i32) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            Some(Point::from((x, 0))),
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    let mut options = Options::default();
    options.overview.consolidated_carousel = Some(niri_config::ConsolidatedCarousel {
        reveal_zoom: 0.4,
        assembled_zoom: 0.15,
    });

    let mut layout = Layout::<TestWindow>::with_options(Clock::with_time(Duration::ZERO), options);

    let a = make_output("A", 0);
    let b = make_output("B", 1280);

    layout.add_output(a.clone(), None, false);
    layout.add_output(b.clone(), None, false);

    layout.add_window(
        TestWindow::new(TestWindowParams::new(0)),
        AddWindowTarget::Output(&a),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );
    layout.add_window(
        TestWindow::new(TestWindowParams::new(1)),
        AddWindowTarget::Output(&b),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );

    (layout, a, b)
}

#[test]
fn pullback_runs_three_phases() {
    let (mut layout, _a, _b) = pullback_test_layout();

    layout.toggle_overview(); // open

    // Finish the overview-open animation.
    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Zoom is parked above the reveal band (assembled_zoom is 0.15), so a
    // rotate request must pull back: zoom out to assembled, rotate, zoom
    // back in to 0.5.
    layout.set_overview_zoom_for_test(0.5);

    layout.carousel_request_rotate(1);

    // Drive the choreography to completion. Each phase transition needs its
    // own `advance_animations` call (mirrors how `overview_zoom_anim` /
    // `carousel_rotation_anim` only clear themselves on the *next* pass
    // after the animation that supersedes them is created), so loop with
    // margin rather than hardcoding an exact call count.
    layout.clock.set_complete_instantly(true);
    for _ in 0..5 {
        layout.advance_animations();
    }
    layout.clock.set_complete_instantly(false);

    assert_eq!(layout.carousel_rotation_target(), 1.0);
    assert_eq!(layout.carousel_rotation(), 1.0);
    assert!(!layout.carousel_rotating());
    assert!(!layout.carousel_pullback_is_active());
    assert_eq!(
        layout.active_monitor_ref().unwrap().overview_zoom_target(),
        0.5
    );
}

#[test]
fn pullback_skipped_when_assembled() {
    let (mut layout, _a, _b) = pullback_test_layout();

    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Zoom is already at the assembled level, so `carousel_reveal() >= 1.`
    // and the request should bypass the pull-back entirely.
    layout.set_overview_zoom_for_test(0.15);

    layout.carousel_request_rotate(1);

    assert!(!layout.carousel_pullback_is_active());

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    assert_eq!(layout.carousel_rotation_target(), 1.0);
    assert_eq!(layout.carousel_rotation(), 1.0);
    assert!(!layout.carousel_pullback_is_active());
}

#[test]
fn pullback_cancelled_by_overview_close() {
    let (mut layout, _a, _b) = pullback_test_layout();

    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    layout.set_overview_zoom_for_test(0.5);

    layout.carousel_request_rotate(1);

    // Settle only the zoom-out phase, moving the pullback into `Rotate`
    // with the rotation animation now in flight (real clock, so it doesn't
    // settle within this same call).
    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    assert!(layout.carousel_pullback_is_active());
    assert!(layout.carousel_rotating());

    layout.toggle_overview(); // close, mid-Rotate

    assert!(!layout.carousel_pullback_is_active());
    assert_eq!(layout.carousel_rotation_target(), 0.0);
    assert_eq!(layout.carousel_rotation(), 0.0);
    assert!(!layout.carousel_rotating());
}

#[test]
fn pullback_cancelled_by_user_zoom() {
    let (mut layout, _a, _b) = pullback_test_layout();

    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Zoom is parked above the reveal band, so a rotate request pulls back.
    layout.set_overview_zoom_for_test(0.5);

    layout.carousel_request_rotate(1);

    assert!(layout.carousel_pullback_is_active());

    // Mimic the input path's zoom entry point (`Action::OverviewZoomIn` /
    // `OverviewZoomOut` / `OverviewZoomCycle` all call this before doing
    // anything else): the user's own zoom input must cancel the pullback
    // rather than let the pullback hijack it, or vice versa.
    layout.cancel_carousel_pullback();

    assert!(!layout.carousel_pullback_is_active());

    // Drive animations to completion; since the pullback was cancelled
    // before it ever reached `Rotate`, no rotation should have happened.
    layout.clock.set_complete_instantly(true);
    for _ in 0..5 {
        layout.advance_animations();
    }
    layout.clock.set_complete_instantly(false);

    assert_eq!(layout.carousel_rotation_target(), 0.0);
    assert_eq!(layout.carousel_rotation(), 0.0);
    assert!(!layout.carousel_rotating());
    assert!(!layout.carousel_pullback_is_active());
}

#[test]
fn pullback_cancelled_by_active_monitor_change() {
    let (mut layout, a, b) = pullback_test_layout();

    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    layout.set_overview_zoom_for_test(0.5);

    layout.carousel_request_rotate(1);

    assert!(layout.carousel_pullback_is_active());

    // Mimic the input path's monitor-focus entry point (`focus_output`,
    // called from e.g. `Action::FocusMonitor*`): switching the active
    // monitor mid-pullback must cancel it, since the phase driver re-resolves
    // `active_monitor()` on every step.
    let current = layout.active_output().cloned().unwrap();
    let other = if current == a { &b } else { &a };
    layout.focus_output(other);

    assert!(!layout.carousel_pullback_is_active());
}

#[test]
fn pullback_cancelled_by_cross_output_window_activation() {
    let (mut layout, _a, _b) = pullback_test_layout();

    // Overview opens on the active monitor (A, since it was added first);
    // window 1 lives on B.
    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    layout.set_overview_zoom_for_test(0.5);

    layout.carousel_request_rotate(1);

    assert!(layout.carousel_pullback_is_active());

    // Mimic the input path's window-focus entry points (`Action::FocusWindow`
    // / `FocusWindowPrevious`): activating a window on another monitor
    // mid-pullback must cancel it too, same as `focus_output`, since it also
    // flips `active_monitor_idx` out from under the phase driver.
    layout.activate_window(&1);

    assert!(!layout.carousel_pullback_is_active());
}

#[test]
fn lens_recenters_when_target_leaves_ring() {
    let (mut layout, _a, _b) = pullback_test_layout();

    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Zoom is already at the assembled level for `pullback_test_layout`'s
    // `assembled_zoom` (0.15), so a direct rotate settles without a
    // pull-back.
    layout.set_overview_zoom_for_test(0.15);
    layout.rotate_carousel(1);

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Settled on B's ring position.
    assert_eq!(layout.carousel_rotation_target(), 1.0);
    assert_eq!(layout.carousel_rotation(), 1.0);
    assert!(!layout.carousel_rotating());

    // B's only window closes, dropping B out of `carousel_outputs()`
    // (filtered by `has_windows`). The rotation target (1.0) no longer
    // corresponds to any ring position — a plain `advance_animations` pass
    // must notice and snap the rotation home rather than leave it wedged.
    layout.remove_window(&1, Transaction::new());

    layout.advance_animations();

    assert_eq!(layout.carousel_rotation_target(), 0.0);
    assert_eq!(layout.carousel_rotation(), 0.0);
    assert!(!layout.carousel_rotating());
    assert!(!layout.carousel_pullback_is_active());
}

#[test]
fn pullback_rerequest_during_zoom_in_rotates_again() {
    let (mut layout, _a, _b) = pullback_test_layout();

    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Zoom is parked above the reveal band, so a rotate request pulls back.
    layout.set_overview_zoom_for_test(0.5);

    layout.carousel_request_rotate(1);
    assert!(layout.carousel_pullback_is_active());

    // Drive ZoomOut to completion, which starts Rotate.
    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Drive Rotate to completion, which starts ZoomIn.
    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    assert_eq!(layout.carousel_rotation_target(), 1.0);
    assert!(layout.carousel_pullback_is_active());

    // Re-request a rotation mid-ZoomIn (pressing -> twice in normal
    // succession). Before the fix, `start_or_retarget_pullback` only
    // re-rotated when the existing pullback's phase was `Rotate`; a request
    // during `ZoomIn` would store the new target but the ZoomIn leg would
    // complete and drop the pullback, silently swallowing the request.
    layout.carousel_request_rotate(1);
    assert!(layout.carousel_pullback_is_active());

    // Drive the choreography (now reset to ZoomOut) to completion: ZoomOut,
    // Rotate, ZoomIn again.
    layout.clock.set_complete_instantly(true);
    for _ in 0..6 {
        layout.advance_animations();
    }
    layout.clock.set_complete_instantly(false);

    // `pullback_test_layout` only has 2 outputs, so a second `delta: 1`
    // request computed from the settled target of 1.0 wraps back to 0.0 —
    // the *second* target, distinct from the first (1.0). Without the fix,
    // the re-request is dropped and rotation stays wedged at 1.0 (the first
    // target) once the ZoomIn leg completes. The zoom must also return to
    // the original restore_zoom (0.5) rather than getting stuck partway.
    assert_eq!(layout.carousel_rotation_target(), 0.0);
    assert_eq!(layout.carousel_rotation(), 0.0);
    assert!(!layout.carousel_rotating());
    assert!(!layout.carousel_pullback_is_active());
    assert_eq!(
        layout.active_monitor_ref().unwrap().overview_zoom_target(),
        0.5
    );
}

#[test]
fn cross_output_activation_refreshes_overview_state() {
    let (mut layout, _a, b) = pullback_test_layout();

    // Overview opens on the active monitor (A, since it was added first);
    // window 1 lives on B.
    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    let mons = layout.monitors().collect::<Vec<_>>();
    assert!(mons[0].overview_open, "active output A is in overview");
    assert!(!mons[1].overview_open, "non-active output B stays live");

    // A plain `FocusWindow`-style bind to a window on another output, with
    // no `focus_output`/`close_overview()` call around it (unlike the
    // carousel lens focus-jump, which calls both). Before the fix, this
    // left B's `overview_open` stale (still false) instead of becoming the
    // new active monitor's overview participation.
    layout.activate_window(&1);

    layout.verify_invariants();

    assert_eq!(layout.active_output(), Some(&b));

    let active = layout.active_output().cloned().unwrap();
    for mon in layout.monitors() {
        let should_be_open = mon.output() == &active;
        assert_eq!(
            mon.overview_open,
            should_be_open,
            "overview_open for {:?} should match whether it is now active",
            mon.output_name()
        );
    }
}

#[test]
fn output_removal_recenters_ring() {
    fn make_output(name: &str, x: i32) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            Some(Point::from((x, 0))),
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    let mut options = Options::default();
    options.overview.consolidated_carousel = Some(niri_config::ConsolidatedCarousel {
        reveal_zoom: 0.4,
        assembled_zoom: 0.15,
    });

    let mut layout = Layout::<TestWindow>::with_options(Clock::with_time(Duration::ZERO), options);

    let a = make_output("A", 0);
    let b = make_output("B", 1280);
    let c = make_output("C", 2560);

    layout.add_output(a.clone(), None, false);
    layout.add_output(b.clone(), None, false);
    layout.add_output(c.clone(), None, false);

    layout.add_window(
        TestWindow::new(TestWindowParams::new(0)),
        AddWindowTarget::Output(&a),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );
    layout.add_window(
        TestWindow::new(TestWindowParams::new(1)),
        AddWindowTarget::Output(&b),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );
    layout.add_window(
        TestWindow::new(TestWindowParams::new(2)),
        AddWindowTarget::Output(&c),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );

    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    layout.set_overview_zoom_for_test(0.15);

    // Rotate twice to settle on C's ring position.
    layout.rotate_carousel(1);
    layout.rotate_carousel(1);

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    assert_eq!(layout.carousel_rotation_target(), 2.0);
    assert_eq!(layout.carousel_rotation(), 2.0);

    layout.remove_output(&c);

    assert_eq!(layout.carousel_rotation_target(), 0.0);
    assert_eq!(layout.carousel_rotation(), 0.0);
    assert!(!layout.carousel_rotating());
    assert!(!layout.carousel_pullback_is_active());
}

#[test]
fn rotate_noop_single_output() {
    fn make_output(name: &str, x: i32) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            Some(Point::from((x, 0))),
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    let mut options = Options::default();
    options.overview.consolidated_carousel = Some(niri_config::ConsolidatedCarousel {
        reveal_zoom: 0.4,
        assembled_zoom: 0.15,
    });

    let mut layout = Layout::<TestWindow>::with_options(Clock::with_time(Duration::ZERO), options);

    let a = make_output("A", 0);
    layout.add_output(a.clone(), None, false);
    layout.add_window(
        TestWindow::new(TestWindowParams::new(0)),
        AddWindowTarget::Output(&a),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );

    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // `carousel_ring().len() <= 1` should make this an outright no-op: no
    // rotation, and (crucially) no pull-back kicked off either, since a
    // single-output ring has nowhere to rotate to.
    layout.carousel_request_rotate(1);

    assert_eq!(layout.carousel_rotation_target(), 0.0);
    assert_eq!(layout.carousel_rotation(), 0.0);
    assert!(!layout.carousel_rotating());
    assert!(!layout.carousel_pullback_is_active());
}

#[test]
fn active_output_change_rebases_ring() {
    let (mut layout, a, b) = pullback_test_layout();

    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    layout.set_overview_zoom_for_test(0.15);

    // Settle rotated onto B's ring position.
    layout.rotate_carousel(1);

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    assert_eq!(layout.carousel_rotation_target(), 1.0);

    let current = layout.active_output().cloned().unwrap();
    let other = if current == a { &b } else { &a };
    layout.focus_output(other);

    assert_eq!(layout.carousel_rotation_target(), 0.0);
    assert_eq!(layout.carousel_rotation(), 0.0);
    assert!(!layout.carousel_rotating());
}

#[test]
fn active_output_change_restores_stranded_pullback_zoom() {
    let (mut layout, a, b) = pullback_test_layout();

    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Park the zoom above the reveal band and request a rotate: this starts
    // a pull-back that immediately retargets the active monitor's zoom to
    // `assembled_zoom` (0.15) for the ZoomOut phase.
    layout.set_overview_zoom_for_test(0.5);
    layout.carousel_request_rotate(1);

    assert!(layout.carousel_pullback_is_active());

    let prev_active = layout.active_output().cloned().unwrap();
    let prev_zoom_target = layout.active_monitor_ref().unwrap().overview_zoom_target();
    // The pull-back drove it to assembled zoom, not the 0.5 the user parked.
    assert_eq!(prev_zoom_target, 0.15);

    let other = if prev_active == a { &b } else { &a };
    layout.focus_output(other);

    assert!(!layout.carousel_pullback_is_active());

    // The previous monitor must not be left stranded at the assembled zoom;
    // it should be restored (instantly) to the 0.5 the user had parked.
    let prev_monitor = layout
        .monitors()
        .find(|m| m.output() == &prev_active)
        .unwrap();
    assert_eq!(prev_monitor.overview_zoom_target(), 0.5);
}

/// Forces a `TestWindow`'s size and applies it to its `bbox` (mirroring what the fuzz harness's
/// `Op::SetForcedSize` + `Op::Communicate` do for a real client's configure ack), so its tile's
/// activation region actually covers a sane chunk of the workspace. `TestWindow`'s default `bbox`
/// is a small fixed 100x200 placeholder — nowhere near big enough to be hit-tested against a
/// point near the workspace center, and `communicate()` alone is a no-op until either
/// `forced_size` or `requested_size` (the latter only set once the real column-width layout pass
/// runs) is populated.
fn set_window_size_and_communicate(
    layout: &mut Layout<TestWindow>,
    id: usize,
    size: Size<i32, Logical>,
) {
    if let Some((_, w)) = layout.windows().find(|(_, w)| *w.id() == id) {
        w.0.forced_size.set(Some(size));
    }
    let changed = layout
        .windows()
        .find(|(_, w)| *w.id() == id)
        .map(|(_, w)| w.communicate())
        .unwrap_or(false);
    if changed {
        layout.update_window(&id, None);
    }
}

#[test]
fn focus_jump_activates_window_on_settled_remote_output() {
    let (mut layout, _a, b) = pullback_test_layout();
    set_window_size_and_communicate(&mut layout, 0, Size::from((1280, 720)));
    set_window_size_and_communicate(&mut layout, 1, Size::from((1280, 720)));

    // Overview opens on the active monitor (A, since it was added first);
    // window 1 lives on B.
    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    // Assembled zoom skips the pull-back choreography (mirrors
    // `pullback_skipped_when_assembled`), so a single `advance_animations`
    // pass after the rotate request settles the ring on B.
    layout.set_overview_zoom_for_test(0.15);

    layout.carousel_request_rotate(1);

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    assert_eq!(layout.carousel_rotation_target(), 1.0);
    assert!(!layout.carousel_rotating());
    assert!(layout.in_carousel_lens());

    // Center of the settled panel's quad should land on B's only window.
    assert!(layout.carousel_focus_jump((0.5, 0.5)));

    assert!(!layout.is_overview_open());
    assert_eq!(layout.active_output(), Some(&b));
    assert_eq!(
        layout
            .active_monitor_ref()
            .and_then(|m| m.active_window())
            .map(|w| *w.id()),
        Some(1)
    );
}

#[test]
fn focus_jump_is_noop_outside_carousel_lens() {
    let (mut layout, a, _b) = pullback_test_layout();

    layout.toggle_overview(); // open, settled at rotation 0 (host)

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    assert!(!layout.in_carousel_lens());
    assert!(!layout.carousel_focus_jump((0.5, 0.5)));

    assert!(layout.is_overview_open());
    assert_eq!(layout.active_output(), Some(&a));
}

#[test]
fn focus_jump_to_focused_uses_targets_active_window_skipping_uv() {
    let (mut layout, _a, b) = pullback_test_layout();

    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    layout.set_overview_zoom_for_test(0.15);
    layout.carousel_request_rotate(1);

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    assert!(layout.in_carousel_lens());
    assert!(layout.carousel_focus_jump_to_focused());

    assert!(!layout.is_overview_open());
    assert_eq!(layout.active_output(), Some(&b));
    assert_eq!(
        layout
            .active_monitor_ref()
            .and_then(|m| m.active_window())
            .map(|w| *w.id()),
        Some(1)
    );
}

#[test]
fn focus_jump_miss_returns_false() {
    let (mut layout, a, _b) = pullback_test_layout();

    // Deliberately do NOT call `set_window_size_and_communicate`: the default `TestWindow` bbox
    // (100x200) stays small, leaving backdrop/gap around it within the workspace's render-geo
    // box that a uv can land on and miss.
    layout.toggle_overview(); // open

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    layout.set_overview_zoom_for_test(0.15);
    layout.carousel_request_rotate(1);

    layout.clock.set_complete_instantly(true);
    layout.advance_animations();
    layout.clock.set_complete_instantly(false);

    assert!(layout.in_carousel_lens());

    // Near a corner of the settled panel's quad: the small default-sized window doesn't reach
    // there, so this lands on backdrop/gap, not a window.
    assert!(!layout.carousel_focus_jump((0.01, 0.99)));

    // No-op on a miss: overview stays open, active output unchanged (still A, the host — a
    // successful jump would have moved it to B, see
    // `focus_jump_activates_window_on_settled_remote_output`).
    assert!(layout.is_overview_open());
    assert_eq!(layout.active_output(), Some(&a));
}

#[test]
fn rotate_carousel_single_output_is_noop() {
    fn make_output(name: &str) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            None,
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    // Single eligible output -- rotate_carousel's ring-len < 2 guard must make this a no-op.
    let mut layout = Layout::<TestWindow>::default();

    let a = make_output("A");
    layout.add_output(a.clone(), None, false);

    layout.add_window(
        TestWindow::new(TestWindowParams::new(0)),
        AddWindowTarget::Output(&a),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );

    layout.reset_carousel_center();
    assert_eq!(layout.carousel_rotation_target(), 0.0);

    layout.rotate_carousel(1);
    assert_eq!(layout.carousel_rotation_target(), 0.0);

    layout.rotate_carousel(-1);
    assert_eq!(layout.carousel_rotation_target(), 0.0);
}

#[test]
fn reset_carousel_center_noop_when_active_not_in_set() {
    fn make_output(name: &str) -> Output {
        let output = Output::new(
            name.to_owned(),
            PhysicalProperties {
                size: Size::from((1280, 720)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: Size::from((1280, 720)),
                refresh: 60000,
            }),
            None,
            None,
            None,
        );
        output.user_data().insert_if_missing(|| OutputName {
            connector: name.to_owned(),
            make: None,
            model: None,
            serial: None,
        });
        output
    }

    // "A" is the active output (first-added, empty -- no window, so excluded from
    // carousel_outputs()). "B" and "C" have windows and are in the carousel set.
    let mut layout = Layout::<TestWindow>::default();

    let a = make_output("A");
    let b = make_output("B");
    let c = make_output("C");

    layout.add_output(a.clone(), None, false);
    layout.add_output(b.clone(), None, false);
    layout.add_output(c.clone(), None, false);

    layout.add_window(
        TestWindow::new(TestWindowParams::new(0)),
        AddWindowTarget::Output(&b),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );
    layout.add_window(
        TestWindow::new(TestWindowParams::new(1)),
        AddWindowTarget::Output(&c),
        None,
        None,
        false,
        false,
        false,
        ActivateWindow::default(),
        false,
    );

    let names: Vec<String> = layout
        .carousel_outputs()
        .iter()
        .map(|m| m.output_name().clone())
        .collect();
    assert_eq!(names, vec!["B".to_string(), "C".to_string()]); // "A" excluded (empty)

    // Directly set a known, non-default rotation target (private field, accessible
    // from this child module) to prove reset_carousel_center() leaves it untouched.
    layout.carousel_rotation = 1.0;

    layout.reset_carousel_center();
    assert_eq!(layout.carousel_rotation_target(), 1.0); // unchanged: active "A" not in set
}

#[test]
fn vertical_main_axis_preserves_physical_struts_and_bounds() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;
    options.layout.struts = Struts {
        left: FloatOrInt(31.),
        right: FloatOrInt(73.),
        top: FloatOrInt(19.),
        bottom: FloatOrInt(47.),
    };
    let layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
        ],
    );
    let ws = layout.active_workspace().unwrap();
    let bounds = ws
        .scrolling()
        .new_window_toplevel_bounds(&ResolvedWindowRules::default());
    assert_eq!(
        bounds,
        Size::from((1280 - 31 - 73 - 32, 720 - 19 - 47 - 32))
    );
    let (_, position, _) = ws.tiles_with_render_positions().next().unwrap();
    assert_eq!(position, Point::from((31. + 16., 19. + 16.)));
}

#[test]
fn vertical_main_axis_carousel_geometry_matches_live_overview() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;
    let layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::FocusWorkspaceDown,
            Op::AddWindow {
                params: TestWindowParams::new(2),
            },
            Op::ToggleOverview,
            Op::CompleteAnimations,
        ],
    );
    let monitor = layout.monitors().next().unwrap();
    let zoom = monitor.overview_zoom();
    let live: Vec<_> = monitor
        .workspaces_with_render_geo()
        .map(|(ws, geo)| (ws.id(), geo))
        .collect();
    let panel: Vec<_> = monitor
        .workspaces_with_render_geo_at_zoom(zoom)
        .map(|(ws, geo)| (ws.id(), geo))
        .collect();
    assert_eq!(live, panel);
    assert!(live.len() > 1);
}

#[test]
fn vertical_main_axis_output_and_workspace_overrides_reload_and_transfer() {
    let vertical = niri_config::LayoutPart {
        main_axis: Some(MainAxis::Vertical),
        ..Default::default()
    };
    let horizontal = niri_config::LayoutPart {
        main_axis: Some(MainAxis::Horizontal),
        ..Default::default()
    };
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddScaledOutput {
            id: 2,
            scale: 1.25,
            layout_config: Some(Box::new(vertical.clone())),
        },
        Op::FocusOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
    ]);
    assert_eq!(
        layout.active_workspace().unwrap().main_axis(),
        MainAxis::Vertical
    );
    let positions: Vec<_> = layout
        .active_workspace()
        .unwrap()
        .tiles_with_render_positions()
        .map(|(_, pos, _)| pos)
        .collect();
    assert_eq!(positions[0].x, positions[1].x);
    assert_ne!(positions[0].y, positions[1].y);
    check_ops_on_layout(
        &mut layout,
        [Op::UpdateOutputLayoutConfig {
            id: 2,
            layout_config: Some(Box::new(horizontal.clone())),
        }],
    );
    assert_eq!(
        layout.active_workspace().unwrap().main_axis(),
        MainAxis::Horizontal
    );
    check_ops_on_layout(
        &mut layout,
        [Op::AddNamedWorkspace {
            ws_name: 1,
            output_names: vec![2],
            layout_config: Some(Box::new(vertical)),
        }],
    );
    let ws = layout
        .workspaces()
        .find(|(_, _, ws)| ws.name().is_some())
        .unwrap()
        .2;
    assert_eq!(ws.main_axis(), MainAxis::Vertical);
    check_ops_on_layout(&mut layout, [Op::RemoveOutput(2)]);
    // A named workspace keeps its override after migration to a landscape output.
    let ws = layout
        .workspaces()
        .find(|(_, _, ws)| ws.name().is_some())
        .unwrap()
        .2;
    assert_eq!(ws.main_axis(), MainAxis::Vertical);
}

#[test]
fn vertical_main_axis_directional_focus_follows_screen() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::AddWindow {
                params: TestWindowParams::new(2),
            },
            Op::AddWindow {
                params: TestWindowParams::new(3),
            },
            Op::ConsumeOrExpelWindowLeft { id: None },
        ],
    );
    assert_eq!(*layout.focus().unwrap().id(), 3);
    layout.focus_left();
    assert_eq!(*layout.focus().unwrap().id(), 2);
    layout.focus_up();
    assert_eq!(*layout.focus().unwrap().id(), 1);
    layout.focus_down();
    assert_eq!(*layout.focus().unwrap().id(), 2);
    layout.focus_right();
    assert_eq!(*layout.focus().unwrap().id(), 3);
    layout.verify_invariants();
}

#[test]
fn vertical_main_axis_tab_indicator_hit_stays_on_physical_top() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;
    options.layout.default_column_display = ColumnDisplay::Tabbed;
    options.layout.tab_indicator.position = TabIndicatorPosition::Top;
    options.layout.tab_indicator.place_within_column = true;
    options.layout.tab_indicator.hide_when_single_tab = false;
    options.layout.tab_indicator.width = 8.;
    options.layout.tab_indicator.gap = 4.;
    let layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::CompleteAnimations,
        ],
    );
    let ws = layout.active_workspace().unwrap();
    let (tile, pos, _) = ws.tiles_with_render_positions().next().unwrap();
    let pointer = pos + Point::from((tile.tile_size().w / 2., -8.));
    let (window, hit) = ws
        .scrolling()
        .window_under(pointer)
        .expect("top tab must be clickable");
    assert_eq!(*window.id(), 1);
    assert!(matches!(
        hit,
        HitType::Activate {
            is_tab_indicator: true
        }
    ));
    assert_eq!(pos.y, 16. + 12.);
}

#[test]
fn vertical_main_axis_drag_keeps_physical_pointer_anchor() {
    let mut options = Options::default();
    options.layout.main_axis = MainAxis::Vertical;
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::CompleteAnimations,
        ],
    );
    let ws = layout.active_workspace().unwrap();
    let (tile, pos, _) = ws.tiles_with_render_positions().next().unwrap();
    let size = tile.window_size();
    let pointer = pos + tile.window_loc() + Point::from((size.w * 0.25, size.h * 0.75));
    let output = layout.outputs().next().unwrap().clone();
    assert!(layout.interactive_move_begin(1, &output, pointer));
    let Some(InteractiveMoveState::Starting {
        pointer_ratio_within_window,
        ..
    }) = &layout.interactive_move
    else {
        panic!("drag must start");
    };
    assert_eq!(*pointer_ratio_within_window, (0.25, 0.75));

    let delta = Point::from((0., 300.));
    let pointer = pointer + delta;
    assert!(layout.interactive_move_update(&1, delta, output, pointer));
    check_ops_on_layout(&mut layout, [Op::CompleteAnimations]);
    let Some(InteractiveMoveState::Moving(moving)) = &layout.interactive_move else {
        panic!("drag must detach after crossing the threshold");
    };
    let actual_anchor = moving.tile_render_location(1.)
        + moving.tile.window_loc()
        + Point::from((
            moving.tile.window_size().w * 0.25,
            moving.tile.window_size().h * 0.75,
        ));
    assert!((actual_anchor.x - pointer.x).abs() <= 1.);
    assert!((actual_anchor.y - pointer.y).abs() <= 1.);
}

#[test]
fn rainbow_ripple_redraw_visibility() {
    decoration_redraw_visibility(false, false);
}

#[test]
fn custom_decoration_redraw_visibility() {
    decoration_redraw_visibility(true, false);
}

#[test]
fn lit_decoration_redraw_visibility() {
    decoration_redraw_visibility(true, true);
}

fn decoration_redraw_visibility(custom: bool, light: bool) {
    let mut options = Options::default();
    if custom {
        options.layout.focus_ring.shader = niri_config::Config::parse_mem(
            r#"layout { focus-ring { shader { source "vec4 ring_color(vec2 p) { return vec4(1.); }"; }; }; }"#,
        ).unwrap().layout.focus_ring.shader;
    } else {
        options.layout.focus_ring.rainbow_ripple = Some(Default::default());
    }
    if light {
        options.layout.focus_ring.shader.as_mut().unwrap().light = niri_config::Config::parse_mem(
            r#"layout { focus-ring { shader { source "x"; light; }; }; }"#,
        )
        .unwrap()
        .layout
        .focus_ring
        .shader
        .unwrap()
        .light;
    }
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::CompleteAnimations,
        ],
    );
    let output = layout.outputs().next().unwrap().clone();
    layout.update_render_elements(Some(&output));
    assert!(layout.decorations_are_animating(&output));

    layout.clock.set_complete_instantly(true);
    assert!(!layout.decorations_are_animating(&output));
    layout.clock.set_complete_instantly(false);
    layout.clock.set_rate(0.);
    assert!(!layout.decorations_are_animating(&output));
    layout.clock.set_rate(1.);

    check_ops_on_layout(
        &mut layout,
        [Op::FocusWorkspaceDown, Op::CompleteAnimations],
    );
    layout.update_render_elements(Some(&output));
    assert!(
        !layout.decorations_are_animating(&output),
        "a ring on a hidden workspace must not redraw"
    );

    check_ops_on_layout(&mut layout, [Op::FocusWorkspaceUp, Op::CompleteAnimations]);
    layout.update_render_elements(Some(&output));
    assert!(layout.decorations_are_animating(&output));

    check_ops_on_layout(
        &mut layout,
        [
            Op::SetFullscreenWindow {
                window: 1,
                is_fullscreen: true,
            },
            Op::Communicate(1),
            Op::CompleteAnimations,
        ],
    );
    layout.update_render_elements(Some(&output));
    assert!(
        !layout.decorations_are_animating(&output),
        "fullscreen hides the ring"
    );
    check_ops_on_layout(
        &mut layout,
        [
            Op::SetFullscreenWindow {
                window: 1,
                is_fullscreen: false,
            },
            Op::Communicate(1),
            Op::CompleteAnimations,
        ],
    );
    layout.update_render_elements(Some(&output));
    assert!(layout.decorations_are_animating(&output));

    let mut options = (*layout.options).clone();
    if custom {
        options.layout.focus_ring.shader.as_mut().unwrap().speed.0 = 0.;
    } else {
        options
            .layout
            .focus_ring
            .rainbow_ripple
            .as_mut()
            .unwrap()
            .speed
            .0 = 0.;
    }
    layout.update_options(options);
    layout.update_render_elements(Some(&output));
    assert!(
        !layout.decorations_are_animating(&output),
        "a frozen rainbow needs no redraws"
    );
}

#[test]
fn egl_decoration_light_precedes_floating_and_tiled_content() {
    decoration_light_stacking_and_fades(false);
}

#[test]
fn egl_focus_fade_keeps_shader_light_in_live_and_carousel_renders() {
    decoration_light_stacking_and_fades(true);
}

fn decoration_light_stacking_and_fades(fade: bool) {
    use smithay::backend::egl::native::EGLSurfacelessDisplay;
    use smithay::backend::egl::{EGLContext, EGLDisplay};
    use smithay::backend::renderer::element::{Element as _, Id};
    use smithay::backend::renderer::gles::GlesRenderer;

    use crate::render_helpers::{resources, shaders, RenderCtx, RenderTarget};

    let config = niri_config::Config::parse_mem(r#"
        layout {
            border { on; }
            focus-ring { shader { source "vec4 ring_color(vec2 p) { return vec4(0.2, 0.8, 1., 1.); }"; light; }; }
        }
    "#).unwrap();
    let mut renderer = unsafe {
        let display = EGLDisplay::new(EGLSurfacelessDisplay).unwrap();
        let context = EGLContext::new(&display).unwrap();
        GlesRenderer::new(context).unwrap()
    };
    resources::init(&mut renderer);
    shaders::init(&mut renderer);
    shaders::set_decoration_programs(&mut renderer, &config);
    let mut options = Options {
        layout: config.layout,
        ..Default::default()
    };
    if fade {
        options.animations.focus_ring.0 = niri_config::Animation {
            off: false,
            kind: niri_config::animations::Kind::Easing(niri_config::animations::EasingParams {
                duration_ms: 1000,
                curve: niri_config::animations::Curve::Linear,
            }),
        };
    }
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::AddWindow {
                params: TestWindowParams {
                    is_floating: true,
                    ..TestWindowParams::new(2)
                },
            },
            Op::FocusWindow(1),
            Op::CompleteAnimations,
        ],
    );
    let output = layout.outputs().next().unwrap().clone();
    layout.refresh(true);
    layout.update_render_elements(Some(&output));
    let light_ids: Vec<_> = layout
        .active_workspace()
        .unwrap()
        .tiles()
        .filter_map(|tile| tile.focus_ring().light_id())
        .collect();
    assert_eq!(light_ids.len(), 1);
    let render = |layout: &Layout<TestWindow>, renderer: &mut GlesRenderer, overview| {
        let mut ids: Vec<Id> = Vec::new();
        let ctx = RenderCtx {
            renderer,
            target: RenderTarget::Output,
            xray: None,
            shader_time: 0.,
        };
        let monitor = layout.monitor_for_output(&output).unwrap();
        if overview {
            monitor.render_overview_at_zoom(ctx, 0.5, true, &mut |e| ids.push(e.id().clone()));
        } else {
            monitor.render_workspaces(ctx, true, &mut |e| ids.push(e.id().clone()));
        }
        ids
    };
    let normal = render(&layout, &mut renderer, false);
    let overview = render(&layout, &mut renderer, true);
    for ids in [&normal, &overview] {
        assert!(
            light_ids.contains(&ids[0]),
            "spill must appear above floating content too"
        );
        assert!(
            ids.len() > light_ids.len(),
            "fixture must contain ordinary decorations"
        );
        assert!(!ids[light_ids.len()..]
            .iter()
            .any(|id| light_ids.contains(id)));
    }
    if fade {
        layout.focus_floating();
        layout.refresh(true);
        Op::AdvanceAnimations { msec_delta: 400 }.apply(&mut layout);
        layout.update_render_elements(Some(&output));
        for overview in [false, true] {
            let ids = render(&layout, &mut renderer, overview);
            assert!(
                ids.contains(&light_ids[0]),
                "outgoing shader light must survive during fade-out"
            );
        }
        Op::CompleteAnimations.apply(&mut layout);
        layout.update_render_elements(Some(&output));
        for overview in [false, true] {
            assert!(!render(&layout, &mut renderer, overview).contains(&light_ids[0]));
        }
        layout.focus_tiling();
        layout.refresh(true);
        Op::CompleteAnimations.apply(&mut layout);
        layout.update_render_elements(Some(&output));
    }
    let mut options = (*layout.options).clone();
    options.layout.focus_ring.shader.as_mut().unwrap().light = None;
    layout.update_options(options);
    layout.update_render_elements(Some(&output));
    for (before, overview) in [(normal, false), (overview, true)] {
        let ordinary: Vec<_> = before
            .into_iter()
            .filter(|id| !light_ids.contains(id))
            .collect();
        assert_eq!(
            ordinary,
            render(&layout, &mut renderer, overview),
            "normal stacking order must not change"
        );
    }
}

#[test]
fn popup_overflow_uses_output_bounds_on_both_axes() {
    for main_axis in [MainAxis::Horizontal, MainAxis::Vertical] {
        let mut options = Options::default();
        options.layout.main_axis = main_axis;
        let mut layout = check_ops_with_options(
            options,
            [
                Op::AddOutput(1),
                Op::AddWindow {
                    params: TestWindowParams::new(1),
                },
                Op::AddWindow {
                    params: TestWindowParams::new(2),
                },
            ],
        );
        layout.clock.set_complete_instantly(true);
        layout.advance_animations();
        let ws = layout.active_workspace().unwrap();
        for (tile, position, _) in ws.tiles_with_render_positions() {
            let id = tile.window().id();
            let window_origin = position + tile.window_loc();
            let overflow = layout.popup_target_rect(id, true);
            assert_eq!(overflow.size, Size::from((1280., 720.)), "{main_axis:?}");
            assert_eq!(
                overflow.loc + window_origin,
                Point::from((0., 0.)),
                "{main_axis:?}"
            );

            let regular = layout.popup_target_rect(id, false);
            match main_axis {
                MainAxis::Horizontal => {
                    assert_eq!(regular.size.w, tile.window_size().w);
                    assert_eq!(regular.size.h, 720.);
                    assert_eq!(regular.loc.x, 0.);
                    assert_eq!(regular.loc.y + window_origin.y, 0.);
                }
                MainAxis::Vertical => {
                    assert_eq!(regular.size.w, 1280.);
                    assert_eq!(regular.size.h, tile.window_size().h);
                    assert_eq!(regular.loc.x + window_origin.x, 0.);
                    assert_eq!(regular.loc.y, 0.);
                }
            }
        }
    }
}
