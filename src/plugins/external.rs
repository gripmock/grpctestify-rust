use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use super::{Plugin, PluginContext, PluginResult};
use crate::assert::engine::AssertionResult;

#[derive(Debug, Serialize)]
struct ExternalPluginRequest<'a> {
    version: String,
    context: RequestContext<'a>,
    call: PluginCall<'a>,
}

#[derive(Debug, Serialize)]
struct RequestContext<'a> {
    response: &'a Value,
    headers: Option<&'a HashMap<String, String>>,
    trailers: Option<&'a HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
struct PluginCall<'a> {
    function: String,
    arguments: &'a [Value],
}

#[derive(Debug, Serialize, Deserialize)]
struct ExternalPluginResponse {
    result: Value,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub functions: Vec<PluginFunction>,
    pub executable: String,
    pub language: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginFunction {
    pub name: String,
    pub description: String,
    pub min_args: usize,
    pub max_args: usize,
}

pub struct ExternalPlugin {
    pub name: String,
    pub description: String,
    pub executable_path: PathBuf,
    // Store manifest functions if needed later
}

impl ExternalPlugin {
    pub fn new(name: String, executable_path: PathBuf, description: String) -> Self {
        Self {
            name,
            description,
            executable_path,
        }
    }
}

impl Plugin for ExternalPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        let request = ExternalPluginRequest {
            version: "1.0.0".to_string(),
            context: RequestContext {
                response: context.response,
                headers: context.headers,
                trailers: context.trailers,
            },
            call: PluginCall {
                function: self.name.clone(),
                arguments: args,
            },
        };

        let request_json =
            serde_json::to_string(&request).context("Failed to serialize plugin request")?;

        let mut child = Command::new(&self.executable_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| {
                format!(
                    "Failed to spawn plugin executable: {}",
                    self.executable_path.display()
                )
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(request_json.as_bytes())
                .context("Failed to write to plugin stdin")?;
        }

        let output = child
            .wait_with_output()
            .context("Failed to wait for plugin execution")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Plugin execution failed with status: {}",
                output.status
            ));
        }

        let response: ExternalPluginResponse = serde_json::from_slice(&output.stdout)
            .with_context(|| {
                format!(
                    "Failed to parse plugin response: {}",
                    String::from_utf8_lossy(&output.stdout)
                )
            })?;

        if let Some(error) = response.error {
            return Err(anyhow::anyhow!("Plugin error: {}", error));
        }

        // Determine if result is assertion or value
        // Heuristic: if boolean, it's assertion pass/fail?
        // Or specific structure?
        // Let's assume boolean true = pass, false = fail (generic).
        // Or if it returns an object { "pass": true, ... }

        // For now, treat boolean as assertion result, others as value.
        match response.result {
            Value::Bool(b) => {
                if b {
                    Ok(PluginResult::Assertion(AssertionResult::Pass))
                } else {
                    Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                        "Plugin {} returned false",
                        self.name
                    ))))
                }
            }
            val => Ok(PluginResult::Value(val)),
        }
    }
}
