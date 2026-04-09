//! TeamDeleteTool — disbands a team and cleans up.
//!
//! Mirrors `src/tools/TeamDeleteTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::{MemberStatus, SharedState};

pub struct TeamDeleteTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for TeamDeleteTool {
    fn name(&self) -> &'static str { "TeamDelete" }

    fn description(&self) -> &str {
        "Disband the current team and clean up all associated state."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        _input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let mut state = self.state.write().await;

        let team = match &state.team {
            Some(t) => t.clone(),
            None => return Ok(ToolResult::error("Not currently leading a team.")),
        };

        // Check for active non-lead members
        let active_members: Vec<_> = team.members.iter()
            .filter(|m| m.status == MemberStatus::Active)
            .collect();

        if !active_members.is_empty() {
            let names: Vec<_> = active_members.iter().map(|m| m.name.as_str()).collect();
            return Ok(ToolResult::error(format!(
                "Cannot delete team — active members: {}. Stop them first.",
                names.join(", ")
            )));
        }

        // Clean up team directory
        let _ = tokio::fs::remove_dir_all(&team.task_dir).await;

        let team_name = team.name.clone();
        state.team = None;
        state.agent_mailboxes.clear();

        let _ = output_tx.send(ToolOutput {
            text: format!("Disbanded team \"{team_name}\""),
            is_error: false,
        }).await;

        Ok(ToolResult::ok(json!({
            "team_name": team_name,
            "status": "disbanded"
        }).to_string()))
    }
}
