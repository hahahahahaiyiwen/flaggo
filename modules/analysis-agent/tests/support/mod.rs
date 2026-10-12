use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use serde_json::Value;
use tracing::{
    Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};
use tracing_subscriber::{
    Layer,
    layer::{Context, SubscriberExt},
    registry::LookupSpan,
};

#[derive(Clone, Debug)]
pub struct Observation {
    pub name: String,
    pub target: String,
    pub level: String,
    pub fields: BTreeMap<String, Value>,
    pub parent: Option<u64>,
    pub closed: bool,
}

#[derive(Clone, Default)]
pub struct Capture {
    pub events: Arc<Mutex<Vec<Observation>>>,
    pub spans: Arc<Mutex<BTreeMap<u64, Observation>>>,
}

impl Capture {
    pub async fn run<F: std::future::Future>(&self, future: F) -> F::Output {
        use tracing::instrument::WithSubscriber as _;
        future
            .with_subscriber(tracing_subscriber::registry().with(self.clone()))
            .await
    }
}

struct Fields<'a>(&'a mut BTreeMap<String, Value>);

impl Visit for Fields<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0
            .insert(field.name().to_owned(), Value::String(value.to_owned()));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_owned(), value.into());
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().to_owned(), value.into());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().to_owned(), format!("{value:?}").into());
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        attrs.record(&mut Fields(&mut fields));
        let parent = attrs.parent().cloned().or_else(|| {
            if attrs.is_contextual() {
                ctx.current_span().id().cloned()
            } else {
                None
            }
        });
        self.spans.lock().unwrap().insert(
            id.into_u64(),
            Observation {
                name: attrs.metadata().name().to_owned(),
                target: attrs.metadata().target().to_owned(),
                level: attrs.metadata().level().to_string(),
                fields,
                parent: parent.map(|parent| parent.into_u64()),
                closed: false,
            },
        );
    }
    fn on_record(&self, id: &Id, values: &Record<'_>, _ctx: Context<'_, S>) {
        values.record(&mut Fields(
            &mut self
                .spans
                .lock()
                .unwrap()
                .get_mut(&id.into_u64())
                .unwrap()
                .fields,
        ));
    }
    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        event.record(&mut Fields(&mut fields));
        self.events.lock().unwrap().push(Observation {
            name: event.metadata().name().to_owned(),
            target: event.metadata().target().to_owned(),
            level: event.metadata().level().to_string(),
            fields,
            parent: ctx.event_span(event).map(|span| span.id().into_u64()),
            closed: false,
        });
    }
    fn on_close(&self, id: Id, _ctx: Context<'_, S>) {
        self.spans
            .lock()
            .unwrap()
            .get_mut(&id.into_u64())
            .unwrap()
            .closed = true;
    }
}
