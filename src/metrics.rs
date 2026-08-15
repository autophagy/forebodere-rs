use opentelemetry::metrics::{Counter, Histogram, Meter};
use opentelemetry::KeyValue;
use opentelemetry_otlp::MetricExporter;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use std::fmt;

pub struct Metrics {
    pub commands: Counter<u64>,
    pub command_duration: Histogram<f64>,
}

pub enum CommandDimension {
    Quote,
    QuoteAll,
    GenQuote,
    AddQuote,
    MarkovQuote,
    Status,
    Slap,
    Help,
}

impl fmt::Display for CommandDimension {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CommandDimension::Quote => write!(f, "quote"),
            CommandDimension::QuoteAll => write!(f, "quoteall"),
            CommandDimension::GenQuote => write!(f, "gen"),
            CommandDimension::AddQuote => write!(f, "addquote"),
            CommandDimension::MarkovQuote => write!(f, "markov"),
            CommandDimension::Status => write!(f, "status"),
            CommandDimension::Slap => write!(f, "slap"),
            CommandDimension::Help => write!(f, "help"),
        }
    }
}

pub struct CommandTimer<'a> {
    metrics: &'a Metrics,
    command: CommandDimension,
    start: std::time::Instant,
}

impl<'a> CommandTimer<'a> {
    pub fn start(metrics: &'a Metrics, command: CommandDimension) -> Self {
        increment_command(&metrics.commands, &command);
        Self {
            metrics,
            command,
            start: std::time::Instant::now(),
        }
    }
}

impl Drop for CommandTimer<'_> {
    fn drop(&mut self) {
        add_command_duration(
            &self.metrics.command_duration,
            self.start.elapsed().as_secs_f64(),
            &self.command,
        );
    }
}

pub fn init_meter_provider(exporter: MetricExporter) -> SdkMeterProvider {
    SdkMeterProvider::builder()
        .with_periodic_exporter(exporter)
        .build()
}

pub fn init_metrics(meter: Meter) -> Metrics {
    let commands = meter
        .u64_counter("forebodere.commands")
        .with_description("The number of bot commands invoked")
        .build();

    let command_duration = meter
        .f64_histogram("forebodere.command.duration")
        .with_unit("s")
        .with_boundaries(vec![
            0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
        ])
        .with_description("The durations that particular commands took")
        .build();

    Metrics {
        commands,
        command_duration,
    }
}

pub fn increment_command(counter: &Counter<u64>, command: &CommandDimension) {
    counter.add(1, &[KeyValue::new("command", command.to_string())]);
}

pub fn add_command_duration(histogram: &Histogram<f64>, duration: f64, command: &CommandDimension) {
    histogram.record(duration, &[KeyValue::new("command", command.to_string())]);
}
