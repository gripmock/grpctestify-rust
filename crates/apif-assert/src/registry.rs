use crate::engine::AssertionResult;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct PluginContext<'a> {
    pub response: &'a Value,
    pub headers: Option<&'a HashMap<String, String>>,
    pub trailers: Option<&'a HashMap<String, String>>,
    pub timing: Option<&'a AssertionTiming>,
    pub protocol: Option<&'a str>,
}

impl<'a> PluginContext<'a> {
    pub fn new(response: &'a Value) -> Self {
        Self {
            response,
            headers: None,
            trailers: None,
            timing: None,
            protocol: None,
        }
    }
    pub fn with_headers(mut self, headers: Option<&'a HashMap<String, String>>) -> Self {
        self.headers = headers;
        self
    }
    pub fn with_trailers(mut self, trailers: Option<&'a HashMap<String, String>>) -> Self {
        self.trailers = trailers;
        self
    }
    pub fn with_timing(mut self, timing: Option<&'a AssertionTiming>) -> Self {
        self.timing = timing;
        self
    }
    pub fn with_protocol(mut self, protocol: Option<&'a str>) -> Self {
        self.protocol = protocol;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionTiming {
    pub elapsed_ms: u64,
    pub total_elapsed_ms: u64,
    pub scope_message_count: usize,
    pub scope_index: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginResult {
    Assertion(AssertionResult),
    Value(Value),
}

pub trait PluginApi: Send + Sync {
    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult>;
}

pub trait PluginRegistry: Send + Sync {
    fn get_plugin(&self, name: &str) -> Option<Arc<dyn PluginApi>>;
}

pub struct NoopPluginRegistry;

impl PluginRegistry for NoopPluginRegistry {
    fn get_plugin(&self, _name: &str) -> Option<Arc<dyn PluginApi>> {
        None
    }
}
