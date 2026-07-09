// Builder utilities for constructing .gctf documents programmatically.

use std::collections::HashMap;

use serde_json::Value;

use apif_ast::{
    DocumentMetadata, FileMeta, GctfDocument, InlineOptions, Section, SectionContent, SectionType,
};

#[derive(Debug, Clone)]
pub struct GctfDocumentBuilder {
    file_path: String,
    sections: Vec<Section>,
}

impl GctfDocumentBuilder {
    pub fn new() -> Self {
        Self {
            file_path: String::new(),
            sections: Vec::new(),
        }
    }

    pub fn with_file_path(mut self, file_path: impl Into<String>) -> Self {
        self.file_path = file_path.into();
        self
    }

    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.push_section(
            SectionType::Endpoint,
            SectionContent::Single(endpoint.into()),
        );
        self
    }

    pub fn address(mut self, address: impl Into<String>) -> Self {
        self.push_section(SectionType::Address, SectionContent::Single(address.into()));
        self
    }

    pub fn request_headers(mut self, headers: HashMap<String, String>) -> Self {
        if !headers.is_empty() {
            self.push_section(
                SectionType::RequestHeaders,
                SectionContent::KeyValues(headers),
            );
        }
        self
    }

    pub fn request(mut self, request: Value) -> Self {
        self.push_section(SectionType::Request, SectionContent::Json(request));
        self
    }

    pub fn response(mut self, response: Value) -> Self {
        self.push_section(SectionType::Response, SectionContent::Json(response));
        self
    }

    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.push_section(SectionType::Error, SectionContent::Single(error.into()));
        self
    }

    pub fn tls(mut self, tls: HashMap<String, String>) -> Self {
        if !tls.is_empty() {
            self.push_section(SectionType::Tls, SectionContent::KeyValues(tls));
        }
        self
    }

    pub fn options(mut self, options: HashMap<String, String>) -> Self {
        if !options.is_empty() {
            self.push_section(SectionType::Options, SectionContent::KeyValues(options));
        }
        self
    }

    pub fn proto(mut self, proto: HashMap<String, String>) -> Self {
        if !proto.is_empty() {
            self.push_section(SectionType::Proto, SectionContent::KeyValues(proto));
        }
        self
    }

    pub fn meta(mut self, meta: FileMeta) -> Self {
        if !meta.is_empty() {
            self.push_section(SectionType::Meta, SectionContent::Meta(meta));
        }
        self
    }

    pub fn build(self) -> GctfDocument {
        GctfDocument {
            file_path: self.file_path,
            sections: self.sections,
            metadata: DocumentMetadata {
                source: None,
                mtime: None,
                parsed_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                ..Default::default()
            },
            next_document: None,
        }
    }

    pub fn render(self) -> String {
        let doc = self.build();
        crate::core::serialize_gctf(&doc)
    }

    fn push_section(&mut self, section_type: SectionType, content: SectionContent) {
        self.sections.push(Section {
            section_type,
            content,
            inline_options: InlineOptions::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: Vec::new(),
        });
    }
}

impl Default for GctfDocumentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builder_renders_minimal_document() {
        let output = GctfDocumentBuilder::new()
            .address("localhost:4770")
            .endpoint("auth.AuthService/CheckAccess")
            .request(json!({"action": "delete"}))
            .render();

        assert!(output.contains("--- ADDRESS ---\nlocalhost:4770"));
        assert!(output.contains("--- ENDPOINT ---\nauth.AuthService/CheckAccess"));
        assert!(output.contains("--- REQUEST ---"));
    }

    #[test]
    fn builder_skips_empty_maps() {
        let output = GctfDocumentBuilder::new()
            .address("localhost:4770")
            .endpoint("svc/method")
            .request_headers(HashMap::new())
            .options(HashMap::new())
            .proto(HashMap::new())
            .request(json!({}))
            .render();

        assert!(!output.contains("REQUEST_HEADERS"));
        assert!(!output.contains("OPTIONS"));
        assert!(!output.contains("PROTO"));
    }
}
