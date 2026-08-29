use interprex::{ProviderTextRecord, TextRecordsProvider};

use crate::GithubProvider;

const CARRIER_START: &str = "<!-- ";
const CARRIER_END: &str = "\n-->";

impl TextRecordsProvider for GithubProvider {
    fn embed_record(&self, text: &str, record: &ProviderTextRecord) -> String {
        embed_record(text, record)
    }

    fn extract_records(&self, text: &str) -> Vec<ProviderTextRecord> {
        extract_records(text)
    }
}

pub(super) fn embed_record(text: &str, record: &ProviderTextRecord) -> String {
    let payload = serde_json::to_string(record.value())
        .expect("a provider text record contains a serde_json value");
    // Valid JSON can contain adjacent hyphens only inside a string. JSON
    // Unicode escapes preserve that string while keeping the HTML comment
    // data free of the delimiter's double hyphen.
    let payload = payload.replace("--", "\\u002d\\u002d");
    let carrier = format!(
        "{CARRIER_START}{}:{}\n{payload}{CARRIER_END}",
        record.namespace(),
        record.name()
    );
    if text.is_empty() {
        carrier
    } else {
        format!("{text}\n\n{carrier}")
    }
}

pub(super) fn extract_records(text: &str) -> Vec<ProviderTextRecord> {
    text.match_indices(CARRIER_START)
        .filter_map(|(offset, _)| parse_carrier(&text[offset..]))
        .collect()
}

fn parse_carrier(candidate: &str) -> Option<ProviderTextRecord> {
    let candidate = candidate.strip_prefix(CARRIER_START)?;
    let (identifier, body) = candidate.split_once('\n')?;
    let (namespace, name) = identifier.split_once(':')?;
    let payload_end = body.find(CARRIER_END)?;
    let payload = &body[..payload_end];
    if payload.is_empty() || payload.contains(['\n', '\r']) || payload.contains("--") {
        return None;
    }
    let value = serde_json::from_str(payload).ok()?;
    ProviderTextRecord::new(namespace, name, value).ok()
}

#[cfg(test)]
mod tests {
    use interprex::{ProviderTextRecord, TextRecordsProvider};

    use crate::{GithubConfig, from_config};

    fn provider() -> crate::GithubProvider {
        from_config(GithubConfig::default()).expect("provider")
    }

    fn record(namespace: &str, name: &str, value: serde_json::Value) -> ProviderTextRecord {
        ProviderTextRecord::new(namespace, name, value).expect("record")
    }

    #[test]
    fn carrier_keeps_visible_text_and_round_trips_unicode_and_delimiters() {
        let provider = provider();
        let record = record(
            "comitia",
            "loop-event",
            serde_json::json!({
                "version": 3,
                "message": "Revisão 👋 keeps -- and --> as application data",
                "offset": -2
            }),
        );

        let body = provider.embed_record("Comitia planned round 3.", &record);

        assert!(body.starts_with("Comitia planned round 3.\n\n<!-- comitia:loop-event\n"));
        let carrier = body
            .strip_prefix("Comitia planned round 3.\n\n")
            .expect("visible text remains an exact prefix");
        let payload = carrier
            .strip_prefix("<!-- comitia:loop-event\n")
            .and_then(|value| value.strip_suffix("\n-->"))
            .expect("fixed carrier");
        assert!(
            !payload.contains("--"),
            "HTML comment data has no double hyphen"
        );
        assert_eq!(provider.extract_records(&body), [record]);
    }

    #[test]
    fn extractor_returns_several_records_in_source_order_with_trailing_text() {
        let provider = provider();
        let first = record(
            "comitia",
            "loop-event",
            serde_json::json!({"version": 1, "round": 1}),
        );
        let second = record(
            "other.app",
            "future_record",
            serde_json::json!({"version": 91, "content": ["opaque"]}),
        );
        let body = provider.embed_record("Visible", &first);
        let body = provider.embed_record(&body, &second);
        let body = format!("{body}\n\nTrailing text remains visible.");

        assert_eq!(provider.extract_records(&body), [first, second]);
    }

    #[test]
    fn extractor_omits_malformed_carriers_and_continues_scanning() {
        let provider = provider();
        let valid = record(
            "comitia",
            "loop-event",
            serde_json::json!({"version": 1, "kind": "round-planned"}),
        );
        let valid_carrier = provider.embed_record("", &valid);
        let body = format!(
            "<!-- comitia:loop-event\nnot json\n-->\n\
             <!-- bad namespace:loop-event\n{{\"version\":1}}\n-->\n\
             <!-- comitia:loop-event\n{{\"version\":0}}\n-->\n\
             <!-- comitia:loop-event\n{{\"version\":1,\"unsafe\":\"-->\"}}\n-->\n\
             <!-- comitia:loop-event\n{{\"version\":1}}\n\
             {valid_carrier}"
        );

        assert_eq!(provider.extract_records(&body), [valid]);
    }

    #[test]
    fn generic_records_coexist_with_existing_finding_resolution_markers() {
        let provider = provider();
        let finding = "<!-- interprex:finding-resolution\n{\"version\":1,\"resolution_reason\":\"ADDRESSED\",\"addressing_severity\":\"minor\"}\n-->";
        let loop_event = record(
            "comitia",
            "loop-event",
            serde_json::json!({"version": 1, "kind": "round-planned"}),
        );
        let body = provider.embed_record(finding, &loop_event);

        assert_eq!(
            provider.extract_records(&body),
            [
                record(
                    "interprex",
                    "finding-resolution",
                    serde_json::json!({
                        "version": 1,
                        "resolution_reason": "ADDRESSED",
                        "addressing_severity": "minor"
                    }),
                ),
                loop_event,
            ]
        );
    }
}
