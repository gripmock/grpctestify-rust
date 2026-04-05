use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::plugins::{
    Plugin, PluginContext, PluginPurity, PluginResult, PluginReturnKind, PluginSignature,
};

pub struct ElapsedMsPlugin;

impl Plugin for ElapsedMsPlugin {
    fn name(&self) -> &str {
        "elapsed_ms"
    }

    fn description(&self) -> &str {
        "Returns elapsed duration in milliseconds for the current assertion scope"
    }

    fn execute(&self, _args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        let timing = context
            .timing
            .ok_or_else(|| anyhow!("timing context is unavailable for @elapsed_ms()"))?;

        Ok(PluginResult::Value(Value::from(timing.elapsed_ms)))
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_kind: PluginReturnKind::Number,
            purity: PluginPurity::ContextDependent,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
            arg_names: &[],
        }
    }
}

pub struct TotalElapsedMsPlugin;

impl Plugin for TotalElapsedMsPlugin {
    fn name(&self) -> &str {
        "total_elapsed_ms"
    }

    fn description(&self) -> &str {
        "Returns cumulative elapsed duration in milliseconds across assertion scopes"
    }

    fn execute(&self, _args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        let timing = context
            .timing
            .ok_or_else(|| anyhow!("timing context is unavailable for @total_elapsed_ms()"))?;

        Ok(PluginResult::Value(Value::from(timing.total_elapsed_ms)))
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_kind: PluginReturnKind::Number,
            purity: PluginPurity::ContextDependent,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
            arg_names: &[],
        }
    }
}

pub struct ScopeMessageCountPlugin;

impl Plugin for ScopeMessageCountPlugin {
    fn name(&self) -> &str {
        "scope_message_count"
    }

    fn description(&self) -> &str {
        "Returns number of response messages in the current assertion scope"
    }

    fn execute(&self, _args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        let timing = context
            .timing
            .ok_or_else(|| anyhow!("timing context is unavailable for @scope_message_count()"))?;

        Ok(PluginResult::Value(Value::from(timing.scope_message_count)))
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_kind: PluginReturnKind::Number,
            purity: PluginPurity::ContextDependent,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
            arg_names: &[],
        }
    }
}

pub struct ScopeIndexPlugin;

impl Plugin for ScopeIndexPlugin {
    fn name(&self) -> &str {
        "scope_index"
    }

    fn description(&self) -> &str {
        "Returns current assertion scope index (1-based)"
    }

    fn execute(&self, _args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        let timing = context
            .timing
            .ok_or_else(|| anyhow!("timing context is unavailable for @scope_index()"))?;

        Ok(PluginResult::Value(Value::from(timing.scope_index)))
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_kind: PluginReturnKind::Number,
            purity: PluginPurity::ContextDependent,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
            arg_names: &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::AssertionTiming;

    fn context_with_timing(timing: AssertionTiming) -> PluginContext<'static> {
        let timing_ref = Box::leak(Box::new(timing));
        PluginContext::new(&Value::Null).with_timing(Some(timing_ref))
    }

    #[test]
    fn test_elapsed_ms_plugin_returns_value() {
        let plugin = ElapsedMsPlugin;
        let context = context_with_timing(AssertionTiming {
            elapsed_ms: 15,
            total_elapsed_ms: 23,
            scope_message_count: 2,
            scope_index: 3,
        });

        let result = plugin.execute(&[], &context).unwrap();
        assert!(matches!(result, PluginResult::Value(Value::Number(_))));
    }

    #[test]
    fn test_total_elapsed_ms_plugin_returns_value() {
        let plugin = TotalElapsedMsPlugin;
        let context = context_with_timing(AssertionTiming {
            elapsed_ms: 10,
            total_elapsed_ms: 15,
            scope_message_count: 1,
            scope_index: 2,
        });

        let result = plugin.execute(&[], &context).unwrap();
        assert!(matches!(result, PluginResult::Value(Value::Number(_))));
    }

    #[test]
    fn test_plugins_fail_without_timing_context() {
        let plugin = ElapsedMsPlugin;
        let context = PluginContext::new(&Value::Null);

        let result = plugin.execute(&[], &context);
        assert!(result.is_err());
    }
}
