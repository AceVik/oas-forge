//! `src/dsl/operation.rs`: operation-level directives.

use crate::common::operation;
use serde_json::{Value, json};

mod security {
    use super::*;

    fn security(directives: &[&str]) -> Value {
        let mut src = vec!["@route GET /x"];
        src.extend_from_slice(directives);
        operation(&src, "/x", "get")["security"].clone()
    }

    #[test]
    fn scheme_without_scopes() {
        assert_eq!(security(&["@security Basic"]), json!([{ "Basic": [] }]));
    }

    #[test]
    fn quoted_scopes_are_unquoted() {
        assert_eq!(
            security(&["@security OAuth2(\"read\", \"write\")"]),
            json!([{ "OAuth2": ["read", "write"] }])
        );
    }

    #[test]
    fn multiple_requirements_accumulate() {
        assert_eq!(
            security(&["@security Basic", "@security oidcAuth(\"read\")"]),
            json!([{ "Basic": [] }, { "oidcAuth": ["read"] }])
        );
    }

    #[test]
    fn empty_parentheses_mean_no_scopes() {
        assert_eq!(security(&["@security OAuth2()"]), json!([{ "OAuth2": [] }]));
    }

    #[test]
    fn unclosed_parenthesis_does_not_panic_or_truncate() {
        // Regression: `OAuth2(` panicked and `OAuth2(read` lost its last character.
        assert_eq!(security(&["@security OAuth2("]), json!([{ "OAuth2": [] }]));
        assert_eq!(
            security(&["@security OAuth2(read"]),
            json!([{ "OAuth2": ["read"] }])
        );
    }
}

mod directives {
    use super::*;
    use crate::common::try_route;
    use crate::dsl::findings;
    use oas_forge::diagnostics::Code;

    #[test]
    fn summary_description_and_flags() {
        let op = operation(
            &[
                "First plain line",
                "@summary Explicit summary",
                "@description Starts here",
                "and continues",
                "",
                "responses: stays prose inside a description",
                "@deprecated",
                "@operation-id customId",
                "@external-docs https://docs \"Guide\"",
                "@server https://eu \"EU\"",
                "@server https://us",
                "@route GET /x",
            ],
            "/x",
            "get",
        );
        assert_eq!(op["summary"], "Explicit summary");
        assert_eq!(
            op["description"],
            "Starts here\nand continues\n\nresponses: stays prose inside a description"
        );
        assert_eq!(op["deprecated"], true);
        assert_eq!(op["operationId"], "customId");
        assert_eq!(
            op["externalDocs"],
            json!({ "url": "https://docs", "description": "Guide" })
        );
        assert_eq!(
            op["servers"],
            json!([{ "url": "https://eu", "description": "EU" }, { "url": "https://us" }])
        );
        assert!(op.get("responses").is_some());
    }

    #[test]
    fn invalid_operation_directives_are_reported() {
        for bad in [
            "@external-docs",
            "@external-docs a b",
            "@server",
            "@security",
            "@security +",
        ] {
            assert_eq!(
                findings(&["@route GET /x", bad]),
                [Code::InvalidDirective],
                "{bad}"
            );
        }
        assert_eq!(
            findings(&["@route GET /x", "@operation-id"]),
            [Code::UnknownDirective]
        );
    }

    #[test]
    fn security_alternatives_and_combinations() {
        let security = |lines: &[&str]| -> Value {
            let mut all = vec!["@route GET /x"];
            all.extend_from_slice(lines);
            operation(&all, "/x", "get")["security"].clone()
        };
        assert_eq!(security(&["@security none"]), json!([]));
        assert_eq!(
            security(&["@security oauth(read, \"write\") + key", "@security basic"]),
            json!([{ "oauth": ["read", "write"], "key": [] }, { "basic": [] }])
        );
        assert_eq!(
            security(&["@security oauth(read)", "@security none", "@security none"]),
            json!([{ "oauth": ["read"] }, {}]),
            "`none` next to schemes makes authentication optional"
        );
    }

    #[test]
    fn standard_and_additional_methods() {
        let (document, _) = try_route(&["@route query /search"]);
        assert!(document.unwrap()["paths"]["/search"]["query"].is_object());
        let (document, _) = try_route(&["@route Purge /cache"]);
        let document = document.unwrap();
        assert!(document["paths"]["/cache"]["additionalOperations"]["PURGE"].is_object());
        assert_eq!(
            document["paths"]["/cache"]["additionalOperations"]["PURGE"]["operationId"],
            "op"
        );
    }

    #[test]
    fn webhooks() {
        let lines: Vec<String> = ["@webhook orderCreated", "@body $Order", "@return 200:"]
            .iter()
            .map(|l| l.to_string())
            .collect();
        let location = crate::common::here();
        let compiled = oas_forge::dsl::compile(
            oas_forge::dsl::RouteSource::new(&lines, &location),
            &mut oas_forge::diagnostics::Diagnostics::new(),
        )
        .unwrap();
        assert!(compiled.webhook);
        assert_eq!(
            (compiled.path.as_str(), compiled.method.as_str()),
            ("orderCreated", "post")
        );
        let operation = &compiled.document["webhooks"]["orderCreated"]["post"];
        assert_eq!(operation["operationId"], "post_ordercreated");
        assert!(operation["requestBody"].is_object());

        let (document, _) = try_route(&["@webhook PUT refund"]);
        assert!(document.unwrap()["webhooks"]["refund"]["put"].is_object());
        assert_eq!(findings(&["@webhook"]), [Code::InvalidRoute]);
        assert_eq!(findings(&["@webhook a b c"]), [Code::InvalidRoute]);
    }

    #[test]
    fn a_block_may_have_several_routes() {
        let (document, diagnostics) = try_route(&["@route GET /a", "@route POST /b"]);
        let document = document.unwrap();
        assert_eq!(document["paths"]["/a"]["get"]["operationId"], "op_get_a");
        assert_eq!(document["paths"]["/b"]["post"]["operationId"], "op_post_b");
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }
}
