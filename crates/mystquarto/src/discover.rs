//! The conversion direction and its per-direction names. Which files a run
//! touches is decided by `mystquarto_core::closure`.

/// Conversion direction: fixed by which binary, or which `mystquarto`
/// subcommand, is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    MystToQuarto,
    QuartoToMyst,
}

impl Direction {
    /// The content-file extension (without the leading dot) this direction
    /// reads from.
    #[must_use]
    pub fn source_extension(self) -> &'static str {
        match self {
            Direction::MystToQuarto => "md",
            Direction::QuartoToMyst => "qmd",
        }
    }

    /// The content-file extension this direction writes to.
    #[must_use]
    pub fn target_extension(self) -> &'static str {
        match self {
            Direction::MystToQuarto => "qmd",
            Direction::QuartoToMyst => "md",
        }
    }

    /// The config file name this direction reads from.
    #[must_use]
    pub fn source_config_name(self) -> &'static str {
        match self {
            Direction::MystToQuarto => "myst.yml",
            Direction::QuartoToMyst => "_quarto.yml",
        }
    }

    /// The config file name this direction writes to.
    #[must_use]
    pub fn target_config_name(self) -> &'static str {
        match self {
            Direction::MystToQuarto => "_quarto.yml",
            Direction::QuartoToMyst => "myst.yml",
        }
    }
}
