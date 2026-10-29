use serde::Deserialize;

const PANE_FOCUSED_EVENT: &str = "pane_focused";

#[derive(Deserialize)]
struct HookEvent {
    event: String,
    #[serde(default)]
    data: HookEventData,
}

#[derive(Deserialize, Default)]
struct HookEventData {
    pane_id: Option<String>,
}

pub fn announced_pane(event_json: &str) -> Result<Option<String>, serde_json::Error> {
    let event: HookEvent = serde_json::from_str(event_json)?;
    Ok(event
        .data
        .pane_id
        .filter(|_| event.event == PANE_FOCUSED_EVENT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_focused_event_names_its_pane() {
        let event = r#"{"event":"pane_focused","data":{"pane_id":"p1","workspace_id":"w1"}}"#;

        let pane = announced_pane(event).unwrap();

        assert_eq!(pane.as_deref(), Some("p1"));
    }

    #[test]
    fn other_events_name_no_pane() {
        let event = r#"{"event":"workspace_focused","data":{"workspace_id":"w1"}}"#;

        let pane = announced_pane(event).unwrap();

        assert_eq!(pane, None);
    }

    #[test]
    fn a_malformed_event_is_a_json_syntax_error() {
        let error = announced_pane("not json").unwrap_err();

        assert!(error.is_syntax());
    }
}
