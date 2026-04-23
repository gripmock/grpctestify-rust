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

fn is_content_section(t: &SectionType) -> bool {
    matches!(
        t,
        SectionType::Request
            | SectionType::Response
            | SectionType::Error
            | SectionType::Asserts
            | SectionType::Extract
    )
}

/// Split owned sections into documents based on ENDPOINT boundaries.
///
/// Each ENDPOINT with meaningful content before it starts a new document.
pub fn split_sections_by_boundary_owned(sections: Vec<Section>) -> Vec<Vec<Section>> {
    if sections.is_empty() {
        return Vec::new();
    }

    let mut docs: Vec<Vec<Section>> = Vec::new();
    let mut current: Vec<Section> = Vec::new();
    let mut current_has_content = false;

    for section in sections {
        if section.section_type == SectionType::Endpoint && current_has_content {
            let mut preamble: Vec<Section> = Vec::new();
            while let Some(last) = current.last() {
                if is_preamble(&last.section_type) {
                    preamble.push(current.pop().unwrap());
                } else {
                    break;
                }
            }
            preamble.reverse();
            docs.push(std::mem::take(&mut current));
            current = preamble;
            current_has_content = false;
        }

        if is_content_section(&section.section_type) {
            current_has_content = true;
        }
        current.push(section);
    }

    if !current.is_empty() {
        docs.push(current);
    }

    docs
}

/// Split sections into documents based on ENDPOINT boundaries.
///
/// Convenience wrapper for borrowed input.
pub fn split_sections_by_boundary(sections: &[Section]) -> Vec<Vec<Section>> {
    split_sections_by_boundary_owned(sections.to_vec())
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

    #[test]
    fn test_split_empty_input() {
        let sections: Vec<Section> = vec![];
        let docs = split_sections_by_boundary(&sections);
        assert!(docs.is_empty());
    }

    #[test]
    fn test_split_no_endpoint_single_doc() {
        let sections = vec![
            section(SectionType::Address, 0),
            section(SectionType::Request, 2),
            section(SectionType::Response, 4),
        ];
        let docs = split_sections_by_boundary(&sections);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].len(), 3);
    }

    #[test]
    fn test_split_only_preamble_sections() {
        let sections = vec![
            section(SectionType::Address, 0),
            section(SectionType::Tls, 2),
            section(SectionType::Proto, 4),
        ];
        let docs = split_sections_by_boundary(&sections);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].len(), 3);
    }

    #[test]
    fn test_split_preambles_without_content_no_split() {
        // Without content before first endpoint, preambles stay in single doc
        let sections = vec![
            section(SectionType::Address, 0),
            section(SectionType::Tls, 2),
            section(SectionType::Proto, 4),
            section(SectionType::Options, 6),
            section(SectionType::RequestHeaders, 8),
            section(SectionType::Endpoint, 10),
            section(SectionType::Request, 12),
            section(SectionType::Response, 14),
        ];
        let docs = split_sections_by_boundary(&sections);
        // No split - no content before first endpoint
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].len(), 8); // all sections in one doc
    }

    #[test]
    fn test_split_endpoint_at_start() {
        let sections = vec![
            section(SectionType::Endpoint, 0),
            section(SectionType::Request, 2),
            section(SectionType::Response, 4),
            section(SectionType::Endpoint, 6),
            section(SectionType::Request, 8),
        ];
        let docs = split_sections_by_boundary(&sections);
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].len(), 3);
        assert_eq!(docs[1].len(), 2);
    }

    #[test]
    fn test_split_extract_as_terminal() {
        let sections = vec![
            section(SectionType::Endpoint, 0),
            section(SectionType::Request, 2),
            section(SectionType::Response, 4),
            section(SectionType::Extract, 6), // terminal
            section(SectionType::Endpoint, 8),
            section(SectionType::Request, 10),
        ];
        let docs = split_sections_by_boundary(&sections);
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn test_split_asserts_as_terminal() {
        let sections = vec![
            section(SectionType::Endpoint, 0),
            section(SectionType::Request, 2),
            section(SectionType::Asserts, 4), // terminal
            section(SectionType::Endpoint, 6),
            section(SectionType::Request, 8),
        ];
        let docs = split_sections_by_boundary(&sections);
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn test_split_error_as_terminal() {
        let sections = vec![
            section(SectionType::Endpoint, 0),
            section(SectionType::Request, 2),
            section(SectionType::Error, 4), // terminal
            section(SectionType::Endpoint, 6),
            section(SectionType::Request, 8),
        ];
        let docs = split_sections_by_boundary(&sections);
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn test_preamble_selection() {
        let sections = vec![
            section(SectionType::Endpoint, 0),
            section(SectionType::Request, 2),
            section(SectionType::Response, 4),
            section(SectionType::Address, 6),         // preamble
            section(SectionType::Tls, 8),             // preamble
            section(SectionType::RequestHeaders, 10), // preamble
            section(SectionType::Endpoint, 12),
            section(SectionType::Request, 14),
        ];
        let docs = split_sections_by_boundary(&sections);
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].len(), 3);
        assert_eq!(docs[1].len(), 5); // 3 preambles + 2 new sections
    }
}
