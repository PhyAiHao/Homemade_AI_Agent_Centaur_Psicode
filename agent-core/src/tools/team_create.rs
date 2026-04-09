//! TeamCreateTool — creates a multi-agent swarm team.
//!
//! Mirrors `src/tools/TeamCreateTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::{SharedState, TeamState};

pub struct TeamCreateTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &'static str { "TeamCreate" }

    fn description(&self) -> &str {
        "Create a multi-agent swarm team with a team lead and task list infrastructure."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "team_name": {
                    "type": "string",
                    "description": "Name for the team"
                },
                "description": {
                    "type": "string",
                    "description": "Team purpose"
                },
                "agent_type": {
                    "type": "string",
                    "description": "Lead agent type (e.g., 'researcher')"
                }
            },
            "required": ["team_name"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let team_name = input.get("team_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let description = input.get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if team_name.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: team_name"));
        }

        let mut state = self.state.write().await;

        // Check not already leading a team
        if state.team.is_some() {
            return Ok(ToolResult::error("Already leading a team. Delete the current team first."));
        }

        // Generate deterministic lead agent ID
        let lead_id = Uuid::new_v4().to_string();

        // Create task directory
        let task_dir = state.cwd.join(".agent").join("teams").join(&team_name);
        tokio::fs::create_dir_all(&task_dir).await.ok();

        state.team = Some(TeamState {
            name: team_name.clone(),
            description: description.clone(),
            lead_agent_id: lead_id.clone(),
            members: Vec::new(),
            task_dir: task_dir.clone(),
        });

        let _ = output_tx.send(ToolOutput {
            text: format!("Created team \"{team_name}\" (lead: {lead_id})"),
            is_error: false,
        }).await;

        Ok(ToolResult::ok(json!({
            "team_name": team_name,
            "lead_agent_id": lead_id,
            "task_dir": task_dir.to_string_lossy(),
            "description": description,
        }).to_string()))
    }
}
