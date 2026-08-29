use interprex::{ProviderTextRecord, TextRecordsProvider};

use crate::FakeProvider;

const CARRIER_START: &str = "<!-- ";
const CARRIER_END: &str = "\n-->";

impl TextRecordsProvider for FakeProvider {
    fn embed_record(&self, text: &str, record: &ProviderTextRecord) -> String {
        let payload = serde_json::to_string(record.value())
            .expect("a provider text record contains a serde_json value");
        // Match the GitHub adapter: valid JSON can contain adjacent hyphens
        // only inside a string, where JSON Unicode escapes preserve them.
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

    fn extract_records(&self, text: &str) -> Vec<ProviderTextRecord> {
        text.match_indices(CARRIER_START)
            .filter_map(|(offset, _)| parse_carrier(&text[offset..]))
            .collect()
    }
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
