//! `src/attrs/`: rename rules, validation constraints and how serde,
//! validator, garde and `@openapi` directives shape reflected schemas.

use crate::common::{codes, extract, extract_with_diagnostics, schema};
use oas_forge::attrs::{Constraints, LengthUnit, RenameRule, Rule};
use oas_forge::diagnostics::Code;
use serde_json::{Number, Value, json};

/// The schema of field `field: ty` of `struct S` with the given field attributes.
fn field_schema(field_attrs: &str, ty: &str) -> Value {
    let items = extract(&format!(
        "/// @openapi\nstruct S {{\n{field_attrs}\nfield: {ty},\n}}"
    ));
    schema(&items, "S")["properties"]["field"].clone()
}

fn required(code: &str, name: &str) -> Value {
    schema(&extract(code), name)
        .get("required")
        .cloned()
        .unwrap_or(json!([]))
}

mod rename_rule {
    use super::*;

    #[test]
    fn spellings_round_trip() {
        for (name, rule) in RenameRule::ALL {
            assert_eq!(RenameRule::parse(name), Some(rule));
            assert_eq!(rule.as_str(), name);
            assert_eq!(rule.to_string(), name);
        }
        for text in ["Train-Case", "camelcase", "", " snake_case"] {
            assert_eq!(RenameRule::parse(text), None, "{text:?}");
        }
    }

    /// serde's own test vectors (`serde_derive/src/internals/case.rs`).
    #[test]
    fn variants_are_renamed_like_serde() {
        use RenameRule::*;
        for (original, lower, upper, camel, snake, screaming, kebab, screaming_kebab) in [
            (
                "Outcome", "outcome", "OUTCOME", "outcome", "outcome", "OUTCOME", "outcome",
                "OUTCOME",
            ),
            (
                "VeryTasty",
                "verytasty",
                "VERYTASTY",
                "veryTasty",
                "very_tasty",
                "VERY_TASTY",
                "very-tasty",
                "VERY-TASTY",
            ),
            ("A", "a", "A", "a", "a", "A", "a", "A"),
            ("Z42", "z42", "Z42", "z42", "z42", "Z42", "z42", "Z42"),
        ] {
            assert_eq!(Lower.apply_to_variant(original), lower);
            assert_eq!(Upper.apply_to_variant(original), upper);
            assert_eq!(Pascal.apply_to_variant(original), original);
            assert_eq!(Camel.apply_to_variant(original), camel);
            assert_eq!(Snake.apply_to_variant(original), snake);
            assert_eq!(ScreamingSnake.apply_to_variant(original), screaming);
            assert_eq!(Kebab.apply_to_variant(original), kebab);
            assert_eq!(ScreamingKebab.apply_to_variant(original), screaming_kebab);
        }
    }

    /// serde's own test vectors (`serde_derive/src/internals/case.rs`).
    #[test]
    fn fields_are_renamed_like_serde() {
        use RenameRule::*;
        for (original, upper, pascal, camel, screaming, kebab, screaming_kebab) in [
            (
                "outcome", "OUTCOME", "Outcome", "outcome", "OUTCOME", "outcome", "OUTCOME",
            ),
            (
                "very_tasty",
                "VERY_TASTY",
                "VeryTasty",
                "veryTasty",
                "VERY_TASTY",
                "very-tasty",
                "VERY-TASTY",
            ),
            ("a", "A", "A", "a", "A", "a", "A"),
            ("z42", "Z42", "Z42", "z42", "Z42", "z42", "Z42"),
        ] {
            assert_eq!(Lower.apply_to_field(original), original);
            assert_eq!(Upper.apply_to_field(original), upper);
            assert_eq!(Pascal.apply_to_field(original), pascal);
            assert_eq!(Camel.apply_to_field(original), camel);
            assert_eq!(Snake.apply_to_field(original), original);
            assert_eq!(ScreamingSnake.apply_to_field(original), screaming);
            assert_eq!(Kebab.apply_to_field(original), kebab);
            assert_eq!(ScreamingKebab.apply_to_field(original), screaming_kebab);
        }
    }

    #[test]
    fn serde_quirks_are_kept() {
        // Field rules assume snake_case input; variant rules PascalCase.
        assert_eq!(RenameRule::Snake.apply_to_field("lastName"), "lastName");
        assert_eq!(RenameRule::Lower.apply_to_field("Mixed"), "Mixed");
        assert_eq!(
            RenameRule::Snake.apply_to_variant("HTTPServer"),
            "h_t_t_p_server"
        );
        // ASCII case mapping only, like serde.
        assert_eq!(RenameRule::Lower.apply_to_variant("Ärger"), "Ärger");
        assert_eq!(RenameRule::Camel.apply_to_variant("Ärger"), "Ärger");
        assert_eq!(RenameRule::Camel.apply_to_variant(""), "");
        assert_eq!(RenameRule::Pascal.apply_to_field("a__b"), "AB");
    }
}

mod constraints {
    use super::*;

    fn applied(rules: Vec<Rule>, schema: Value) -> Value {
        let mut schema = schema;
        Constraints::new(rules).apply(&mut schema);
        schema
    }

    fn length(min: Option<u64>, max: Option<u64>, unit: LengthUnit) -> Rule {
        Rule::Length { min, max, unit }
    }

    #[test]
    fn empty_constraints_change_nothing() {
        let constraints = Constraints::default();
        assert!(constraints.is_empty());
        assert!(!constraints.requires_value());
        assert_eq!(
            applied(vec![], json!({ "type": "string" })),
            json!({ "type": "string" })
        );
    }

    #[test]
    fn length_follows_the_type() {
        let rule = || vec![length(Some(1), Some(5), LengthUnit::Chars)];
        assert_eq!(
            applied(rule(), json!({ "type": "string" })),
            json!({ "type": "string", "minLength": 1, "maxLength": 5 })
        );
        assert_eq!(
            applied(rule(), json!({ "type": "array" })),
            json!({ "type": "array", "minItems": 1, "maxItems": 5 })
        );
        assert_eq!(
            applied(rule(), json!({ "type": "object" })),
            json!({ "type": "object", "minProperties": 1, "maxProperties": 5 })
        );
        assert_eq!(
            applied(rule(), json!({ "type": ["array", "null"] })),
            json!({ "type": ["array", "null"], "minItems": 1, "maxItems": 5 })
        );
        assert_eq!(
            applied(rule(), json!({ "$ref": "$Name" })),
            json!({ "$ref": "$Name", "minLength": 1, "maxLength": 5 })
        );
    }

    #[test]
    fn string_length_units_become_sound_character_bounds() {
        let string = || json!({ "type": "string" });
        assert_eq!(
            applied(vec![length(Some(8), Some(16), LengthUnit::Bytes)], string()),
            json!({ "type": "string", "minLength": 2, "maxLength": 16 })
        );
        assert_eq!(
            applied(vec![length(Some(3), Some(4), LengthUnit::Utf16)], string()),
            json!({ "type": "string", "minLength": 2, "maxLength": 4 })
        );
        assert_eq!(
            applied(
                vec![length(Some(3), Some(4), LengthUnit::Graphemes)],
                string()
            ),
            json!({ "type": "string", "minLength": 3 })
        );
        assert_eq!(
            applied(
                vec![length(Some(8), None, LengthUnit::Bytes)],
                json!({ "type": "array" })
            ),
            json!({ "type": "array", "minItems": 8 }),
            "units only matter for strings"
        );
    }

    #[test]
    fn ranges_formats_and_patterns() {
        let schema = applied(
            vec![
                Rule::Range {
                    min: Some(Number::from(-5)),
                    max: None,
                    exclusive_min: None,
                    exclusive_max: Number::from_f64(1.5),
                },
                Rule::Format("email".into()),
                Rule::Pattern("^a".into()),
                Rule::Pattern("b$".into()),
                Rule::NotPattern("x".into()),
                Rule::NotPattern("y".into()),
                Rule::Required,
            ],
            json!({ "type": "string" }),
        );
        assert_eq!(
            schema,
            json!({
                "type": "string", "minimum": -5, "exclusiveMaximum": 1.5, "format": "email",
                "pattern": "^a", "not": { "pattern": "x" },
                "allOf": [{ "pattern": "b$" }, { "not": { "pattern": "y" } }],
            })
        );
    }

    #[test]
    fn ip_accepts_both_versions() {
        assert_eq!(
            applied(vec![Rule::Ip], json!({ "type": "string", "format": "x" })),
            json!({ "type": "string", "anyOf": [{ "format": "ipv4" }, { "format": "ipv6" }] })
        );
    }

    #[test]
    fn inner_rules_apply_to_items_values_or_the_value_itself() {
        let inner = || vec![Rule::Inner(vec![length(Some(1), None, LengthUnit::Chars)])];
        assert_eq!(
            applied(
                inner(),
                json!({ "type": "array", "items": { "type": "string" } })
            ),
            json!({ "type": "array", "items": { "type": "string", "minLength": 1 } })
        );
        assert_eq!(
            applied(
                inner(),
                json!({ "type": "object", "additionalProperties": { "type": "string" } })
            ),
            json!({ "type": "object", "additionalProperties": { "type": "string", "minLength": 1 } })
        );
        assert_eq!(
            applied(inner(), json!({ "type": "string" })),
            json!({ "type": "string", "minLength": 1 })
        );
    }

    #[test]
    fn pattern_constants_leave_a_marker_until_resolved() {
        assert_eq!(
            applied(
                vec![Rule::PatternConstant("RE".into())],
                json!({ "type": "string" })
            ),
            json!({ "type": "string", "x-oas-forge-pattern": "RE" })
        );
        assert_eq!(
            applied(vec![Rule::Format("uri".into())], json!(true)),
            json!(true)
        );
    }
}

mod validator {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(
            field_schema("#[validate(email)]", "String")["format"],
            "email"
        );
        assert_eq!(field_schema("#[validate(url)]", "String")["format"], "uri");
        assert_eq!(
            field_schema("#[validate(ip(v4))]", "String")["format"],
            "ipv4"
        );
        assert_eq!(
            field_schema("#[validate(ip(v6 = true))]", "String")["format"],
            "ipv6"
        );
        assert_eq!(
            field_schema("#[validate(ip)]", "String")["anyOf"],
            json!([{ "format": "ipv4" }, { "format": "ipv6" }])
        );
    }

    #[test]
    fn length_counts_characters_items_or_entries() {
        assert_eq!(
            field_schema(
                "#[validate(length(min = 3, max = 20, message = \"x\"))]",
                "String"
            ),
            json!({ "type": "string", "minLength": 3, "maxLength": 20 })
        );
        assert_eq!(
            field_schema("#[validate(length(min = 1))]", "Vec<u8>")["minItems"],
            1
        );
        assert_eq!(
            field_schema("#[validate(length(max = 2))]", "HashMap<String, u8>")["maxProperties"],
            2
        );
        assert_eq!(
            field_schema("#[validate(length(equal = 4))]", "String"),
            json!({ "type": "string", "minLength": 4, "maxLength": 4 })
        );
        assert_eq!(
            field_schema("#[validate(length(min = 2))]", "Option<String>"),
            json!({ "type": ["string", "null"], "minLength": 2 })
        );
    }

    #[test]
    fn ranges_accept_negative_float_and_exclusive_bounds() {
        assert_eq!(
            field_schema("#[validate(range(min = -5, max = 5))]", "i64"),
            json!({ "type": "integer", "format": "int64", "minimum": -5, "maximum": 5 })
        );
        assert_eq!(
            field_schema("#[validate(range(min = 0.5, max = 1.5))]", "f64"),
            json!({ "type": "number", "format": "double", "minimum": 0.5, "maximum": 1.5 })
        );
        assert_eq!(
            field_schema(
                "#[validate(range(exclusive_min = 0, exclusive_max = 100))]",
                "f32"
            ),
            json!({ "type": "number", "format": "float", "exclusiveMinimum": 0, "exclusiveMaximum": 100 })
        );
    }

    #[test]
    fn contains_and_does_not_contain_are_escaped_patterns() {
        assert_eq!(
            field_schema("#[validate(contains(pattern = \"a.b\"))]", "String")["pattern"],
            "a\\.b"
        );
        assert_eq!(
            field_schema("#[validate(contains = \"@\")]", "String")["pattern"],
            "@"
        );
        assert_eq!(
            field_schema("#[validate(does_not_contain(pattern = \"(x)\"))]", "String")["not"],
            json!({ "pattern": "\\(x\\)" })
        );
    }

    #[test]
    fn regex_paths_name_constants() {
        for attrs in [
            "#[validate(regex(path = *SLUG))]",
            "#[validate(regex(path = SLUG))]",
            "#[validate(regex(path = \"SLUG\"))]",
            "#[validate(regex = \"crate::re::SLUG\")]",
            "#[validate(regex(path = &crate::SLUG))]",
            "#[validate(regex(path = (SLUG)))]",
        ] {
            assert_eq!(
                field_schema(attrs, "String")["x-oas-forge-pattern"],
                "SLUG",
                "{attrs}"
            );
        }
    }

    #[test]
    fn required_makes_options_required_and_not_nullable() {
        let code = "/// @openapi\nstruct S {\n#[validate(required)]\na: Option<String>,\n#[validate(required_nested)]\nb: Option<String>,\nc: Option<String>,\n}";
        assert_eq!(required(code, "S"), json!(["a", "b"]));
        assert_eq!(
            schema(&extract(code), "S")["properties"]["a"],
            json!({ "type": "string" })
        );
    }

    #[test]
    fn literal_patterns_from_0_1_still_work() {
        assert_eq!(
            field_schema("#[validate(pattern = \"^[a-z]+$\")]", "String")["pattern"],
            "^[a-z]+$"
        );
    }

    #[test]
    fn multiple_attributes_are_combined() {
        assert_eq!(
            field_schema(
                "#[validate(email)]\n#[validate(length(max = 64))]",
                "String"
            ),
            json!({ "type": "string", "format": "email", "maxLength": 64 })
        );
    }

    #[test]
    fn non_literal_values_are_reported() {
        let (items, diagnostics) = extract_with_diagnostics(
            "/// @openapi\nstruct S {\n#[validate(length(min = MIN, max = 5), range(max = LIMIT))]\na: String,\n}",
        );
        assert_eq!(
            schema(&items, "S")["properties"]["a"],
            json!({ "type": "string", "maxLength": 5 })
        );
        assert_eq!(
            codes(&diagnostics),
            [Code::NonLiteralConstraint, Code::NonLiteralConstraint]
        );
        let first = diagnostics.iter().next().unwrap();
        assert!(first.message().contains("length(min = MIN)"), "{first}");
        assert_eq!(first.location().unwrap().line(), 3);
    }

    #[test]
    fn unsupported_or_malformed_rules_are_ignored() {
        for attrs in [
            "#[validate]",
            "#[validate = \"x\"]",
            "#[validate(pattern = 5)]",
            "#[validate(length(7))]",
            "#[validate(length(other = 7))]",
            "#[validate(range(other = 1))]",
            "#[validate(range)]",
            "#[validate(length)]",
            "#[validate(credit_card, non_control_character, nested)]",
            "#[validate(must_match(other = \"b\"))]",
            "#[validate(custom(function = \"check\"))]",
            "#[validate(contains)]",
            "#[validate(contains(pattern = 5))]",
            "#[validate(regex)]",
            "#[validate(regex(path = 5))]",
            "#[validate(email = )]",
            "#[validate(crate::email)]",
            "#[serde(default)]",
        ] {
            assert_eq!(
                field_schema(attrs, "String"),
                json!({ "type": "string" }),
                "{attrs}"
            );
        }
    }

    #[test]
    fn constraints_apply_before_field_overrides() {
        assert_eq!(
            field_schema(
                "#[validate(length(max = 5))]\n/// @openapi maxLength: 10",
                "String"
            ),
            json!({ "type": "string", "maxLength": 10 })
        );
    }
}

mod garde {
    use super::*;

    #[test]
    fn length_modes() {
        assert_eq!(
            field_schema("#[garde(length(min = 8, max = 64))]", "String"),
            json!({ "type": "string", "minLength": 2, "maxLength": 64 }),
            "garde counts bytes by default"
        );
        assert_eq!(
            field_schema("#[garde(length(chars, min = 8))]", "String")["minLength"],
            8
        );
        assert_eq!(
            field_schema("#[garde(length(utf16, max = 3))]", "String")["maxLength"],
            3
        );
        assert_eq!(
            field_schema("#[garde(length(graphemes, min = 2, max = 9))]", "String"),
            json!({ "type": "string", "minLength": 2 })
        );
        assert_eq!(
            field_schema("#[garde(byte_length(max = 4))]", "String")["maxLength"],
            4
        );
        assert_eq!(
            field_schema("#[garde(char_count(min = 4))]", "String")["minLength"],
            4
        );
        assert_eq!(
            field_schema("#[garde(length(simple, min = 1, max = 3))]", "Vec<String>"),
            json!({ "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 3 })
        );
    }

    #[test]
    fn formats_ranges_and_text_rules() {
        assert_eq!(field_schema("#[garde(email)]", "String")["format"], "email");
        assert_eq!(field_schema("#[garde(url)]", "String")["format"], "uri");
        assert_eq!(field_schema("#[garde(ipv4)]", "String")["format"], "ipv4");
        assert_eq!(field_schema("#[garde(ipv6)]", "String")["format"], "ipv6");
        assert!(
            field_schema("#[garde(ip)]", "String")
                .get("anyOf")
                .is_some()
        );
        assert_eq!(
            field_schema("#[garde(range(min = 1, max = 9))]", "u8"),
            json!({ "type": "integer", "format": "int32", "minimum": 1, "maximum": 9 })
        );
        assert_eq!(
            field_schema("#[garde(range(equal = 3))]", "i32"),
            json!({ "type": "integer", "format": "int32", "minimum": 3, "maximum": 3 })
        );
        assert_eq!(
            field_schema("#[garde(contains(\"+\"))]", "String")["pattern"],
            "\\+"
        );
        assert_eq!(
            field_schema("#[garde(prefix(\"v1.\"))]", "String")["pattern"],
            "^v1\\."
        );
        assert_eq!(
            field_schema("#[garde(suffix(\"$\"))]", "String")["pattern"],
            "\\$$"
        );
        assert_eq!(
            field_schema("#[garde(pattern(r\"^\\d+$\"))]", "String")["pattern"],
            "^\\d+$"
        );
        assert_eq!(
            field_schema("#[garde(pattern(DIGITS))]", "String")["x-oas-forge-pattern"],
            "DIGITS"
        );
        assert_eq!(
            field_schema("#[garde(ascii)]", "String")["pattern"],
            "^[\\x00-\\x7F]*$"
        );
    }

    #[test]
    fn inner_and_required() {
        assert_eq!(
            field_schema(
                "#[garde(inner(length(chars, min = 1), ascii))]",
                "Vec<String>"
            )["items"],
            json!({ "type": "string", "minLength": 1, "pattern": "^[\\x00-\\x7F]*$" })
        );
        assert_eq!(
            field_schema("#[garde(inner(skip))]", "Vec<String>"),
            json!({ "type": "array", "items": { "type": "string" } })
        );
        let code = "/// @openapi\nstruct S {\n#[garde(required)]\na: Option<u8>,\n}";
        assert_eq!(required(code, "S"), json!(["a"]));
    }

    #[test]
    fn rules_without_schema_keywords_are_ignored() {
        for attrs in [
            "#[garde(skip)]",
            "#[garde(dive)]",
            "#[garde(alphanumeric, credit_card, phone_number)]",
            "#[garde(custom(check))]",
            "#[garde(matches(other))]",
            "#[garde(length)]",
            "#[garde(length(bytes))]",
            "#[garde(range)]",
            "#[garde(pattern)]",
            "#[garde(pattern(5))]",
            "#[garde(crate::email)]",
        ] {
            assert_eq!(
                field_schema(attrs, "String"),
                json!({ "type": "string" }),
                "{attrs}"
            );
        }
    }

    #[test]
    fn non_literal_arguments_are_reported() {
        let (_, diagnostics) = extract_with_diagnostics(
            "/// @openapi\nstruct S {\n#[garde(prefix(PREFIX), length(max = MAX))]\na: String,\n}",
        );
        assert_eq!(
            codes(&diagnostics),
            [Code::NonLiteralConstraint, Code::NonLiteralConstraint]
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
    fn serialize_names_are_preferred() {
        let items = extract(
            r#"
            /// @openapi
            #[serde(rename(serialize = "Out", deserialize = "In"), rename_all(deserialize = "kebab-case"))]
            struct S {
                #[serde(rename(deserialize = "only_in"))]
                a_b: String,
            }
            "#,
        );
        assert!(schema(&items, "Out")["properties"].get("only_in").is_some());
        let items = extract(
            "/// @openapi\n#[serde(rename_all(deserialize = \"kebab-case\"))]\nstruct T { a_b: String }",
        );
        assert!(schema(&items, "T")["properties"].get("a-b").is_some());
    }

    #[test]
    fn cfg_attr_is_read() {
        let items = extract(
            r#"
            /// @openapi
            #[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
            #[cfg_attr(all(feature = "a", feature = "b"), cfg_attr(test, serde(rename = "Renamed")))]
            struct S {
                #[cfg_attr(feature = "serde", serde(skip))]
                hidden: String,
                #[cfg_attr(feature = "validate", validate(length(min = 1)))]
                first_name: String,
            }
            "#,
        );
        let properties = &schema(&items, "Renamed")["properties"];
        assert_eq!(properties.as_object().unwrap().len(), 1);
        assert_eq!(properties["firstName"]["minLength"], 1);
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
            #[serde(bound = "T: Clone", crate = "serde", expecting = "x")]
            #[serde(rename = 5, tag = true)]
            #[serde(rename = NAME, rename_all = RULE)]
            #[serde(rename_all = "Train-Case")]
            #[serde(rename(5), rename_all(other = "x"))]
            #[serde(crate::path = "x")]
            #[cfg_attr(feature = "x")]
            #[cfg_attr(feature = "x", derive(Debug), serde)]
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
        let (items, diagnostics) = extract_with_diagnostics(
            "/// @openapi\n/// x: 1\n/// type: [unclosed\nstruct S { a: String }",
        );
        assert_eq!(schema(&items, "S")["type"], "object");
        let diagnostic = diagnostics.iter().next().expect("a diagnostic");
        assert_eq!(diagnostic.code(), Code::InvalidOverride);
        assert_eq!(diagnostic.location().unwrap().line(), 3);
    }
}
