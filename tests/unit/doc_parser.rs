use oas_forge::doc_parser::{
    apply_casing, extract_doc_comments, extract_naming_and_doc, extract_validation, serde_attr,
};
use serde_json::{Value, json};
use syn::Attribute;

/// Parses the outer attributes written in front of a dummy struct.
fn attrs(src: &str) -> Vec<Attribute> {
    let item: syn::ItemStruct =
        syn::parse_str(&format!("{src}\nstruct Dummy;")).expect("valid attributes");
    item.attrs
}

mod doc_comments {
    use super::*;

    #[test]
    fn collects_doc_lines_in_order() {
        let attrs = attrs("/// first\n#[derive(Debug)]\n/// second\n#[doc = \"third\"]");
        assert_eq!(extract_doc_comments(&attrs), [" first", " second", "third"]);
    }

    #[test]
    fn skips_doc_attributes_that_are_not_string_literals() {
        let attrs = attrs("#[doc(hidden)]\n#[doc = include_str!(\"x.md\")]\n#[doc]\n#[doc = 5]");
        assert!(extract_doc_comments(&attrs).is_empty());
    }
}

mod casing {
    use super::*;

    fn check(rule: &str, cases: &[(&str, &str)]) {
        for (input, expected) in cases {
            assert_eq!(apply_casing(input, rule), *expected, "{rule}({input:?})");
        }
    }

    #[test]
    fn lowercase_and_uppercase() {
        check("lowercase", &[("SuperAdmin", "superadmin")]);
        check("UPPERCASE", &[("SuperAdmin", "SUPERADMIN")]);
    }

    #[test]
    fn pascal_case() {
        check(
            "PascalCase",
            &[
                ("max_connections", "MaxConnections"),
                ("a__b", "AB"),
                ("camelCase", "CamelCase"),
                ("Already", "Already"),
                ("", ""),
            ],
        );
    }

    #[test]
    fn camel_case() {
        check(
            "camelCase",
            &[
                ("birth_place", "birthPlace"),
                ("Simple_id", "simpleId"),
                ("a__b", "aB"),
                ("PendingValidation", "pendingValidation"),
                ("", ""),
            ],
        );
    }

    #[test]
    fn snake_case_matches_serde() {
        check(
            "snake_case",
            &[
                ("SuperAdmin", "super_admin"),
                ("camelCase", "camel_case"),
                ("already_snake", "already_snake"),
                ("HTTPServer", "h_t_t_p_server"),
            ],
        );
    }

    #[test]
    fn screaming_and_kebab_variants() {
        check(
            "SCREAMING_SNAKE_CASE",
            &[("SuperAdmin", "SUPER_ADMIN"), ("first_name", "FIRST_NAME")],
        );
        check(
            "kebab-case",
            &[("SuperAdmin", "super-admin"), ("first_name", "first-name")],
        );
        check("SCREAMING-KEBAB-CASE", &[("SuperAdmin", "SUPER-ADMIN")]);
    }

    #[test]
    fn unknown_rule_keeps_the_text() {
        check("Train-Case", &[("SuperAdmin", "SuperAdmin")]);
    }
}

mod naming_and_doc {
    use super::*;

    struct Naming {
        name: String,
        description: String,
        rename_rule: Option<String>,
        doc_lines: Vec<String>,
        tag: Option<String>,
        content: Option<String>,
    }

    fn naming(src: &str) -> Naming {
        let (name, description, rename_rule, doc_lines, tag, content) =
            extract_naming_and_doc(&attrs(src), "Default");
        Naming {
            name,
            description,
            rename_rule,
            doc_lines,
            tag,
            content,
        }
    }

    #[test]
    fn defaults_without_attributes() {
        let n = naming("");
        assert_eq!(n.name, "Default");
        assert_eq!(n.description, "");
        assert_eq!(n.rename_rule, None);
        assert!(n.doc_lines.is_empty());
        assert_eq!((n.tag, n.content), (None, None));
    }

    #[test]
    fn reads_serde_container_attributes() {
        let n = naming(
            "#[serde(rename = \"Renamed\", rename_all = \"camelCase\", tag = \"type\", content = \"data\")]",
        );
        assert_eq!(n.name, "Renamed");
        assert_eq!(n.rename_rule.as_deref(), Some("camelCase"));
        assert_eq!(n.tag.as_deref(), Some("type"));
        assert_eq!(n.content.as_deref(), Some("data"));
    }

    #[test]
    fn ignores_unrelated_or_malformed_serde_attributes() {
        let n = naming(
            "#[serde]\n#[serde(default, deny_unknown_fields)]\n#[serde(rename = 5, tag = true)]\n#[serde(rename = )]\n#[serde(skip_serializing_if = \"Option::is_none\")]",
        );
        assert_eq!(n.name, "Default");
        assert_eq!(n.tag, None);
    }

    #[test]
    fn ignores_non_literal_serde_values() {
        let n = naming("#[serde(rename = NAME, rename_all = RULE, tag = TAG, content = C)]");
        assert_eq!(n.name, "Default");
        assert_eq!((n.rename_rule, n.tag, n.content), (None, None, None));
    }

    #[test]
    fn openapi_directives_override_serde() {
        let n = naming(
            "#[serde(rename = \"Serde\", rename_all = \"snake_case\")]\n/// @openapi rename \"Doc\"\n/// @openapi rename-all \"kebab-case\"",
        );
        assert_eq!(n.name, "Doc");
        assert_eq!(n.rename_rule.as_deref(), Some("kebab-case"));
    }

    #[test]
    fn description_stops_at_the_openapi_block() {
        let n = naming("///  First line \n/// Second line\n/// @openapi\n/// type: string");
        assert_eq!(n.description, "First line Second line");
        assert_eq!(
            n.doc_lines,
            [
                "  First line ",
                " Second line",
                " @openapi",
                " type: string"
            ]
        );
    }

    #[test]
    fn non_literal_doc_attributes_are_skipped() {
        let n = naming("#[doc = concat!(\"a\", \"b\")]\n#[doc = 5]\n#[doc(hidden)]\n/// Real");
        assert_eq!(n.description, "Real");
        assert_eq!(n.doc_lines, [" Real"]);
    }
}

mod serde_attribute {
    use super::*;

    #[test]
    fn reads_a_string_value_by_key() {
        let attrs = attrs(
            "#[derive(Serialize)]\n#[serde(tag = \"type\", rename_all_fields = \"camelCase\")]",
        );
        assert_eq!(
            serde_attr(&attrs, "rename_all_fields").as_deref(),
            Some("camelCase")
        );
        assert_eq!(serde_attr(&attrs, "tag").as_deref(), Some("type"));
        assert_eq!(serde_attr(&attrs, "content"), None);
    }

    #[test]
    fn last_occurrence_wins_across_attributes() {
        let attrs = attrs(
            "#[serde(rename_all_fields = \"camelCase\")]\n#[serde(rename_all_fields = \"snake_case\")]",
        );
        assert_eq!(
            serde_attr(&attrs, "rename_all_fields").as_deref(),
            Some("snake_case")
        );
    }

    #[test]
    fn ignores_non_string_and_malformed_attributes() {
        let attrs = attrs(
            "#[serde]\n#[serde(rename_all_fields = 5)]\n#[serde(rename_all_fields = )]\n#[serde(default)]",
        );
        assert_eq!(serde_attr(&attrs, "rename_all_fields"), None);
        assert_eq!(serde_attr(&attrs, "default"), None);
    }
}

mod validation {
    use super::*;

    fn validation(src: &str) -> Value {
        extract_validation(&attrs(src))
    }

    #[test]
    fn formats() {
        assert_eq!(
            validation("#[validate(email)]"),
            json!({ "format": "email" })
        );
        assert_eq!(validation("#[validate(url)]"), json!({ "format": "uri" }));
    }

    #[test]
    fn length_and_range() {
        assert_eq!(
            validation("#[validate(length(min = 3, max = 20), range(min = 18, max = 100))]"),
            json!({ "minLength": 3, "maxLength": 20, "minimum": 18, "maximum": 100 })
        );
    }

    #[test]
    fn literal_pattern() {
        assert_eq!(
            validation("#[validate(pattern = \"^[a-z]+$\")]"),
            json!({ "pattern": "^[a-z]+$" })
        );
    }

    #[test]
    fn multiple_validate_attributes_are_combined() {
        assert_eq!(
            validation("#[validate(email)]\n#[validate(length(max = 64))]"),
            json!({ "format": "email", "maxLength": 64 })
        );
    }

    #[test]
    fn unsupported_or_malformed_rules_are_ignored() {
        for src in [
            "#[validate]",
            "#[validate(regex = \"path::to::REGEX\")]",
            "#[validate(pattern = 5)]",
            "#[validate(length(equal = 5, min = \"3\", 7))]",
            "#[validate(range(exclusive_min = 1, max = \"x\", 2))]",
            "#[validate(length(min = ))]",
            "#[validate(range(min = ))]",
            "#[validate(length(min = \"3\", max))]",
            "#[validate(range(min = \"1\", max))]",
            "#[validate(length(max = 99999999999999999999999))]",
            "#[validate(range(max = 99999999999999999999999))]",
            "#[validate(custom(function = \"check\"))]",
            "#[validate(email = )]",
            "#[serde(default)]",
        ] {
            assert_eq!(validation(src), json!({}), "{src}");
        }
    }
}
