//! Agent Creation Wizard — multi-step wizard for creating custom agent definitions.
//!
//! Mirrors `src/components/agents/new-agent-creation/CreateAgentWizard.tsx`
//! and its 12 step files.
//!
//! Two navigation paths:
//! 1. Generate: Location → Method("generate") → GeneratePrompt → [skip to] Tools → Model → Color → Confirm
//! 2. Manual:   Location → Method("manual") → [skip to] Type → Prompt → Description → Tools → Model → Color → Confirm
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Widget, Wrap};
use std::path::PathBuf;

use crate::tui::theme::Theme;

// ─── Wizard step enum ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WizardStep {
    Location,       // 0 - Choose project or personal
    Method,         // 1 - Generate or manual
    Generate,       // 2 - AI generation prompt (generate path only)
    AgentType,      // 3 - Agent identifier
    Prompt,         // 4 - System prompt
    Description,    // 5 - When to use
    Tools,          // 6 - Select tools
    Model,          // 7 - Select model
    Color,          // 8 - Choose color
    Confirm,        // 9 - Review and save
}

impl WizardStep {
    fn title(&self) -> &str {
        match self {
            Self::Location => "Choose Location",
            Self::Method => "Creation Method",
            Self::Generate => "Generate Agent",
            Self::AgentType => "Agent Type (Identifier)",
            Self::Prompt => "System Prompt",
            Self::Description => "Description (When to Use)",
            Self::Tools => "Select Tools",
            Self::Model => "Select Model",
            Self::Color => "Choose Color",
            Self::Confirm => "Review & Save",
        }
    }

    fn step_number(&self) -> usize {
        match self {
            Self::Location => 1,
            Self::Method => 2,
            Self::Generate => 3,
            Self::AgentType => 3,
            Self::Prompt => 4,
            Self::Description => 5,
            Self::Tools => 6,
            Self::Model => 7,
            Self::Color => 8,
            Self::Confirm => 9,
        }
    }
}

// ─── Agent location ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AgentLocation {
    /// Project-scoped: <cwd>/.claude/agents/
    Project,
    /// User-scoped: ~/.claude/agents/
    Personal,
}

impl AgentLocation {
    pub fn display(&self) -> &str {
        match self {
            Self::Project => "Project (.claude/agents/)",
            Self::Personal => "Personal (~/.claude/agents/)",
        }
    }

    pub fn directory(&self) -> PathBuf {
        match self {
            Self::Project => {
                let cwd = std::env::current_dir().unwrap_or_default();
                cwd.join(".claude").join("agents")
            }
            Self::Personal => {
                let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
                home.join(".claude").join("agents")
            }
        }
    }
}

// ─── Agent color ────────────────────────────────────────────────────────────

const AGENT_COLORS: &[(&str, Color)] = &[
    ("automatic", Color::Reset),
    ("blue", Color::Blue),
    ("green", Color::Green),
    ("yellow", Color::Yellow),
    ("red", Color::Red),
    ("magenta", Color::Magenta),
    ("cyan", Color::Cyan),
    ("white", Color::White),
];

// ─── Available models ───────────────────────────────────────────────────────

const AVAILABLE_MODELS: &[(&str, &str)] = &[
    ("default", "Inherit from parent (recommended)"),
    ("claude-sonnet-4-6", "Claude Sonnet 4.6 — fast, balanced"),
    ("claude-opus-4-6", "Claude Opus 4.6 — most capable"),
    ("claude-haiku-4-5-20251001", "Claude Haiku 4.5 — fastest, cheapest"),
];

// ─── Wizard data ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[derive(Default)]
pub struct AgentWizardData {
    pub location: Option<AgentLocation>,
    pub method: Option<String>,          // "generate" or "manual"
    pub agent_type: String,              // identifier like "test-runner"
    pub system_prompt: String,
    pub when_to_use: String,             // description
    pub selected_tools: Option<Vec<String>>, // None = all tools
    pub selected_model: Option<String>,  // None = default
    pub selected_color: Option<String>,
    pub generation_prompt: String,       // for generate path
}


// ─── Validation ─────────────────────────────────────────────────────────────

fn validate_agent_type(s: &str) -> Result<(), String> {
    if s.is_empty() {
        return Err("Agent type is required".into());
    }
    if s.len() < 3 {
        return Err("Agent type must be at least 3 characters".into());
    }
    if s.len() > 50 {
        return Err("Agent type must be at most 50 characters".into());
    }
    let re = regex::Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9-]*[a-zA-Z0-9]$").unwrap();
    if !re.is_match(s) {
        return Err("Must start/end with alphanumeric, only hyphens allowed in middle".into());
    }
    Ok(())
}

// ─── Wizard state ───────────────────────────────────────────────────────────

pub struct AgentWizardState {
    pub step: WizardStep,
    pub data: AgentWizardData,
    pub select_index: usize,          // for select-based steps
    pub text_input: String,           // for text input steps
    pub error: Option<String>,
    pub done: bool,                   // wizard completed
    pub cancelled: bool,              // wizard cancelled
    pub saved_path: Option<PathBuf>,  // path where agent was saved
    pub available_tools: Vec<String>, // populated from tool registry
    pub tool_selected: Vec<bool>,     // checkboxes for tools step
}

impl AgentWizardState {
    pub fn new(available_tools: Vec<String>) -> Self {
        let tool_count = available_tools.len();
        AgentWizardState {
            step: WizardStep::Location,
            data: AgentWizardData::default(),
            select_index: 0,
            text_input: String::new(),
            error: None,
            done: false,
            cancelled: false,
            saved_path: None,
            tool_selected: vec![true; tool_count], // all selected by default
            available_tools,
        }
    }

    /// Handle a key press. Returns true if the wizard should close.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;

        // Global: Esc cancels
        if key.code == KeyCode::Esc {
            if self.step == WizardStep::Location {
                self.cancelled = true;
                return true;
            }
            self.go_back();
            return false;
        }

        self.error = None;

        match self.step {
            WizardStep::Location => self.handle_select_step(key, &["Project", "Personal"]),
            WizardStep::Method => self.handle_select_step(key, &["Generate with Claude", "Manual configuration"]),
            WizardStep::Generate => self.handle_text_step(key),
            WizardStep::AgentType => self.handle_text_step(key),
            WizardStep::Prompt => self.handle_text_step(key),
            WizardStep::Description => self.handle_text_step(key),
            WizardStep::Tools => self.handle_tools_step(key),
            WizardStep::Model => self.handle_select_step(key, &["default", "sonnet", "opus", "haiku"]),
            WizardStep::Color => self.handle_select_step(key, &["automatic", "blue", "green", "yellow", "red", "magenta", "cyan", "white"]),
            WizardStep::Confirm => self.handle_confirm_step(key),
        }

        self.done || self.cancelled
    }

    fn handle_select_step(&mut self, key: crossterm::event::KeyEvent, options: &[&str]) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_index = self.select_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_index = (self.select_index + 1).min(options.len().saturating_sub(1));
            }
            KeyCode::Enter => {
                self.apply_select(options);
            }
            _ => {}
        }
    }

    fn handle_text_step(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Enter => {
                self.apply_text();
            }
            KeyCode::Backspace => {
                self.text_input.pop();
            }
            KeyCode::Char(c) => {
                self.text_input.push(c);
            }
            _ => {}
        }
    }

    fn handle_tools_step(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_index = self.select_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_index = (self.select_index + 1).min(self.available_tools.len().saturating_sub(1));
            }
            KeyCode::Char(' ') => {
                // Toggle tool selection
                if self.select_index < self.tool_selected.len() {
                    self.tool_selected[self.select_index] = !self.tool_selected[self.select_index];
                }
            }
            KeyCode::Char('a') => {
                // Select/deselect all
                let all_selected = self.tool_selected.iter().all(|&s| s);
                for s in self.tool_selected.iter_mut() {
                    *s = !all_selected;
                }
            }
            KeyCode::Enter => {
                // Collect selected tools
                let selected: Vec<String> = self.available_tools.iter()
                    .zip(self.tool_selected.iter())
                    .filter(|(_, &sel)| sel)
                    .map(|(name, _)| name.clone())
                    .collect();
                self.data.selected_tools = if selected.len() == self.available_tools.len() {
                    None // all tools = None
                } else {
                    Some(selected)
                };
                self.select_index = 0;
                self.step = WizardStep::Model;
            }
            _ => {}
        }
    }

    fn handle_confirm_step(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Enter | KeyCode::Char('s') => {
                match self.save_agent() {
                    Ok(path) => {
                        self.saved_path = Some(path);
                        self.done = true;
                    }
                    Err(e) => {
                        self.error = Some(format!("Save failed: {e}"));
                    }
                }
            }
            _ => {}
        }
    }

    fn apply_select(&mut self, _options: &[&str]) {
        match self.step {
            WizardStep::Location => {
                self.data.location = Some(match self.select_index {
                    0 => AgentLocation::Project,
                    _ => AgentLocation::Personal,
                });
                self.select_index = 0;
                self.step = WizardStep::Method;
            }
            WizardStep::Method => {
                match self.select_index {
                    0 => {
                        self.data.method = Some("generate".into());
                        self.text_input.clear();
                        self.step = WizardStep::Generate;
                    }
                    _ => {
                        self.data.method = Some("manual".into());
                        self.text_input.clear();
                        self.step = WizardStep::AgentType;
                    }
                }
                self.select_index = 0;
            }
            WizardStep::Model => {
                self.data.selected_model = match self.select_index {
                    0 => None, // default
                    1 => Some("claude-sonnet-4-6".into()),
                    2 => Some("claude-opus-4-6".into()),
                    3 => Some("claude-haiku-4-5-20251001".into()),
                    _ => None,
                };
                self.select_index = 0;
                self.step = WizardStep::Color;
            }
            WizardStep::Color => {
                self.data.selected_color = if self.select_index == 0 {
                    None // automatic
                } else {
                    Some(AGENT_COLORS.get(self.select_index)
                        .map(|(name, _)| name.to_string())
                        .unwrap_or_default())
                };
                self.step = WizardStep::Confirm;
            }
            _ => {}
        }
    }

    fn apply_text(&mut self) {
        match self.step {
            WizardStep::Generate => {
                if self.text_input.trim().is_empty() {
                    self.error = Some("Please describe the agent you want to create".into());
                    return;
                }
                self.data.generation_prompt = self.text_input.clone();
                // For now, create a basic agent from the description
                // (full AI generation would need IPC to Python)
                let prompt = self.text_input.trim().to_string();
                self.data.agent_type = prompt.split_whitespace()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("-")
                    .to_lowercase()
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-')
                    .collect();
                if self.data.agent_type.len() < 3 {
                    self.data.agent_type = "custom-agent".into();
                }
                self.data.system_prompt = format!(
                    "You are a specialized AI agent. Your purpose: {prompt}"
                );
                self.data.when_to_use = format!(
                    "Use this agent when the user needs help with: {prompt}"
                );
                self.text_input.clear();
                self.select_index = 0;
                self.step = WizardStep::Tools; // skip to tools
            }
            WizardStep::AgentType => {
                let input = self.text_input.trim().to_string();
                if let Err(e) = validate_agent_type(&input) {
                    self.error = Some(e);
                    return;
                }
                self.data.agent_type = input;
                self.text_input.clear();
                self.step = WizardStep::Prompt;
            }
            WizardStep::Prompt => {
                let input = self.text_input.trim().to_string();
                if input.is_empty() {
                    self.error = Some("System prompt is required".into());
                    return;
                }
                if input.len() < 20 {
                    self.error = Some("System prompt should be at least 20 characters".into());
                    return;
                }
                self.data.system_prompt = input;
                self.text_input.clear();
                self.step = WizardStep::Description;
            }
            WizardStep::Description => {
                let input = self.text_input.trim().to_string();
                if input.is_empty() {
                    self.error = Some("Description is required".into());
                    return;
                }
                self.data.when_to_use = input;
                self.text_input.clear();
                self.select_index = 0;
                self.step = WizardStep::Tools;
            }
            _ => {}
        }
    }

    fn go_back(&mut self) {
        self.error = None;
        self.text_input.clear();
        self.select_index = 0;
        match self.step {
            WizardStep::Location => { self.cancelled = true; }
            WizardStep::Method => { self.step = WizardStep::Location; }
            WizardStep::Generate => { self.step = WizardStep::Method; }
            WizardStep::AgentType => { self.step = WizardStep::Method; }
            WizardStep::Prompt => { self.step = WizardStep::AgentType; }
            WizardStep::Description => { self.step = WizardStep::Prompt; }
            WizardStep::Tools => {
                if self.data.method.as_deref() == Some("generate") {
                    self.step = WizardStep::Generate;
                } else {
                    self.step = WizardStep::Description;
                }
            }
            WizardStep::Model => { self.step = WizardStep::Tools; }
            WizardStep::Color => { self.step = WizardStep::Model; }
            WizardStep::Confirm => { self.step = WizardStep::Color; }
        }
    }

    // ─── Save to disk ───────────────────────────────────────────────────

    fn save_agent(&self) -> Result<PathBuf, String> {
        let location = self.data.location.as_ref()
            .ok_or("Location not set")?;
        let dir = location.directory();

        // Create directory
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create directory: {e}"))?;

        let filename = format!("{}.md", self.data.agent_type);
        let path = dir.join(&filename);

        // Check if file already exists
        if path.exists() {
            return Err(format!(
                "Agent '{}' already exists at {}",
                self.data.agent_type,
                path.display()
            ));
        }

        // Build markdown content with YAML frontmatter
        let content = self.format_agent_markdown();

        std::fs::write(&path, content)
            .map_err(|e| format!("Failed to write file: {e}"))?;

        Ok(path)
    }

    fn format_agent_markdown(&self) -> String {
        let mut frontmatter = Vec::new();
        frontmatter.push(format!("name: {}", self.data.agent_type));

        // Escape description for YAML
        let desc_escaped = self.data.when_to_use
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        frontmatter.push(format!("description: \"{}\"", desc_escaped));

        if let Some(ref tools) = self.data.selected_tools {
            if !tools.is_empty() {
                frontmatter.push(format!("tools: {}", tools.join(", ")));
            }
        }

        if let Some(ref model) = self.data.selected_model {
            frontmatter.push(format!("model: {model}"));
        }

        if let Some(ref color) = self.data.selected_color {
            frontmatter.push(format!("color: {color}"));
        }

        format!(
            "---\n{}\n---\n\n{}",
            frontmatter.join("\n"),
            self.data.system_prompt
        )
    }
}

// ─── Rendering ──────────────────────────────────────────────────────────────

pub struct AgentWizardWidget<'a> {
    state: &'a AgentWizardState,
    theme: &'a Theme,
}

impl<'a> AgentWizardWidget<'a> {
    pub fn new(state: &'a AgentWizardState, theme: &'a Theme) -> Self {
        AgentWizardWidget { state, theme }
    }
}

impl<'a> Widget for AgentWizardWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = 70u16.min(area.width.saturating_sub(4));
        let height = 24u16.min(area.height.saturating_sub(2));
        let x = (area.width.saturating_sub(width)) / 2 + area.x;
        let y = (area.height.saturating_sub(height)) / 2 + area.y;
        let dialog_area = Rect::new(x, y, width, height);

        Clear.render(dialog_area, buf);

        let step = &self.state.step;
        let title = format!(
            " Create Agent — Step {}: {} ",
            step.step_number(), step.title()
        );

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(self.theme.accent_style());

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        match self.state.step {
            WizardStep::Location => self.render_select(inner, buf, &[
                ("Project (.claude/agents/)", "Scoped to this project"),
                ("Personal (~/.claude/agents/)", "Available in all projects"),
            ]),
            WizardStep::Method => self.render_select(inner, buf, &[
                ("Generate with Claude (recommended)", "Describe what you want and Claude builds it"),
                ("Manual configuration", "Set each field yourself"),
            ]),
            WizardStep::Generate => self.render_text_input(inner, buf,
                "Describe the agent you want to create:",
                "e.g., An agent that runs tests and fixes failures...",
            ),
            WizardStep::AgentType => self.render_text_input(inner, buf,
                "Enter the agent type (identifier):",
                "e.g., test-runner, tech-lead, code-reviewer",
            ),
            WizardStep::Prompt => self.render_text_input(inner, buf,
                "Enter the system prompt:",
                "Be comprehensive for best results...",
            ),
            WizardStep::Description => self.render_text_input(inner, buf,
                "When should Claude use this agent?",
                "e.g., Use this agent after writing code to run tests...",
            ),
            WizardStep::Tools => self.render_tools(inner, buf),
            WizardStep::Model => self.render_select(inner, buf, &[
                ("Default (inherit)", "Use the parent's model"),
                ("Claude Sonnet 4.6", "Fast, balanced"),
                ("Claude Opus 4.6", "Most capable"),
                ("Claude Haiku 4.5", "Fastest, cheapest"),
            ]),
            WizardStep::Color => self.render_colors(inner, buf),
            WizardStep::Confirm => self.render_confirm(inner, buf),
        }
    }
}

impl<'a> AgentWizardWidget<'a> {
    fn render_select(&self, area: Rect, buf: &mut Buffer, options: &[(&str, &str)]) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);

        let items: Vec<ListItem> = options.iter().enumerate().map(|(i, (label, desc))| {
            let marker = if i == self.state.select_index { "▸ " } else { "  " };
            let style = if i == self.state.select_index {
                Style::default().reversed()
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::raw(marker),
                Span::styled(*label, style.bold()),
                Span::styled(format!("  {desc}"), self.theme.dim_style()),
            ]))
        }).collect();

        Widget::render(List::new(items), chunks[0], buf);

        let footer = self.build_footer();
        Paragraph::new(footer).render(chunks[1], buf);
    }

    fn render_text_input(&self, area: Rect, buf: &mut Buffer, prompt: &str, placeholder: &str) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area);

        // Prompt
        Paragraph::new(Span::styled(prompt, self.theme.accent_style()))
            .render(chunks[0], buf);

        // Input
        let display = if self.state.text_input.is_empty() {
            Span::styled(placeholder, self.theme.dim_style())
        } else {
            Span::styled(&self.state.text_input, self.theme.input_style())
        };
        Paragraph::new(Line::from(vec![
            Span::raw("▸ "),
            display,
            Span::styled("_", Style::default().rapid_blink()),
        ]))
        .block(Block::default().borders(Borders::BOTTOM).border_style(self.theme.dim_style()))
        .render(chunks[1], buf);

        // Error
        if let Some(ref err) = self.state.error {
            Paragraph::new(Span::styled(err, self.theme.error_style()))
                .render(chunks[2], buf);
        }

        let footer = self.build_footer();
        Paragraph::new(footer).render(chunks[3], buf);
    }

    fn render_tools(&self, area: Rect, buf: &mut Buffer) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);

        let selected_count = self.state.tool_selected.iter().filter(|&&s| s).count();
        Paragraph::new(Span::styled(
            format!("Select tools ({}/{}). Space=toggle, a=all, Enter=confirm",
                selected_count, self.state.available_tools.len()),
            self.theme.accent_style(),
        )).render(chunks[0], buf);

        // Scrollable tool list
        let visible_height = chunks[1].height as usize;
        let scroll_offset = self.state.select_index.saturating_sub(visible_height / 2);

        let items: Vec<ListItem> = self.state.available_tools.iter()
            .zip(self.state.tool_selected.iter())
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, (name, &selected))| {
                let marker = if i == self.state.select_index { "▸" } else { " " };
                let checkbox = if selected { "[✓]" } else { "[ ]" };
                let style = if i == self.state.select_index {
                    Style::default().reversed()
                } else if selected {
                    Style::default().fg(Color::Green)
                } else {
                    self.theme.dim_style()
                };
                ListItem::new(Span::styled(
                    format!("{marker} {checkbox} {name}"), style
                ))
            })
            .collect();

        Widget::render(List::new(items), chunks[1], buf);

        let footer = self.build_footer();
        Paragraph::new(footer).render(chunks[2], buf);
    }

    fn render_colors(&self, area: Rect, buf: &mut Buffer) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);

        let items: Vec<ListItem> = AGENT_COLORS.iter().enumerate().map(|(i, (name, color))| {
            let marker = if i == self.state.select_index { "▸ " } else { "  " };
            let swatch = if *color == Color::Reset { "◉" } else { "●" };
            let style = if i == self.state.select_index {
                Style::default().fg(*color).reversed()
            } else {
                Style::default().fg(*color)
            };
            ListItem::new(Line::from(vec![
                Span::raw(marker),
                Span::styled(swatch, Style::default().fg(*color)),
                Span::styled(format!(" {name}"), style),
            ]))
        }).collect();

        Widget::render(List::new(items), chunks[0], buf);

        let footer = self.build_footer();
        Paragraph::new(footer).render(chunks[1], buf);
    }

    fn render_confirm(&self, area: Rect, buf: &mut Buffer) {
        let data = &self.state.data;
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled("Review your agent:", self.theme.accent_style().bold())));
        lines.push(Line::from(""));

        // Type
        lines.push(Line::from(vec![
            Span::styled("  Type:        ", self.theme.assistant_style()),
            Span::styled(&data.agent_type, Style::default().bold()),
        ]));

        // Description (truncated)
        let desc = if data.when_to_use.len() > 60 {
            format!("{}...", &data.when_to_use[..57])
        } else {
            data.when_to_use.clone()
        };
        lines.push(Line::from(vec![
            Span::styled("  Description: ", self.theme.assistant_style()),
            Span::styled(desc, self.theme.dim_style()),
        ]));

        // Prompt (truncated)
        let prompt_preview = if data.system_prompt.len() > 60 {
            format!("{}...", &data.system_prompt[..57])
        } else {
            data.system_prompt.clone()
        };
        lines.push(Line::from(vec![
            Span::styled("  Prompt:      ", self.theme.assistant_style()),
            Span::styled(prompt_preview, self.theme.dim_style()),
        ]));

        // Tools
        let tools_display = match &data.selected_tools {
            None => "All tools".to_string(),
            Some(t) if t.is_empty() => "None".to_string(),
            Some(t) => format!("{} tools selected", t.len()),
        };
        lines.push(Line::from(vec![
            Span::styled("  Tools:       ", self.theme.assistant_style()),
            Span::styled(tools_display, self.theme.dim_style()),
        ]));

        // Model
        lines.push(Line::from(vec![
            Span::styled("  Model:       ", self.theme.assistant_style()),
            Span::styled(
                data.selected_model.as_deref().unwrap_or("default"),
                self.theme.dim_style(),
            ),
        ]));

        // Color
        lines.push(Line::from(vec![
            Span::styled("  Color:       ", self.theme.assistant_style()),
            Span::styled(
                data.selected_color.as_deref().unwrap_or("automatic"),
                self.theme.dim_style(),
            ),
        ]));

        // Location
        if let Some(ref loc) = data.location {
            lines.push(Line::from(vec![
                Span::styled("  Location:    ", self.theme.assistant_style()),
                Span::styled(loc.display(), self.theme.dim_style()),
            ]));

            let path = loc.directory().join(format!("{}.md", data.agent_type));
            lines.push(Line::from(vec![
                Span::styled("  File:        ", self.theme.assistant_style()),
                Span::styled(path.display().to_string(), self.theme.dim_style()),
            ]));
        }

        lines.push(Line::from(""));

        // Error
        if let Some(ref err) = self.state.error {
            lines.push(Line::from(Span::styled(err, self.theme.error_style())));
            lines.push(Line::from(""));
        }

        lines.push(Line::from(vec![
            Span::styled("[Enter/s] ", Style::default().fg(Color::Green).bold()),
            Span::styled("Save", Style::default().fg(Color::Green)),
            Span::raw("  "),
            Span::styled("[Esc] ", self.theme.dim_style()),
            Span::styled("Back", self.theme.dim_style()),
        ]));

        Paragraph::new(lines).wrap(Wrap { trim: false }).render(area, buf);
    }

    fn build_footer(&self) -> Line<'a> {
        Line::from(vec![
            Span::styled("Enter=select  ", self.theme.dim_style()),
            Span::styled("Esc=back  ", self.theme.dim_style()),
            Span::styled(
                format!("Step {}/9", self.state.step.step_number()),
                self.theme.dim_style(),
            ),
        ])
    }
}
