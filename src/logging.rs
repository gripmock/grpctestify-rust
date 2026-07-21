use chrono::Local;
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

pub struct CustomFormatter;

impl<S, N> FormatEvent<S, N> for CustomFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let metadata = event.metadata();
        let level = metadata.level();
        let timestamp = Local::now().format("%H:%M:%S");

        // Legacy format mappings
        // debug) formatted_message="🐛 DEBUG [$timestamp]: $message"
        // trace) formatted_message="🔬 TRACE [$timestamp]: $message"
        // info)  formatted_message="ℹ️  INFO [$timestamp]: $message"
        // warn)  formatted_message="⚠️  WARN [$timestamp]: $message"
        // error) formatted_message="❌ ERROR [$timestamp]: $message"

        let (emoji, level_str) = match *level {
            tracing::Level::TRACE => ("🔬", "TRACE"),
            tracing::Level::DEBUG => ("🐛", "DEBUG"),
            tracing::Level::INFO => ("ℹ️ ", "INFO"),
            tracing::Level::WARN => ("⚠️ ", "WARN"),
            tracing::Level::ERROR => ("❌", "ERROR"),
        };

        write!(writer, "{} {} [{}]: ", emoji, level_str, timestamp)?;

        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}
