// Split GCTF sections into multiple documents based on ENDPOINT boundaries.
//
// "Preamble" sections (ADDRESS, TLS, PROTO, OPTIONS, REQUEST_HEADERS) that appear
// immediately before ENDPOINT are moved to the new document — they configure the
// next request, not the previous one.

use crate::parser::ast::{Section, SectionType};

/// Sections that are "preambles" — they belong to the next document, not the current one.
fn is_preamble(t: &SectionType) -> bool {
    matches!(
        t,
        SectionType::Address
            | SectionType::Tls
            | SectionType::Proto
            | SectionType::Options
            | SectionType::RequestHeaders
    )
}

/// Sections that indicate a completed request/response cycle.
fn has_content(sections: &[Section]) -> bool {
    sections.iter().any(|s| {
        matches!(
            s.section_type,
            SectionType::Request
                | SectionType::Response
                | SectionType::Error
                | SectionType::Asserts
                | SectionType::Extract
        )
    })
}

/// Split sections into documents based on ENDPOINT boundaries.
///
/// Each ENDPOINT with meaningful content before it starts a new document.
/// Preamble sections перед ENDPOINT перемещаются в новый документ.
pub fn split_sections_by_boundary(sections: &[Section]) -> Vec<Vec<Section>> {
    if sections.is_empty() {
        return Vec::new();
    }

    let mut docs: Vec<Vec<Section>> = Vec::new();
    let mut current: Vec<Section> = Vec::new();

    for section in sections {
        if section.section_type == SectionType::Endpoint && has_content(&current) {
            let mut preamble: Vec<Section> = Vec::new();
            while let Some(last) = current.last() {
                if is_preamble(&last.section_type) {
                    preamble.insert(0, current.pop().unwrap());
                } else {
                    break;
                }
            }
            docs.push(std::mem::take(&mut current));
            current = preamble;
        }
        current.push(section.clone());
    }

    if !current.is_empty() {
        docs.push(current);
    }

    docs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{Section, SectionContent, SectionType};

    fn section(stype: SectionType, line: usize) -> Section {
        Section {
            section_type: stype,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: line,
            end_line: line,
        }
    }

    #[test]
    fn test_split_single_document() {
        let sections = vec![
            section(SectionType::Endpoint, 0),
            section(SectionType::Request, 2),
            section(SectionType::Response, 4),
        ];
        let docs = split_sections_by_boundary(&sections);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].len(), 3);
    }

    #[test]
    fn test_split_two_documents_with_preamble() {
        let sections = vec![
            section(SectionType::Endpoint, 0),
            section(SectionType::Request, 2),
            section(SectionType::Response, 4),
            section(SectionType::Address, 7), // preamble for next
            section(SectionType::Endpoint, 9),
            section(SectionType::Request, 11),
            section(SectionType::Response, 13),
        ];
        let docs = split_sections_by_boundary(&sections);
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].len(), 3); // endpoint + request + response
        assert_eq!(docs[1].len(), 4); // address + endpoint + request + response
    }
}
