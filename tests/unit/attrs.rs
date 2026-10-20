//! `src/attrs/`: rename rules, validation constraints and how serde,
//! validator and `@openapi` directives shape reflected schemas.

use crate::common::{extract, extract_with_diagnostics, schema};
use oas_forge::attrs::{Constraints, RenameRule};
use serde_json::{Value, json};

/// The schema of field `field` of `struct S` with the given field attributes.
fn field_schema(field_attrs: &str) -> Value {
    let items = extract(&format!(
        "/// @openapi\nstruct S {{\n{field_attrs}\nfield: String,\n}}"
    ));
    schema(&items, "S")["properties"]["field"].clone()
}

mod rename_rule {
    use super::*;

    fn check(rule: RenameRule, cases: &[(&str, &str)]) {
        for (input, expected) in cases {
            assert_eq!(rule.apply(input), *expected, "{rule}({input:?})");
        }
    }

    #[test]
    fn spellings_round_trip() {
        for (name, rule) in RenameRule::ALL {
            assert_eq!(RenameRule::parse(name), Some(rule));
            assert_eq!(rule.as_str(), name);
            assert_eq!(rule.to_string(), name);
        }
    }

    #[test]
    fn unknown_spellings_are_rejected() {
        for text in ["Train-Case", "camelcase", "", " snake_case"] {
            assert_eq!(RenameRule::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn lowercase_and_uppercase() {
        check(RenameRule::Lower, &[("SuperAdmin", "superadmin")]);
        check(RenameRule::Upper, &[("SuperAdmin", "SUPERADMIN")]);
    }

    #[test]
    fn pascal_case() {
        check(
            RenameRule::Pascal,
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
            RenameRule::Camel,
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
            RenameRule::Snake,
            &[
                ("SuperAdmin", "super_admin"),
                ("camelCase", "camel_case"),
                ("already_snake", "already_snake"),
                ("HTTPServer", "h_t_t_p_server"),
                ("Ärger", "ärger"),
            ],
        );
    }

    #[test]
    fn screaming_and_kebab_variants() {
        check(
            RenameRule::ScreamingSnake,
            &[("SuperAdmin", "SUPER_ADMIN"), ("first_name", "FIRST_NAME")],
        );
        check(
            RenameRule::Kebab,
            &[("SuperAdmin", "super-admin"), ("first_name", "first-name")],
        );
        check(RenameRule::ScreamingKebab, &[("SuperAdmin", "SUPER-ADMIN")]);
    }
}

mod constraints {
    use super::*;

    #[test]
    fn empty_constraints_have_no_keywords() {
        let constraints = Constraints::default();
        assert!(constraints.is_empty());
        assert!(constraints.to_keywords().is_empty());
    }

    #[test]
    fn keywords_follow_a_fixed_order() {
        let mut constraints = Constraints::default();
        constraints.pattern = Some("^a".into());
        constraints.maximum = Some(9);
        constraints.minimum = Some(-1);
        constraints.max_length = Some(5);
        constraints.min_length = Some(1);
        constraints.format = Some("email".into());
        assert!(!constraints.is_empty());

        let keywords = constraints.to_keywords();
        let keys: Vec<_> = keywords.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "format",
                "minLength",
                "maxLength",
                "minimum",
                "maximum",
                "pattern"
            ]
        );
        assert_eq!(keywords["minimum"], json!(-1));
    }
}

mod validate_attribute {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(field_schema("#[validate(email)]")["format"], "email");
        assert_eq!(field_schema("#[validate(url)]")["format"], "uri");
    }

    #[test]
    fn length_and_range() {
        assert_eq!(
            field_schema("#[validate(length(min = 3, max = 20), range(min = 18, max = 100))]"),
            json!({ "type": "string", "minLength": 3, "maxLength": 20, "minimum": 18, "maximum": 100 })
        );
    }

    #[test]
    fn literal_pattern() {
        assert_eq!(
            field_schema("#[validate(pattern = \"^[a-z]+$\")]")["pattern"],
            "^[a-z]+$"
        );
    }

    #[test]
    fn multiple_validate_attributes_are_combined() {
        assert_eq!(
            field_schema("#[validate(email)]\n#[validate(length(max = 64))]"),
            json!({ "type": "string", "format": "email", "maxLength": 64 })
        );
    }

    #[test]
    fn unsupported_or_malformed_rules_are_ignored() {
        for attrs in [
            "#[validate]",
            "#[validate = \"x\"]",
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
            assert_eq!(field_schema(attrs), json!({ "type": "string" }), "{attrs}");
        }
    }

    #[test]
    fn constraints_apply_before_field_overrides() {
        assert_eq!(
            field_schema("#[validate(length(max = 5))]\n/// @openapi maxLength: 10"),
            json!({ "type": "string", "maxLength": 10 })
        );
    }
}

mod serde_attribute {
    use super::*;

    #[test]
    fn container_rename_and_rename_all() {
        let items = extract(
            "/// @openapi\n#[serde(rename = \"Account\", rename_all = \"camelCase\")]\nstruct User { first_name: String }",
        );
        assert!(
            schema(&items, "Account")["properties"]
                .get("firstName")
                .is_some()
        );
    }

    #[test]
    fn last_value_wins_across_attributes() {
        let items = extract(
            "/// @openapi\n#[serde(rename_all = \"camelCase\")]\n#[serde(rename_all = \"kebab-case\")]\nstruct S { first_name: String }",
        );
        assert!(
            schema(&items, "S")["properties"]
                .get("first-name")
                .is_some()
        );
    }

    #[test]
    fn unrelated_malformed_or_non_literal_attributes_are_ignored() {
        let items = extract(
            r#"
            /// @openapi
            #[serde]
            #[serde = "x"]
            #[serde(default, deny_unknown_fields)]
            #[serde(rename = 5, tag = true)]
            #[serde(rename = NAME, rename_all = RULE)]
            #[serde(rename_all = "Train-Case")]
            #[serde(skip_serializing_if = "Option::is_none")]
            #[serde(crate::path = "x")]
            struct Plain { first_name: String }
            "#,
        );
        assert!(
            schema(&items, "Plain")["properties"]
                .get("first_name")
                .is_some()
        );
    }

    #[test]
    fn openapi_directives_override_serde() {
        let items = extract(
            r#"
            #[serde(rename = "Serde", rename_all = "snake_case")]
            /// @openapi rename "Doc"
            /// @openapi rename-all "kebab-case"
            struct S { first_name: String }
            "#,
        );
        assert!(
            schema(&items, "Doc")["properties"]
                .get("first-name")
                .is_some()
        );
    }

    #[test]
    fn field_rename_beats_the_container_rule() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(rename_all = "camelCase")]
            struct S {
                #[serde(rename = "ID")]
                user_id: String,
                /// @openapi rename "the_name"
                full_name: String,
            }
            "#,
        );
        let properties = &schema(&items, "S")["properties"];
        assert!(properties.get("ID").is_some() && properties.get("the_name").is_some());
    }
}

mod directives {
    use super::*;
    use oas_forge::diagnostics::Code;

    #[test]
    fn description_is_the_text_before_the_openapi_block() {
        let items = extract(
            "///  First line \n/// Second line\n///\n/// Next paragraph\n/// @openapi\n/// type: object\nstruct S;",
        );
        assert_eq!(
            schema(&items, "S")["description"],
            "First line Second line\n\nNext paragraph"
        );
    }

    #[test]
    fn non_literal_doc_attributes_are_skipped() {
        let items = extract(
            "#[doc = concat!(\"a\", \"b\")]\n#[doc = 5]\n#[doc(hidden)]\n/// Real\n/// @openapi\nstruct S { a: String }",
        );
        assert_eq!(schema(&items, "S")["description"], "Real");
    }

    #[test]
    fn openapi_must_start_the_line_to_export() {
        let items = extract("/// Mentions @openapi in passing.\nstruct S { a: String }");
        assert!(items.is_empty());
    }

    #[test]
    fn marker_directives_carry_no_yaml() {
        let items = extract("/// @openapi\n/// @openapi-ignored Name\nstruct S { a: String }");
        assert_eq!(
            schema(&items, "S"),
            json!({ "type": "object", "properties": { "a": { "type": "string" } }, "required": ["a"] })
        );
    }

    #[test]
    fn invalid_override_yaml_is_reported_at_its_line() {
        let (items, diagnostics) =
            extract_with_diagnostics("/// @openapi\n/// type: [unclosed\nstruct S { a: String }");
        assert_eq!(schema(&items, "S")["type"], "object");
        let diagnostic = diagnostics.iter().next().expect("a diagnostic");
        assert_eq!(diagnostic.code(), Code::InvalidOverride);
        assert_eq!(diagnostic.location().unwrap().line(), 2);
    }
}
