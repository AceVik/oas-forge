//! Built-in type names that a documented schema of the same name replaces.
//!
//! A field of type `Duration` gets the built-in schema, unless the crate
//! documents its own `Duration`: then it refers to that schema. Which names
//! are documented is only known when every file has been read, so a
//! reflected schema that names such types bare carries its alternatives
//! under [`KEY`], and the references stage picks one with [`choose`].

use super::types::{MappedType, TypeExpr, TypeMapper};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The marker key of a schema with alternatives (a [`Marker`]).
pub(crate) const KEY: &str = "x-oas-forge-builtin";

/// Above this many names in one type, the built-in schemas are kept.
const MAX_NAMES: usize = 6;

/// The alternatives of a schema that names built-in types bare.
#[derive(Serialize, Deserialize)]
struct Marker {
    /// The bare built-in names in the type.
    names: BTreeSet<String>,
    /// The schemas for subsets of `names` that stand for documented schemas.
    variants: Vec<Alternative>,
}

/// One alternative of a [`Marker`]: the schema for when exactly `names`
/// stand for documented schemas.
#[derive(Serialize, Deserialize)]
struct Alternative {
    /// The built-in names that refer to documented schemas here.
    names: BTreeSet<String>,
    /// The schema with those names as references (`{"$ref": "$Duration"}`).
    schema: Value,
}

/// `eager` (the built-in schemas) with the alternatives `build` makes for
/// every non-empty subset of `names` that stands for documented schemas.
/// Alternatives equal to `eager` are left out; without any, `eager` is
/// returned as it is.
pub(crate) fn with_alternatives(
    mut eager: Value,
    names: &BTreeSet<String>,
    build: impl Fn(&BTreeSet<String>) -> Value,
) -> Value {
    if names.is_empty() || names.len() > MAX_NAMES {
        return eager;
    }
    let all: Vec<&String> = names.iter().collect();
    let variants: Vec<Alternative> = (1..(1_usize << all.len()))
        .map(|mask| {
            all.iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, name)| (*name).clone())
                .collect::<BTreeSet<String>>()
        })
        .map(|names| Alternative {
            schema: build(&names),
            names,
        })
        .filter(|alternative| alternative.schema != eager)
        .collect();
    let marker = Marker {
        names: names.clone(),
        variants,
    };
    if let (false, Value::Object(map), Ok(marker)) = (
        marker.variants.is_empty(),
        &mut eager,
        serde_json::to_value(marker),
    ) {
        map.insert(KEY.into(), marker);
    }
    eager
}

/// The schema `build` makes from the mapping of `ty` by `types`, with the
/// alternatives for documented schemas of the built-in names `ty` writes
/// bare.
pub(crate) fn typed(
    types: &TypeMapper,
    ty: &TypeExpr,
    build: impl Fn(MappedType) -> Value,
) -> Value {
    let eager = build(types.map(ty));
    let names = types.shadowable_names(ty);
    with_alternatives(eager, &names, |subset| {
        build(types.shadowing(subset).map(ty))
    })
}

/// Applies `f` to `schema` and to each of its alternatives.
pub(crate) fn for_each(schema: &mut Value, mut f: impl FnMut(&mut Value)) {
    let marker = schema
        .as_object_mut()
        .and_then(|map| map.shift_remove(KEY))
        .and_then(|marker| serde_json::from_value::<Marker>(marker).ok());
    f(schema);
    if let Some(mut marker) = marker {
        for alternative in &mut marker.variants {
            f(&mut alternative.schema);
        }
        if let (Value::Object(map), Ok(marker)) = (schema, serde_json::to_value(marker)) {
            map.insert(KEY.into(), marker);
        }
    }
}

/// Replaces `map`, a schema with alternatives, by the alternative for the
/// names that `is_documented`, and removes the marker.
pub(crate) fn choose(map: &mut Map<String, Value>, is_documented: impl Fn(&str) -> bool) {
    // Keys merged into the schema after the marker keep their order.
    let Some(Ok(marker)) = map.shift_remove(KEY).map(serde_json::from_value::<Marker>) else {
        return;
    };
    let documented: BTreeSet<String> = marker
        .names
        .into_iter()
        .filter(|name| is_documented(name))
        .collect();
    let chosen = marker
        .variants
        .into_iter()
        .find(|alternative| !documented.is_empty() && alternative.names == documented);
    if let Some(Alternative {
        schema: Value::Object(schema),
        ..
    }) = chosen
    {
        *map = schema;
    }
}
