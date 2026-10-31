use std::str::FromStr;
use std::time::Duration;

use bitflags::bitflags;
use knuffel::errors::DecodeError;
use miette::miette;
use niri_ipc::{
    ColumnDisplay, LayoutSwitchTarget, PositionChange, SizeChange, WorkspaceReferenceArg,
};
use smithay::input::keyboard::keysyms::KEY_NoSymbol;
use smithay::input::keyboard::xkb::{keysym_from_name, KEYSYM_CASE_INSENSITIVE, KEYSYM_NO_FLAGS};
use smithay::input::keyboard::Keysym;
use tracing::warn;

use crate::input::ModKey;
use crate::recent_windows::{MruDirection, MruFilter, MruScope};
use crate::utils::{expect_only_children, MergeWith};

#[derive(Debug, Default, PartialEq)]
pub struct Binds(pub Vec<Bind>);

#[derive(Debug, Clone, PartialEq)]
pub struct Bind {
    pub key: Key,
    pub action: BoundAction,
    pub repeat: bool,
    pub pass_through: Option<bool>,
    pub cooldown: Option<Duration>,
    pub allow_when_locked: bool,
    pub allow_inhibiting: bool,
    pub hotkey_overlay_title: Option<Option<String>>,
}

/// The actions a bind runs. Each list is non-empty and runs in order.
#[derive(Debug, Clone, PartialEq)]
pub enum BoundAction {
    Press(Vec<Action>),
    Release(Vec<Action>),
    Both {
        press: Vec<Action>,
        release: Vec<Action>,
    },
}

impl BoundAction {
    /// Iterates over all press actions, then all release actions.
    pub fn iter(&self) -> impl Iterator<Item = &Action> {
        let (press, release): (&[Action], &[Action]) = match self {
            BoundAction::Press(press) => (press, &[]),
            BoundAction::Release(release) => (&[], release),
            BoundAction::Both { press, release } => (press, release),
        };
        press.iter().chain(release)
    }
}

impl Bind {
    pub fn has_press(&self) -> bool {
        matches!(
            self.action,
            BoundAction::Press(_) | BoundAction::Both { .. }
        )
    }

    pub fn has_release(&self) -> bool {
        matches!(
            self.action,
            BoundAction::Release(_) | BoundAction::Both { .. }
        )
    }

    /// Returns the press actions, or an empty slice for release-only binds.
    pub fn press_actions(&self) -> &[Action] {
        match &self.action {
            BoundAction::Press(actions) => actions,
            BoundAction::Both { press, .. } => press,
            BoundAction::Release(_) => &[],
        }
    }

    /// Returns the release actions, or an empty slice for press-only binds.
    pub fn release_actions(&self) -> &[Action] {
        match &self.action {
            BoundAction::Release(actions) => actions,
            BoundAction::Both { release, .. } => release,
            BoundAction::Press(_) => &[],
        }
    }

    /// Returns the press actions if `pressed`, otherwise the release actions.
    pub fn into_actions(self, pressed: bool) -> Option<Vec<Action>> {
        match (self.action, pressed) {
            (BoundAction::Press(actions), true) | (BoundAction::Release(actions), false) => {
                Some(actions)
            }
            (BoundAction::Both { press, .. }, true) => Some(press),
            (BoundAction::Both { release, .. }, false) => Some(release),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub struct Key {
    pub trigger: Trigger,
    pub modifiers: Modifiers,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum Trigger {
    Keysym(Keysym),
    CompositorMod,
    Modifier(ModKey),
    MouseLeft,
    MouseRight,
    MouseMiddle,
    MouseBack,
    MouseForward,
    WheelScrollDown,
    WheelScrollUp,
    WheelScrollLeft,
    WheelScrollRight,
    TouchpadScrollDown,
    TouchpadScrollUp,
    TouchpadScrollLeft,
    TouchpadScrollRight,
    TabletStylusButton1,
    TabletStylusButton2,
    TabletStylusButton3,
}

/// Returns whether `keysym` is a modifier key like `Shift_L`.
///
/// This function matches what the niri config considers modifiers.
/// So, it does not correspond exactly to `keysym.is_modifier_key()`.
/// For example, unlike XKB, `Caps_Lock` and `ISO_Next_Group` aren't considered modifiers.
pub fn is_keysym_modifier(keysym: Keysym) -> bool {
    ModKey::from_keysym(keysym).is_some()
}

impl Trigger {
    pub fn is_modifier(self) -> bool {
        match self {
            Self::CompositorMod | Self::Modifier(_) => true,
            Self::Keysym(keysym) => is_keysym_modifier(keysym),
            _ => false,
        }
    }

    pub fn is_generic_modifier(self) -> bool {
        matches!(self, Self::CompositorMod | Self::Modifier(_))
    }

    /// The modifier flags that this trigger sets, if it is a modifier key.
    pub fn implied_modifier(self) -> Modifiers {
        match self {
            Self::CompositorMod => Modifiers::COMPOSITOR,
            Self::Modifier(modifier) => modifier.to_modifiers(),
            Self::Keysym(keysym) => ModKey::from_keysym(keysym)
                .map_or(Modifiers::empty(), |modifier| modifier.to_modifiers()),
            _ => Modifiers::empty(),
        }
    }

    pub fn is_keyboard(self) -> bool {
        matches!(
            self,
            Trigger::Keysym(_) | Trigger::CompositorMod | Trigger::Modifier(_)
        )
    }

    pub fn is_scroll(self) -> bool {
        matches!(
            self,
            Trigger::WheelScrollDown
                | Trigger::WheelScrollUp
                | Trigger::WheelScrollLeft
                | Trigger::WheelScrollRight
                | Trigger::TouchpadScrollDown
                | Trigger::TouchpadScrollUp
                | Trigger::TouchpadScrollLeft
                | Trigger::TouchpadScrollRight
        )
    }
}

impl Key {
    /// Whether the two keys are triggered by the same key press.
    ///
    /// Two different keybinds can be triggered by the same key press. For example, a modifier key
    /// can be spelled as the modifier itself (`Alt`) or as its keysym (`Alt_L`).
    pub fn conflicts_with(&self, other: &Self) -> bool {
        // The trigger and the modifiers match exactly.
        //
        // All non-modifier triggers fall into this category.
        if *self == *other {
            return true;
        }

        // Modifiers include the trigger's implied modifier, so differing modifiers can never be
        // triggered by the same key press.
        if self.modifiers != other.modifiers {
            return false;
        }

        // Generic-modifier-only binds are press-order-independent, so only their modifier set
        // (which already includes the trigger implied modifier) has to match.
        (self.trigger.is_generic_modifier() && other.trigger.is_modifier())
            || (self.trigger.is_modifier() && other.trigger.is_generic_modifier())
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Modifiers : u8 {
        const CTRL = 1;
        const SHIFT = 1 << 1;
        const ALT = 1 << 2;
        const SUPER = 1 << 3;
        const ISO_LEVEL3_SHIFT = 1 << 4;
        const ISO_LEVEL5_SHIFT = 1 << 5;
        const COMPOSITOR = 1 << 6;
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct SwitchBinds {
    #[knuffel(child)]
    pub lid_open: Option<SwitchAction>,
    #[knuffel(child)]
    pub lid_close: Option<SwitchAction>,
    #[knuffel(child)]
    pub tablet_mode_on: Option<SwitchAction>,
    #[knuffel(child)]
    pub tablet_mode_off: Option<SwitchAction>,
}

impl MergeWith<SwitchBinds> for SwitchBinds {
    fn merge_with(&mut self, part: &SwitchBinds) {
        merge_clone_opt!(
            (self, part),
            lid_open,
            lid_close,
            tablet_mode_on,
            tablet_mode_off,
        );
    }
}

#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub struct SwitchAction {
    #[knuffel(child, unwrap(arguments))]
    pub spawn: Vec<String>,
}

// Remember to add new actions to the CLI enum too.
#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub enum Action {
    Quit(#[knuffel(property(name = "skip-confirmation"), default)] bool),
    #[knuffel(skip)]
    ChangeVt(i32),
    Suspend,
    PowerOffMonitors(#[knuffel(property(name = "skip-isolated"), default)] bool),
    PowerOnMonitors,
    ToggleDebugTint,
    DebugToggleOpaqueRegions,
    DebugToggleDamage,
    ToggleTouchpad,
    Spawn(#[knuffel(arguments)] Vec<String>),
    SpawnSh(#[knuffel(argument)] String),
    DoScreenTransition(#[knuffel(property(name = "delay-ms"))] Option<u16>),
    #[knuffel(skip)]
    ConfirmScreenshot {
        write_to_disk: bool,
    },
    #[knuffel(skip)]
    CancelScreenshot,
    #[knuffel(skip)]
    ScreenshotTogglePointer,
    Screenshot(
        #[knuffel(property(name = "show-pointer"), default = true)] bool,
        // Path; not settable from knuffel
        Option<String>,
    ),
    ScreenshotScreen(
        #[knuffel(property(name = "write-to-disk"), default = true)] bool,
        #[knuffel(property(name = "show-pointer"), default = true)] bool,
        // Path; not settable from knuffel
        Option<String>,
    ),
    ScreenshotWindow(
        #[knuffel(property(name = "write-to-disk"), default = true)] bool,
        #[knuffel(property(name = "show-pointer"), default = false)] bool,
        // Path; not settable from knuffel
        Option<String>,
    ),
    #[knuffel(skip)]
    ScreenshotWindowById {
        id: u64,
        write_to_disk: bool,
        show_pointer: bool,
        path: Option<String>,
    },
    ToggleKeyboardShortcutsInhibit,
    CloseWindow,
    #[knuffel(skip)]
    CloseWindowById(u64),
    FullscreenWindow,
    #[knuffel(skip)]
    FullscreenWindowById(u64),
    ToggleWindowedFullscreen,
    #[knuffel(skip)]
    ToggleWindowedFullscreenById(u64),
    #[knuffel(skip)]
    FocusWindow(u64),
    FocusWindowInColumn(#[knuffel(argument)] u8),
    FocusWindowPrevious,
    FocusColumnLeft,
    #[knuffel(skip)]
    FocusColumnLeftUnderMouse,
    FocusColumnRight,
    #[knuffel(skip)]
    FocusColumnRightUnderMouse,
    FocusColumnFirst,
    FocusColumnLast,
    FocusColumnRightOrFirst,
    FocusColumnLeftOrLast,
    FocusColumn(#[knuffel(argument)] usize),
    FocusWindowOrMonitorUp,
    FocusWindowOrMonitorDown,
    FocusColumnOrMonitorLeft,
    FocusColumnOrMonitorRight,
    FocusWindowDown,
    FocusWindowUp,
    FocusWindowDownOrColumnLeft,
    FocusWindowDownOrColumnRight,
    FocusWindowUpOrColumnLeft,
    FocusWindowUpOrColumnRight,
    FocusWindowOrWorkspaceDown,
    FocusWindowOrWorkspaceUp,
    FocusWindowTop,
    FocusWindowBottom,
    FocusWindowDownOrTop,
    FocusWindowUpOrBottom,
    MoveColumnLeft,
    MoveColumnRight,
    MoveColumnToFirst,
    MoveColumnToLast,
    MoveColumnLeftOrToMonitorLeft,
    MoveColumnRightOrToMonitorRight,
    MoveColumnToIndex(#[knuffel(argument)] usize),
    MoveWindowDown,
    MoveWindowUp,
    MoveWindowDownOrToWorkspaceDown,
    MoveWindowUpOrToWorkspaceUp,
    ConsumeOrExpelWindowLeft,
    #[knuffel(skip)]
    ConsumeOrExpelWindowLeftById(u64),
    ConsumeOrExpelWindowRight,
    #[knuffel(skip)]
    ConsumeOrExpelWindowRightById(u64),
    ConsumeWindowIntoColumn,
    ExpelWindowFromColumn,
    SwapWindowLeft,
    SwapWindowRight,
    ToggleColumnTabbedDisplay,
    SetColumnDisplay(#[knuffel(argument, str)] ColumnDisplay),
    CenterColumn,
    CenterWindow,
    #[knuffel(skip)]
    CenterWindowById(u64),
    CenterVisibleColumns,
    FocusWorkspaceDown,
    #[knuffel(skip)]
    FocusWorkspaceDownUnderMouse,
    FocusWorkspaceUp,
    #[knuffel(skip)]
    FocusWorkspaceUpUnderMouse,
    FocusWorkspace(#[knuffel(argument)] WorkspaceReference),
    FocusWorkspacePrevious,
    MoveWindowToWorkspaceDown(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveWindowToWorkspaceUp(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveWindowToWorkspace(
        #[knuffel(argument)] WorkspaceReference,
        #[knuffel(property(name = "focus"), default = true)] bool,
    ),
    #[knuffel(skip)]
    MoveWindowToWorkspaceById {
        window_id: u64,
        reference: WorkspaceReference,
        focus: bool,
    },
    MoveColumnToWorkspaceDown(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveColumnToWorkspaceUp(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveColumnToWorkspace(
        #[knuffel(argument)] WorkspaceReference,
        #[knuffel(property(name = "focus"), default = true)] bool,
    ),
    MoveWorkspaceDown,
    MoveWorkspaceUp,
    MoveWorkspaceToIndex(#[knuffel(argument)] usize),
    #[knuffel(skip)]
    MoveWorkspaceToIndexByRef {
        new_idx: usize,
        reference: WorkspaceReference,
    },
    #[knuffel(skip)]
    MoveWorkspaceToMonitorByRef {
        output_name: String,
        reference: WorkspaceReference,
    },
    MoveWorkspaceToMonitor(#[knuffel(argument)] String),
    SetWorkspaceName(#[knuffel(argument)] String),
    #[knuffel(skip)]
    SetWorkspaceNameByRef {
        name: String,
        reference: WorkspaceReference,
    },
    UnsetWorkspaceName,
    #[knuffel(skip)]
    UnsetWorkSpaceNameByRef(#[knuffel(argument)] WorkspaceReference),
    FocusMonitorLeft,
    FocusMonitorRight,
    FocusMonitorDown,
    FocusMonitorUp,
    FocusMonitorPrevious,
    FocusMonitorNext,
    FocusMonitor(#[knuffel(argument)] String),
    MoveWindowToMonitorLeft,
    MoveWindowToMonitorRight,
    MoveWindowToMonitorDown,
    MoveWindowToMonitorUp,
    MoveWindowToMonitorPrevious,
    MoveWindowToMonitorNext,
    MoveWindowToMonitor(#[knuffel(argument)] String),
    #[knuffel(skip)]
    MoveWindowToMonitorById {
        id: u64,
        output: String,
    },
    MoveColumnToMonitorLeft,
    MoveColumnToMonitorRight,
    MoveColumnToMonitorDown,
    MoveColumnToMonitorUp,
    MoveColumnToMonitorPrevious,
    MoveColumnToMonitorNext,
    MoveColumnToMonitor(#[knuffel(argument)] String),
    SetWindowWidth(#[knuffel(argument, str)] SizeChange),
    #[knuffel(skip)]
    SetWindowWidthById {
        id: u64,
        change: SizeChange,
    },
    SetWindowHeight(#[knuffel(argument, str)] SizeChange),
    #[knuffel(skip)]
    SetWindowHeightById {
        id: u64,
        change: SizeChange,
    },
    ResetWindowHeight,
    #[knuffel(skip)]
    ResetWindowHeightById(u64),
    SwitchPresetColumnWidth,
    SwitchPresetColumnWidthBack,
    SwitchPresetWindowWidth,
    SwitchPresetWindowWidthBack,
    #[knuffel(skip)]
    SwitchPresetWindowWidthById(u64),
    #[knuffel(skip)]
    SwitchPresetWindowWidthBackById(u64),
    SwitchPresetWindowHeight,
    SwitchPresetWindowHeightBack,
    #[knuffel(skip)]
    SwitchPresetWindowHeightById(u64),
    #[knuffel(skip)]
    SwitchPresetWindowHeightBackById(u64),
    MaximizeColumn,
    MaximizeWindowToEdges,
    #[knuffel(skip)]
    MaximizeWindowToEdgesById(u64),
    SetColumnWidth(#[knuffel(argument, str)] SizeChange),
    ExpandColumnToAvailableWidth,
    SwitchLayout(#[knuffel(argument, str)] LayoutSwitchTarget),
    ShowHotkeyOverlay,
    ToggleWorkspaceVisibility(
        #[knuffel(argument, str)] String,
        #[knuffel(property(name = "focus"), default = false)] bool,
    ),
    HideWorkspace(#[knuffel(argument, str)] String),
    UnhideWorkspace(
        #[knuffel(argument, str)] String,
        #[knuffel(property(name = "focus"), default = false)] bool,
    ),
    MoveWorkspaceToMonitorLeft,
    MoveWorkspaceToMonitorRight,
    MoveWorkspaceToMonitorDown,
    MoveWorkspaceToMonitorUp,
    MoveWorkspaceToMonitorPrevious,
    MoveWorkspaceToMonitorNext,
    ToggleWindowFloating,
    #[knuffel(skip)]
    ToggleWindowFloatingById(u64),
    ToggleWindowShader,
    #[knuffel(skip)]
    ToggleWindowShaderById(u64),
    CycleWindowShader,
    #[knuffel(skip)]
    CycleWindowShaderById(u64),
    ToggleOutputShader(#[knuffel(argument)] Option<String>),
    CycleOutputShader(#[knuffel(argument)] Option<String>),
    ToggleWindowSticky,
    #[knuffel(skip)]
    ToggleWindowStickyById(u64),
    MoveWindowToFloating,
    #[knuffel(skip)]
    MoveWindowToFloatingById(u64),
    MoveWindowToTiling,
    #[knuffel(skip)]
    MoveWindowToTilingById(u64),
    FocusFloating,
    FocusTiling,
    SwitchFocusBetweenFloatingAndTiling,
    #[knuffel(skip)]
    MoveFloatingWindowById {
        id: Option<u64>,
        x: PositionChange,
        y: PositionChange,
    },
    ToggleWindowRuleOpacity,
    #[knuffel(skip)]
    ToggleWindowRuleOpacityById(u64),
    SetDynamicCastWindow,
    #[knuffel(skip)]
    SetDynamicCastWindowById(u64),
    SetDynamicCastMonitor(#[knuffel(argument)] Option<String>),
    ClearDynamicCastTarget,
    #[knuffel(skip)]
    StopCast(u64),
    ToggleDwt,
    ToggleOverview,
    OpenOverview,
    CloseOverview,
    OverviewZoomCycle(#[knuffel(property(name = "reverse"), default)] bool),
    OverviewZoomIn,
    OverviewZoomOut,
    #[knuffel(skip)]
    ToggleWindowUrgent(u64),
    #[knuffel(skip)]
    SetWindowUrgent(u64),
    #[knuffel(skip)]
    UnsetWindowUrgent(u64),
    #[knuffel(skip)]
    LoadConfigFile(#[knuffel(argument)] Option<String>),
    #[knuffel(skip)]
    MruAdvance {
        direction: MruDirection,
        scope: Option<MruScope>,
        filter: Option<MruFilter>,
    },
    #[knuffel(skip)]
    MruConfirm,
    #[knuffel(skip)]
    MruCancel,
    #[knuffel(skip)]
    MruCloseCurrentWindow,
    #[knuffel(skip)]
    MruFirst,
    #[knuffel(skip)]
    MruLast,
    #[knuffel(skip)]
    MruSetScope(MruScope),
    #[knuffel(skip)]
    MruCycleScope,

    // Doesn't look like knuffel allows gating on #[cfg(test)]...
    #[knuffel(skip)]
    TestAction,
}

impl From<niri_ipc::Action> for Action {
    fn from(value: niri_ipc::Action) -> Self {
        match value {
            niri_ipc::Action::Quit { skip_confirmation } => Self::Quit(skip_confirmation),
            niri_ipc::Action::PowerOffMonitors { skip_isolated } => {
                Self::PowerOffMonitors(skip_isolated)
            }
            niri_ipc::Action::PowerOnMonitors {} => Self::PowerOnMonitors,
            niri_ipc::Action::Spawn { command } => Self::Spawn(command),
            niri_ipc::Action::SpawnSh { command } => Self::SpawnSh(command),
            niri_ipc::Action::DoScreenTransition { delay_ms } => Self::DoScreenTransition(delay_ms),
            niri_ipc::Action::Screenshot { show_pointer, path } => {
                Self::Screenshot(show_pointer, path)
            }
            niri_ipc::Action::ScreenshotScreen {
                write_to_disk,
                show_pointer,
                path,
            } => Self::ScreenshotScreen(write_to_disk, show_pointer, path),
            niri_ipc::Action::ScreenshotWindow {
                id: None,
                write_to_disk,
                show_pointer,
                path,
            } => Self::ScreenshotWindow(write_to_disk, show_pointer, path),
            niri_ipc::Action::ScreenshotWindow {
                id: Some(id),
                write_to_disk,
                show_pointer,
                path,
            } => Self::ScreenshotWindowById {
                id,
                write_to_disk,
                show_pointer,
                path,
            },
            niri_ipc::Action::ToggleKeyboardShortcutsInhibit {} => {
                Self::ToggleKeyboardShortcutsInhibit
            }
            niri_ipc::Action::CloseWindow { id: None } => Self::CloseWindow,
            niri_ipc::Action::CloseWindow { id: Some(id) } => Self::CloseWindowById(id),
            niri_ipc::Action::FullscreenWindow { id: None } => Self::FullscreenWindow,
            niri_ipc::Action::FullscreenWindow { id: Some(id) } => Self::FullscreenWindowById(id),
            niri_ipc::Action::ToggleWindowedFullscreen { id: None } => {
                Self::ToggleWindowedFullscreen
            }
            niri_ipc::Action::ToggleWindowedFullscreen { id: Some(id) } => {
                Self::ToggleWindowedFullscreenById(id)
            }
            niri_ipc::Action::FocusWindow { id } => Self::FocusWindow(id),
            niri_ipc::Action::FocusWindowInColumn { index } => Self::FocusWindowInColumn(index),
            niri_ipc::Action::FocusWindowPrevious {} => Self::FocusWindowPrevious,
            niri_ipc::Action::FocusColumnLeft {} => Self::FocusColumnLeft,
            niri_ipc::Action::FocusColumnRight {} => Self::FocusColumnRight,
            niri_ipc::Action::FocusColumnFirst {} => Self::FocusColumnFirst,
            niri_ipc::Action::FocusColumnLast {} => Self::FocusColumnLast,
            niri_ipc::Action::FocusColumnRightOrFirst {} => Self::FocusColumnRightOrFirst,
            niri_ipc::Action::FocusColumnLeftOrLast {} => Self::FocusColumnLeftOrLast,
            niri_ipc::Action::FocusColumn { index } => Self::FocusColumn(index),
            niri_ipc::Action::FocusWindowOrMonitorUp {} => Self::FocusWindowOrMonitorUp,
            niri_ipc::Action::FocusWindowOrMonitorDown {} => Self::FocusWindowOrMonitorDown,
            niri_ipc::Action::FocusColumnOrMonitorLeft {} => Self::FocusColumnOrMonitorLeft,
            niri_ipc::Action::FocusColumnOrMonitorRight {} => Self::FocusColumnOrMonitorRight,
            niri_ipc::Action::FocusWindowDown {} => Self::FocusWindowDown,
            niri_ipc::Action::FocusWindowUp {} => Self::FocusWindowUp,
            niri_ipc::Action::FocusWindowDownOrColumnLeft {} => Self::FocusWindowDownOrColumnLeft,
            niri_ipc::Action::FocusWindowDownOrColumnRight {} => Self::FocusWindowDownOrColumnRight,
            niri_ipc::Action::FocusWindowUpOrColumnLeft {} => Self::FocusWindowUpOrColumnLeft,
            niri_ipc::Action::FocusWindowUpOrColumnRight {} => Self::FocusWindowUpOrColumnRight,
            niri_ipc::Action::FocusWindowOrWorkspaceDown {} => Self::FocusWindowOrWorkspaceDown,
            niri_ipc::Action::FocusWindowOrWorkspaceUp {} => Self::FocusWindowOrWorkspaceUp,
            niri_ipc::Action::FocusWindowTop {} => Self::FocusWindowTop,
            niri_ipc::Action::FocusWindowBottom {} => Self::FocusWindowBottom,
            niri_ipc::Action::FocusWindowDownOrTop {} => Self::FocusWindowDownOrTop,
            niri_ipc::Action::FocusWindowUpOrBottom {} => Self::FocusWindowUpOrBottom,
            niri_ipc::Action::MoveColumnLeft {} => Self::MoveColumnLeft,
            niri_ipc::Action::MoveColumnRight {} => Self::MoveColumnRight,
            niri_ipc::Action::MoveColumnToFirst {} => Self::MoveColumnToFirst,
            niri_ipc::Action::MoveColumnToLast {} => Self::MoveColumnToLast,
            niri_ipc::Action::MoveColumnToIndex { index } => Self::MoveColumnToIndex(index),
            niri_ipc::Action::MoveColumnLeftOrToMonitorLeft {} => {
                Self::MoveColumnLeftOrToMonitorLeft
            }
            niri_ipc::Action::MoveColumnRightOrToMonitorRight {} => {
                Self::MoveColumnRightOrToMonitorRight
            }
            niri_ipc::Action::MoveWindowDown {} => Self::MoveWindowDown,
            niri_ipc::Action::MoveWindowUp {} => Self::MoveWindowUp,
            niri_ipc::Action::MoveWindowDownOrToWorkspaceDown {} => {
                Self::MoveWindowDownOrToWorkspaceDown
            }
            niri_ipc::Action::MoveWindowUpOrToWorkspaceUp {} => Self::MoveWindowUpOrToWorkspaceUp,
            niri_ipc::Action::ConsumeOrExpelWindowLeft { id: None } => {
                Self::ConsumeOrExpelWindowLeft
            }
            niri_ipc::Action::ConsumeOrExpelWindowLeft { id: Some(id) } => {
                Self::ConsumeOrExpelWindowLeftById(id)
            }
            niri_ipc::Action::ConsumeOrExpelWindowRight { id: None } => {
                Self::ConsumeOrExpelWindowRight
            }
            niri_ipc::Action::ConsumeOrExpelWindowRight { id: Some(id) } => {
                Self::ConsumeOrExpelWindowRightById(id)
            }
            niri_ipc::Action::ConsumeWindowIntoColumn {} => Self::ConsumeWindowIntoColumn,
            niri_ipc::Action::ExpelWindowFromColumn {} => Self::ExpelWindowFromColumn,
            niri_ipc::Action::SwapWindowRight {} => Self::SwapWindowRight,
            niri_ipc::Action::SwapWindowLeft {} => Self::SwapWindowLeft,
            niri_ipc::Action::ToggleColumnTabbedDisplay {} => Self::ToggleColumnTabbedDisplay,
            niri_ipc::Action::SetColumnDisplay { display } => Self::SetColumnDisplay(display),
            niri_ipc::Action::CenterColumn {} => Self::CenterColumn,
            niri_ipc::Action::CenterWindow { id: None } => Self::CenterWindow,
            niri_ipc::Action::CenterWindow { id: Some(id) } => Self::CenterWindowById(id),
            niri_ipc::Action::CenterVisibleColumns {} => Self::CenterVisibleColumns,
            niri_ipc::Action::FocusWorkspaceDown {} => Self::FocusWorkspaceDown,
            niri_ipc::Action::FocusWorkspaceUp {} => Self::FocusWorkspaceUp,
            niri_ipc::Action::FocusWorkspace { reference } => {
                Self::FocusWorkspace(WorkspaceReference::from(reference))
            }
            niri_ipc::Action::FocusWorkspacePrevious {} => Self::FocusWorkspacePrevious,
            niri_ipc::Action::ToggleWorkspaceVisibility { name, focus } => {
                Self::ToggleWorkspaceVisibility(name, focus)
            }
            niri_ipc::Action::HideWorkspace { name } => Self::HideWorkspace(name),
            niri_ipc::Action::UnhideWorkspace { name, focus } => Self::UnhideWorkspace(name, focus),
            niri_ipc::Action::MoveWindowToWorkspaceDown { focus } => {
                Self::MoveWindowToWorkspaceDown(focus)
            }
            niri_ipc::Action::MoveWindowToWorkspaceUp { focus } => {
                Self::MoveWindowToWorkspaceUp(focus)
            }
            niri_ipc::Action::MoveWindowToWorkspace {
                window_id: None,
                reference,
                focus,
            } => Self::MoveWindowToWorkspace(WorkspaceReference::from(reference), focus),
            niri_ipc::Action::MoveWindowToWorkspace {
                window_id: Some(window_id),
                reference,
                focus,
            } => Self::MoveWindowToWorkspaceById {
                window_id,
                reference: WorkspaceReference::from(reference),
                focus,
            },
            niri_ipc::Action::MoveColumnToWorkspaceDown { focus } => {
                Self::MoveColumnToWorkspaceDown(focus)
            }
            niri_ipc::Action::MoveColumnToWorkspaceUp { focus } => {
                Self::MoveColumnToWorkspaceUp(focus)
            }
            niri_ipc::Action::MoveColumnToWorkspace { reference, focus } => {
                Self::MoveColumnToWorkspace(WorkspaceReference::from(reference), focus)
            }
            niri_ipc::Action::MoveWorkspaceDown {} => Self::MoveWorkspaceDown,
            niri_ipc::Action::MoveWorkspaceUp {} => Self::MoveWorkspaceUp,
            niri_ipc::Action::SetWorkspaceName {
                name,
                workspace: None,
            } => Self::SetWorkspaceName(name),
            niri_ipc::Action::SetWorkspaceName {
                name,
                workspace: Some(reference),
            } => Self::SetWorkspaceNameByRef {
                name,
                reference: WorkspaceReference::from(reference),
            },
            niri_ipc::Action::UnsetWorkspaceName { reference: None } => Self::UnsetWorkspaceName,
            niri_ipc::Action::UnsetWorkspaceName {
                reference: Some(reference),
            } => Self::UnsetWorkSpaceNameByRef(WorkspaceReference::from(reference)),
            niri_ipc::Action::FocusMonitorLeft {} => Self::FocusMonitorLeft,
            niri_ipc::Action::FocusMonitorRight {} => Self::FocusMonitorRight,
            niri_ipc::Action::FocusMonitorDown {} => Self::FocusMonitorDown,
            niri_ipc::Action::FocusMonitorUp {} => Self::FocusMonitorUp,
            niri_ipc::Action::FocusMonitorPrevious {} => Self::FocusMonitorPrevious,
            niri_ipc::Action::FocusMonitorNext {} => Self::FocusMonitorNext,
            niri_ipc::Action::FocusMonitor { output } => Self::FocusMonitor(output),
            niri_ipc::Action::MoveWindowToMonitorLeft {} => Self::MoveWindowToMonitorLeft,
            niri_ipc::Action::MoveWindowToMonitorRight {} => Self::MoveWindowToMonitorRight,
            niri_ipc::Action::MoveWindowToMonitorDown {} => Self::MoveWindowToMonitorDown,
            niri_ipc::Action::MoveWindowToMonitorUp {} => Self::MoveWindowToMonitorUp,
            niri_ipc::Action::MoveWindowToMonitorPrevious {} => Self::MoveWindowToMonitorPrevious,
            niri_ipc::Action::MoveWindowToMonitorNext {} => Self::MoveWindowToMonitorNext,
            niri_ipc::Action::MoveWindowToMonitor { id: None, output } => {
                Self::MoveWindowToMonitor(output)
            }
            niri_ipc::Action::MoveWindowToMonitor {
                id: Some(id),
                output,
            } => Self::MoveWindowToMonitorById { id, output },
            niri_ipc::Action::MoveColumnToMonitorLeft {} => Self::MoveColumnToMonitorLeft,
            niri_ipc::Action::MoveColumnToMonitorRight {} => Self::MoveColumnToMonitorRight,
            niri_ipc::Action::MoveColumnToMonitorDown {} => Self::MoveColumnToMonitorDown,
            niri_ipc::Action::MoveColumnToMonitorUp {} => Self::MoveColumnToMonitorUp,
            niri_ipc::Action::MoveColumnToMonitorPrevious {} => Self::MoveColumnToMonitorPrevious,
            niri_ipc::Action::MoveColumnToMonitorNext {} => Self::MoveColumnToMonitorNext,
            niri_ipc::Action::MoveColumnToMonitor { output } => Self::MoveColumnToMonitor(output),
            niri_ipc::Action::SetWindowWidth { id: None, change } => Self::SetWindowWidth(change),
            niri_ipc::Action::SetWindowWidth {
                id: Some(id),
                change,
            } => Self::SetWindowWidthById { id, change },
            niri_ipc::Action::SetWindowHeight { id: None, change } => Self::SetWindowHeight(change),
            niri_ipc::Action::SetWindowHeight {
                id: Some(id),
                change,
            } => Self::SetWindowHeightById { id, change },
            niri_ipc::Action::ResetWindowHeight { id: None } => Self::ResetWindowHeight,
            niri_ipc::Action::ResetWindowHeight { id: Some(id) } => Self::ResetWindowHeightById(id),
            niri_ipc::Action::SwitchPresetColumnWidth {} => Self::SwitchPresetColumnWidth,
            niri_ipc::Action::SwitchPresetColumnWidthBack {} => Self::SwitchPresetColumnWidthBack,
            niri_ipc::Action::SwitchPresetWindowWidth { id: None } => Self::SwitchPresetWindowWidth,
            niri_ipc::Action::SwitchPresetWindowWidthBack { id: None } => {
                Self::SwitchPresetWindowWidthBack
            }
            niri_ipc::Action::SwitchPresetWindowWidth { id: Some(id) } => {
                Self::SwitchPresetWindowWidthById(id)
            }
            niri_ipc::Action::SwitchPresetWindowWidthBack { id: Some(id) } => {
                Self::SwitchPresetWindowWidthBackById(id)
            }
            niri_ipc::Action::SwitchPresetWindowHeight { id: None } => {
                Self::SwitchPresetWindowHeight
            }
            niri_ipc::Action::SwitchPresetWindowHeightBack { id: None } => {
                Self::SwitchPresetWindowHeightBack
            }
            niri_ipc::Action::SwitchPresetWindowHeight { id: Some(id) } => {
                Self::SwitchPresetWindowHeightById(id)
            }
            niri_ipc::Action::SwitchPresetWindowHeightBack { id: Some(id) } => {
                Self::SwitchPresetWindowHeightBackById(id)
            }
            niri_ipc::Action::MaximizeColumn {} => Self::MaximizeColumn,
            niri_ipc::Action::MaximizeWindowToEdges { id: None } => Self::MaximizeWindowToEdges,
            niri_ipc::Action::MaximizeWindowToEdges { id: Some(id) } => {
                Self::MaximizeWindowToEdgesById(id)
            }
            niri_ipc::Action::SetColumnWidth { change } => Self::SetColumnWidth(change),
            niri_ipc::Action::ExpandColumnToAvailableWidth {} => Self::ExpandColumnToAvailableWidth,
            niri_ipc::Action::SwitchLayout { layout } => Self::SwitchLayout(layout),
            niri_ipc::Action::ShowHotkeyOverlay {} => Self::ShowHotkeyOverlay,
            niri_ipc::Action::MoveWorkspaceToMonitorLeft {} => Self::MoveWorkspaceToMonitorLeft,
            niri_ipc::Action::MoveWorkspaceToMonitorRight {} => Self::MoveWorkspaceToMonitorRight,
            niri_ipc::Action::MoveWorkspaceToMonitorDown {} => Self::MoveWorkspaceToMonitorDown,
            niri_ipc::Action::MoveWorkspaceToMonitorUp {} => Self::MoveWorkspaceToMonitorUp,
            niri_ipc::Action::MoveWorkspaceToMonitorPrevious {} => {
                Self::MoveWorkspaceToMonitorPrevious
            }
            niri_ipc::Action::MoveWorkspaceToIndex {
                index,
                reference: Some(reference),
            } => Self::MoveWorkspaceToIndexByRef {
                new_idx: index,
                reference: WorkspaceReference::from(reference),
            },
            niri_ipc::Action::MoveWorkspaceToIndex {
                index,
                reference: None,
            } => Self::MoveWorkspaceToIndex(index),
            niri_ipc::Action::MoveWorkspaceToMonitor {
                output,
                reference: Some(reference),
            } => Self::MoveWorkspaceToMonitorByRef {
                output_name: output,
                reference: WorkspaceReference::from(reference),
            },
            niri_ipc::Action::MoveWorkspaceToMonitor {
                output,
                reference: None,
            } => Self::MoveWorkspaceToMonitor(output),
            niri_ipc::Action::MoveWorkspaceToMonitorNext {} => Self::MoveWorkspaceToMonitorNext,
            niri_ipc::Action::ToggleDebugTint {} => Self::ToggleDebugTint,
            niri_ipc::Action::DebugToggleOpaqueRegions {} => Self::DebugToggleOpaqueRegions,
            niri_ipc::Action::DebugToggleDamage {} => Self::DebugToggleDamage,
            niri_ipc::Action::ToggleTouchpad {} => Self::ToggleTouchpad,
            niri_ipc::Action::ToggleWindowFloating { id: None } => Self::ToggleWindowFloating,
            niri_ipc::Action::ToggleWindowFloating { id: Some(id) } => {
                Self::ToggleWindowFloatingById(id)
            }
            niri_ipc::Action::ToggleWindowShader { id: None } => Self::ToggleWindowShader,
            niri_ipc::Action::ToggleWindowShader { id: Some(id) } => {
                Self::ToggleWindowShaderById(id)
            }
            niri_ipc::Action::CycleWindowShader { id: None } => Self::CycleWindowShader,
            niri_ipc::Action::CycleWindowShader { id: Some(id) } => Self::CycleWindowShaderById(id),
            niri_ipc::Action::ToggleOutputShader { output } => Self::ToggleOutputShader(output),
            niri_ipc::Action::CycleOutputShader { output } => Self::CycleOutputShader(output),
            niri_ipc::Action::ToggleWindowSticky { id: None } => Self::ToggleWindowSticky,
            niri_ipc::Action::ToggleWindowSticky { id: Some(id) } => {
                Self::ToggleWindowStickyById(id)
            }
            niri_ipc::Action::MoveWindowToFloating { id: None } => Self::MoveWindowToFloating,
            niri_ipc::Action::MoveWindowToFloating { id: Some(id) } => {
                Self::MoveWindowToFloatingById(id)
            }
            niri_ipc::Action::MoveWindowToTiling { id: None } => Self::MoveWindowToTiling,
            niri_ipc::Action::MoveWindowToTiling { id: Some(id) } => {
                Self::MoveWindowToTilingById(id)
            }
            niri_ipc::Action::FocusFloating {} => Self::FocusFloating,
            niri_ipc::Action::FocusTiling {} => Self::FocusTiling,
            niri_ipc::Action::SwitchFocusBetweenFloatingAndTiling {} => {
                Self::SwitchFocusBetweenFloatingAndTiling
            }
            niri_ipc::Action::MoveFloatingWindow { id, x, y } => {
                Self::MoveFloatingWindowById { id, x, y }
            }
            niri_ipc::Action::ToggleWindowRuleOpacity { id: None } => Self::ToggleWindowRuleOpacity,
            niri_ipc::Action::ToggleWindowRuleOpacity { id: Some(id) } => {
                Self::ToggleWindowRuleOpacityById(id)
            }
            niri_ipc::Action::SetDynamicCastWindow { id: None } => Self::SetDynamicCastWindow,
            niri_ipc::Action::SetDynamicCastWindow { id: Some(id) } => {
                Self::SetDynamicCastWindowById(id)
            }
            niri_ipc::Action::SetDynamicCastMonitor { output } => {
                Self::SetDynamicCastMonitor(output)
            }
            niri_ipc::Action::ClearDynamicCastTarget {} => Self::ClearDynamicCastTarget,
            niri_ipc::Action::StopCast { session_id } => Self::StopCast(session_id),
            niri_ipc::Action::ToggleDwt {} => Self::ToggleDwt,
            niri_ipc::Action::ToggleOverview {} => Self::ToggleOverview,
            niri_ipc::Action::OpenOverview {} => Self::OpenOverview,
            niri_ipc::Action::CloseOverview {} => Self::CloseOverview,
            niri_ipc::Action::OverviewZoomCycle { reverse } => Self::OverviewZoomCycle(reverse),
            niri_ipc::Action::OverviewZoomIn {} => Self::OverviewZoomIn,
            niri_ipc::Action::OverviewZoomOut {} => Self::OverviewZoomOut,
            niri_ipc::Action::ToggleWindowUrgent { id } => Self::ToggleWindowUrgent(id),
            niri_ipc::Action::SetWindowUrgent { id } => Self::SetWindowUrgent(id),
            niri_ipc::Action::UnsetWindowUrgent { id } => Self::UnsetWindowUrgent(id),
            niri_ipc::Action::LoadConfigFile { path } => Self::LoadConfigFile(path),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WorkspaceReference {
    Id(u64),
    Index(u8),
    Name(String),
}

impl From<WorkspaceReferenceArg> for WorkspaceReference {
    fn from(reference: WorkspaceReferenceArg) -> WorkspaceReference {
        match reference {
            WorkspaceReferenceArg::Id(id) => Self::Id(id),
            WorkspaceReferenceArg::Index(i) => Self::Index(i),
            WorkspaceReferenceArg::Name(n) => Self::Name(n),
        }
    }
}

impl<S: knuffel::traits::ErrorSpan> knuffel::DecodeScalar<S> for WorkspaceReference {
    fn type_check(
        type_name: &Option<knuffel::span::Spanned<knuffel::ast::TypeName, S>>,
        ctx: &mut knuffel::decode::Context<S>,
    ) {
        if let Some(type_name) = &type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }
    }

    fn raw_decode(
        val: &knuffel::span::Spanned<knuffel::ast::Literal, S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<WorkspaceReference, DecodeError<S>> {
        match &**val {
            knuffel::ast::Literal::String(ref s) => Ok(WorkspaceReference::Name(s.clone().into())),
            knuffel::ast::Literal::Int(ref value) => match value.try_into() {
                Ok(v) => Ok(WorkspaceReference::Index(v)),
                Err(e) => {
                    ctx.emit_error(DecodeError::conversion(val, e));
                    Ok(WorkspaceReference::Index(0))
                }
            },
            _ => {
                ctx.emit_error(DecodeError::unsupported(
                    val,
                    "Unsupported value, only numbers and strings are recognized",
                ));
                Ok(WorkspaceReference::Index(0))
            }
        }
    }
}

impl<S> knuffel::Decode<S> for Binds
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        expect_only_children(node, ctx);

        let mut nodes: Vec<&knuffel::ast::SpannedNode<S>> = Vec::new();
        let mut binds: Vec<Bind> = Vec::new();

        for child in node.children() {
            match Bind::decode_node(child, ctx) {
                Err(e) => {
                    ctx.emit_error(e);
                }
                Ok(bind) => {
                    let conflict = binds
                        .iter()
                        .position(|prev| prev.key.conflicts_with(&bind.key));
                    if let Some(idx) = conflict {
                        // Even though it's technically incorrect, we use
                        // `DecodeError::Missing` here because it labels the bind with
                        // "node starts here", which is the least bad option
                        ctx.emit_error(DecodeError::missing(
                            nodes[idx],
                            "keybind first defined here",
                        ));

                        ctx.emit_error(DecodeError::unexpected(
                            &child.node_name,
                            "keybind",
                            if binds[idx].key == bind.key {
                                "duplicate keybind later defined here"
                            } else {
                                "conflicting keybind later defined here"
                            },
                        ));
                    } else {
                        nodes.push(child);
                        binds.push(bind);
                    }
                }
            }
        }

        Ok(Self(binds))
    }
}

impl<S> knuffel::Decode<S> for Bind
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        if let Some(type_name) = &node.type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }

        for val in node.arguments.iter() {
            ctx.emit_error(DecodeError::unexpected(
                &val.literal,
                "argument",
                "no arguments expected for this node",
            ));
        }

        let key = node
            .node_name
            .parse::<Key>()
            .map_err(|e| DecodeError::conversion(&node.node_name, e.wrap_err("invalid keybind")))?;

        let mut repeat = None;
        let mut repeat_node = None;
        let mut pass_through = None;
        let mut cooldown = None;
        let mut allow_when_locked = false;
        let mut allow_when_locked_node = None;
        let mut allow_inhibiting = true;
        let mut hotkey_overlay_title = None;

        for (name, val) in &node.properties {
            match &***name {
                "repeat" => {
                    repeat = Some(knuffel::traits::DecodeScalar::decode(val, ctx)?);
                    repeat_node = Some(name);
                }
                "pass-through" => {
                    pass_through = Some(knuffel::traits::DecodeScalar::decode(val, ctx)?);

                    // Supporting this for mouse binds would be a bit weird. Mouse binds trigger
                    // compositor gestures, so should pass-through also trigger the corresponding
                    // gesture, on top of the bind? If yes, then there are some conflicts, e.g. with
                    // toggle-overview and the Mod+LMB move grab or LMB press-to-close-overview.
                    //
                    // All in all, this needs design, and I'm not sure why you'd want pass-through
                    // mouse binds anyway, so, let's do keyboard only for now.
                    if !key.trigger.is_keyboard() {
                        ctx.emit_error(DecodeError::unexpected(
                            name,
                            "property",
                            "pass-through is only supported for keyboard binds",
                        ));
                    }
                }
                "cooldown-ms" => {
                    cooldown = Some(Duration::from_millis(
                        knuffel::traits::DecodeScalar::decode(val, ctx)?,
                    ));
                }
                "allow-when-locked" => {
                    allow_when_locked = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                    allow_when_locked_node = Some(name);
                }
                "allow-inhibiting" => {
                    allow_inhibiting = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                }
                "hotkey-overlay-title" => {
                    hotkey_overlay_title = Some(knuffel::traits::DecodeScalar::decode(val, ctx)?);
                }
                name_str => {
                    ctx.emit_error(DecodeError::unexpected(
                        name,
                        "property",
                        format!("unexpected property `{}`", name_str.escape_default()),
                    ));
                }
            }
        }

        // If the actions are invalid but the key is fine, we still want to return something.
        // That way, the parent can handle the existence of duplicate keybinds,
        // even if their contents are not valid.
        let dummy = Self {
            key,
            action: BoundAction::Press(vec![Action::Spawn(vec![])]),
            repeat: true,
            pass_through: None,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: true,
            hotkey_overlay_title: None,
        };

        // Direct actions are action nodes that are not wrapped in a press or release section.
        let mut direct_actions = Vec::new();
        let mut press_actions = Vec::new();
        let mut release_actions = Vec::new();
        let mut has_direct_actions = false;
        let mut has_press_section = false;
        let mut has_release_section = false;
        let mut had_error = false;

        let decode_into = |ctx: &mut knuffel::decode::Context<S>,
                           child: &knuffel::ast::SpannedNode<S>,
                           actions: &mut Vec<Action>,
                           had_error: &mut bool| {
            match Action::decode_node(child, ctx) {
                // Emit the error and keep going, so that a typo in one action still surfaces
                // the errors in the actions after it.
                Err(e) => {
                    ctx.emit_error(e);
                    *had_error = true;
                }
                Ok(action) => actions.push(action),
            }
        };

        for child in node.children() {
            let child_name = &*child.node_name;
            let child_name: &str = child_name.as_ref();

            if child_name == "press" || child_name == "release" {
                let (seen, actions) = if child_name == "press" {
                    (&mut has_press_section, &mut press_actions)
                } else {
                    (&mut has_release_section, &mut release_actions)
                };

                if *seen {
                    ctx.emit_error(DecodeError::unexpected(
                        &child.node_name,
                        "section",
                        format!("duplicate `{child_name}` section"),
                    ));
                    had_error = true;
                    continue;
                }

                if has_direct_actions {
                    ctx.emit_error(DecodeError::unexpected(
                        &child.node_name,
                        "section",
                        "cannot mix direct actions with press/release sections",
                    ));
                    had_error = true;
                    continue;
                }

                *seen = true;

                let mut has_children = false;
                for action_child in child.children() {
                    has_children = true;
                    decode_into(ctx, action_child, actions, &mut had_error);
                }

                if !has_children {
                    ctx.emit_error(DecodeError::missing(
                        child,
                        format!("expected an action for this {child_name} section"),
                    ));
                    had_error = true;
                }
            } else {
                if has_press_section || has_release_section {
                    ctx.emit_error(DecodeError::unexpected(
                        &child.node_name,
                        "node",
                        "cannot mix direct actions with press/release sections",
                    ));
                    had_error = true;
                    continue;
                }

                has_direct_actions = true;
                decode_into(ctx, child, &mut direct_actions, &mut had_error);
            }
        }

        // Direct actions and press/release sections are mutually exclusive, so at most one of
        // these is non-empty.
        press_actions.append(&mut direct_actions);

        if has_release_section && key.trigger.is_scroll() {
            ctx.emit_error(DecodeError::unexpected(
                node,
                "bind",
                "release sections are not supported for scroll binds",
            ));
            had_error = true;
            release_actions.clear();
        }

        if let Some(node) = repeat_node {
            if repeat == Some(true) && press_actions.is_empty() && !release_actions.is_empty() {
                ctx.emit_error(DecodeError::unexpected(
                    node,
                    "property",
                    "release-only binds cannot repeat",
                ));
            }
        }

        // allow-when-locked must hold for *every* action: handle_bind passes one flag into
        // every do_action call, so a mixed bind would let a non-spawn action run from the
        // lock screen.
        if let Some(node) = allow_when_locked_node {
            if !press_actions
                .iter()
                .chain(&release_actions)
                .all(|a| matches!(a, Action::Spawn(_) | Action::SpawnSh(_)))
            {
                ctx.emit_error(DecodeError::unexpected(
                    node,
                    "property",
                    "allow-when-locked can only be set on binds where every action is spawn",
                ));
            }
        }

        // The toggle-inhibit action must always be uninhibitable. Otherwise, it would be
        // impossible to trigger it.
        if press_actions
            .iter()
            .chain(&release_actions)
            .any(|a| matches!(a, Action::ToggleKeyboardShortcutsInhibit))
        {
            allow_inhibiting = false;
        }

        let repeat = repeat.unwrap_or(release_actions.is_empty());

        let action = match (press_actions.is_empty(), release_actions.is_empty()) {
            (false, false) => BoundAction::Both {
                press: press_actions,
                release: release_actions,
            },
            (false, true) => BoundAction::Press(press_actions),
            (true, false) => BoundAction::Release(release_actions),
            (true, true) => {
                // If anything failed to decode, an error was already emitted above.
                if !had_error {
                    ctx.emit_error(DecodeError::missing(
                        node,
                        "expected an action for this keybind",
                    ));
                }
                return Ok(dummy);
            }
        };

        Ok(Self {
            key,
            action,
            repeat,
            pass_through,
            cooldown,
            allow_when_locked,
            allow_inhibiting,
            hotkey_overlay_title,
        })
    }
}

impl FromStr for Key {
    type Err = miette::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains('\0') {
            return Err(miette!("invalid key with a null byte: {s:?}"));
        }

        let mut written_modifiers: Vec<(Modifiers, &str)> = Vec::new();

        let mut split = s.split('+');
        let key = split.next_back().unwrap();

        for part in split {
            let part = part.trim();
            let modifier = if part.eq_ignore_ascii_case("mod") {
                Modifiers::COMPOSITOR
            } else if part.eq_ignore_ascii_case("ctrl") || part.eq_ignore_ascii_case("control") {
                Modifiers::CTRL
            } else if part.eq_ignore_ascii_case("shift") {
                Modifiers::SHIFT
            } else if part.eq_ignore_ascii_case("alt") {
                Modifiers::ALT
            } else if part.eq_ignore_ascii_case("super") || part.eq_ignore_ascii_case("win") {
                Modifiers::SUPER
            } else if part.eq_ignore_ascii_case("iso_level3_shift")
                || part.eq_ignore_ascii_case("mod5")
            {
                Modifiers::ISO_LEVEL3_SHIFT
            } else if part.eq_ignore_ascii_case("iso_level5_shift")
                || part.eq_ignore_ascii_case("mod3")
            {
                Modifiers::ISO_LEVEL5_SHIFT
            } else {
                return Err(miette!("invalid modifier: {part}"));
            };

            written_modifiers.push((modifier, part));
        }

        let trigger = if key.eq_ignore_ascii_case("MouseLeft") {
            Trigger::MouseLeft
        } else if key.eq_ignore_ascii_case("MouseRight") {
            Trigger::MouseRight
        } else if key.eq_ignore_ascii_case("MouseMiddle") {
            Trigger::MouseMiddle
        } else if key.eq_ignore_ascii_case("MouseBack") {
            Trigger::MouseBack
        } else if key.eq_ignore_ascii_case("MouseForward") {
            Trigger::MouseForward
        } else if key.eq_ignore_ascii_case("WheelScrollDown") {
            Trigger::WheelScrollDown
        } else if key.eq_ignore_ascii_case("WheelScrollUp") {
            Trigger::WheelScrollUp
        } else if key.eq_ignore_ascii_case("WheelScrollLeft") {
            Trigger::WheelScrollLeft
        } else if key.eq_ignore_ascii_case("WheelScrollRight") {
            Trigger::WheelScrollRight
        } else if key.eq_ignore_ascii_case("TouchpadScrollDown") {
            Trigger::TouchpadScrollDown
        } else if key.eq_ignore_ascii_case("TouchpadScrollUp") {
            Trigger::TouchpadScrollUp
        } else if key.eq_ignore_ascii_case("TouchpadScrollLeft") {
            Trigger::TouchpadScrollLeft
        } else if key.eq_ignore_ascii_case("TouchpadScrollRight") {
            Trigger::TouchpadScrollRight
        } else if key.eq_ignore_ascii_case("TabletStylusButton1") {
            Trigger::TabletStylusButton1
        } else if key.eq_ignore_ascii_case("TabletStylusButton2") {
            Trigger::TabletStylusButton2
        } else if key.eq_ignore_ascii_case("TabletStylusButton3") {
            Trigger::TabletStylusButton3
        } else if key.eq_ignore_ascii_case("Mod") {
            Trigger::CompositorMod
        } else if key.eq_ignore_ascii_case("Ctrl") || key.eq_ignore_ascii_case("Control") {
            Trigger::Modifier(ModKey::Ctrl)
        } else if key.eq_ignore_ascii_case("Shift") {
            Trigger::Modifier(ModKey::Shift)
        } else if key.eq_ignore_ascii_case("Alt") {
            Trigger::Modifier(ModKey::Alt)
        } else if key.eq_ignore_ascii_case("Super") || key.eq_ignore_ascii_case("Win") {
            Trigger::Modifier(ModKey::Super)
        } else if key.eq_ignore_ascii_case("ISO_Level3_Shift") || key.eq_ignore_ascii_case("Mod5") {
            Trigger::Modifier(ModKey::IsoLevel3Shift)
        } else if key.eq_ignore_ascii_case("ISO_Level5_Shift") || key.eq_ignore_ascii_case("Mod3") {
            Trigger::Modifier(ModKey::IsoLevel5Shift)
        } else {
            let mut keysym = keysym_from_name(key, KEYSYM_CASE_INSENSITIVE);
            // The keyboard event handling code can receive either
            // XF86ScreenSaver or XF86Screensaver, because there is no
            // case mapping defined between these keysyms. If we just
            // use the case-insensitive version of keysym_from_name it
            // is not possible to bind the uppercase version, because the
            // case-insensitive match prefers the lowercase version when
            // there is a choice.
            //
            // Therefore, when we match this key with the initial
            // case-insensitive match we try a further case-sensitive match
            // (so that either key can be bound). If that fails, we change
            // to the uppercase version because:
            //
            // - A comment in xkb_keysym_from_name (in libxkbcommon) tells us that the uppercase
            //   version is the "best" of the two. [0]
            // - The xkbcommon crate only has a constant for ScreenSaver. [1]
            //
            // [0]: https://github.com/xkbcommon/libxkbcommon/blob/45a118d5325b051343b4b174f60c1434196fa7d4/src/keysym.c#L276
            // [1]: https://docs.rs/xkbcommon/latest/xkbcommon/xkb/keysyms/index.html#:~:text=KEY%5FXF86ScreenSaver
            //
            // See https://github.com/niri-wm/niri/issues/1969
            if keysym == Keysym::XF86_Screensaver {
                keysym = keysym_from_name(key, KEYSYM_NO_FLAGS);
                if keysym.raw() == KEY_NoSymbol {
                    keysym = Keysym::XF86_ScreenSaver;
                }
            }
            if keysym.raw() == KEY_NoSymbol {
                return Err(miette!("invalid key: {key}"));
            }
            Trigger::Keysym(keysym)
        };

        let mut modifiers = trigger.implied_modifier();
        for (modifier, spelling) in written_modifiers {
            if modifiers.contains(modifier) {
                warn!("redundant modifier `{spelling}` in keybind `{s}`");
            }

            modifiers |= modifier;
        }

        Ok(Key { trigger, modifiers })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_multiple_actions_in_order() {
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Mod+G { focus-column-right; consume-or-expel-window-left; }
            }
            "#,
        )
        .unwrap();

        assert_eq!(config.binds.0.len(), 1);
        assert_eq!(
            config.binds.0[0].press_actions(),
            [Action::FocusColumnRight, Action::ConsumeOrExpelWindowLeft],
        );
    }

    #[test]
    fn parse_multiple_actions_in_press_and_release_sections() {
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Mod+G {
                    press { focus-column-right; consume-or-expel-window-left; }
                    release { toggle-overview; close-window; }
                }
                Mod+H {
                    release { focus-column-left; focus-window-down; }
                }
            }
            "#,
        )
        .unwrap();

        assert_eq!(
            config.binds.0[0].action,
            BoundAction::Both {
                press: vec![Action::FocusColumnRight, Action::ConsumeOrExpelWindowLeft],
                release: vec![Action::ToggleOverview, Action::CloseWindow],
            }
        );
        assert!(!config.binds.0[0].repeat);
        assert_eq!(
            config.binds.0[1].action,
            BoundAction::Release(vec![Action::FocusColumnLeft, Action::FocusWindowDown])
        );
    }

    #[test]
    fn allow_when_locked_checks_release_actions() {
        assert!(crate::Config::parse_mem(
            r#"
            binds {
                Mod+X allow-when-locked=true {
                    press { spawn "foo"; }
                    release { spawn "bar"; quit; }
                }
            }
            "#,
        )
        .is_err());
    }

    #[test]
    fn parse_bind_with_no_actions_still_errors() {
        assert!(crate::Config::parse_mem(
            r#"
            binds {
                Mod+G { }
            }
            "#,
        )
        .is_err());
    }

    #[test]
    fn parse_bad_action_among_several_keeps_the_others() {
        // Two bad actions in the middle must not swallow each other's, or the good
        // action's, decoding: both bad names must be reported as separate errors,
        // which is what actually exercises the continue-on-error loop. knuffel's
        // generic "expected one of N others" message text doesn't embed the bad
        // node's name, so we recover it from each diagnostic's label span instead.
        use miette::Diagnostic;

        let config_text = r#"
            binds {
                Mod+G { focus-column-right; not-a-real-action; also-not-real; close-window; }
            }
            "#;

        let err = crate::Config::parse_mem(config_text).unwrap_err();

        let bad_snippets: Vec<&str> = err
            .related()
            .into_iter()
            .flatten()
            .flat_map(|e| e.labels().into_iter().flatten())
            .map(|label| &config_text[label.offset()..label.offset() + label.len()])
            .collect();

        assert!(
            bad_snippets.contains(&"not-a-real-action"),
            "expected a label pointing at `not-a-real-action`, got: {bad_snippets:?}"
        );
        assert!(
            bad_snippets.contains(&"also-not-real"),
            "expected a label pointing at `also-not-real`, got: {bad_snippets:?}"
        );
    }

    #[test]
    fn allow_when_locked_rejected_on_mixed_bind() {
        // Every action must be spawn/spawn-sh, otherwise allow-when-locked would let a
        // non-spawn action run from the lock screen.
        assert!(crate::Config::parse_mem(
            r#"
            binds {
                Mod+X allow-when-locked=true { spawn "foo"; quit; }
            }
            "#,
        )
        .is_err());
    }

    #[test]
    fn allow_when_locked_accepted_on_all_spawn_bind() {
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Mod+X allow-when-locked=true { spawn "foo"; spawn-sh "bar"; }
            }
            "#,
        )
        .unwrap();

        assert!(config.binds.0[0].allow_when_locked);
        assert_eq!(config.binds.0[0].press_actions().len(), 2);
    }

    #[test]
    fn output_shader_actions_compose_with_multi_action_binds() {
        // The output-shader actions landed alongside multi-action binds; this is the seam
        // between them, so check both spellings survive in one bind's action list.
        let config = crate::Config::parse_mem(
            r##"
            binds {
                Mod+X { toggle-output-shader; cycle-output-shader "DP-2"; }
            }
            "##,
        )
        .unwrap();

        assert_eq!(
            config.binds.0[0].press_actions(),
            [
                Action::ToggleOutputShader(None),
                Action::CycleOutputShader(Some(String::from("DP-2"))),
            ]
        );
    }

    #[test]
    fn toggle_inhibit_anywhere_in_list_forces_allow_inhibiting_false() {
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Mod+Escape { close-window; toggle-keyboard-shortcuts-inhibit; }
            }
            "#,
        )
        .unwrap();

        assert!(!config.binds.0[0].allow_inhibiting);
    }

    #[test]
    fn parse_window_shader_actions() {
        let config = crate::Config::parse_mem(
            r#"
            binds {
                Mod+S { toggle-window-shader; }
                Mod+Shift+S { cycle-window-shader; }
            }
            "#,
        )
        .unwrap();
        let actions: Vec<_> = config
            .binds
            .0
            .iter()
            .map(|b| b.press_actions()[0].clone())
            .collect();
        assert_eq!(
            actions,
            [Action::ToggleWindowShader, Action::CycleWindowShader]
        );

        assert_eq!(
            Action::from(niri_ipc::Action::ToggleWindowShader { id: Some(5) }),
            Action::ToggleWindowShaderById(5),
        );
        assert_eq!(
            Action::from(niri_ipc::Action::CycleWindowShader { id: None }),
            Action::CycleWindowShader,
        );
    }

    #[test]
    fn output_shader_binds_parse() {
        let binds = crate::Config::parse_mem(
            r##"
            binds {
                Mod+Shift+G { toggle-output-shader; }
                Mod+Shift+H { toggle-output-shader "eDP-1"; }
                Mod+Shift+C { cycle-output-shader; }
                Mod+Shift+V { cycle-output-shader "DP-2"; }
            }
            "##,
        )
        .unwrap()
        .binds;

        let actions: Vec<_> = binds
            .0
            .iter()
            .map(|b| b.press_actions()[0].clone())
            .collect();
        assert_eq!(
            actions,
            [
                Action::ToggleOutputShader(None),
                Action::ToggleOutputShader(Some(String::from("eDP-1"))),
                Action::CycleOutputShader(None),
                Action::CycleOutputShader(Some(String::from("DP-2"))),
            ]
        );

        assert_eq!(
            Action::from(niri_ipc::Action::ToggleOutputShader { output: None }),
            Action::ToggleOutputShader(None)
        );
        assert_eq!(
            Action::from(niri_ipc::Action::ToggleOutputShader {
                output: Some(String::from("DP-2"))
            }),
            Action::ToggleOutputShader(Some(String::from("DP-2")))
        );
        assert_eq!(
            Action::from(niri_ipc::Action::CycleOutputShader { output: None }),
            Action::CycleOutputShader(None)
        );
        assert_eq!(
            Action::from(niri_ipc::Action::CycleOutputShader {
                output: Some(String::from("eDP-1"))
            }),
            Action::CycleOutputShader(Some(String::from("eDP-1")))
        );
    }

    #[test]
    fn parse_xf86_screensaver() {
        assert_eq!(
            "XF86ScreenSaver".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::XF86_ScreenSaver),
                modifiers: Modifiers::empty(),
            },
        );
        assert_eq!(
            "XF86Screensaver".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::XF86_Screensaver),
                modifiers: Modifiers::empty(),
            }
        );
        assert_eq!(
            "xf86screensaver".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::XF86_ScreenSaver),
                modifiers: Modifiers::empty(),
            }
        );
    }

    #[test]
    fn parse_iso_level_shifts() {
        assert_eq!(
            "ISO_Level3_Shift+A".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::a),
                modifiers: Modifiers::ISO_LEVEL3_SHIFT
            },
        );
        assert_eq!(
            "Mod5+A".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::a),
                modifiers: Modifiers::ISO_LEVEL3_SHIFT
            },
        );

        assert_eq!(
            "ISO_Level5_Shift+A".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::a),
                modifiers: Modifiers::ISO_LEVEL5_SHIFT
            },
        );
        assert_eq!(
            "Mod3+A".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::a),
                modifiers: Modifiers::ISO_LEVEL5_SHIFT
            },
        );
    }

    #[test]
    fn parse_bare_modifier_spellings() {
        // Each modifier can be spelled with its canonical name or an alias. A bare modifier key
        // parses to `Trigger::Modifier`.
        for (text, modifier) in [
            ("Ctrl", ModKey::Ctrl),
            ("Control", ModKey::Ctrl),
            ("Shift", ModKey::Shift),
            ("Alt", ModKey::Alt),
            ("Super", ModKey::Super),
            ("Win", ModKey::Super),
            ("Mod5", ModKey::IsoLevel3Shift),
            ("ISO_Level3_Shift", ModKey::IsoLevel3Shift),
            ("Mod3", ModKey::IsoLevel5Shift),
            ("ISO_Level5_Shift", ModKey::IsoLevel5Shift),
        ] {
            assert_eq!(
                text.parse::<Key>().unwrap(),
                Key {
                    trigger: Trigger::Modifier(modifier),
                    modifiers: modifier.to_modifiers(),
                },
                "parsing `{text}`",
            );
        }
    }

    #[test]
    fn parse_mod_as_trigger() {
        // The mod key keeps its own trigger, which is resolved to a real modifier at runtime.
        assert_eq!(
            "Mod".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::CompositorMod,
                modifiers: Modifiers::COMPOSITOR,
            },
        );

        // It can also be used together with other held modifiers.
        assert_eq!(
            "Ctrl+Mod".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::CompositorMod,
                modifiers: Modifiers::CTRL | Modifiers::COMPOSITOR,
            },
        );

        // `Mod` also works as a held modifier of another trigger.
        assert_eq!(
            "Mod+Control_L".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::Control_L),
                modifiers: Modifiers::COMPOSITOR | Modifiers::CTRL,
            },
        );
    }

    #[test]
    fn parse_multi_modifier_triggers() {
        // A modifier key as the trigger with another held modifier, either spelled as the modifier
        // itself or as its keysym.
        assert_eq!(
            "Ctrl+Alt".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Modifier(ModKey::Alt),
                modifiers: Modifiers::CTRL | Modifiers::ALT,
            },
        );
        assert_eq!(
            "Ctrl+Alt_L".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::Alt_L),
                modifiers: Modifiers::CTRL | Modifiers::ALT,
            },
        );
    }

    #[test]
    fn parse_normalizes_redundant_modifiers() {
        let key = |text: &str| text.parse::<Key>().unwrap();

        // Modifier triggers can be written with or without the redundant modifier.
        assert_eq!(key("Alt+Alt_L"), key("Alt_L"));
        assert_eq!(key("Alt+Alt_R"), key("Alt_R"));
        assert_eq!(key("Alt+Alt"), key("Alt"));
        assert_eq!(key("Alt+Alt+Alt_L"), key("Alt_L"));
        assert_eq!(key("Ctrl+Shift+Control_L"), key("Shift+Control_L"));
        assert_eq!(key("Mod+Mod"), key("Mod"));

        // Duplicate modifiers are fine with non-modifier triggers.
        assert_eq!(key("Alt+Alt+Q"), key("Alt+Q"));
        assert_eq!(key("Mod+Mod+Q"), key("Mod+Q"));

        // Still works if spelled differently.
        assert_eq!(key("Ctrl+Control+Q"), key("Ctrl+Q"));
        assert_eq!(key("Super+Win+Q"), key("Super+Q"));
    }

    #[test]
    fn parse_modifier_trigger_includes_implied_modifier() {
        // The modifier that the trigger implies is part of the key.
        assert_eq!(
            "Alt_L".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::Alt_L),
                modifiers: Modifiers::ALT,
            },
        );
        assert_eq!(
            "Alt+Alt".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Modifier(ModKey::Alt),
                modifiers: Modifiers::ALT,
            },
        );
        // `Mod` is only resolved at runtime, so no trigger implies it.
        assert_eq!(
            "Alt+Mod".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::CompositorMod,
                modifiers: Modifiers::ALT | Modifiers::COMPOSITOR,
            },
        );
    }

    #[test]
    fn key_conflicts() {
        let key = |text: &str| text.parse::<Key>().unwrap();

        // Alt_L and Alt_R conflict with Alt, and vice versa.
        let alt = key("Alt");
        let alt_l = key("Alt_L");
        let alt_r = key("Alt_R");
        assert!(alt.conflicts_with(&alt_l));
        assert!(alt.conflicts_with(&alt_r));
        assert!(alt_l.conflicts_with(&alt));
        assert!(alt_r.conflicts_with(&alt));

        // The left and right keysyms are different keys.
        assert!(!alt_l.conflicts_with(&alt_r));

        // Different modifiers mean different binds, even for the same trigger.
        assert!(!alt.conflicts_with(&key("Ctrl+Alt_L")));

        // Non-overlapping triggers don't conflict, even with the same modifiers.
        assert!(!key("Mod+Q").conflicts_with(&key("Mod+L")));
        assert!(!key("Q").conflicts_with(&key("Ctrl")));

        // Identical keys are duplicates.
        assert!(key("Q").conflicts_with(&key("Q")));
        assert!(alt.conflicts_with(&alt));

        // The compositor mod key is only resolved at runtime, so `Mod` isn't compared against the
        // mod key's own spellings.
        let compositor_mod = key("Mod");
        assert!(!compositor_mod.conflicts_with(&key("Super_L")));
        assert!(!compositor_mod.conflicts_with(&key("Super")));

        // Generic-modifier-only keys are order-independent.
        assert!(key("Ctrl+Alt").conflicts_with(&key("Alt+Ctrl")));
        assert!(key("Mod+Shift+Alt").conflicts_with(&key("Shift+Alt+Mod")));
        assert!(key("Ctrl+Alt").conflicts_with(&key("Alt+Control_L")));
        assert!(key("Ctrl+Alt_L").conflicts_with(&key("Alt+Ctrl")));

        assert!(!key("Ctrl+Alt").conflicts_with(&key("Ctrl+Alt+A")));
    }

    #[test]
    fn null_byte() {
        // Shouldn't panic.
        let _ = "W\0".parse::<Key>();
    }
}
