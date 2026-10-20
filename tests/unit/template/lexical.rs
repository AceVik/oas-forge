use crate::common::{codes, here};
use oas_forge::diagnostics::{Code, Diagnostics};
use oas_forge::template::lexical::{
    ESCAPED_DOLLAR, EXTEND_KEY, expand, split_args, substitute_params,
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
    fn recursion_is_bounded() {
        let reg = registry(&[("Loop", &[], "x: 1\n@insert Loop")]);
        let (out, diagnostics) = run("@insert Loop", &reg);
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
