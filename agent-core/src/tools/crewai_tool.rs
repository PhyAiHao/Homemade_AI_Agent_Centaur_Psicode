//! CrewAI tool — spawns a CrewAI crew from the Centaur Psicode agent.
//!
//! When the LLM needs multi-role orchestration (researcher + writer, coder + reviewer),
//! it calls this tool with a crew configuration. The Python brain creates a CrewAI crew,
//! runs it, and returns the final result.
//!
//! The crew config is a JSON object:
//! ```json
//! {
//!   "agents": [{"role": "...", "goal": "...", "backstory": "...", "name": "..."}],
//!   "tasks": [{"description": "...", "expected_output": "...", "agent": "name", "context_indices": [0]}],
//!   "process": "sequential" | "hierarchical"
//! }
//! ```

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct CrewAITool;

#[async_trait]
impl Tool for CrewAITool {
    fn name(&self) -> &'static str { "CrewAI" }

    fn description(&self) -> &str {
        "Run a multi-agent CrewAI crew for tasks requiring multiple roles. \
         Define agents with roles/goals and tasks with descriptions, then \
         the crew executes them sequentially or with a manager (hierarchical). \
         Use when a task benefits from distinct perspectives (e.g., \
         researcher + writer, coder + reviewer, analyst + presenter)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "crew_config": {
                    "type": "object",
                    "description": "Crew definition with agents, tasks, and process type",
                    "properties": {
                        "agents": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string", "description": "Unique agent identifier" },
                                    "role": { "type": "string", "description": "Agent's role (e.g., 'Researcher')" },
                                    "goal": { "type": "string", "description": "What the agent aims to achieve" },
                                    "backstory": { "type": "string", "description": "Agent's background context" },
                                    "llm": { "type": "string", "description": "Model to use (default: claude-sonnet-4-6)" }
                                },
                                "required": ["name", "role", "goal"]
                            }
                        },
                        "tasks": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "description": { "type": "string", "description": "Task description" },
                                    "expected_output": { "type": "string", "description": "What the task should produce" },
                                    "agent": { "type": "string", "description": "Name of the agent to assign this task to" },
                                    "context_indices": {
                                        "type": "array",
                                        "items": { "type": "integer" },
                                        "description": "Indices of previous tasks whose output feeds into this task"
                                    }
                                },
                                "required": ["description", "agent"]
                            }
                        },
                        "process": {
                            "type": "string",
                            "enum": ["sequential", "hierarchical"],
                            "description": "Execution mode (default: sequential)"
                        }
                    },
                    "required": ["agents", "tasks"]
                },
                "inputs": {
                    "type": "object",
                    "description": "Input variables to pass to the crew (e.g., {\"topic\": \"AI safety\"})"
                }
            },
            "required": ["crew_config"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let crew_config = input.get("crew_config").cloned().unwrap_or(json!({}));
        let inputs = input.get("inputs").cloned().unwrap_or(json!({}));

        // Validate minimum structure
        let agents = crew_config.get("agents").and_then(|v| v.as_array());
        let tasks = crew_config.get("tasks").and_then(|v| v.as_array());

        if agents.map(|a| a.is_empty()).unwrap_or(true) {
            return Ok(ToolResult::error("crew_config must define at least one agent"));
        }
        if tasks.map(|t| t.is_empty()).unwrap_or(true) {
            return Ok(ToolResult::error("crew_config must define at least one task"));
        }

        let agent_count = agents.map(|a| a.len()).unwrap_or(0);
        let task_count = tasks.map(|t| t.len()).unwrap_or(0);
        let process = crew_config.get("process")
            .and_then(|v| v.as_str())
            .unwrap_or("sequential");

        let _ = output_tx.send(ToolOutput {
            text: format!(
                "Running CrewAI crew: {agent_count} agents, {task_count} tasks, {process} process..."
            ),
            is_error: false,
        }).await;

        // Send to Python brain via IPC
        let socket = std::env::var("AGENT_IPC_SOCKET")
            .unwrap_or_else(|_| "/tmp/agent-ipc.sock".into());

        let mut ipc = match crate::ipc::IpcClient::connect_to(&socket).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("IPC connection failed: {e}"))),
        };

        let mut payload = HashMap::new();
        payload.insert("crew_config".to_string(), crew_config);
        payload.insert("inputs".to_string(), inputs);

        let request = crate::ipc::IpcMessage::MemoryRequest(crate::ipc::MemoryRequest {
            request_id: crate::ipc::IpcClient::new_request_id(),
            action: "crewai_run".to_string(),
            payload,
        });

        // Use long timeout — crews can take minutes
        let timeout = std::time::Duration::from_secs(600);
        match ipc.request_with_timeout(request, timeout).await {
            Ok(crate::ipc::IpcMessage::MemoryResponse(resp)) if resp.ok => {
                let result = resp.payload.get("result")
                    .and_then(|v| v.as_str())
                    .unwrap_or("[No result from crew]");

                let _ = output_tx.send(ToolOutput {
                    text: format!("CrewAI crew completed ({agent_count} agents, {task_count} tasks)"),
                    is_error: false,
                }).await;

                Ok(ToolResult::ok(result))
            }
            Ok(crate::ipc::IpcMessage::MemoryResponse(resp)) => {
                let err = resp.error.unwrap_or_else(|| "unknown error".to_string());
                Ok(ToolResult::error(format!("CrewAI execution failed: {err}")))
            }
            Ok(_) => Ok(ToolResult::error("Unexpected response from CrewAI service")),
            Err(e) => Ok(ToolResult::error(format!("CrewAI IPC error: {e}"))),
        }
    }
}
