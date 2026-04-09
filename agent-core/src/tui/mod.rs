//! Terminal UI — full Ratatui-based TUI replacing the React/Ink frontend.
//!
//! Mirrors `src/ink/` (64 files), `src/components/`, `src/hooks/`, `src/screens/`.
//!
//! Architecture:
//! - `App` struct owns all UI state and runs the main event loop
//! - `renderer` draws frames via Ratatui
//! - `events` merges terminal events + query engine events
//! - `widgets/` contains all UI components
//! - `theme` provides consistent styling
//! - `markdown` renders markdown to terminal spans

pub mod renderer;
pub mod events;
pub mod layout;
pub mod theme;
pub mod markdown;
pub mod input_handler;
pub mod widgets;
pub mod components;
pub mod design_system;
pub mod screens;
pub mod buddy;
use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::query::query_loop::QueryEvent;
use crate::util::now_ms;

use self::events::{AppEvent, EventStream};
use self::input_handler::InputHandler;
use self::renderer::draw_frame;
use self::theme::Theme;
use self::components::notices::NotificationManager;

// ─── Application State ─────────────────────────────────────────────────────

/// The main view mode.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewMode {
    /// Normal chat/REPL mode.
    Chat,
    /// Permission prompt — waiting for user to approve/deny.
    PermissionPrompt,
    /// Compact/processing indicator.
    Processing { message: String },
    /// History search mode (Ctrl+R).
    HistorySearch,
    /// Keyboard shortcuts help overlay.
    ShortcutsHelp,
    /// Settings display overlay.
    Settings,
    /// Agent creation wizard.
    AgentWizard,
    /// Setup wizard — provider/model/API key selection.
    SetupWizard,
}

/// A user submission carrying prompt + model/provider from the TUI.
#[derive(Debug, Clone)]
pub struct SubmitMessage {
    pub prompt: String,
    pub model: String,
    pub provider: String,
}

/// Conversation message for display.
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub role: DisplayRole,
    pub content: String,
    pub timestamp: u64,
    pub tool_info: Option<ToolDisplayInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayRole {
    User,
    Assistant,
    System,
    ToolUse,
    ToolResult,
}

#[derive(Debug, Clone)]
pub struct ToolDisplayInfo {
    pub tool_name: String,
    pub tool_id: String,
    pub is_error: bool,
    pub duration_ms: Option<u64>,
}

/// Complete TUI application state.
pub struct App {
    /// Display messages in the conversation.
    pub messages: Vec<DisplayMessage>,
    /// Current input text.
    pub input: InputHandler,
    /// Current view mode.
    pub view_mode: ViewMode,
    /// Scroll offset for the message list.
    pub scroll_offset: u16,
    /// Whether the app is running.
    pub running: bool,
    /// Active theme.
    pub theme: Theme,
    /// Status line text.
    pub status_left: String,
    pub status_right: String,
    /// Active tool operations.
    pub active_tools: Vec<ActiveTool>,
    /// Cost summary.
    pub cost_display: String,
    /// Model name.
    pub model: String,
    /// Whether assistant is currently streaming.
    pub is_streaming: bool,
    /// Current streaming text (accumulated).
    pub streaming_text: String,
    /// Session ID.
    pub session_id: String,
    /// Permission dialog state (when in PermissionPrompt mode).
    pub permission_dialog: Option<widgets::permission_dialog::PermissionDialogState>,
    /// Total input tokens (for status line).
    pub total_input_tokens: u64,
    /// Total output tokens (for status line).
    pub total_output_tokens: u64,
    /// Context usage percentage (for status line).
    pub context_pct: f64,
    /// Permission mode display string.
    pub permission_mode: String,
    /// Plan mode toggle (R2).
    pub plan_mode: bool,
    /// Notification toast manager (R6).
    pub notification_manager: NotificationManager,
    /// History search query string (R3).
    pub history_search_query: String,
    /// History search filtered results (R3).
    pub history_search_results: Vec<String>,
    /// History search selected index (R3).
    pub history_search_selected: usize,
    /// Count of active agents (for /agents command).
    pub agents_count: usize,
    /// Count of active tasks (for /tasks command).
    pub tasks_count: usize,
    /// Count of MCP servers (for /mcp command).
    pub mcp_server_count: usize,
    /// Agent creation wizard state.
    pub agent_wizard: Option<components::agent_wizard::AgentWizardState>,
    /// Animated companion character shown in the sidebar.
    pub buddy: buddy::Buddy,
    /// Setup wizard state (provider/model/API key selection).
    pub setup_wizard: Option<screens::setup_wizard::SetupWizardState>,
    /// LLM provider (mirrors config.provider).
    pub provider: String,
}

#[derive(Debug, Clone)]
pub struct ActiveTool {
    pub id: String,
    pub name: String,
    pub started_at: std::time::Instant,
}

impl App {
    pub fn new(config: &Config, session_id: &str) -> Self {
        let theme = Theme::from_name(&config.theme);

        App {
            messages: Vec::new(),
            input: InputHandler::new(),
            view_mode: ViewMode::Chat,
            scroll_offset: 0,
            running: true,
            theme,
            status_left: String::new(),
            status_right: String::new(),
            active_tools: Vec::new(),
            cost_display: String::new(),
            model: config.model.clone(),
            is_streaming: false,
            streaming_text: String::new(),
            session_id: session_id.to_string(),
            permission_dialog: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            context_pct: 0.0,
            permission_mode: "default".to_string(),
            plan_mode: false,
            notification_manager: NotificationManager::new(),
            history_search_query: String::new(),
            history_search_results: Vec::new(),
            history_search_selected: 0,
            agents_count: 0,
            tasks_count: 0,
            mcp_server_count: 0,
            agent_wizard: None,
            buddy: buddy::Buddy::new(),
            setup_wizard: None,
            provider: config.provider.clone(),
        }
    }

    /// Advance the buddy's animation frame. Called on every `AppEvent::Tick`.
    pub fn tick_buddy(&mut self) {
        self.buddy.tick();
    }

    /// Process a query event from the engine.
    /// Also drives the buddy's mood and stage_message so it reflects the
    /// current query-loop stage in real time.
    pub fn handle_query_event(&mut self, event: QueryEvent) {
        use buddy::BuddyMood;

        match event {
            QueryEvent::TextDelta(text) => {
                self.is_streaming = true;
                self.streaming_text.push_str(&text);
                if self.view_mode == ViewMode::Chat {
                    self.view_mode = ViewMode::Processing {
                        message: "Generating response…".to_string(),
                    };
                }
                self.buddy.mood = BuddyMood::Thinking;
                self.buddy.stage_message = "Generating…".to_string();
            }

            QueryEvent::ThinkingDelta(thinking) => {
                let preview = thinking.chars().take(60).collect::<String>();
                self.status_left = format!("Thinking: {preview}…");
                self.buddy.mood = BuddyMood::Thinking;
                self.buddy.stage_message = "Thinking…".to_string();
            }

            QueryEvent::ToolStart { id, name } => {
                self.buddy.mood = BuddyMood::Working;
                self.buddy.stage_message = format!("Running: {name}");
                self.active_tools.push(ActiveTool {
                    id,
                    name,
                    started_at: std::time::Instant::now(),
                });
            }

            QueryEvent::ToolDone { id, result } => {
                let duration = self.active_tools.iter()
                    .find(|t| t.id == id)
                    .map(|t| t.started_at.elapsed().as_millis() as u64);
                let tool_name = self.active_tools.iter()
                    .find(|t| t.id == id)
                    .map(|t| t.name.clone())
                    .unwrap_or_default();

                self.active_tools.retain(|t| t.id != id);

                // Compact summary instead of full tool output
                let icon = if result.is_error { "✗" } else { "✓" };
                let dur = duration.map(|d| format!(" ({d}ms)")).unwrap_or_default();
                let summary = format!("{icon} {tool_name}{dur}");

                self.messages.push(DisplayMessage {
                    role: DisplayRole::ToolResult,
                    content: summary,
                    timestamp: now_ms(),
                    tool_info: Some(ToolDisplayInfo {
                        tool_name: tool_name.clone(),
                        tool_id: id,
                        is_error: result.is_error,
                        duration_ms: duration,
                    }),
                });

                // Stay in Working if more tools are still running, else Thinking
                if self.active_tools.is_empty() {
                    self.buddy.mood = BuddyMood::Thinking;
                    self.buddy.stage_message = "Processing results…".to_string();
                } else {
                    self.buddy.stage_message = format!(
                        "Running: {}",
                        self.active_tools.first().map(|t| t.name.as_str()).unwrap_or("tool")
                    );
                }
            }

            QueryEvent::AssistantMessage(_msg) => {
                if !self.streaming_text.is_empty() {
                    self.messages.push(DisplayMessage {
                        role: DisplayRole::Assistant,
                        content: std::mem::take(&mut self.streaming_text),
                        timestamp: now_ms(),
                        tool_info: None,
                    });
                }
                self.is_streaming = false;
                if matches!(self.view_mode, ViewMode::Processing { .. }) {
                    self.view_mode = ViewMode::Chat;
                }
                self.buddy.mood = BuddyMood::Happy;
                self.buddy.stage_message = "Done!".to_string();
            }

            QueryEvent::UsageUpdate { cost_usd, input_tokens, output_tokens } => {
                self.total_input_tokens = input_tokens;
                self.total_output_tokens = output_tokens;
                self.cost_display = format!("${cost_usd:.4}");
                let context_window = 200_000u64;
                self.context_pct = (input_tokens as f64 / context_window as f64 * 100.0).min(100.0);
            }

            QueryEvent::Done(reason) => {
                self.is_streaming = false;
                if matches!(self.view_mode, ViewMode::Processing { .. }) {
                    self.view_mode = ViewMode::Chat;
                }
                self.status_left = format!("Done: {}", reason.display());
                if reason.is_error() {
                    self.buddy.mood = BuddyMood::Error;
                    self.buddy.stage_message = reason.display().to_string();
                } else {
                    self.buddy.mood = BuddyMood::Happy;
                    self.buddy.stage_message = "Done!".to_string();
                }
            }

            QueryEvent::Error(e) => {
                self.messages.push(DisplayMessage {
                    role: DisplayRole::System,
                    content: format!("Error: {e}"),
                    timestamp: now_ms(),
                    tool_info: None,
                });
                self.buddy.mood = BuddyMood::Error;
                self.buddy.stage_message = "Error!".to_string();
            }

            QueryEvent::RetryWait { attempt, max, delay_ms, reason } => {
                self.status_left = format!(
                    "Retry {attempt}/{max} in {:.1}s: {reason}",
                    delay_ms as f64 / 1000.0
                );
                self.buddy.mood = BuddyMood::Thinking;
                self.buddy.stage_message = format!("Retry {attempt}/{max}…");
            }

            QueryEvent::Compacted { messages_before, messages_after } => {
                self.messages.push(DisplayMessage {
                    role: DisplayRole::System,
                    content: format!(
                        "Context auto-compacted: {messages_before} → {messages_after} messages"
                    ),
                    timestamp: now_ms(),
                    tool_info: None,
                });
                self.buddy.mood = BuddyMood::Working;
                self.buddy.stage_message = "Compacting context…".to_string();
            }

            QueryEvent::ToolOutput(_output) => {
                // Tool output is consumed silently — the compact summary
                // is shown when ToolDone fires. No raw content in the TUI.
            }
        }
    }

    /// Add a user message to the display.
    pub fn add_user_message(&mut self, text: &str) {
        // Immediately show the buddy is working — don't wait for first token
        self.buddy.mood = buddy::BuddyMood::Thinking;
        self.buddy.stage_message = "Sending to model…".to_string();
        self.is_streaming = true;

        self.messages.push(DisplayMessage {
            role: DisplayRole::User,
            content: text.to_string(),
            timestamp: now_ms(),
            tool_info: None,
        });
    }

    /// Scroll up in the message list.
    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    /// Scroll down in the message list.
    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Scroll to the bottom (follow mode).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Update the status line with rich information.
    /// Mirrors the original StatusLine.tsx: model | cost | tokens | context % | mode
    pub fn update_status(&mut self) {
        // Left side: model + activity
        self.status_left = if self.is_streaming {
            let tool_count = self.active_tools.len();
            if tool_count > 0 {
                let names: Vec<_> = self.active_tools.iter()
                    .map(|t| t.name.as_str())
                    .collect();
                format!("{} | Running: {}", self.model, names.join(", "))
            } else {
                format!("{} | Thinking...", self.model)
            }
        } else {
            format!("{} | {}", self.model, self.permission_mode)
        };

        // Right side: cost | tokens | context usage
        let token_display = if self.total_input_tokens > 0 {
            format!("{}↓ {}↑", self.total_input_tokens, self.total_output_tokens)
        } else {
            String::new()
        };

        let context_display = if self.context_pct > 0.0 {
            let color_hint = if self.context_pct > 90.0 { "!" }
                else if self.context_pct > 70.0 { "~" }
                else { "" };
            format!("{}{:.0}%", color_hint, self.context_pct)
        } else {
            String::new()
        };

        let parts: Vec<&str> = [
            self.cost_display.as_str(),
            token_display.as_str(),
            context_display.as_str(),
        ].iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect();

        self.status_right = parts.join(" | ");
    }
}

// ─── Terminal Setup / Teardown ──────────────────────────────────────────────

/// Initialize the terminal for TUI rendering.
pub fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to normal mode.
pub fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// Run the full TUI application loop.
pub async fn run_tui(
    config: &Config,
    session_id: &str,
    mut query_rx: mpsc::Receiver<QueryEvent>,
    submit_tx: mpsc::Sender<SubmitMessage>,
    perm_request_rx: std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Receiver<(String, String)>>>,
    perm_response_tx: std::sync::mpsc::Sender<bool>,
) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let mut app = App::new(config, session_id);

    // Show setup wizard on first launch so user can pick provider/model/key
    app.setup_wizard = Some(screens::setup_wizard::SetupWizardState::new());
    app.view_mode = ViewMode::SetupWizard;

    let event_stream = EventStream::new();

    while app.running {
        // ── Check for pending permission requests from the gate ──────
        if app.view_mode == ViewMode::Chat {
            if let Ok(rx) = perm_request_rx.lock() {
                if let Ok((tool_name, tool_input)) = rx.try_recv() {
                    // Show the permission dialog
                    app.permission_dialog = Some(
                        widgets::permission_dialog::PermissionDialogState::new(
                            &tool_name, &tool_input
                        )
                    );
                    app.view_mode = ViewMode::PermissionPrompt;
                }
            }
        }

        // Update status
        app.update_status();

        // Draw
        terminal.draw(|frame| draw_frame(frame, &app))?;

        // Handle events (terminal input + query events)
        tokio::select! {
            // Terminal events (keyboard, resize, tick)
            event = event_stream.next() => {
                match event {
                    AppEvent::Key(key) => {
                        handle_key_event(&mut app, key, &submit_tx).await;

                        // If permission dialog just closed, send response back to gate
                        if app.view_mode == ViewMode::Chat {
                            if let Some(ref dialog) = app.permission_dialog {
                                if let Some(ref choice) = dialog.choice {
                                    use widgets::permission_dialog::PermissionChoice;
                                    let approved = matches!(
                                        choice,
                                        PermissionChoice::Allow
                                        | PermissionChoice::AlwaysAllow
                                        | PermissionChoice::AlwaysAllowDir
                                        | PermissionChoice::AllowWithFeedback(_)
                                    );
                                    let _ = perm_response_tx.send(approved);
                                    app.permission_dialog = None;
                                }
                            }
                        }
                    }
                    AppEvent::Mouse(mouse) => {
                        use crossterm::event::MouseEventKind;
                        match mouse.kind {
                            MouseEventKind::ScrollUp   => app.scroll_up(3),
                            MouseEventKind::ScrollDown => app.scroll_down(3),
                            _ => {}
                        }
                    }
                    AppEvent::Resize(_w, _h) => {
                        // Terminal handles resize automatically
                    }
                    AppEvent::Tick => {
                        // Periodic refresh for animations / spinners
                        app.notification_manager.tick();
                        // Advance buddy animation frame (called at 100 ms = ~10 fps)
                        app.tick_buddy();
                        // Advance setup wizard cursor blink
                        if let Some(ref mut wiz) = app.setup_wizard {
                            wiz.tick();
                        }
                        // Return buddy to Idle once the engine has been quiet for
                        // a few ticks (Happy/Error state persists briefly then relaxes)
                        if !app.is_streaming && app.active_tools.is_empty() {
                            use buddy::BuddyMood;
                            match app.buddy.mood {
                                // After Happy/Error, relax back to Idle after ~1.6 s
                                BuddyMood::Happy | BuddyMood::Error
                                    if app.buddy.frame.is_multiple_of(16) =>
                                {
                                    app.buddy.mood = BuddyMood::Idle;
                                    app.buddy.stage_message.clear();
                                }
                                // After being Idle for a long time (~30 s), take a nap
                                BuddyMood::Idle if app.buddy.frame.is_multiple_of(4)
                                    && app.buddy.frame / 4 >= 75 =>
                                {
                                    app.buddy.mood = BuddyMood::Sleeping;
                                    app.buddy.stage_message.clear();
                                }
                                // Wake up from sleep the moment any key / event arrives
                                // (handled below via the input path; Sleeping → Idle is
                                // set whenever `add_user_message` is called)
                                _ => {}
                            }
                        }
                    }
                    // R4: Handle bracketed paste
                    AppEvent::Paste(text) => {
                        // If setup wizard is active and on API key step, paste into it
                        if app.view_mode == ViewMode::SetupWizard {
                            if let Some(ref mut wiz) = app.setup_wizard {
                                wiz.handle_paste(&text);
                            }
                        } else {
                            app.input.handle_paste(&text);
                        }
                    }
                }
            }
            // Query engine events
            Some(query_event) = query_rx.recv() => {
                app.handle_query_event(query_event);
                app.scroll_to_bottom();
            }
        }
    }

    restore_terminal(&mut terminal)?;
    Ok(())
}

/// Handle a key event in the main TUI loop.
async fn handle_key_event(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    submit_tx: &mpsc::Sender<SubmitMessage>,
) {
    use crossterm::event::{KeyCode, KeyModifiers};

    // If permission dialog is active, delegate to it
    if app.view_mode == ViewMode::PermissionPrompt {
        if let Some(ref mut dialog) = app.permission_dialog {
            if dialog.handle_key(key) {
                // Dialog closed — the response is sent in the main event loop
                // after this function returns (see run_tui above).
                app.view_mode = ViewMode::Chat;
            }
        }
        return;
    }

    // R3: History search mode (Ctrl+R)
    if app.view_mode == ViewMode::HistorySearch {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                // Cancel history search
                app.history_search_query.clear();
                app.history_search_results.clear();
                app.history_search_selected = 0;
                app.view_mode = ViewMode::Chat;
            }
            (_, KeyCode::Enter) => {
                // Inject selected history item into input buffer
                if let Some(selected) = app.history_search_results.get(app.history_search_selected) {
                    app.input.set_buffer(selected.clone());
                }
                app.history_search_query.clear();
                app.history_search_results.clear();
                app.history_search_selected = 0;
                app.view_mode = ViewMode::Chat;
            }
            (_, KeyCode::Up) => {
                if app.history_search_selected > 0 {
                    app.history_search_selected -= 1;
                }
            }
            (_, KeyCode::Down) => {
                if app.history_search_selected + 1 < app.history_search_results.len() {
                    app.history_search_selected += 1;
                }
            }
            (_, KeyCode::Backspace) => {
                app.history_search_query.pop();
                app.history_search_results = app.input.search_history(&app.history_search_query);
                app.history_search_selected = 0;
            }
            (_, KeyCode::Char(c)) => {
                app.history_search_query.push(c);
                app.history_search_results = app.input.search_history(&app.history_search_query);
                app.history_search_selected = 0;
            }
            _ => {}
        }
        return;
    }

    // R5: Shortcuts help overlay
    if app.view_mode == ViewMode::ShortcutsHelp {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.view_mode = ViewMode::Chat;
            }
            _ => {}
        }
        return;
    }

    // W6: Agent wizard
    if app.view_mode == ViewMode::AgentWizard {
        if let Some(ref mut wizard) = app.agent_wizard {
            let should_close = wizard.handle_key(key);
            if should_close {
                if wizard.done {
                    if let Some(ref path) = wizard.saved_path {
                        app.messages.push(DisplayMessage {
                            role: DisplayRole::System,
                            content: format!("Agent saved to: {}", path.display()),
                            timestamp: now_ms(),
                            tool_info: None,
                        });
                    }
                }
                app.agent_wizard = None;
                app.view_mode = ViewMode::Chat;
            }
        }
        return;
    }

    // R9: Settings overlay
    if app.view_mode == ViewMode::Settings {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.view_mode = ViewMode::Chat;
            }
            _ => {}
        }
        return;
    }

    // Setup wizard overlay
    if app.view_mode == ViewMode::SetupWizard {
        if let Some(ref mut wizard) = app.setup_wizard {
            let should_close = wizard.handle_key(key);
            if should_close {
                if wizard.done {
                    // Apply the wizard's choices
                    app.provider = wizard.chosen_provider.clone();
                    app.model = wizard.chosen_model.clone();
                    // Set the API key as env var so the Python brain picks it up
                    if !wizard.chosen_api_key.is_empty() {
                        let env_key = match wizard.chosen_provider.as_str() {
                            "anthropic" => "ANTHROPIC_API_KEY",
                            "openai" => "OPENAI_API_KEY",
                            "gemini" => "GEMINI_API_KEY",
                            _ => "",
                        };
                        if !env_key.is_empty() {
                            std::env::set_var(env_key, &wizard.chosen_api_key);
                        }
                    }
                    app.messages.push(DisplayMessage {
                        role: DisplayRole::System,
                        content: format!(
                            "Setup complete!\n  Provider: {}\n  Model: {}\n  API key: {}",
                            app.provider,
                            app.model,
                            if wizard.chosen_api_key.is_empty() { "not needed" } else { "set" },
                        ),
                        timestamp: now_ms(),
                        tool_info: None,
                    });
                }
                app.setup_wizard = None;
                app.view_mode = ViewMode::Chat;
            }
        }
        return;
    }

    match (key.modifiers, key.code) {
        // Ctrl-C or Ctrl-Q — clean quit
        (KeyModifiers::CONTROL, KeyCode::Char('c'))
        | (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
            app.running = false;
        }
        // R3: Ctrl-R — enter history search mode
        (m, KeyCode::Char('r')) if m.contains(KeyModifiers::CONTROL) => {
            app.history_search_query.clear();
            app.history_search_results = app.input.search_history("");
            app.history_search_selected = 0;
            app.view_mode = ViewMode::HistorySearch;
        }
        // Shift+Enter — insert newline (multi-line support)
        (m, KeyCode::Enter) if m.contains(KeyModifiers::SHIFT) => {
            app.input.handle_key(crossterm::event::KeyEvent::new(
                KeyCode::Char('\n'), KeyModifiers::NONE
            ));
        }
        // R10: Tab — accept ghost text if present
        (KeyModifiers::NONE, KeyCode::Tab) => {
            if app.input.has_ghost_text() {
                app.input.accept_ghost_text();
            }
        }
        // R5: ? key when input is empty — show shortcuts help
        (KeyModifiers::NONE, KeyCode::F(1)) => {
            app.view_mode = ViewMode::ShortcutsHelp;
        }
        // Enter — submit input (or handle slash command)
        (KeyModifiers::NONE, KeyCode::Enter) => {
            let text = app.input.submit();
            if !text.is_empty() {
                if text.starts_with('/') {
                    // Slash command handling
                    handle_slash_command(app, &text);
                } else {
                    app.add_user_message(&text);
                    app.scroll_to_bottom();
                    let _ = submit_tx.send(SubmitMessage {
                        prompt: text,
                        model: app.model.clone(),
                        provider: app.provider.clone(),
                    }).await;
                }
            }
        }
        // Scroll — mouse wheel handled in AppEvent::Mouse above;
        // keyboard alternatives for accessibility / non-mouse users
        (KeyModifiers::NONE, KeyCode::PageUp)   => app.scroll_up(10),
        (KeyModifiers::NONE, KeyCode::PageDown) => app.scroll_down(10),
        (KeyModifiers::SHIFT, KeyCode::Up)      => app.scroll_up(3),
        (KeyModifiers::SHIFT, KeyCode::Down)    => app.scroll_down(3),
        // Delegate to input handler
        _ => {
            app.input.handle_key(key);
        }
    }
}

/// Handle slash commands locally in the TUI.
fn handle_slash_command(app: &mut App, cmd: &str) {
    let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
    let command = parts[0];
    let _args = parts.get(1).unwrap_or(&"");

    match command {
        "/help" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: "Available commands:\n\
                    /help         — Show this help\n\
                    /clear        — Clear the conversation\n\
                    /model        — Show/change model\n\
                    /cost         — Show cost summary\n\
                    /compact      — Compact conversation history\n\
                    /vim          — Toggle vim mode\n\
                    /plan         — Toggle plan mode\n\
                    /memory       — Memory system info\n\
                    /skills       — List available skills\n\
                    /agents       — Show active agents\n\
                    /tasks        — Show active tasks\n\
                    /mcp          — Show MCP servers\n\
                    /theme        — Cycle theme (dark/light/system)\n\
                    /resume       — Resume a previous session\n\
                    /keybindings  — Show keyboard shortcuts\n\
                    /shortcuts    — Show keyboard shortcuts overlay\n\
                    /settings     — Show current settings\n\
                    /dream        — Consolidate memories from recent sessions\n\
                    /setup        — Change provider/model/API key\n\
                    /buddy        — Toggle the companion dog sidebar\n\
                    /exit         — Exit the agent\n\
                    /version      — Show version".to_string(),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/clear" => {
            app.messages.clear();
            app.streaming_text.clear();
        }
        "/model" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: format!("Current model: {}", app.model),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/cost" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: format!(
                    "Cost: {} | Tokens: {} in / {} out | Context: {:.0}%",
                    app.cost_display, app.total_input_tokens, app.total_output_tokens, app.context_pct
                ),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/version" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: format!("Centaur Psicode v{}", env!("CARGO_PKG_VERSION")),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/exit" | "/quit" | "/q" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: "Shutting down... (cleaning up Python brain + socket)".to_string(),
                timestamp: now_ms(),
                tool_info: None,
            });
            app.running = false;
        }
        // ─── R2: New slash commands ────────────────────────────────────────
        "/compact" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: "Compacting conversation... (actual compact is performed via IPC)".to_string(),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/vim" => {
            app.input.toggle_vim();
            let state = if app.input.vim_enabled { "enabled" } else { "disabled" };
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: format!("Vim mode {state}"),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/plan" => {
            app.plan_mode = !app.plan_mode;
            let state = if app.plan_mode { "enabled" } else { "disabled" };
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: format!("Plan mode {state}. The assistant will outline steps before executing."),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/memory" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: "Use the memory system via prompts or via /memory list|add|delete".to_string(),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/skills" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: "Skills are available. Use slash commands to invoke them:\n\
                    /commit    — Create a git commit\n\
                    /review-pr — Review a pull request\n\
                    /loop      — Run a recurring task\n\
                    /schedule  — Manage scheduled agents\n\
                    Type /help to see all commands.".to_string(),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/agents" => {
            let msg = if app.agents_count > 0 {
                format!("{} agent(s) currently running", app.agents_count)
            } else {
                "No agents running".to_string()
            };
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: msg,
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/tasks" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: format!("{} task(s) tracked", app.tasks_count),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/mcp" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: format!("{} MCP server(s) configured", app.mcp_server_count),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/theme" => {
            // Cycle: dark -> light -> system -> dark
            let next = match app.theme.name.as_str() {
                "dark" => "light",
                "light" => "system",
                _ => "dark",
            };
            app.theme = Theme::from_name(next);
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: format!("Theme switched to: {}", app.theme.name),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/resume" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: "Use: agent --resume to load a previous session".to_string(),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/keybindings" | "/shortcuts" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: "Keyboard shortcuts:\n\
                    \n\
                    Navigation:\n\
                      Scroll wheel     — Scroll messages\n\
                      PageUp/PageDown  — Scroll messages (10 lines)\n\
                      Shift+↑/↓        — Scroll messages (3 lines)\n\
                      Ctrl+R           — Search history\n\
                      F1               — Show shortcuts overlay\n\
                    \n\
                    Editing:\n\
                      Ctrl+A           — Move to start of line\n\
                      Ctrl+E           — Move to end of line\n\
                      Ctrl+U           — Delete to start of line\n\
                      Ctrl+K           — Delete to end of line\n\
                      Ctrl+W           — Delete previous word\n\
                      Ctrl+B / Alt+Left  — Move back one word\n\
                      Ctrl+F / Alt+Right — Move forward one word\n\
                      Tab              — Accept ghost text suggestion\n\
                      Shift+Enter      — Insert newline (multi-line)\n\
                      Up/Down          — History navigation\n\
                    \n\
                    Commands:\n\
                      Ctrl+C           — Quit\n\
                      Enter            — Submit input\n\
                      /help            — Show available commands\n\
                      Esc              — Close overlays/dialogs".to_string(),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        "/settings" => {
            // R9: Show settings overlay
            app.view_mode = ViewMode::Settings;
        }
        "/agent-new" | "/create-agent" => {
            // Get tool names from a static list (the registry isn't accessible here)
            let tool_names = vec![
                "Bash", "FileRead", "FileWrite", "FileEdit", "Glob", "Grep",
                "Agent", "Sleep", "TaskCreate", "TaskGet", "TaskList",
                "SendMessage", "AskUser", "Brief", "ConfigTool", "REPL",
                "SkillTool", "ToolSearch", "TodoWrite",
            ].into_iter().map(|s| s.to_string()).collect();
            app.agent_wizard = Some(components::agent_wizard::AgentWizardState::new(tool_names));
            app.view_mode = ViewMode::AgentWizard;
        }
        "/dream" => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: "Dream (memory consolidation) triggered.\n\
                    The agent will review recent sessions and organize memories.\n\
                    This runs in the background — you can continue chatting.".to_string(),
                timestamp: now_ms(),
                tool_info: None,
            });
            // Actual dream execution is handled by the engine's DreamState
            // via the stop-hook mechanism after each turn.
            // For manual trigger, we send a special prompt.
            // TODO: Wire direct IPC dream trigger here
        }
        "/setup" | "/provider" => {
            app.setup_wizard = Some(screens::setup_wizard::SetupWizardState::new());
            app.view_mode = ViewMode::SetupWizard;
        }
        "/buddy" => {
            app.buddy.enabled = !app.buddy.enabled;
            let state = if app.buddy.enabled { "shown" } else { "hidden" };
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: format!("{} is now {}. (/buddy to toggle)", app.buddy.name, state),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
        _ => {
            app.messages.push(DisplayMessage {
                role: DisplayRole::System,
                content: format!("Unknown command: {command}. Type /help for available commands."),
                timestamp: now_ms(),
                tool_info: None,
            });
        }
    }
}

