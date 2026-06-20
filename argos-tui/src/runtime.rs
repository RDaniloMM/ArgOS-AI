use std::error::Error;
use std::io;
use std::panic::{self, PanicHookInfo};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event as CrosstermEvent};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::app::{handle_action, handle_async, Command};
use crate::event::{AsyncEvent, Event};
use crate::keymap::map_key;
use crate::services::{AppServices, RealServices};
use crate::state::AppState;
use crate::ui;

type PanicHook = Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOptions {
    pub smoke_exit: bool,
}

impl RunOptions {
    pub fn from_env() -> Self {
        Self {
            smoke_exit: std::env::args().any(|arg| arg == "--smoke-exit"),
        }
    }
}

pub async fn run(options: RunOptions) -> Result<(), Box<dyn Error>> {
    let services: Arc<dyn AppServices> = Arc::new(RealServices::new()?);
    let mut state = AppState::new();
    let (event_tx, mut rx) = mpsc::unbounded_channel();
    let command_tx = event_tx.clone();
    let mut terminal = TerminalSession::enter()?;
    let _input = InputThread::spawn(event_tx);

    dispatch_commands(
        handle_action(&mut state, crate::action::Action::Refresh),
        services.clone(),
        command_tx.clone(),
    );

    let mut has_drawn_once = false;
    loop {
        terminal.draw(|frame| ui::render(frame, &state))?;

        if options.smoke_exit && has_drawn_once {
            break;
        }
        has_drawn_once = true;

        let Some(event) = rx.recv().await else {
            break;
        };

        let (commands, should_exit) = process_event(&mut state, event);
        if should_exit {
            break;
        }

        dispatch_commands(commands, services.clone(), command_tx.clone());
    }

    Ok(())
}

fn process_event(state: &mut AppState, event: Event) -> (Vec<Command>, bool) {
    let commands = match event {
        Event::Input(key) => map_key(key, state.focus)
            .map(|action| handle_action(state, action))
            .unwrap_or_default(),
        Event::Resize(_, _) => Vec::new(),
        Event::Async(async_event) => handle_async(state, async_event),
    };

    (commands, state.should_quit)
}

fn dispatch_commands(
    commands: Vec<Command>,
    services: Arc<dyn AppServices>,
    tx: mpsc::UnboundedSender<Event>,
) {
    for command in commands {
        let services = services.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let event = match command {
                Command::LoadSnapshot => {
                    Event::Async(AsyncEvent::SnapshotLoaded(services.load_snapshot().await))
                }
                Command::SubmitPrompt { prompt } => Event::Async(AsyncEvent::PromptCompleted {
                    prompt: prompt.clone(),
                    result: services.submit_prompt(prompt).await,
                }),
                Command::RunWorkflow {
                    workflow_id,
                    workflow_name,
                } => Event::Async(AsyncEvent::WorkflowCompleted {
                    workflow_id: workflow_id.clone(),
                    workflow_name,
                    result: services.run_workflow(workflow_id).await,
                }),
            };
            let _ = tx.send(event);
        });
    }
}

struct InputThread {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl InputThread {
    fn spawn(tx: mpsc::UnboundedSender<Event>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let join = thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                match event::poll(Duration::from_millis(100)) {
                    Ok(true) => match event::read() {
                        Ok(CrosstermEvent::Key(key)) => {
                            if tx.send(Event::Input(key)).is_err() {
                                break;
                            }
                        }
                        Ok(CrosstermEvent::Resize(width, height)) => {
                            if tx.send(Event::Resize(width, height)).is_err() {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(err) => {
                            let _ = tx.send(Event::Async(AsyncEvent::InputError(format!(
                                "crossterm read failed: {err}"
                            ))));
                            break;
                        }
                    },
                    Ok(false) => {}
                    Err(err) => {
                        let _ = tx.send(Event::Async(AsyncEvent::InputError(format!(
                            "crossterm poll failed: {err}"
                        ))));
                        break;
                    }
                }
            }
        });

        Self {
            stop,
            join: Some(join),
        }
    }
}

impl Drop for InputThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    previous_hook: Arc<Mutex<Option<PanicHook>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupAction {
    ShowCursor,
    LeaveAlternateScreen,
    DisableRawMode,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct TerminalCleanupState {
    raw_mode_enabled: bool,
    alternate_screen_entered: bool,
    cursor_hidden: bool,
}

impl TerminalCleanupState {
    fn cleanup_actions(self) -> Vec<CleanupAction> {
        let mut actions = Vec::new();
        if self.cursor_hidden {
            actions.push(CleanupAction::ShowCursor);
        }
        if self.alternate_screen_entered {
            actions.push(CleanupAction::LeaveAlternateScreen);
        }
        if self.raw_mode_enabled {
            actions.push(CleanupAction::DisableRawMode);
        }
        actions
    }
}

#[derive(Debug, Default)]
struct TerminalEnterGuard {
    state: TerminalCleanupState,
    armed: bool,
}

impl TerminalEnterGuard {
    fn arm(&mut self) {
        self.armed = true;
    }

    fn mark_raw_mode_enabled(&mut self) {
        self.state.raw_mode_enabled = true;
    }

    fn mark_alternate_screen_entered(&mut self) {
        self.state.alternate_screen_entered = true;
    }

    fn mark_cursor_hidden(&mut self) {
        self.state.cursor_hidden = true;
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TerminalEnterGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = restore_terminal_with_state(self.state);
        }
    }
}

impl TerminalSession {
    fn enter() -> Result<Self, Box<dyn Error>> {
        let mut guard = TerminalEnterGuard::default();
        guard.arm();

        enable_raw_mode()?;
        guard.mark_raw_mode_enabled();

        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        guard.mark_alternate_screen_entered();
        execute!(stdout, Hide)?;
        guard.mark_cursor_hidden();

        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        let previous_hook = Arc::new(Mutex::new(Some(panic::take_hook())));
        let hook_ref = previous_hook.clone();
        panic::set_hook(Box::new(move |info| {
            let _ = restore_terminal();
            if let Ok(hook) = hook_ref.lock() {
                if let Some(previous) = hook.as_ref() {
                    previous(info);
                }
            }
        }));

        guard.disarm();

        Ok(Self {
            terminal,
            previous_hook,
        })
    }

    fn draw<F>(&mut self, render: F) -> Result<(), Box<dyn Error>>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        self.terminal.draw(render)?;
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = restore_terminal();
        if let Ok(mut hook) = self.previous_hook.lock() {
            if let Some(previous) = hook.take() {
                panic::set_hook(previous);
            }
        }
    }
}

fn restore_terminal() -> io::Result<()> {
    restore_terminal_with_state(TerminalCleanupState {
        raw_mode_enabled: true,
        alternate_screen_entered: true,
        cursor_hidden: true,
    })
}

fn restore_terminal_with_state(state: TerminalCleanupState) -> io::Result<()> {
    let mut stdout = io::stdout();
    let mut first_err = None;

    for action in state.cleanup_actions() {
        let result = match action {
            CleanupAction::ShowCursor => execute!(stdout, Show),
            CleanupAction::LeaveAlternateScreen => execute!(stdout, LeaveAlternateScreen),
            CleanupAction::DisableRawMode => disable_raw_mode(),
        };

        if let Err(err) = result {
            if first_err.is_none() {
                first_err = Some(err);
            }
        }
    }

    match first_err {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{process_event, CleanupAction, Event, TerminalCleanupState};
    use crate::event::AsyncEvent;
    use crate::state::AppState;

    #[test]
    fn cleanup_plan_matches_completed_terminal_stages() {
        let state = TerminalCleanupState {
            raw_mode_enabled: true,
            alternate_screen_entered: true,
            cursor_hidden: false,
        };

        assert_eq!(
            state.cleanup_actions(),
            vec![
                CleanupAction::LeaveAlternateScreen,
                CleanupAction::DisableRawMode,
            ]
        );
    }

    #[test]
    fn cleanup_plan_restores_full_terminal_state_in_reverse_order() {
        let state = TerminalCleanupState {
            raw_mode_enabled: true,
            alternate_screen_entered: true,
            cursor_hidden: true,
        };

        assert_eq!(
            state.cleanup_actions(),
            vec![
                CleanupAction::ShowCursor,
                CleanupAction::LeaveAlternateScreen,
                CleanupAction::DisableRawMode,
            ]
        );
    }

    #[test]
    fn fatal_input_event_requests_loop_exit() {
        let mut state = AppState::new();

        let (_, should_exit) = process_event(
            &mut state,
            Event::Async(AsyncEvent::InputError("poll failed".into())),
        );

        assert!(should_exit);
        assert!(state.should_quit);
    }
}
