//! Setup wizard — interactive provider/model/API-key selection overlay.
//!
//! Shows on first launch or via `/setup`. The user navigates with arrow keys,
//! types an API key, and presses Enter to confirm.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::tui::theme::Theme;

/// Available LLM providers.
const PROVIDERS: &[(&str, &str)] = &[
    ("anthropic", "Anthropic (Claude)"),
    ("openai",    "OpenAI (GPT-4o, o3, ...)"),
    ("gemini",    "Google Gemini"),
    ("ollama",    "Ollama (local models)"),
];

/// Suggested models per provider.
const MODELS_ANTHROPIC: &[&str] = &[
    "claude-sonnet-4-6",
    "claude-opus-4-6",
    "claude-haiku-4-5-20251001",
];
const MODELS_OPENAI: &[&str] = &[
    "gpt-4o",
    "gpt-4o-mini",
    "o3",
    "o4-mini",
    "glm-5",
    "glm-5-reasoning",
];
const MODELS_GEMINI: &[&str] = &[
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.0-flash",
];
const MODELS_OLLAMA: &[&str] = &[
    "gemma4",
    "gemma4:31b",
    "llama3.1",
    "codellama",
    "mistral",
    "deepseek-coder-v2",
    "qwen3",
    "phi4",
];

/// Which step the wizard is on.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    ChooseProvider,
    ChooseModel,
    EnterApiKey,
    Done,
}

/// Mutable state for the setup wizard.
#[derive(Debug, Clone)]
pub struct SetupWizardState {
    step: Step,
    provider_idx: usize,
    model_idx: usize,
    api_key_buf: String,
    /// Set to true when the user confirms and the wizard should close.
    pub done: bool,
    /// Selected provider key (e.g. "openai").
    pub chosen_provider: String,
    /// Selected model name (e.g. "gpt-4o").
    pub chosen_model: String,
    /// Entered API key (empty for ollama).
    pub chosen_api_key: String,
    /// Whether the cursor is blinking (for visual feedback).
    cursor_visible: bool,
    tick: usize,
}

impl SetupWizardState {
    pub fn new() -> Self {
        Self {
            step: Step::ChooseProvider,
            provider_idx: 0,
            model_idx: 0,
            api_key_buf: String::new(),
            done: false,
            chosen_provider: String::new(),
            chosen_model: String::new(),
            chosen_api_key: String::new(),
            cursor_visible: true,
            tick: 0,
        }
    }

    /// Advance cursor blink (called on tick).
    pub fn tick(&mut self) {
        self.tick += 1;
        self.cursor_visible = (self.tick / 5).is_multiple_of(2);
    }

    fn models_for_provider(&self) -> &'static [&'static str] {
        match PROVIDERS[self.provider_idx].0 {
            "anthropic" => MODELS_ANTHROPIC,
            "openai" => MODELS_OPENAI,
            "gemini" => MODELS_GEMINI,
            "ollama" => MODELS_OLLAMA,
            _ => MODELS_ANTHROPIC,
        }
    }

    fn provider_needs_key(&self) -> bool {
        PROVIDERS[self.provider_idx].0 != "ollama"
    }

    fn env_var_hint(&self) -> &'static str {
        match PROVIDERS[self.provider_idx].0 {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "gemini" => "GEMINI_API_KEY",
            _ => "",
        }
    }

    /// Handle a key event. Returns true if the wizard should close.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match self.step {
            Step::ChooseProvider => self.handle_provider_key(key),
            Step::ChooseModel => self.handle_model_key(key),
            Step::EnterApiKey => self.handle_apikey_key(key),
            Step::Done => true,
        }
    }

    fn handle_provider_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Up => {
                if self.provider_idx > 0 {
                    self.provider_idx -= 1;
                }
            }
            KeyCode::Down => {
                if self.provider_idx + 1 < PROVIDERS.len() {
                    self.provider_idx += 1;
                }
            }
            KeyCode::Enter => {
                self.chosen_provider = PROVIDERS[self.provider_idx].0.to_string();
                self.model_idx = 0;
                self.step = Step::ChooseModel;
            }
            KeyCode::Esc => return true, // cancel
            _ => {}
        }
        false
    }

    fn handle_model_key(&mut self, key: KeyEvent) -> bool {
        let models = self.models_for_provider();
        match key.code {
            KeyCode::Up => {
                if self.model_idx > 0 {
                    self.model_idx -= 1;
                }
            }
            KeyCode::Down => {
                if self.model_idx + 1 < models.len() {
                    self.model_idx += 1;
                }
            }
            KeyCode::Enter => {
                self.chosen_model = models[self.model_idx].to_string();
                if self.provider_needs_key() {
                    // Check if env var is already set
                    let env_key = self.env_var_hint();
                    if let Ok(val) = std::env::var(env_key) {
                        if !val.is_empty() {
                            self.chosen_api_key = val;
                            self.done = true;
                            self.step = Step::Done;
                            return true;
                        }
                    }
                    self.api_key_buf.clear();
                    self.step = Step::EnterApiKey;
                } else {
                    // Ollama doesn't need a key
                    self.chosen_api_key.clear();
                    self.done = true;
                    self.step = Step::Done;
                    return true;
                }
            }
            KeyCode::Esc | KeyCode::Backspace => {
                self.step = Step::ChooseProvider;
            }
            _ => {}
        }
        false
    }

    /// Handle pasted text (for API key entry).
    pub fn handle_paste(&mut self, text: &str) {
        if self.step == Step::EnterApiKey {
            // Strip whitespace/newlines from pasted keys
            self.api_key_buf.push_str(text.trim());
        }
    }

    fn handle_apikey_key(&mut self, key: KeyEvent) -> bool {
        match (key.modifiers, key.code) {
            (_, KeyCode::Enter) => {
                if !self.api_key_buf.trim().is_empty() {
                    self.chosen_api_key = self.api_key_buf.trim().to_string();
                    self.done = true;
                    self.step = Step::Done;
                    return true;
                }
            }
            (_, KeyCode::Esc) => {
                self.step = Step::ChooseModel;
            }
            (_, KeyCode::Backspace) => {
                self.api_key_buf.pop();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.api_key_buf.clear();
            }
            // Paste-friendly: allow Ctrl-V passthrough (handled by bracketed paste)
            (_, KeyCode::Char(c)) => {
                self.api_key_buf.push(c);
            }
            _ => {}
        }
        false
    }
}

/// Widget that renders the setup wizard overlay.
pub struct SetupWizardWidget<'a> {
    pub state: &'a SetupWizardState,
    pub theme: &'a Theme,
}

impl<'a> SetupWizardWidget<'a> {
    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let width = 56u16.min(area.width.saturating_sub(4));
        let height = 22u16.min(area.height.saturating_sub(4));
        let x = (area.width.saturating_sub(width)) / 2 + area.x;
        let y = (area.height.saturating_sub(height)) / 2 + area.y;
        let dialog_area = Rect::new(x, y, width, height);

        frame.render_widget(Clear, dialog_area);

        let title = match self.state.step {
            Step::ChooseProvider => " Setup — Choose Provider ",
            Step::ChooseModel => " Setup — Choose Model ",
            Step::EnterApiKey => " Setup — Enter API Key ",
            Step::Done => " Setup — Complete ",
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(self.theme.accent_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let lines = match self.state.step {
            Step::ChooseProvider => self.provider_lines(),
            Step::ChooseModel => self.model_lines(),
            Step::EnterApiKey => self.apikey_lines(),
            Step::Done => vec![Line::from("Setup complete!")],
        };

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }

    fn provider_lines(&self) -> Vec<Line<'a>> {
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Select your LLM provider:",
                self.theme.assistant_style(),
            )),
            Line::from(""),
        ];

        for (i, (_, label)) in PROVIDERS.iter().enumerate() {
            let marker = if i == self.state.provider_idx { " ▸ " } else { "   " };
            let style = if i == self.state.provider_idx {
                self.theme.accent_style().bold()
            } else {
                self.theme.assistant_style()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{label}"),
                style,
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  ↑↓ Navigate  Enter Confirm  Esc Cancel",
            self.theme.dim_style(),
        )));
        lines
    }

    fn model_lines(&self) -> Vec<Line<'a>> {
        let provider_label = PROVIDERS[self.state.provider_idx].1;
        let models = self.state.models_for_provider();

        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  Models for {provider_label}:"),
                self.theme.assistant_style(),
            )),
            Line::from(""),
        ];

        for (i, model) in models.iter().enumerate() {
            let marker = if i == self.state.model_idx { " ▸ " } else { "   " };
            let style = if i == self.state.model_idx {
                self.theme.accent_style().bold()
            } else {
                self.theme.assistant_style()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{model}"),
                style,
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  ↑↓ Navigate  Enter Confirm  Esc Back",
            self.theme.dim_style(),
        )));
        lines
    }

    fn apikey_lines(&self) -> Vec<Line<'a>> {
        let env_var = self.state.env_var_hint();
        let masked: String = if self.state.api_key_buf.is_empty() {
            String::new()
        } else {
            let len = self.state.api_key_buf.len();
            if len <= 4 {
                "*".repeat(len)
            } else {
                format!("{}{}",
                    "*".repeat(len - 4),
                    &self.state.api_key_buf[len - 4..],
                )
            }
        };

        let cursor = if self.state.cursor_visible { "▌" } else { " " };

        vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  Provider: {}", PROVIDERS[self.state.provider_idx].1),
                self.theme.assistant_style(),
            )),
            Line::from(Span::styled(
                format!("  Model:    {}", self.state.chosen_model),
                self.theme.assistant_style(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("  Enter your API key ({env_var}):"),
                self.theme.assistant_style(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("  > {masked}{cursor}"),
                self.theme.input_style(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("  Tip: or set {env_var} env var instead"),
                self.theme.dim_style(),
            )),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "  Enter Confirm  Esc Back  Ctrl+U Clear",
                self.theme.dim_style(),
            )),
        ]
    }
}
