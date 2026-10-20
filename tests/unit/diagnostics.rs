use oas_forge::diagnostics::{Code, Diagnostic, Diagnostics, Location, Severity};
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn locations_display_file_line_and_column() {
    let location = Location::new("src/api.rs", 12);
    assert_eq!(location.to_string(), "src/api.rs:12");
    assert_eq!(location.file(), Path::new("src/api.rs"));
    assert_eq!(location.line(), 12);
    assert_eq!(location.column(), None);

    let with_column = location.clone().with_column(4);
    assert_eq!(with_column.to_string(), "src/api.rs:12:4");
    assert_eq!(with_column.offset(3).to_string(), "src/api.rs:15");
}

#[test]
fn codes_are_unique_stable_and_categorized() {
    let texts: BTreeSet<&str> = Code::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(texts.len(), Code::ALL.len(), "every code is unique");
    for code in Code::ALL {
        let text = code.as_str();
        assert!(text.starts_with("OF") && text.len() == 6, "{text}");
        assert_eq!(code.to_string(), text);
    }
    assert_eq!(Code::InvalidRoute.as_str(), "OF2001");
    assert_eq!(Code::InvalidRoute.default_severity(), Severity::Error);
    assert_eq!(
        Code::UnresolvedReference.default_severity(),
        Severity::Warning
    );
    assert_eq!(Code::OutputNotRead.default_severity(), Severity::Note);
}

#[test]
fn diagnostics_render_severity_code_message_and_location() {
    let diagnostic = Diagnostic::new(Code::UnknownFragment, "unknown fragment `X`")
        .at(Location::new("lib.rs", 3));
    assert_eq!(
        diagnostic.to_string(),
        "warning[OF3001]: unknown fragment `X` (lib.rs:3)"
    );
    assert_eq!(diagnostic.code(), Code::UnknownFragment);
    assert_eq!(diagnostic.message(), "unknown fragment `X`");
    assert_eq!(diagnostic.location(), Some(&Location::new("lib.rs", 3)));
    assert!(!diagnostic.is_error());

    let raised = diagnostic.with_severity(Severity::Error).at_opt(None);
    assert_eq!(raised.to_string(), "error[OF3001]: unknown fragment `X`");
    assert!(raised.is_error());
    for (severity, text) in [
        (Severity::Error, "error"),
        (Severity::Warning, "warning"),
        (Severity::Note, "note"),
    ] {
        assert_eq!(severity.to_string(), text);
    }
}

#[test]
fn collection_tracks_errors_and_can_deny_warnings() {
    let mut diagnostics = Diagnostics::new();
    assert!(diagnostics.is_empty());
    diagnostics.push(Diagnostic::new(Code::UnknownFragment, "w"));
    diagnostics.extend([Diagnostic::new(Code::OutputNotRead, "n")]);
    assert_eq!(diagnostics.len(), 2);
    assert!(!diagnostics.has_errors());

    diagnostics.deny_warnings();
    let severities: Vec<_> = diagnostics.iter().map(Diagnostic::severity).collect();
    assert_eq!(severities, [Severity::Error, Severity::Note]);
    assert!(diagnostics.has_errors());

    let borrowed: Vec<_> = (&diagnostics).into_iter().map(Diagnostic::code).collect();
    let owned: Vec<_> = diagnostics.clone().into_iter().map(|d| d.code()).collect();
    assert_eq!(borrowed, owned);
    assert_eq!(diagnostics.into_vec().len(), 2);
}
