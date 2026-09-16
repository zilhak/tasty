//! Positive persisted evidence and explicit incomplete-history diagnostics.
use super::{Binding, Event, protocol::Client};
use anyhow::{Result, bail};
use serde_json::{Value, json};
pub struct Scan {
    pub matched: bool,
    pub evidence: Value,
}
impl Client {
    pub fn recorded(&mut self, binding: &Binding, event: &Event) -> Result<Scan> {
        let marker = &event.delivery_id;
        let metadata = self.call(
            "thread/read",
            json!({"threadId":binding.thread_id,"includeTurns":false}),
        )?;
        let mode = metadata["thread"]["historyMode"]
            .as_str()
            .unwrap_or("legacy");
        if mode == "legacy" {
            let history = self.call(
                "thread/read",
                json!({"threadId":binding.thread_id,"includeTurns":true}),
            )?;
            let turns = history["thread"]["turns"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("history_incomplete: turns missing"))?;
            let complete = turns.iter().all(|t| {
                t["itemsView"].as_str().unwrap_or("full") == "full" && t["items"].is_array()
            });
            return Ok(Scan {
                matched: contains_output(&history, marker),
                evidence: json!({"history_mode":mode,"complete":complete,"pages":1,"items_view":if complete{"full"}else{"incomplete"},"subscription":"resume_verified_for_this_connection","absence_proves_rejection":false}),
            });
        }
        let mut cursor = Value::Null;
        let mut cursors = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            cursors.push(cursor.clone());
            let page=self.call("thread/items/list",json!({"threadId":binding.thread_id,"cursor":cursor,"limit":100,"sortDirection":"asc"}))?;
            if !page["data"].is_array() {
                bail!("history_incomplete: malformed items page");
            }
            let matched = contains_output(&page, marker);
            cursor = page["nextCursor"].clone();
            if matched || cursor.is_null() {
                return Ok(Scan {
                    matched,
                    evidence: json!({"history_mode":mode,"complete":cursor.is_null(),"pages":cursors.len(),"cursors":cursors,"next_cursor":cursor,"items_view":"full","subscription":"resume_verified_for_this_connection","absence_proves_rejection":false}),
                });
            }
            if !seen.insert(cursor.to_string()) {
                bail!("history_incomplete: cursor cycle");
            }
        }
        bail!("history_incomplete: page budget exceeded")
    }
}
fn contains_output(value: &Value, marker: &str) -> bool {
    match value {
        Value::Object(obj) => {
            if obj.get("type").and_then(Value::as_str) == Some("functionCallOutput") {
                return obj
                    .get("output")
                    .and_then(Value::as_str)
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                    .is_some_and(|v| v["event_id"].as_str() == Some(marker));
            }
            obj.values().any(|v| contains_output(v, marker))
        }
        Value::Array(values) => values.iter().any(|v| contains_output(v, marker)),
        _ => false,
    }
}
