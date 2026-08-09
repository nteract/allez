use std::{
    collections::BTreeSet,
    env,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use allez::ephemeral::{PackageRequest, PackageSpec, ReadyEnvironment};
use condarc::{ChannelPriority, ResolvedChannels};
use rattler_conda_types::Channel;
use tracing::{Event, Subscriber, field::Visit};
use tracing_subscriber::{Layer, layer::Context, prelude::*, registry::LookupSpan};

static EVENT_CAPTURE: OnceLock<EventCapture> = OnceLock::new();

pub(crate) struct TestContext {
    _temporary_directory: tempfile::TempDir,
    root: PathBuf,
}

impl TestContext {
    pub(crate) fn new(name: &str) -> Self {
        let temporary_directory = tempfile::tempdir().unwrap();
        let root = temporary_directory.path().join(name);
        let ambient_keys = env::vars_os()
            .map(|(key, _)| key)
            .filter(|key| {
                let key = key.to_string_lossy();
                key.starts_with("CONDA") || matches!(key.as_ref(), "HTTP_PROXY" | "HTTPS_PROXY")
            })
            .collect::<Vec<_>>();

        // SAFETY: Category 13, library contract. Every caller is a current-thread
        // test holding serial_test's process-wide lock, so no test can access the
        // process environment while these mutations run.
        unsafe {
            for key in ambient_keys {
                env::remove_var(key);
            }
            env::set_var("ALLEZ_EPHEMERAL_ROOT", &root);
        }

        Self {
            _temporary_directory: temporary_directory,
            root,
        }
    }

    pub(crate) fn environment_location(&self, id: impl ToString) -> PathBuf {
        self.root.join("envs").join(id.to_string())
    }
}

pub(crate) fn package_specs(packages: &[&str]) -> Vec<PackageSpec> {
    packages
        .iter()
        .map(|package| PackageSpec::parse(package).unwrap())
        .collect()
}

/// A request exercising no configured default package set at all.
pub(crate) fn explicit_only(packages: &[&str]) -> PackageRequest {
    PackageRequest {
        explicit: package_specs(packages),
        defaults: Vec::new(),
    }
}

/// The name of every package a `create_ephemeral_environment` call installed.
pub(crate) fn installed_names(ready: &ReadyEnvironment) -> BTreeSet<&str> {
    ready
        .installed_packages
        .iter()
        .map(|package| package.name.as_str())
        .collect()
}

pub(crate) fn fixture_channel(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ephemeral_channel")
        .join(relative);
    Channel::try_from_directory(&path).unwrap().canonical_name()
}

pub(crate) fn root_fixture_config(priority: ChannelPriority) -> ResolvedChannels {
    let mut config = ResolvedChannels::from_channels(vec![fixture_channel("")]);
    config.channel_priority = priority;
    config
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CapturedLifecycleEvent {
    pub(crate) environment_id: Option<String>,
    pub(crate) operation: Option<String>,
    pub(crate) packages: Option<String>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) outcome: Option<String>,
    pub(crate) failure_category: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct EventCapture(Arc<Mutex<CaptureState>>);

#[derive(Default)]
struct CaptureState {
    events: Vec<CapturedLifecycleEvent>,
}

impl EventCapture {
    pub(crate) fn install() -> Self {
        let capture = EVENT_CAPTURE
            .get_or_init(|| {
                let capture = Self::default();
                tracing::subscriber::set_global_default(
                    tracing_subscriber::registry().with(capture.clone()),
                )
                .unwrap();
                capture
            })
            .clone();
        let mut state = capture.0.lock().unwrap();
        state.events.clear();
        drop(state);
        capture
    }

    pub(crate) fn events(&self) -> Vec<CapturedLifecycleEvent> {
        self.0.lock().unwrap().events.clone()
    }
}

impl<S> Layer<S> for EventCapture
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut visitor = LifecycleEventVisitor::default();
        event.record(&mut visitor);
        if visitor.event.operation.is_some() {
            self.0.lock().unwrap().events.push(visitor.event);
        }
    }
}

#[derive(Default)]
struct LifecycleEventVisitor {
    event: CapturedLifecycleEvent,
}

impl Visit for LifecycleEventVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let value = format!("{value:?}");
        match field.name() {
            "environment_id" => self.event.environment_id = Some(value),
            "packages" => self.event.packages = Some(value),
            "failure_category" => {
                self.event.failure_category = value
                    .strip_prefix("Some(\"")
                    .and_then(|category| category.strip_suffix("\")"))
                    .map(str::to_string);
            }
            _ => {}
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "operation" => self.event.operation = Some(value.to_string()),
            "outcome" => self.event.outcome = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "duration_ms" {
            self.event.duration_ms = Some(value);
        }
    }
}
