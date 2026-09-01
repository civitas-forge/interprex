use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use interprex::{CodeHostingProvider, ProviderError, Repository};
use interprex_github::{GithubConfig, from_config};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

#[derive(Clone, Default)]
struct RecordingSubscriber {
    next_id: Arc<AtomicU64>,
    spans: Arc<Mutex<Vec<RecordedSpan>>>,
}

#[derive(Debug)]
struct RecordedSpan {
    name: &'static str,
    fields: BTreeMap<String, String>,
}

impl Subscriber for RecordingSubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let mut visitor = FieldVisitor::default();
        attributes.record(&mut visitor);
        self.spans
            .lock()
            .expect("span recorder")
            .push(RecordedSpan {
                name: attributes.metadata().name(),
                fields: visitor.fields,
            });
        Id::from_u64(self.next_id.fetch_add(1, Ordering::Relaxed) + 1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, _event: &Event<'_>) {}

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[derive(Default)]
struct FieldVisitor {
    fields: BTreeMap<String, String>,
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_owned(), value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
}

#[tokio::test]
async fn provider_operation_emits_a_bounded_span_before_authentication() {
    let subscriber = RecordingSubscriber::default();
    let recorded = Arc::clone(&subscriber.spans);
    let _guard = tracing::subscriber::set_default(subscriber);
    let provider = from_config(GithubConfig::default()).expect("provider");
    let repository = Repository::new("example", "project").expect("repository");

    let error = provider
        .repository(&repository)
        .await
        .expect_err("missing credentials");
    assert!(matches!(error, ProviderError::MissingCredential { .. }));

    let spans = recorded.lock().expect("span recorder");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].name, "interprex.provider.code_hosting.repository");
    assert_eq!(
        spans[0].fields,
        BTreeMap::from([("interprex.provider.name".to_owned(), "github".to_owned())])
    );
}
