//! Built-in type names that a documented schema of the same name replaces.
//!
//! A field of type `Duration` gets the built-in schema, unless the crate
//! documents its own `Duration`: then it refers to that schema. Which names
//! are documented is only known when every file has been read, so the
//! built-in schema of a name written bare carries its alternative, the
//! reference `{"$ref": "$Duration"}`, under [`KEY`] where the name is
//! mapped ([`TypeMapper::marking`]), and the references stage picks one
//! with [`choose`]. What a stage does to such a schema before that, it does
//! to the alternative too ([`for_each`]): the cost of an alternative is that
//! of the one name, however large the type around it is.

use super::types::{MappedType, TypeExpr, TypeMapper};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// The marker key of a schema with an alternative (a [`Marker`]).
pub(crate) const KEY: &str = "x-oas-forge-builtin";

/// The alternative of the built-in schema of a name written bare. Only a
/// value of exactly this shape under [`KEY`] is taken for the generator's
/// marker ([`take`]).
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Marker {
    /// The bare built-in name.
    name: String,
    /// The schema for when the name stands for a documented schema
    /// (`{"$ref": "$Duration"}`, with what the stages added).
    pub(crate) schema: Value,
}

/// Takes the marker off `map`, if its [`KEY`] holds one. A key of that
/// name with another value is not the generator's (a source wrote it where
/// no check read it): it stays, for the sweep of the finished document.
fn take(map: &mut Map<String, Value>) -> Option<Marker> {
    let marker = Marker::deserialize(map.get(KEY)?).ok()?;
    map.shift_remove(KEY);
    Some(marker)
}

/// `schema`, the built-in schema of the bare `name`, with its alternative
/// for a documented schema of that name. A schema that is no object has no
/// place for the marker and is returned as it is.
pub(crate) fn mark(mut schema: Value, name: &str) -> Value {
    let marker = Marker {
        name: name.to_string(),
        schema: json!({ "$ref": format!("${name}") }),
    };
    attach(&mut schema, marker);
    schema
}

/// Takes the marker off `schema` (see [`take`]).
pub(crate) fn detach(schema: &mut Value) -> Option<Marker> {
    schema.as_object_mut().and_then(take)
}

/// Puts `marker` on `schema`, an object (any other value has no place for
/// it and loses the alternative).
pub(crate) fn attach(schema: &mut Value, marker: Marker) {
    if let (Value::Object(map), Ok(marker)) = (schema, serde_json::to_value(marker)) {
        map.insert(KEY.into(), marker);
    }
}

/// The schema `build` makes from the mapping of `ty` by `types`, with the
/// alternatives for documented schemas of the built-in names `ty` writes
/// bare: `build` is applied to the schema and, when the type itself is such
/// a name, to its alternative. An alternative equal to the schema (a
/// `serde(with)` module replaced both) is dropped.
pub(crate) fn typed(
    types: &TypeMapper,
    ty: &TypeExpr,
    build: impl Fn(MappedType) -> Value,
) -> Value {
    let MappedType {
        schema,
        required,
        nullable,
    } = types.marking().map(ty);
    let mut schema = schema;
    let marker = detach(&mut schema);
    let mut schema = build(MappedType {
        schema,
        required,
        nullable,
    });
    if let Some(mut marker) = marker {
        marker.schema = build(MappedType {
            schema: marker.schema,
            required,
            nullable,
        });
        if marker.schema != schema {
            attach(&mut schema, marker);
        }
    }
    schema
}

/// Applies `f` to `schema` and to its alternative.
pub(crate) fn for_each(schema: &mut Value, mut f: impl FnMut(&mut Value)) {
    let marker = detach(schema);
    f(schema);
    if let Some(mut marker) = marker {
        f(&mut marker.schema);
        attach(schema, marker);
    }
}

/// Replaces `map`, a schema with an alternative, by the alternative when
/// its name `is_documented`, and removes the marker (see [`take`]).
pub(crate) fn choose(map: &mut Map<String, Value>, is_documented: impl Fn(&str) -> bool) {
    // Keys merged into the schema after the marker keep their order.
    let Some(marker) = take(map) else {
        return;
    };
    if let (true, Value::Object(schema)) = (is_documented(&marker.name), marker.schema) {
        *map = schema;
    }
}
