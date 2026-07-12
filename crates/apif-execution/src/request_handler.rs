use apif_ast::SectionContent;
use serde_json::Value;

#[derive(Default)]
pub struct RequestHandler;

impl RequestHandler {
    pub fn new() -> Self {
        Self
    }
    pub fn build_request(&self, content: &SectionContent) -> Option<Value> {
        match content {
            SectionContent::Json(v) => Some(v.clone()),
            SectionContent::Empty => Some(Value::Object(serde_json::Map::new())),
            _ => None,
        }
    }
}
