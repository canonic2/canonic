//! Owned QA runs workflows and platform coordination.
use super::arguments::{BeginGoal, BeginRun, EndRun};
use super::{Run, Server, id};
use crate::error::{Error, Result};
use serde_json::{Value, json};

impl Server {
    pub(super) fn begin(&mut self, input: BeginRun) -> Result<Value> {
        if self.runs.len() >= 32 {
            return Err(Error::new(
                "RESOURCE_LIMIT",
                "Connection already owns 32 active runs",
            ));
        }
        if input.goal_ids.is_empty()
            || input.goal_ids.len() > 100
            || input.goal_ids.iter().any(|s| s.is_empty() || s.len() > 128)
        {
            return Err(Error::invalid("Declare 1..100 goal IDs, each 1..128 bytes"));
        }
        if input.label.as_ref().is_some_and(|label| label.len() > 256) {
            return Err(Error::invalid("label exceeds 256 bytes"));
        }
        let run_id = id();
        let evidence = std::env::temp_dir().join(format!("testrun-qa-{run_id}"));
        crate::artifacts::directory(&evidence)?;
        self.runs.insert(
            run_id.clone(),
            Run {
                goals: input.goal_ids.into_iter().collect(),
                evidence: evidence.clone(),
                end_attempted: false,
            },
        );
        if let Some(scheduling) = &self.scheduling {
            scheduling.register(&run_id, &self.runs[&run_id]);
        }
        Ok(json!({"runId":run_id,"evidenceDir":evidence}))
    }
    pub(super) fn begin_goal(&mut self, goal: BeginGoal) -> Result<Value> {
        let run = self
            .runs
            .get_mut(&goal.run_id)
            .ok_or_else(|| Error::new("RUN_NOT_FOUND", "Run is not active"))?;
        if !run.goals.contains(&goal.goal_id) {
            return Err(Error::new(
                "GOAL_NOT_DECLARED",
                "Goal was not declared when the run began",
            ));
        }
        Ok(json!({"goalId":goal.goal_id,"runId":goal.run_id}))
    }
    pub(super) async fn end(&mut self, end: EndRun) -> Result<Value> {
        let run = self
            .runs
            .get_mut(&end.run_id)
            .ok_or_else(|| Error::new("RUN_NOT_FOUND", "Run is not active"))?;
        let summary = json!({"runId":end.run_id,"goals":run.goals,"evidenceDir":run.evidence});
        let evidence = run.evidence.clone();
        let first_attempt = !std::mem::replace(&mut run.end_attempted, true);
        if let Some(scheduling) = self.scheduling.clone() {
            scheduling.finish_run(&end.run_id).await?;
        }
        let sessions: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.run_id == end.run_id)
            .map(|(id, _)| id.clone())
            .collect();
        let mut prior_failure = None;
        for id in sessions {
            if first_attempt && prior_failure.is_none() {
                prior_failure = self.sessions[&id].cleanup_error.clone();
            }
            self.close_session(&id).await?;
        }
        self.cancel_startup().await?;
        if let Some(error) = prior_failure {
            return Err(error);
        }
        crate::artifacts::write(
            &evidence.join("summary.json"),
            &serde_json::to_vec_pretty(&summary)
                .map_err(|_| Error::new("INTERNAL", "Cannot encode run summary"))?,
        )?;
        self.runs.remove(&end.run_id);
        if let Some(scheduling) = &self.scheduling {
            scheduling.retire_run(&end.run_id);
        }
        Ok(json!({"summary":summary}))
    }
}
