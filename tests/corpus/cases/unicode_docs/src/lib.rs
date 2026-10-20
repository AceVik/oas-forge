//! @openapi
//! openapi: 3.1.0
//! info:
//!   title: Corpus
//!   version: 1.0.0

/// @openapi
pub struct Adresse {
    pub ort: String,
}

/// Kundendaten für die Größenberechnung – „ÄÖÜ“ 🚀
/// @openapi
pub struct Kunde {
    /// Wohnort des Kunden
    pub adresse: Adresse,
}

/// Größe abfragen – liefert $Kunde
/// @route GET /kunden/{id: u32 "Kunden-Nr."}
/// @return 200: $Kunde "Gefunden ✓"
pub fn kunde() {}
