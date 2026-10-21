# 0005 – Cross-crate template wire format

## Context
Headless fragment output transports blueprints and fragments to other crates
through the vendor extensions `x-oas-forge-templates` and
`x-oas-forge-fragments`, each entry being `{ params: [...], body: <string> }`.

## Decision
The format stays readable forever. 0.2 writes the same two extensions and adds
`x-oas-forge-format: 1` next to them. Readers accept documents with and
without the marker; unknown future versions produce a diagnostic. Bodies stay
text because they are expanded before parsing (lexical stage).

## Consequences
Fragments produced by 0.1.x keep working with 0.2 consumers and vice versa.
