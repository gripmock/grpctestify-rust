pub fn generate_multi_doc_content(num_docs: usize) -> String {
    let mut out = String::new();
    for i in 0..num_docs {
        if i > 0 {
            out.push_str("\n--- ENDPOINT ---\n");
        } else {
            out.push_str("--- ENDPOINT ---\n");
        }
        out.push_str(&format!("svc.Method{}\n", i));
        out.push_str("\n--- REQUEST ---\n");
        out.push_str(&format!("{{\"id\": {}, \"name\": \"doc{}\"}}\n", i, i));
        out.push_str("\n--- RESPONSE ---\n");
        out.push_str(&format!("{{\"status\": \"ok\", \"doc\": {}}}\n", i));
        if i % 3 == 0 {
            out.push_str("\n--- EXTRACT ---\n");
            out.push_str(&format!("var_{} = .status\n", i));
        }
    }
    out
}

#[allow(dead_code)]
pub fn single_doc_content() -> &'static str {
    r#"--- ENDPOINT ---
svc.Method
--- REQUEST ---
{"id": 1}
--- RESPONSE ---
{"status": "ok"}
"#
}
