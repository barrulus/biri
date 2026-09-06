use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use clap_complete::Shell;
use niri_ipc::{Action, OutputAction};

use crate::utils::version;

#[derive(Parser)]
#[command(author, version = version(), about, long_about = None)]
#[command(args_conflicts_with_subcommands = true)]
#[command(subcommand_value_name = "SUBCOMMAND")]
#[command(subcommand_help_heading = "Subcommands")]
pub struct Cli {
    /// Path to config file (default: `$XDG_CONFIG_HOME/niri/config.kdl`).
    ///
    /// This can also be set with the `NIRI_CONFIG` environment variable. If both are set, the
    /// command line argument takes precedence.
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    /// Import environment globally to systemd and D-Bus, run D-Bus services.
    ///
    /// Set this flag in a systemd service started by your display manager, or when running
    /// manually as your main compositor instance. Do not set when running as a nested window, or
    /// on a TTY as your non-main compositor instance, to avoid messing up the global environment.
    #[arg(long)]
    pub session: bool,
    /// Command to run upon compositor startup.
    #[arg(last = true)]
    pub command: Vec<OsString>,

    #[command(subcommand)]
    pub subcommand: Option<Sub>,
}

#[derive(Subcommand)]
pub enum Sub {
    /// Communicate with the running niri instance.
    Msg {
        #[command(subcommand)]
        msg: Msg,
        /// Format output as JSON.
        #[arg(short, long)]
        json: bool,
    },
    /// Validate the config file.
    Validate {
        /// Path to config file (default: `$XDG_CONFIG_HOME/niri/config.kdl`).
        ///
        /// This can also be set with the `NIRI_CONFIG` environment variable. If both are set, the
        /// command line argument takes precedence.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Cause a panic to check if the backtraces are good.
    Panic,
    /// Generate shell completions.
    Completions { shell: CompletionShell },
}

#[derive(Subcommand)]
pub enum Msg {
    /// List connected outputs.
    Outputs,
    /// List workspaces.
    Workspaces,
    /// List all workspaces, including hidden
    WorkspacesWithHidden,
    /// List open windows.
    Windows,
    /// List open layer-shell surfaces.
    Layers,
    /// Get the configured keyboard layouts.
    KeyboardLayouts,
    /// Print information about the focused output.
    FocusedOutput,
    /// Print information about the focused window.
    FocusedWindow,
    /// Pick a window with the mouse and print information about it.
    PickWindow,
    /// Pick a color from the screen with the mouse.
    PickColor,
    /// Perform an action.
    Action {
        #[command(subcommand)]
        action: Action,
    },
    /// Perform several actions as one atomic sequence.
    ///
    /// Each argument is one action, for example:
    ///
    ///   niri msg actions "toggle-workspace-visibility stash" "focus-workspace stash"
    ///
    /// With no arguments, actions are read from stdin, one per line.
    Actions {
        /// One action per argument. Reads actions from stdin, one per line, if empty.
        #[arg()]
        actions: Vec<String>,
    },
    /// Change output configuration temporarily.
    ///
    /// The configuration is changed temporarily and not saved into the config file. If the output
    /// configuration subsequently changes in the config file, these temporary changes will be
    /// forgotten.
    Output {
        /// Output name.
        ///
        /// Run `niri msg outputs` to see the output names.
        #[arg()]
        output: String,
        /// Configuration to apply.
        #[command(subcommand)]
        action: OutputAction,
    },
    /// Start continuously receiving events from the compositor.
    EventStream,
    /// Print the version of the running niri instance.
    Version,
    /// Request an error from the running niri instance.
    RequestError,
    /// Print the overview state.
    OverviewState,
    /// List screencasts.
    Casts,
}

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
    Nushell,
}

impl TryFrom<CompletionShell> for Shell {
    type Error = &'static str;

    fn try_from(shell: CompletionShell) -> Result<Self, Self::Error> {
        match shell {
            CompletionShell::Bash => Ok(Shell::Bash),
            CompletionShell::Elvish => Ok(Shell::Elvish),
            CompletionShell::Fish => Ok(Shell::Fish),
            CompletionShell::PowerShell => Ok(Shell::PowerShell),
            CompletionShell::Zsh => Ok(Shell::Zsh),
            CompletionShell::Nushell => Err("Nushell should be handled separately"),
        }
    }
}

/// Wrapper that lets a single `Action` be parsed from a bare word list.
///
/// `Action` is a clap `Subcommand`, so it needs a `Parser` around it, and
/// `no_binary_name` so that the first word is read as the subcommand rather than as
/// argv[0].
#[derive(clap::Parser)]
#[command(name = "", no_binary_name = true)]
struct ActionParser {
    #[command(subcommand)]
    action: Action,
}

/// Parses one action from an already-lexed word list.
pub fn parse_action_words(words: Vec<String>) -> anyhow::Result<Action> {
    use clap::Parser as _;

    Ok(ActionParser::try_parse_from(words)?.action)
}

/// Parses a batch of actions from CLI arguments, falling back to stdin when there are none.
///
/// Argument form and stdin form must produce identical output.
pub fn parse_actions(args: &[String], stdin: &str) -> anyhow::Result<Vec<Action>> {
    use anyhow::{anyhow, Context as _};

    let sources: Vec<(String, String)> = if args.is_empty() {
        stdin
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(idx, line)| (format!("stdin line {}", idx + 1), line.to_owned()))
            .collect()
    } else {
        args.iter()
            .enumerate()
            .map(|(idx, arg)| (format!("argument {}", idx + 1), arg.clone()))
            .collect()
    };

    if sources.is_empty() {
        return Err(anyhow!(
            "no actions given; pass one action per argument, or one per line on stdin"
        ));
    }

    let mut actions = Vec::with_capacity(sources.len());
    for (label, text) in sources {
        let words =
            shlex::split(&text).ok_or_else(|| anyhow!("{label}: unbalanced quotes in {text:?}"))?;
        if words.is_empty() {
            return Err(anyhow!("{label}: empty action"));
        }
        actions.push(parse_action_words(words).with_context(|| label.clone())?);
    }

    Ok(actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_and_stdin_produce_the_same_actions() {
        let args = vec![
            String::from("toggle-workspace-visibility stash"),
            String::from("focus-workspace stash"),
        ];
        let from_args = parse_actions(&args, "").unwrap();

        let stdin = "toggle-workspace-visibility stash\nfocus-workspace stash\n";
        let from_stdin = parse_actions(&[], stdin).unwrap();

        assert_eq!(from_args.len(), 2);
        assert_eq!(format!("{from_args:?}"), format!("{from_stdin:?}"));
    }

    #[test]
    fn blank_stdin_lines_are_skipped() {
        let stdin = "focus-column-right\n\n   \nclose-window\n";
        let actions = parse_actions(&[], stdin).unwrap();
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn quoted_arguments_survive_lexing() {
        let args = vec![String::from(r#"spawn -- sh -c "echo hello world""#)];
        let actions = parse_actions(&args, "").unwrap();
        let Action::Spawn { command } = &actions[0] else {
            panic!("expected Spawn, got {:?}", actions[0]);
        };
        assert_eq!(command, &["sh", "-c", "echo hello world"]);
    }

    #[test]
    fn a_bad_action_names_its_position() {
        let args = vec![
            String::from("focus-column-right"),
            String::from("not-a-real-action"),
        ];
        let err = parse_actions(&args, "").unwrap_err().to_string();
        assert!(err.contains('2'), "error should name argument 2: {err}");
    }

    #[test]
    fn no_args_and_no_stdin_is_an_error() {
        assert!(parse_actions(&[], "").is_err());
    }
}
