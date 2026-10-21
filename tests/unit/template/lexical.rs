use crate::common::{codes, here};
use oas_forge::diagnostics::{Code, Diagnostics};
use oas_forge::template::lexical::{
    ESCAPED_DOLLAR, EXTEND_KEY, environment_variables, expand, expand_lines, split_args,
    substitute, substitute_params,
};
use oas_forge::template::{Fragment, Registry, Variables};

fn registry(fragments: &[(&str, &[&str], &str)]) -> Registry {
    let mut registry = Registry::new();
    for (name, params, body) in fragments {
        registry.add_fragment(
            *name,
            Fragment {
                params: params.iter().map(|p| p.to_string()).collect(),
                body: body.to_string(),
            },
        );
    }
    registry
}

fn run(text: &str, reg: &Registry) -> (String, Diagnostics) {
    run_with(text, reg, &Variables::new())
}

fn run_with(text: &str, reg: &Registry, vars: &Variables) -> (String, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let out = expand(text, reg, vars, &here(), &mut diagnostics);
    (out, diagnostics)
}

#[test]
fn a_final_line_break_is_kept() {
    // Found by the property tests: `|` block scalars at the end of a file lost
    // their final line break.
    assert_eq!(run("a: |\n  text\n", &Registry::new()).0, "a: |\n  text\n");
    assert_eq!(run("\n", &Registry::new()).0, "\n");
    assert_eq!(
        run("a\r\nb", &Registry::new()).0,
        "a\nb",
        "CRLF is normalized"
    );
}

#[test]
fn text_without_directives_is_unchanged() {
    let text = "a: 1\n  b: [x, y]\n# comment";
    assert_eq!(run(text, &Registry::new()).0, text);
}

mod insert {
    use super::*;

    #[test]
    fn splices_the_fragment_with_the_line_indentation() {
        let reg = registry(&[("Headers", &[], "header: x-val\nother: y-val")]);
        let (out, diagnostics) = run("root:\n  @insert Headers", &reg);
        assert_eq!(out, "root:\n  header: x-val\n  other: y-val");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn dash_prefix_is_dropped_for_known_fragments() {
        let reg = registry(&[("Paging", &[], "- name: page\n  in: query")]);
        assert_eq!(
            run("parameters:\n  - @insert Paging", &reg).0,
            "parameters:\n  - name: page\n    in: query"
        );
    }

    #[test]
    fn substitutes_named_parameters_with_quoted_commas() {
        let reg = registry(&[("Field", &["name", "kind"], "{{name}}: {{kind}}")]);
        assert_eq!(
            run("@insert Field(\"my, name\", text)", &reg).0,
            "my, name: text"
        );
    }

    #[test]
    fn extra_arguments_are_ignored_with_a_warning() {
        let reg = registry(&[("One", &["a"], "x: {{a}}")]);
        let (out, diagnostics) = run("@insert One(1, 2)", &reg);
        assert_eq!(out, "x: 1");
        assert_eq!(codes(&diagnostics), [Code::ExtraFragmentArgument]);
        assert_eq!(
            diagnostics.iter().next().unwrap().message(),
            "fragment `One` expects 1 argument(s), got 2"
        );
        let reg = registry(&[("None", &[], "x: 1")]);
        assert_eq!(
            codes(&run("@insert None(1)", &reg).1),
            [Code::ExtraFragmentArgument]
        );
        assert!(run("@insert None()", &reg).1.is_empty());
    }

    #[test]
    fn missing_arguments_keep_placeholders_and_warn() {
        let reg = registry(&[("Error", &["code"], "description: Error {{code}}")]);
        for text in ["@insert Error", "@insert Error()"] {
            let (out, diagnostics) = run(text, &reg);
            assert_eq!(out, "description: Error {{code}}", "{text}");
            assert_eq!(
                codes(&diagnostics),
                [Code::MissingFragmentArgument],
                "{text}"
            );
        }
    }

    #[test]
    fn fragments_may_insert_fragments() {
        let reg = registry(&[
            ("Outer", &[], "a: 1\n@insert Inner"),
            ("Inner", &[], "b: 2"),
        ]);
        assert_eq!(run("@insert Outer", &reg).0, "a: 1\nb: 2");
    }

    #[test]
    fn a_cycle_is_reported_once_and_skipped() {
        let reg = registry(&[("Loop", &[], "x: 1\n@insert Loop")]);
        let (out, diagnostics) = run("@insert Loop", &reg);
        assert_eq!(out, "x: 1");
        assert_eq!(codes(&diagnostics), [Code::FragmentCycle]);

        // Regression: a fragment inserting itself several times grew
        // exponentially up to the depth limit (6^8 lines and findings).
        let reg = registry(&[("Fan", &[], &"@insert Fan\n".repeat(6))]);
        let (out, diagnostics) = run("a: 1\n@insert Fan", &reg);
        assert_eq!(out, "a: 1");
        assert_eq!(codes(&diagnostics), [Code::FragmentCycle]);
    }

    #[test]
    fn indirect_cycles_are_found() {
        let reg = registry(&[("A", &[], "a: 1\n@insert B"), ("B", &[], "b: 2\n@insert A")]);
        let (out, diagnostics) = run("@insert A", &reg);
        assert_eq!(out, "a: 1\nb: 2");
        assert_eq!(codes(&diagnostics), [Code::FragmentCycle]);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message().contains("A → B → A")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn nesting_is_bounded() {
        // Nine distinct fragments, each inserting the next.
        let names: Vec<String> = (0..10).map(|i| format!("F{i}")).collect();
        let bodies: Vec<String> = (0..10)
            .map(|i| match names.get(i + 1) {
                Some(next) => format!("f{i}: 1\n@insert {next}"),
                None => format!("f{i}: 1"),
            })
            .collect();
        let fragments: Vec<(&str, &[&str], &str)> = names
            .iter()
            .zip(&bodies)
            .map(|(name, body)| (name.as_str(), &[][..], body.as_str()))
            .collect();
        let (out, diagnostics) = run("@insert F0", &registry(&fragments));
        assert_eq!(out.lines().count(), 8);
        assert_eq!(codes(&diagnostics), [Code::UnknownFragment]);
    }

    #[test]
    fn empty_fragment_removes_the_line() {
        let reg = registry(&[("Nothing", &[], "")]);
        assert_eq!(run("a: 1\n@insert Nothing", &reg).0, "a: 1");
    }

    #[test]
    fn unknown_name_becomes_a_parameter_reference() {
        assert_eq!(
            run(
                "parameters:\n  @insert QueryParam\n  - @insert Limit",
                &Registry::new()
            )
            .0,
            "parameters:\n  - $ref: \"#/components/parameters/QueryParam\"\n  - $ref: \"#/components/parameters/Limit\""
        );
    }

    #[test]
    fn unknown_name_with_arguments_is_dropped_with_a_warning() {
        let (out, diagnostics) = run("a: 1\n@insert Missing(\"x\")", &Registry::new());
        assert_eq!(out, "a: 1");
        assert_eq!(codes(&diagnostics), [Code::UnknownFragment]);
    }

    #[test]
    fn only_whole_lines_are_directives() {
        let text = "description: use @insert Name to include";
        assert_eq!(run(text, &registry(&[("Name", &[], "x: 1")])).0, text);
    }
}

mod extend_markers {
    use super::*;

    #[test]
    fn become_unique_quoted_keys() {
        let (out, _) = run(
            "get:\n  @extend Secured(\"admin\")\n  @extend Base\n  @extend O'Brien",
            &Registry::new(),
        );
        assert_eq!(
            out,
            format!(
                "get:\n  {EXTEND_KEY}1: 'Secured(\"admin\")'\n  {EXTEND_KEY}2: 'Base'\n  {EXTEND_KEY}3: 'O''Brien'"
            )
        );
    }
}

mod variables {
    use super::*;

    fn vars() -> Variables {
        Variables::from([("VERSION".to_string(), "1.10".to_string())])
    }

    #[test]
    fn whole_unquoted_values_become_quoted_strings() {
        let reg = Registry::new();
        assert_eq!(
            run_with("version: {{VERSION}}", &reg, &vars()).0,
            "version: '1.10'"
        );
        assert_eq!(run_with("- {{VERSION}}", &reg, &vars()).0, "- '1.10'");
        assert_eq!(
            run_with("  v: {{ VERSION }}  ", &reg, &vars()).0,
            "  v: '1.10'  "
        );
    }

    #[test]
    fn whole_values_of_mappings_in_list_items_become_quoted_strings() {
        let reg = Registry::new();
        assert_eq!(
            run_with("- v: {{VERSION}}\n- - w: {{VERSION}}", &reg, &vars()).0,
            "- v: '1.10'\n- - w: '1.10'"
        );
        assert_eq!(
            run_with("- note: v{{VERSION}}", &reg, &vars()).0,
            "- note: v1.10"
        );
    }

    #[test]
    fn block_scalars_are_text() {
        let reg = Registry::new();
        let text = "description: |\n  Version: {{VERSION}}\n\n  - {{VERSION}}\nversion: {{VERSION}}\nlist:\n  - >-\n    Since: {{VERSION}}\n  - {{VERSION}}";
        assert_eq!(
            run_with(text, &reg, &vars()).0,
            "description: |\n  Version: 1.10\n\n  - 1.10\nversion: '1.10'\nlist:\n  - >-\n    Since: 1.10\n  - '1.10'"
        );
        let mut diagnostics = Diagnostics::new();
        assert_eq!(
            substitute(
                "d: |2 # comment\n   x: {{VERSION}}",
                &vars(),
                &here(),
                &mut diagnostics
            ),
            "d: |2 # comment\n   x: 1.10"
        );
        // Keep and strip chomping indicators open block scalars too.
        for indicator in ["|+", "|-", ">+"] {
            assert_eq!(
                substitute(
                    &format!("d: {indicator}\n  x: {{{{VERSION}}}}"),
                    &vars(),
                    &here(),
                    &mut diagnostics
                ),
                format!("d: {indicator}\n  x: 1.10")
            );
        }
    }

    #[test]
    fn a_placeholder_that_is_a_whole_line_is_yaml() {
        let mut variables = vars();
        variables.insert("EXTRA".into(), "x-team: core".into());
        assert_eq!(
            substitute(
                "info:\n  {{EXTRA}}",
                &variables,
                &here(),
                &mut Diagnostics::new()
            ),
            "info:\n  x-team: core"
        );
    }

    #[test]
    fn placeholders_inside_text_are_inserted_raw() {
        let reg = Registry::new();
        assert_eq!(
            run_with(
                "version: '{{VERSION}}'\nnote: built from {{VERSION}} today",
                &reg,
                &vars()
            )
            .0,
            "version: '1.10'\nnote: built from 1.10 today"
        );
    }

    #[test]
    fn unknown_placeholders_stay() {
        let text = "a: {{UNKNOWN}}\nb: {{VERSION}}";
        assert_eq!(
            run_with(text, &Registry::new(), &vars()).0,
            "a: {{UNKNOWN}}\nb: '1.10'"
        );
        assert_eq!(
            run(text, &Registry::new()).0,
            text,
            "no variables, no change"
        );
    }

    #[test]
    fn quotes_in_values_are_escaped() {
        let vars = Variables::from([("Q".to_string(), "it's".to_string())]);
        assert_eq!(
            run_with("a: {{Q}}", &Registry::new(), &vars).0,
            "a: 'it''s'"
        );
    }

    #[test]
    fn environment_placeholders_are_listed() {
        let names: Vec<&str> =
            environment_variables("a: {{env:A}} {{ env:B.C }}\nb: {{NAME}} {{env:A}}").collect();
        assert_eq!(names, ["A", "B.C", "A"]);
    }

    #[test]
    fn environment_variables_are_read() {
        // Cargo sets `CARGO_PKG_NAME` for test binaries.
        let (out, diagnostics) = run("name: {{env:CARGO_PKG_NAME}}", &Registry::new());
        assert_eq!(out, format!("name: '{}'", env!("CARGO_PKG_NAME")));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn unset_environment_variables_are_kept_and_reported_once() {
        let (out, diagnostics) = run(
            "a: {{env:OAS_FORGE_UNSET_B}}\nb: {{env:OAS_FORGE_UNSET_A}} {{env:OAS_FORGE_UNSET_B}}",
            &Registry::new(),
        );
        assert_eq!(
            out,
            "a: {{env:OAS_FORGE_UNSET_B}}\nb: {{env:OAS_FORGE_UNSET_A}} {{env:OAS_FORGE_UNSET_B}}"
        );
        assert_eq!(
            codes(&diagnostics),
            [Code::UnknownVariable, Code::UnknownVariable]
        );
        // In order of their first use, at its line.
        let found: Vec<(&str, usize)> = diagnostics
            .iter()
            .map(|d| (d.message(), d.location().unwrap().line()))
            .collect();
        assert!(
            found[0].0.contains("`OAS_FORGE_UNSET_B`") && found[0].1 == 1,
            "{found:?}"
        );
        assert!(
            found[1].0.contains("`OAS_FORGE_UNSET_A`") && found[1].1 == 2,
            "{found:?}"
        );
    }

    #[test]
    fn inserted_lines_report_at_the_insert_line() {
        let reg = registry(&[("Env", &[], "a: 1\nb: {{env:OAS_FORGE_UNSET_C}}")]);
        let (_, diagnostics) = run("x: 1\n@insert Env", &reg);
        assert_eq!(codes(&diagnostics), [Code::UnknownVariable]);
        assert_eq!(
            diagnostics
                .iter()
                .next()
                .unwrap()
                .location()
                .unwrap()
                .line(),
            2
        );
    }

    #[test]
    fn a_final_line_break_survives_substitution() {
        let text = "d: |\n  {{VERSION}}\n";
        assert_eq!(
            run_with(text, &Registry::new(), &vars()).0,
            "d: |\n  1.10\n"
        );
        let mut diagnostics = Diagnostics::new();
        assert_eq!(
            substitute(text, &vars(), &here(), &mut diagnostics),
            "d: |\n  1.10\n"
        );
    }

    #[test]
    fn substitute_touches_only_placeholders() {
        let mut diagnostics = Diagnostics::new();
        let text = "@insert Frag\nprice: \\$1\nv: {{VERSION}}";
        assert_eq!(
            substitute(text, &vars(), &here(), &mut diagnostics),
            "@insert Frag\nprice: \\$1\nv: '1.10'"
        );
        assert!(diagnostics.is_empty());
    }
}

#[test]
fn expanded_lines_know_their_source_line() {
    let reg = registry(&[("Two", &[], "a: 1\nb: 2")]);
    let vars = Variables::from([("MULTI".to_string(), "x\ny".to_string())]);
    let text = "first: 1\n  @insert Two\nnote: v{{MULTI}}\nprice: \\$1\r\nlast: 2\n";
    let mut diagnostics = Diagnostics::new();
    let lines = expand_lines(text, &reg, &vars, &here(), &mut diagnostics);
    assert_eq!(
        lines,
        [
            ("first: 1".to_string(), 0),
            ("  a: 1".to_string(), 1),
            ("  b: 2".to_string(), 1),
            ("note: vx".to_string(), 2),
            ("y".to_string(), 2),
            (format!("price: {ESCAPED_DOLLAR}1"), 3),
            ("last: 2".to_string(), 4),
        ]
    );
    // The same lines as `expand`.
    let expanded = run_with(text, &reg, &vars).0;
    assert_eq!(
        expanded.lines().collect::<Vec<_>>(),
        lines.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn escaped_dollars_are_protected() {
    let (out, _) = run("price: \\$100\nref: \\$User", &Registry::new());
    assert_eq!(
        out,
        format!("price: {ESCAPED_DOLLAR}100\nref: {ESCAPED_DOLLAR}User")
    );
}

#[test]
fn arguments_are_substituted_in_one_pass() {
    assert_eq!(
        substitute_params(
            "{{a}} {{ b }} {{c}}",
            &["a".into(), "b".into()],
            &["{{b}}".into(), "2".into()]
        ),
        "{{b}} 2 {{c}}"
    );
}

#[test]
fn argument_helpers() {
    assert_eq!(split_args(""), Vec::<String>::new());
    assert_eq!(split_args(" a , \"b, c\" ,d "), ["a", "b, c", "d"]);
    assert_eq!(
        substitute_params(
            "{{a}}-{{b}}-{{a}}",
            &["a".into(), "b".into()],
            &["1".into()]
        ),
        "1-{{b}}-1"
    );
}

/// `expand_block`: blocks whose lines are not all YAML (route and channel
/// blocks), where only the YAML lines quote whole-value placeholders.
mod blocks {
    use super::*;
    use oas_forge::template::lexical::expand_block;

    fn vars() -> Variables {
        Variables::from([
            ("VERSION".to_string(), "1.10".to_string()),
            ("MULTI".to_string(), "it's\nnew".to_string()),
        ])
    }

    /// The lines of `text` expanded with the `yaml` classification.
    fn block(text: &str, yaml: &[bool]) -> Vec<String> {
        let mut diagnostics = Diagnostics::new();
        let lines = expand_block(
            text,
            &Registry::new(),
            &vars(),
            &here(),
            &mut diagnostics,
            |_| yaml.to_vec(),
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        lines.into_iter().map(|(line, _)| line).collect()
    }

    #[test]
    fn whole_values_are_quoted_on_yaml_lines_only() {
        let text = "Since: {{VERSION}}\nx-since: {{VERSION}}\n- {{VERSION}}\n- {{VERSION}}";
        assert_eq!(
            block(text, &[false, true, false, true]),
            ["Since: 1.10", "x-since: '1.10'", "- 1.10", "- '1.10'"]
        );
    }

    #[test]
    fn values_inside_text_are_the_same_on_every_line() {
        let text = "Built from {{VERSION}}.\nnote: v{{VERSION}}";
        assert_eq!(
            block(text, &[false, true]),
            ["Built from 1.10.", "note: v1.10"]
        );
    }

    #[test]
    fn lines_the_classification_leaves_out_are_yaml() {
        let text = "a: {{VERSION}}\nb: {{VERSION}}";
        assert_eq!(block(text, &[false]), ["a: 1.10", "b: '1.10'"]);
    }

    #[test]
    fn multi_line_values_are_split_and_quoted_across_their_lines() {
        let text = "Note: {{MULTI}}\nnote: {{MULTI}}";
        assert_eq!(
            block(text, &[false, false, true, true]),
            ["Note: it's", "new", "note: 'it''s", "new'"]
        );
    }

    #[test]
    fn the_classification_sees_the_expanded_lines_with_plain_values() {
        let reg = registry(&[("Two", &[], "a: {{VERSION}}\nb: 2")]);
        let mut seen = Vec::new();
        let mut diagnostics = Diagnostics::new();
        let lines = expand_block(
            "x: {{VERSION}}\n  @insert Two\n@extend Common\nprice: \\$1",
            &reg,
            &vars(),
            &here(),
            &mut diagnostics,
            |lines| {
                seen = lines.to_vec();
                vec![true; lines.len()]
            },
        );
        assert_eq!(
            seen,
            [
                "x: 1.10",
                "  a: 1.10",
                "  b: 2",
                &format!("{EXTEND_KEY}1: 'Common'"),
                &format!("price: {ESCAPED_DOLLAR}1"),
            ]
        );
        assert_eq!(
            lines,
            [
                ("x: '1.10'".to_string(), 0),
                ("  a: '1.10'".to_string(), 1),
                ("  b: 2".to_string(), 1),
                (format!("{EXTEND_KEY}1: 'Common'"), 2),
                (format!("price: {ESCAPED_DOLLAR}1"), 3),
            ]
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn unset_environment_variables_are_reported_once() {
        let mut diagnostics = Diagnostics::new();
        let lines = expand_block(
            "a: {{env:OAS_FORGE_UNSET_E}}\nb {{env:OAS_FORGE_UNSET_E}}",
            &Registry::new(),
            &Variables::new(),
            &here(),
            &mut diagnostics,
            |lines| vec![false; lines.len()],
        );
        assert_eq!(lines[0].0, "a: {{env:OAS_FORGE_UNSET_E}}");
        assert_eq!(codes(&diagnostics), [Code::UnknownVariable]);
    }

    #[test]
    fn findings_of_the_expansion_are_reported() {
        let mut diagnostics = Diagnostics::new();
        let lines = expand_block(
            "a: 1\n@insert Missing(1)",
            &Registry::new(),
            &Variables::new(),
            &here(),
            &mut diagnostics,
            |lines| vec![true; lines.len()],
        );
        assert_eq!(lines, [("a: 1".to_string(), 0)]);
        assert_eq!(codes(&diagnostics), [Code::UnknownFragment]);
    }
}

/// `@insert` and `@extend` with the fragment in Markdown backticks.
mod backticks {
    use super::*;

    #[test]
    fn insert_and_extend_accept_backticked_fragments() {
        let reg = registry(&[
            ("Limit", &["max"], "maximum: {{max}}"),
            ("Paging", &[], "- page"),
        ]);
        assert_eq!(
            run(
                "@insert `Limit(10)`\nlist:\n  - @insert `Paging`\n@extend `Audited(1)`",
                &reg
            )
            .0,
            run(
                "@insert Limit(10)\nlist:\n  - @insert Paging\n@extend Audited(1)",
                &reg
            )
            .0
        );
        assert_eq!(run("@insert `Limit(10)`", &reg).0, "maximum: 10");
        assert_eq!(
            run("@extend `Audited(1)`", &reg).0,
            format!("{EXTEND_KEY}1: 'Audited(1)'")
        );
        // Only a whole, singly backticked argument is unwrapped.
        assert_eq!(
            run("@extend `Audited`(1)", &reg).0,
            format!("{EXTEND_KEY}1: '`Audited`(1)'")
        );
    }
}
