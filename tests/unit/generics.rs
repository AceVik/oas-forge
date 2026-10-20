use oas_forge::generics::Monomorphizer;
use oas_forge::index::Registry;

fn registry(blueprints: &[(&str, &[&str], &str)]) -> Registry {
    let mut registry = Registry::new();
    for (name, params, body) in blueprints {
        registry.insert_blueprint(
            name.to_string(),
            params.iter().map(|p| p.to_string()).collect(),
            body.to_string(),
        );
    }
    registry
}

fn process(registry: &mut Registry, text: &str) -> String {
    Monomorphizer::new(registry).process(text)
}

mod process {
    use super::*;

    #[test]
    fn replaces_generic_reference_with_concrete_name() {
        let mut reg = registry(&[("Page", &["T"], "data: $ref: $T")]);

        let result = process(&mut reg, "scheme: $ref: $Page<User>");

        assert_eq!(result, "scheme: $ref: $Page_User");
        assert_eq!(reg.concrete_schemas["Page_User"], "data: $ref: $User");
    }

    #[test]
    fn leaves_text_without_generics_untouched() {
        let mut reg = Registry::new();
        for text in [
            "",
            "plain text",
            "$User",
            "a $Foo b",
            "costs $5",
            "$ alone",
            "end$",
        ] {
            assert_eq!(process(&mut reg, text), text);
        }
        assert!(reg.concrete_schemas.is_empty());
    }

    #[test]
    fn resolves_every_generic_in_the_text() {
        let mut reg = registry(&[("Page", &["T"], "items: $T")]);

        let result = process(&mut reg, "a: $Page<User>\nb: $Page<Order>");

        assert_eq!(result, "a: $Page_User\nb: $Page_Order");
        assert_eq!(reg.concrete_schemas.len(), 2);
    }

    #[test]
    fn resolves_nested_generics_inside_out() {
        let mut reg = registry(&[("Wrapper", &["T"], "wrap: $T"), ("Inner", &["U"], "in: $U")]);

        let result = process(&mut reg, "$Wrapper<$Inner<Item>>");

        assert_eq!(result, "$Wrapper_Inner_Item");
        assert_eq!(reg.concrete_schemas["Inner_Item"], "in: $Item");
        assert_eq!(
            reg.concrete_schemas["Wrapper_Inner_Item"],
            "wrap: $Inner_Item"
        );
    }

    #[test]
    fn keeps_unbalanced_brackets_verbatim() {
        let mut reg = Registry::new();
        assert_eq!(process(&mut reg, "$Foo<Bar"), "$Foo<Bar");
        assert_eq!(process(&mut reg, "x $A<B<C> y"), "x $A<B<C> y");
        assert!(reg.concrete_schemas.is_empty());
    }

    #[test]
    fn handles_non_ascii_text_around_references() {
        // Regression: char indices were used as byte offsets, which corrupted
        // the output ("Größe: : $Us") or panicked on multi-byte characters.
        let mut reg = registry(&[("Page", &["T"], "items: $T")]);

        assert_eq!(process(&mut reg, "Größe: $User"), "Größe: $User");
        assert_eq!(process(&mut reg, "éé $X"), "éé $X");
        assert_eq!(
            process(&mut reg, "Übersicht: $Page<Kunde> – fertig"),
            "Übersicht: $Page_Kunde – fertig"
        );
        assert_eq!(process(&mut reg, "ñ $Foo<Bar"), "ñ $Foo<Bar");
    }
}

mod monomorphize {
    use super::*;

    #[test]
    fn substitutes_multiple_parameters_by_position() {
        let mut reg = registry(&[("Map", &["K", "V"], "key: $K\nvalue: $V")]);

        let name = Monomorphizer::new(&mut reg).monomorphize("Map", "String, User");

        assert_eq!(name, "Map_String_User");
        assert_eq!(reg.concrete_schemas[&name], "key: $String\nvalue: $User");
    }

    #[test]
    fn replaces_every_occurrence_of_a_parameter() {
        let mut reg = registry(&[("Pair", &["T"], "left: $T\nright: $T")]);

        let name = Monomorphizer::new(&mut reg).monomorphize("Pair", "User");

        assert_eq!(reg.concrete_schemas[&name], "left: $User\nright: $User");
    }

    #[test]
    fn parameters_are_replaced_as_whole_identifiers_only() {
        // Regression: `$T` was also replaced inside `$Tag`, producing `$Userag`.
        let mut reg = registry(&[(
            "Page",
            &["T"],
            "items: $T\ntag: $Tag\nraw: $T_x\nsize: $Tä\nnext: [$T, $T]",
        )]);

        let name = Monomorphizer::new(&mut reg).monomorphize("Page", "User");

        assert_eq!(
            reg.concrete_schemas[&name],
            "items: $User\ntag: $Tag\nraw: $T_x\nsize: $Tä\nnext: [$User, $User]"
        );
    }

    #[test]
    fn parameters_are_substituted_simultaneously() {
        // `T` prefixes `TT`, and the argument for `T` is itself a parameter name:
        // sequential replacement used to rewrite already substituted text.
        let mut reg = registry(&[("Pair", &["T", "TT"], "a: $T\nb: $TT")]);

        let name = Monomorphizer::new(&mut reg).monomorphize("Pair", "TT, User");

        assert_eq!(reg.concrete_schemas[&name], "a: $TT\nb: $User");
    }

    #[test]
    fn strips_dollar_prefix_from_arguments() {
        let mut reg = registry(&[("Page", &["T"], "items: $T")]);

        let name = Monomorphizer::new(&mut reg).monomorphize("Page", "$User");

        assert_eq!(name, "Page_User");
        assert_eq!(reg.concrete_schemas[&name], "items: $User");
    }

    #[test]
    fn splits_arguments_only_at_top_level_commas() {
        let mut reg = registry(&[
            ("Map", &["K", "V"], "key: $K\nvalue: $V"),
            ("Pair", &["A", "B"], "a: $A\nb: $B"),
        ]);

        let name = Monomorphizer::new(&mut reg).monomorphize("Map", "String, $Pair<A, B>");

        assert_eq!(name, "Map_String_Pair_A_B");
        assert_eq!(reg.concrete_schemas["Pair_A_B"], "a: $A\nb: $B");
        assert_eq!(
            reg.concrete_schemas["Map_String_Pair_A_B"],
            "key: $String\nvalue: $Pair_A_B"
        );
    }

    #[test]
    fn empty_argument_list_uses_generic_suffix() {
        let mut reg = registry(&[("Page", &[], "type: object")]);

        let name = Monomorphizer::new(&mut reg).monomorphize("Page", "  ");

        assert_eq!(name, "Page_Generic");
        assert_eq!(reg.concrete_schemas[&name], "type: object");
    }

    #[test]
    fn missing_blueprint_returns_name_without_registering_schema() {
        let mut reg = Registry::new();

        let name = Monomorphizer::new(&mut reg).monomorphize("Unknown", "User");

        assert_eq!(name, "Unknown_User");
        assert!(reg.concrete_schemas.is_empty());
    }

    #[test]
    fn argument_count_mismatch_substitutes_what_is_available() {
        crate::common::init_logging();
        let mut reg = registry(&[("Map", &["K", "V"], "key: $K\nvalue: $V")]);

        let name = Monomorphizer::new(&mut reg).monomorphize("Map", "String");

        assert_eq!(name, "Map_String");
        assert_eq!(reg.concrete_schemas[&name], "key: $String\nvalue: $V");
    }

    #[test]
    fn existing_concrete_schema_is_reused() {
        let mut reg = registry(&[("Page", &["T"], "items: $T")]);
        reg.concrete_schemas
            .insert("Page_User".to_string(), "custom".to_string());

        let name = Monomorphizer::new(&mut reg).monomorphize("Page", "User");

        assert_eq!(name, "Page_User");
        assert_eq!(reg.concrete_schemas["Page_User"], "custom");
    }
}
