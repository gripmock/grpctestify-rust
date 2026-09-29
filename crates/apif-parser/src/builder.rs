use serde_json::Value;

use apif_ast::{
    DocumentMetadata, FileMeta, GctfDocument, InlineOptions, OrderedStringMap, Section,
    SectionContent, SectionSpan, SectionType,
};

#[derive(Debug, Clone)]
pub struct GctfDocumentBuilder {
    file_path: String,
    sections: Vec<Section>,
    next: Option<Box<GctfDocumentBuilder>>,
}

impl GctfDocumentBuilder {
    pub fn new() -> Self {
        Self {
            file_path: String::new(),
            sections: Vec::new(),
            next: None,
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

    pub fn endpoint_parallel(mut self, endpoint: impl Into<String>, parallel: bool) -> Self {
        self.push_section(
            SectionType::Endpoint,
            SectionContent::Single(endpoint.into()),
        );
        if parallel && let Some(section) = self.sections.last_mut() {
            section.inline_options.parallel = true;
        }
        self
    }

    pub fn address(mut self, address: impl Into<String>) -> Self {
        self.push_section(SectionType::Address, SectionContent::Single(address.into()));
        self
    }

    pub fn request_headers(mut self, headers: impl IntoIterator<Item = (String, String)>) -> Self {
        let headers: OrderedStringMap = headers.into_iter().collect();
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

    pub fn request_text(mut self, request: impl Into<String>) -> Self {
        self.push_section(SectionType::Request, SectionContent::Single(request.into()));
        self
    }

    pub fn request_stream(mut self, requests: Vec<Value>) -> Self {
        self.push_section(SectionType::Request, SectionContent::JsonLines(requests));
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

    pub fn expectation(
        mut self,
        section_type: SectionType,
        content: SectionContent,
        inline_options: InlineOptions,
    ) -> Self {
        self.push_section(section_type, content);
        if let Some(last) = self.sections.last_mut() {
            last.inline_options = inline_options;
        }
        self
    }

    pub fn tls(mut self, tls: impl IntoIterator<Item = (String, String)>) -> Self {
        let tls: OrderedStringMap = tls.into_iter().collect();
        if !tls.is_empty() {
            self.push_section(SectionType::Tls, SectionContent::KeyValues(tls));
        }
        self
    }

    pub fn options(mut self, options: impl IntoIterator<Item = (String, String)>) -> Self {
        let options: OrderedStringMap = options.into_iter().collect();
        if !options.is_empty() {
            self.push_section(SectionType::Options, SectionContent::KeyValues(options));
        }
        self
    }

    pub fn proto(mut self, proto: impl IntoIterator<Item = (String, String)>) -> Self {
        let proto: OrderedStringMap = proto.into_iter().collect();
        if !proto.is_empty() {
            self.push_section(SectionType::Proto, SectionContent::KeyValues(proto));
        }
        self
    }

    pub fn asserts(mut self, asserts: impl IntoIterator<Item = String>) -> Self {
        let asserts: Vec<String> = asserts
            .into_iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect();
        if !asserts.is_empty() {
            self.push_section(SectionType::Asserts, SectionContent::Assertions(asserts));
        }
        self
    }

    pub fn extract(mut self, extract: impl IntoIterator<Item = (String, String)>) -> Self {
        let extract: OrderedStringMap = extract.into_iter().collect();
        if !extract.is_empty() {
            self.push_section(SectionType::Extract, SectionContent::Extract(extract));
        }
        self
    }

    pub fn bench(mut self, bench: impl IntoIterator<Item = (String, String)>) -> Self {
        let bench: OrderedStringMap = bench.into_iter().collect();
        if !bench.is_empty() {
            self.push_section(SectionType::Bench, SectionContent::KeyValues(bench));
        }
        self
    }

    pub fn dataset(mut self, rows: impl IntoIterator<Item = Value>) -> Self {
        let rows: Vec<Value> = rows.into_iter().collect();
        if !rows.is_empty() {
            self.push_section(SectionType::Dataset, SectionContent::Rows(rows));
        }
        self
    }

    pub fn meta(mut self, meta: FileMeta) -> Self {
        if !meta.is_empty() {
            self.push_section(SectionType::Meta, SectionContent::Meta(meta));
        }
        self
    }

    pub fn then(mut self, next: GctfDocumentBuilder) -> Self {
        match self.next.as_mut() {
            Some(tail) => {
                let tail = std::mem::take(tail.as_mut());
                self.next = Some(Box::new(tail.then(next)));
            }
            None => self.next = Some(Box::new(next)),
        }
        self
    }

    pub fn build(self) -> GctfDocument {
        let file_path = self.file_path;
        GctfDocument {
            file_path: file_path.clone(),
            sections: self.sections,
            metadata: DocumentMetadata {
                placeholder_free: false,
                source: None,
                mtime: None,
                parsed_at: apif_cfg_runtime::now_timestamp(),
            },
            next_document: self
                .next
                .map(|n| Box::new(n.with_file_path(file_path).build())),
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
            span: SectionSpan::default(),
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
    fn builder_renders_asserts_and_extract() {
        let mut extract = OrderedStringMap::new();
        extract.insert("token".to_string(), ".auth.token".to_string());

        let output = GctfDocumentBuilder::new()
            .address("localhost:4770")
            .endpoint("auth.v1.AuthService/Login")
            .request(json!({"email": "a@b.io"}))
            .extract(extract)
            .asserts(vec![
                ".token != \"\"".to_string(),
                "  ".to_string(),
                ".expires_in == 3600".to_string(),
            ])
            .render();

        assert!(output.contains("--- EXTRACT ---"));
        assert!(output.contains("token = .auth.token"));
        assert!(output.contains("--- ASSERTS ---"));
        assert!(output.contains(".token != \"\""));
        assert!(output.contains(".expires_in == 3600"));
        assert!(!output.contains("\n  \n"));
    }

    #[test]
    fn builder_round_trips_through_the_parser() {
        let output = GctfDocumentBuilder::new()
            .address("localhost:4770")
            .endpoint("auth.v1.AuthService/Login")
            .options([("protocol".to_string(), "grpc-web".to_string())])
            .request(json!({"email": "a@b.io"}))
            .asserts(vec![".token != \"\"".to_string()])
            .render();

        let doc = crate::core::parse_gctf_from_str(&output, "round-trip.gctf")
            .expect("builder output must parse");
        assert_eq!(
            doc.get_options().and_then(|o| o.get("protocol").cloned()),
            Some("grpc-web".to_string())
        );
        assert!(
            doc.sections
                .iter()
                .any(|s| s.section_type == SectionType::Asserts)
        );
    }

    #[test]
    fn builder_skips_empty_maps() {
        let output = GctfDocumentBuilder::new()
            .address("localhost:4770")
            .endpoint("svc/method")
            .request_headers(OrderedStringMap::new())
            .options(OrderedStringMap::new())
            .proto(OrderedStringMap::new())
            .request(json!({}))
            .render();

        assert!(!output.contains("REQUEST_HEADERS"));
        assert!(!output.contains("OPTIONS"));
        assert!(!output.contains("PROTO"));
    }
    #[test]
    fn builder_chains_documents_and_round_trips() {
        let mut extract = OrderedStringMap::new();
        extract.insert("token".to_string(), ".auth.token".to_string());

        let output = GctfDocumentBuilder::new()
            .address("localhost:4770")
            .endpoint("auth.v1.AuthService/Login")
            .request(json!({"email": "a@b.io"}))
            .extract(extract)
            .then(
                GctfDocumentBuilder::new()
                    .endpoint("feed.v1.FeedService/List")
                    .request(json!({"token": "{{token}}"}))
                    .asserts(vec![".items | length > 0".to_string()]),
            )
            .then(
                GctfDocumentBuilder::new()
                    .endpoint("feed.v1.FeedService/Close")
                    .request(json!({})),
            )
            .render();

        let doc = crate::parse_gctf_from_str(&output, "chain.gctf").expect("parses");
        let endpoints: Vec<String> = doc
            .iter_chain()
            .map(|d| d.get_endpoint().unwrap_or_default())
            .collect();
        assert_eq!(
            endpoints,
            vec![
                "auth.v1.AuthService/Login",
                "feed.v1.FeedService/List",
                "feed.v1.FeedService/Close",
            ]
        );

        let again = crate::serialize_gctf(&doc);
        let reparsed = crate::parse_gctf_from_str(&again, "chain.gctf").expect("reparses");
        assert_eq!(reparsed.iter_chain().count(), 3);
    }

    #[test]
    fn extract_survives_a_write_read_cycle() {
        let mut extract = OrderedStringMap::new();
        extract.insert("zulu".to_string(), ".z".to_string());
        extract.insert("alpha".to_string(), ".a".to_string());

        let output = GctfDocumentBuilder::new()
            .endpoint("pkg.Svc/M")
            .request(json!({}))
            .extract(extract)
            .render();

        assert!(output.contains("zulu = .z"));

        let doc = crate::parse_gctf_from_str(&output, "x.gctf").expect("parses");
        let vars = doc
            .sections
            .iter()
            .find_map(|s| match &s.content {
                apif_ast::SectionContent::Extract(v) => Some(v.clone()),
                _ => None,
            })
            .expect("EXTRACT section survives");
        let names: Vec<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, vec!["zulu", "alpha"], "author order, not sorted");
    }

    #[test]
    fn meta_is_hoisted_to_the_front() {
        let meta = FileMeta {
            name: Some("login works".to_string()),
            tags: vec!["smoke".to_string()],
            ..Default::default()
        };

        let output = GctfDocumentBuilder::new()
            .endpoint("pkg.Svc/M")
            .request(json!({}))
            .asserts(vec![".ok == true".to_string()])
            .meta(meta)
            .render();

        let first = output
            .lines()
            .find(|l| l.starts_with("--- "))
            .unwrap_or_default();
        assert_eq!(first, "--- META ---");
        assert!(output.find("--- ENDPOINT ---") < output.find("--- REQUEST ---"));
    }

    #[test]
    fn bench_and_dataset_round_trip() {
        let mut bench = OrderedStringMap::new();
        bench.insert("mode".to_string(), "fixed".to_string());
        bench.insert("concurrency".to_string(), "50".to_string());
        bench.insert("duration".to_string(), "60s".to_string());

        let output = GctfDocumentBuilder::new()
            .endpoint("pkg.Svc/M")
            .request(json!({}))
            .asserts(vec![".ok == true".to_string()])
            .bench(bench)
            .dataset(vec![
                json!({"id": "1", "name": "Ada"}),
                json!({"id": "2", "name": "Grace"}),
            ])
            .render();

        let doc = crate::parse_gctf_from_str(&output, "b.gctf").expect("parses");

        let rows = doc
            .sections
            .iter()
            .find_map(|s| match &s.content {
                apif_ast::SectionContent::Rows(r) => Some(r.clone()),
                _ => None,
            })
            .expect("DATASET survives");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], json!("Ada"));

        let keys = doc
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Bench)
            .and_then(|s| match &s.content {
                apif_ast::SectionContent::KeyValues(kv) => Some(kv.clone()),
                _ => None,
            })
            .expect("BENCH survives");
        assert_eq!(keys.get("concurrency").map(String::as_str), Some("50"));
        assert_eq!(keys.get("duration").map(String::as_str), Some("60s"));
    }

    #[test]
    fn an_empty_bench_or_dataset_writes_no_section() {
        let output = GctfDocumentBuilder::new()
            .endpoint("pkg.Svc/M")
            .request(json!({}))
            .bench(Vec::<(String, String)>::new())
            .dataset(Vec::<serde_json::Value>::new())
            .render();
        assert!(!output.contains("--- BENCH ---"));
        assert!(!output.contains("--- DATASET ---"));
    }
}
