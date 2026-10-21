//! The summary and description of a route, webhook or channel block: its
//! plain lines and the `@summary`, `@description` and `@internal`
//! directives.

/// What the plain lines of a block continue: set by each directive.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Text {
    /// The block's own text: the first plain line is the summary, the others
    /// are the description (unless `@description` replaces them).
    #[default]
    Free,
    /// A `@description`: its lines are text, never YAML.
    Description,
    /// An `@internal` note: left out of the document.
    Internal,
}

impl Text {
    /// What the plain lines after the directive `name` continue.
    pub(super) fn after(name: &str) -> Self {
        match name {
            "description" => Self::Description,
            "internal" => Self::Internal,
            _ => Self::Free,
        }
    }
}

/// The summary and description collected from a block.
#[derive(Debug, Default)]
pub(super) struct Prose {
    /// The text of `@summary`.
    summary: Option<String>,
    /// The free plain lines, blank lines included.
    free: Vec<String>,
    /// The lines of `@description` and the plain lines that continue it;
    /// they replace the free lines as description.
    described: Option<Vec<String>>,
}

impl Prose {
    /// Reads a plain or blank line that continues `text`.
    pub(super) fn line(&mut self, text: Text, line: &str) {
        match text {
            Text::Internal => {}
            Text::Description => self.described().push(line.to_string()),
            Text::Free => self.free.push(line.to_string()),
        }
    }

    /// `@summary text`: the summary; then no plain line is.
    pub(super) fn summary(&mut self, text: &str) {
        self.summary = Some(text.to_string());
    }

    /// `@description text`; `margin` is the indentation of its line, so that
    /// the text lines up with the plain lines that continue it.
    pub(super) fn description(&mut self, margin: &str, text: &str) {
        let described = self.described();
        if !text.is_empty() {
            described.push(format!("{margin}{text}"));
        }
    }

    /// The lines of `@description`, started on first use.
    fn described(&mut self) -> &mut Vec<String> {
        self.described.get_or_insert_with(Vec::new)
    }

    /// The summary and the description, `None` where there is none. Without
    /// `@summary`, the first free line that is not blank is the summary and
    /// the free lines after it are the description; with `@summary`, all
    /// free lines are. `@description` replaces the free lines.
    pub(super) fn finish(self) -> (Option<String>, Option<String>) {
        let (summary, free) = match self.summary {
            Some(summary) => (Some(summary), &self.free[..]),
            None => match self.free.iter().position(|l| !l.trim().is_empty()) {
                Some(first) => (
                    self.free.get(first).map(|l| l.trim().to_string()),
                    self.free.get(first + 1..).unwrap_or_default(),
                ),
                None => (None, &[][..]),
            },
        };
        let lines = self.described.as_deref().unwrap_or(free);
        (summary, description_text(lines))
    }
}

/// Description lines with their shared indentation removed; leading and
/// trailing blank lines are dropped, blank lines between paragraphs kept.
fn description_text(lines: &[String]) -> Option<String> {
    let start = lines.iter().position(|l| !l.trim().is_empty())?;
    let end = lines.iter().rposition(|l| !l.trim().is_empty())?;
    let lines = lines.get(start..=end)?;
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().take_while(|c| c.is_whitespace()).count())
        .min()
        .unwrap_or(0);
    Some(
        lines
            .iter()
            .map(|l| l.chars().skip(indent).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The indentation of `line`.
pub(super) fn margin(line: &str) -> &str {
    line.get(..line.len() - line.trim_start().len())
        .unwrap_or_default()
}
