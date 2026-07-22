//! `AllezError`: the fixed set of usage-error categories (FR-011, FR-017).

use std::fmt;

/// A usage error, mapped 1:1 to one of FR-017's fixed `category` strings.
///
/// Every variant maps to exactly one category via [`AllezError::category`],
/// so there is no second, hand-maintained list of category strings that can
/// drift out of sync with the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllezError {
    /// A required argument was missing or empty (e.g. an empty path).
    MissingArgument,
    /// The subcommand name was not one of the six recognized names.
    UnknownSubcommand,
    /// An unrecognized flag was supplied.
    UnknownFlag,
    /// `oneshot`/`run` always require a pass-through command; `sandbox`
    /// requires one only when `--` was present with nothing after it.
    MissingPassThroughCommand,
}

impl AllezError {
    /// The fixed FR-017 category string for this variant.
    pub fn category(&self) -> &'static str {
        match self {
            Self::MissingArgument => "missing_argument",
            Self::UnknownSubcommand => "unknown_subcommand",
            Self::UnknownFlag => "unknown_flag",
            Self::MissingPassThroughCommand => "missing_pass_through_command",
        }
    }
}

impl fmt::Display for AllezError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingArgument => write!(f, "a required argument is missing or empty"),
            Self::UnknownSubcommand => write!(f, "unrecognized subcommand"),
            Self::UnknownFlag => write!(f, "unrecognized flag"),
            Self::MissingPassThroughCommand => {
                write!(f, "a pass-through command is required after `--`")
            }
        }
    }
}

impl std::error::Error for AllezError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_argument_category_matches_fixed_string() {
        assert_eq!(AllezError::MissingArgument.category(), "missing_argument");
    }

    #[test]
    fn unknown_subcommand_category_matches_fixed_string() {
        assert_eq!(
            AllezError::UnknownSubcommand.category(),
            "unknown_subcommand"
        );
    }

    #[test]
    fn unknown_flag_category_matches_fixed_string() {
        assert_eq!(AllezError::UnknownFlag.category(), "unknown_flag");
    }

    #[test]
    fn missing_pass_through_command_category_matches_fixed_string() {
        assert_eq!(
            AllezError::MissingPassThroughCommand.category(),
            "missing_pass_through_command"
        );
    }
}
