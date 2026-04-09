//! Command registry — slash commands and the /command dispatcher.
//!
//! Mirrors `src/commands/` and command handling in the REPL.
//! Commands are prefixed with `/` and provide quick actions.
#![allow(dead_code)]

use anyhow::Result;

/// A registered slash command.
pub struct Command {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub usage: &'static str,
    pub handler: fn(args: &str, ctx: &mut CommandContext) -> Result<CommandResult>,
}

/// Context available to command handlers.
pub struct CommandContext {
    pub config: crate::config::Config,
    pub session_id: String,
    pub cwd: std::path::PathBuf,
    pub model: String,
    pub vim_mode: bool,
    pub plan_mode: bool,
}

/// Result of executing a command.
pub enum CommandResult {
    /// Display text to the user.
    Message(String),
    /// Switch to a different screen.
    Navigate(crate::tui::screens::Screen),
    /// Send an IPC request.
    IpcRequest(Box<crate::ipc::IpcMessage>),
    /// Update configuration.
    ConfigChanged,
    /// Exit the application.
    Exit,
    /// No visible output.
    Silent,
}

/// The command registry.
pub struct CommandRegistry {
    commands: Vec<Command>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        let mut registry = CommandRegistry { commands: Vec::new() };
        registry.register_builtins();
        registry
    }

    fn register_builtins(&mut self) {
        self.commands.extend(vec![
            Command { name: "help", aliases: &["h", "?"], description: "Show help and available commands", usage: "/help [command]", handler: cmd_help },
            Command { name: "clear", aliases: &["cls"], description: "Clear the conversation", usage: "/clear", handler: cmd_clear },
            Command { name: "cost", aliases: &[], description: "Show session cost and token usage", usage: "/cost", handler: cmd_cost },
            Command { name: "model", aliases: &[], description: "Show or change the active model", usage: "/model [name]", handler: cmd_model },
            Command { name: "config", aliases: &["settings"], description: "Show or change configuration", usage: "/config [key] [value]", handler: cmd_config },
            Command { name: "exit", aliases: &["quit", "q"], description: "Exit the agent", usage: "/exit", handler: cmd_exit },
            Command { name: "version", aliases: &["v"], description: "Show version information", usage: "/version", handler: cmd_version },
            Command { name: "compact", aliases: &[], description: "Compact conversation context", usage: "/compact", handler: cmd_compact },
            Command { name: "diff", aliases: &[], description: "Show file changes since session start", usage: "/diff", handler: cmd_diff },
            Command { name: "memory", aliases: &["mem"], description: "List, add, or delete memories", usage: "/memory [list|add|delete] [args]", handler: cmd_memory },
            Command { name: "skills", aliases: &["skill"], description: "List and invoke skills", usage: "/skills [name]", handler: cmd_skills },
            Command { name: "vim", aliases: &[], description: "Toggle vim mode", usage: "/vim", handler: cmd_vim },
            Command { name: "plan", aliases: &[], description: "Enter plan mode", usage: "/plan", handler: cmd_plan },
            Command { name: "resume", aliases: &[], description: "Resume a previous conversation", usage: "/resume [session_id]", handler: cmd_resume },
            Command { name: "context", aliases: &["ctx"], description: "Show current context information", usage: "/context", handler: cmd_context },
            Command { name: "permissions", aliases: &["perms"], description: "Show permission state", usage: "/permissions", handler: cmd_permissions },
            Command { name: "tasks", aliases: &[], description: "Show background tasks", usage: "/tasks", handler: cmd_tasks },
            Command { name: "agents", aliases: &["team"], description: "Show agent list", usage: "/agents", handler: cmd_agents },
            Command { name: "login", aliases: &[], description: "Authenticate with API key", usage: "/login", handler: cmd_login },
            Command { name: "logout", aliases: &[], description: "Remove stored credentials", usage: "/logout", handler: cmd_logout },
            Command { name: "provider", aliases: &["llm"], description: "Show or change LLM provider", usage: "/provider [anthropic|openai|gemini|ollama]", handler: cmd_provider },
        ]);
    }

    /// Find a command by name or alias.
    pub fn find(&self, name: &str) -> Option<&Command> {
        let name = name.trim_start_matches('/');
        self.commands.iter().find(|c| {
            c.name == name || c.aliases.contains(&name)
        })
    }

    /// Execute a command string (e.g., "/help clear").
    pub fn execute(&self, input: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let input = input.trim();
        if !input.starts_with('/') {
            return Ok(CommandResult::Message("Not a command".to_string()));
        }

        let parts: Vec<&str> = input[1..].splitn(2, ' ').collect();
        let cmd_name = parts[0];
        let args = parts.get(1).unwrap_or(&"").trim();

        match self.find(cmd_name) {
            Some(cmd) => (cmd.handler)(args, ctx),
            None => Ok(CommandResult::Message(format!("Unknown command: /{cmd_name}. Type /help for a list."))),
        }
    }

    /// Get all command names for tab completion.
    pub fn command_names(&self) -> Vec<&str> {
        self.commands.iter().map(|c| c.name).collect()
    }

    /// Get help text for all commands.
    pub fn help_text(&self) -> String {
        let mut lines = vec!["Available commands:".to_string(), String::new()];
        for cmd in &self.commands {
            let aliases = if cmd.aliases.is_empty() {
                String::new()
            } else {
                format!(" ({})", cmd.aliases.join(", "))
            };
            lines.push(format!("  /{:<16}{}{}", cmd.name, cmd.description, aliases));
        }
        lines.join("\n")
    }
}

impl Default for CommandRegistry {
    fn default() -> Self { Self::new() }
}

// ─── Command Handlers ───────────────────────────────────────────────────────

fn cmd_help(_args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::Message(CommandRegistry::new().help_text()))
}

fn cmd_clear(_args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::Message("Conversation cleared.".to_string()))
}

fn cmd_cost(_args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::Message("Cost information available in status bar.".to_string()))
}

fn cmd_model(args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    if args.is_empty() {
        Ok(CommandResult::Message(format!("Current model: {}", ctx.model)))
    } else {
        ctx.model = args.to_string();
        ctx.config.model = args.to_string();
        Ok(CommandResult::Message(format!("Model changed to: {args}")))
    }
}

fn cmd_config(args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    if args.is_empty() {
        let json = serde_json::to_string_pretty(&ctx.config)?;
        Ok(CommandResult::Message(format!("Current config:\n{json}")))
    } else {
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        if parts.len() == 1 {
            let val = ctx.config.get_field(parts[0]);
            Ok(CommandResult::Message(format!("{} = {val}", parts[0])))
        } else {
            let val: serde_json::Value = serde_json::from_str(parts[1])
                .unwrap_or(serde_json::Value::String(parts[1].to_string()));
            ctx.config.set_field(parts[0], val.clone());
            Ok(CommandResult::Message(format!("{} set to {val}", parts[0])))
        }
    }
}

fn cmd_exit(_args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::Exit)
}

fn cmd_version(_args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::Message(format!(
        "Centaur Psicode v{}\nRust {} | {} {}",
        env!("CARGO_PKG_VERSION"),
        rustc_version(),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )))
}

fn cmd_compact(_args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::IpcRequest(Box::new(crate::ipc::IpcMessage::CompactRequest(crate::ipc::CompactRequest {
        request_id: crate::ipc::IpcClient::new_request_id(),
        messages: vec![],
        token_budget: None,
    }))))
}

fn cmd_diff(_args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    match crate::git::open_repo(&ctx.cwd) {
        Ok(repo) => {
            let stats = crate::git::diff_stats(&repo)?;
            Ok(CommandResult::Message(format!(
                "{} file(s) changed, +{} -{} lines",
                stats.files_changed, stats.lines_added, stats.lines_removed
            )))
        }
        Err(_) => Ok(CommandResult::Message("Not a git repository.".to_string())),
    }
}

fn cmd_memory(args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    let action = if args.is_empty() { "list" } else { args };
    Ok(CommandResult::IpcRequest(Box::new(crate::ipc::IpcMessage::MemoryRequest(crate::ipc::MemoryRequest {
        request_id: crate::ipc::IpcClient::new_request_id(),
        action: action.to_string(),
        payload: Default::default(),
    }))))
}

fn cmd_skills(args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    let mut arguments = std::collections::HashMap::new();
    if !args.is_empty() {
        arguments.insert("action".to_string(), serde_json::json!("invoke"));
    } else {
        arguments.insert("action".to_string(), serde_json::json!("list"));
    }
    Ok(CommandResult::IpcRequest(Box::new(crate::ipc::IpcMessage::SkillRequest(crate::ipc::SkillRequest {
        request_id: crate::ipc::IpcClient::new_request_id(),
        skill_name: args.to_string(),
        arguments,
    }))))
}

fn cmd_vim(_args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    ctx.vim_mode = !ctx.vim_mode;
    ctx.config.vim_mode = ctx.vim_mode;
    Ok(CommandResult::Message(format!("Vim mode: {}", if ctx.vim_mode { "ON" } else { "OFF" })))
}

fn cmd_plan(_args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    ctx.plan_mode = !ctx.plan_mode;
    Ok(CommandResult::Message(format!("Plan mode: {}", if ctx.plan_mode { "ON" } else { "OFF" })))
}

fn cmd_resume(args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    let session_id = if args.is_empty() { None } else { Some(args.to_string()) };
    Ok(CommandResult::Navigate(crate::tui::screens::Screen::Resume { session_id }))
}

fn cmd_context(_args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::Message(format!(
        "CWD: {}\nModel: {}\nVim: {}\nPlan: {}",
        ctx.cwd.display(), ctx.model, ctx.vim_mode, ctx.plan_mode
    )))
}

fn cmd_permissions(_args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::Message(format!("Permission mode: {}", ctx.config.permission_mode)))
}

fn cmd_tasks(_args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::Message("Use /tasks to view background tasks (displayed in TUI).".to_string()))
}

fn cmd_agents(_args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::Message("Use /agents to view team members (displayed in TUI).".to_string()))
}

fn cmd_login(_args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::Message("Run `agent login` from the terminal to authenticate.".to_string()))
}

fn cmd_logout(_args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::Message("Run `agent logout` from the terminal to remove credentials.".to_string()))
}

fn cmd_provider(args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    if args.is_empty() {
        let display = match ctx.config.provider.as_str() {
            "first_party" => "anthropic (first_party)",
            other => other,
        };
        Ok(CommandResult::Message(format!("Current provider: {display}\n\nAvailable: anthropic, openai, gemini, ollama")))
    } else {
        let provider = match args.trim().to_lowercase().as_str() {
            "anthropic" | "claude" | "first_party" => "first_party",
            "openai" | "gpt" | "chatgpt" => "openai",
            "gemini" | "google" => "gemini",
            "ollama" | "local" => "ollama",
            other => return Ok(CommandResult::Message(
                format!("Unknown provider: {other}\n\nAvailable: anthropic, openai, gemini, ollama")
            )),
        };
        ctx.config.provider = provider.to_string();
        Ok(CommandResult::Message(format!("Provider changed to: {provider}")))
    }
}

fn rustc_version() -> String {
    "stable".to_string()
}
