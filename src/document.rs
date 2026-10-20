//! Walking the operations of an OpenAPI document.

use crate::dsl::METHODS;
use serde_json::{Map, Value};

/// Where an operation is defined.
pub(crate) struct OperationRef<'a> {
    /// `paths` or `webhooks`.
    pub section: &'a str,
    /// The path or webhook name.
    pub name: &'a str,
    /// The method (lowercase fixed field, or an `additionalOperations` key).
    pub method: &'a str,
}

/// Calls `f` for every operation in `paths` and `webhooks`, including
/// `additionalOperations` (OpenAPI 3.2).
pub(crate) fn for_each_operation(
    document: &mut Value,
    mut f: impl FnMut(OperationRef<'_>, &mut Map<String, Value>),
) {
    for section in ["paths", "webhooks"] {
        let Some(items) = document.get_mut(section).and_then(Value::as_object_mut) else {
            continue;
        };
        for (name, item) in items.iter_mut() {
            let Some(item) = item.as_object_mut() else {
                continue;
            };
            for (method, operation) in item.iter_mut() {
                if METHODS.contains(&method.as_str())
                    && let Some(operation) = operation.as_object_mut()
                {
                    f(
                        OperationRef {
                            section,
                            name,
                            method,
                        },
                        operation,
                    );
                }
            }
            if let Some(Value::Object(additional)) = item.get_mut("additionalOperations") {
                for (method, operation) in additional.iter_mut() {
                    if let Some(operation) = operation.as_object_mut() {
                        f(
                            OperationRef {
                                section,
                                name,
                                method,
                            },
                            operation,
                        );
                    }
                }
            }
        }
    }
}
