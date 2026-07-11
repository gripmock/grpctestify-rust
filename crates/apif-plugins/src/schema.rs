use crate::core::{Plugin, PluginContext, PluginResult, PluginSignature};
use crate::type_info::{ArgTypeInfo, TypeInfo};
use anyhow::Result;
use serde_json::Value;

pub struct SchemaPlugin;

impl Plugin for SchemaPlugin {
    fn name(&self) -> &str {
        "schema"
    }

    fn description(&self) -> &str {
        "Validates JSON value against a JSON Schema. Usage: @schema(instance, schema) or @schema(schema) for full response"
    }

    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        let (instance, schema) = match args.len() {
            1 => (context.response, &args[0]),
            2 => (&args[0], &args[1]),
            _ => {
                return Ok(PluginResult::Value(Value::Bool(false)));
            }
        };

        let validator = match jsonschema::validator_for(schema) {
            Ok(v) => v,
            Err(_) => {
                return Ok(PluginResult::Value(Value::Bool(false)));
            }
        };

        let valid = validator.is_valid(instance);
        Ok(PluginResult::Value(Value::Bool(valid)))
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::Bool,
            arg_types: &[
                ArgTypeInfo {
                    expected: TypeInfo::Json,
                    required: false,
                    default: None,
                },
                ArgTypeInfo {
                    expected: TypeInfo::Json,
                    required: true,
                    default: None,
                },
            ],
            purity: crate::PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
            arg_names: &["instance", "schema"],
            replacement: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_ctx(response: &Value) -> PluginContext<'_> {
        PluginContext::new(response)
    }

    #[test]
    fn test_schema_valid() {
        let instance = json!({"name": "Alice", "age": 30});
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name"]
        });
        let null = Value::Null;
        assert_eq!(
            SchemaPlugin
                .execute(&[instance, schema], &make_ctx(&null))
                .unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_schema_invalid() {
        let instance = json!({"name": 42});
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            }
        });
        let null = Value::Null;
        assert_eq!(
            SchemaPlugin
                .execute(&[instance, schema], &make_ctx(&null))
                .unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_schema_one_arg_uses_response() {
        let schema = json!({"type": "object"});
        let response = json!({"key": "val"});
        assert_eq!(
            SchemaPlugin
                .execute(&[schema], &make_ctx(&response))
                .unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_schema_no_args() {
        let null = Value::Null;
        assert_eq!(
            SchemaPlugin.execute(&[], &make_ctx(&null)).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_schema_string_valid() {
        let schema = json!({"type": "string", "minLength": 3});
        let null = Value::Null;
        assert_eq!(
            SchemaPlugin
                .execute(&[json!("hello"), schema], &make_ctx(&null))
                .unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_schema_string_too_short() {
        let schema = json!({"type": "string", "minLength": 3});
        let null = Value::Null;
        assert_eq!(
            SchemaPlugin
                .execute(&[json!("ab"), schema], &make_ctx(&null))
                .unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_schema_name() {
        assert_eq!(SchemaPlugin.name(), "schema");
    }

    #[test]
    fn test_schema_signature() {
        let sig = SchemaPlugin.signature();
        assert_eq!(sig.return_type, TypeInfo::Bool);
    }
}
