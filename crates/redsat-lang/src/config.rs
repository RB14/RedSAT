//! Compiler options and the `redsat.toml` project file.
//!
//! Options are resolved from layers, lowest precedence first: built-in defaults, what `strict`
//! implies, keys set explicitly in `redsat.toml`, and finally command-line flags. A strict
//! project can therefore still opt out of a single check by setting its key explicitly.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;

/// The name of the project configuration file.
pub const CONFIG_FILE_NAME: &str = "redsat.toml";

/// Which variable declarations need a type annotation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExplicitTypes {
    /// Types may always be inferred.
    #[default]
    Off,
    /// Every unknown (`var` without an initializer) needs a type, like TypeScript's
    /// `noImplicitAny`.
    Unknowns,
    /// Every `var` needs a type, even when it has an initializer.
    All,
}

/// What to do with integer unknowns whose domain cannot be inferred.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnboundedInts {
    /// Search with growing bounds (iterative deepening), with a warning.
    #[default]
    Deepen,
    /// Reject the program.
    Error,
}

/// The fully resolved options the compiler runs with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompilerOptions {
    /// Whether strict mode is on (informational; its effects are already resolved below).
    pub strict: bool,
    /// See [`ExplicitTypes`].
    pub explicit_types: ExplicitTypes,
    /// See [`UnboundedInts`].
    pub unbounded_ints: UnboundedInts,
    /// The first magnitude, in bits, tried for unbounded integers.
    pub fallback_int_bits: u32,
    /// Iterative deepening stops beyond this many bits.
    pub max_int_bits: u32,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self::resolve(&OptionLayer::default(), &OptionLayer::default())
            .expect("the defaults are valid")
    }
}

/// One layer of option settings, as read from `redsat.toml` or the command line. Unset keys
/// fall through to lower layers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct OptionLayer {
    /// `strict = true` turns on every strict check.
    pub strict: Option<bool>,
    /// `explicit-types = "off" | "unknowns" | "all"`.
    pub explicit_types: Option<ExplicitTypes>,
    /// `unbounded-ints = "deepen" | "error"`.
    pub unbounded_ints: Option<UnboundedInts>,
    /// `fallback-int-bits = 32`.
    pub fallback_int_bits: Option<u32>,
    /// `max-int-bits = 256`.
    pub max_int_bits: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    #[serde(default)]
    compiler: OptionLayer,
}

impl CompilerOptions {
    /// Combines a configuration-file layer with a command-line layer (which wins).
    ///
    /// # Errors
    ///
    /// Describes inconsistent numeric settings.
    pub fn resolve(file: &OptionLayer, cli: &OptionLayer) -> Result<Self, String> {
        let strict = cli.strict.or(file.strict).unwrap_or(false);
        let options = Self {
            strict,
            explicit_types: cli
                .explicit_types
                .or(file.explicit_types)
                .unwrap_or(if strict {
                    ExplicitTypes::Unknowns
                } else {
                    ExplicitTypes::Off
                }),
            unbounded_ints: cli
                .unbounded_ints
                .or(file.unbounded_ints)
                .unwrap_or(if strict {
                    UnboundedInts::Error
                } else {
                    UnboundedInts::Deepen
                }),
            fallback_int_bits: cli
                .fallback_int_bits
                .or(file.fallback_int_bits)
                .unwrap_or(32),
            max_int_bits: cli.max_int_bits.or(file.max_int_bits).unwrap_or(256),
        };
        if options.fallback_int_bits == 0 || options.fallback_int_bits > options.max_int_bits {
            return Err(format!(
                "fallback-int-bits ({}) must be between 1 and max-int-bits ({})",
                options.fallback_int_bits, options.max_int_bits
            ));
        }
        if options.max_int_bits > 65_536 {
            return Err(format!(
                "max-int-bits ({}) must be at most 65536",
                options.max_int_bits
            ));
        }
        Ok(options)
    }
}

/// Finds the `redsat.toml` that applies to programs in `directory`: the first one found in
/// `directory` or any of its ancestors.
#[must_use]
pub fn find_config(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .map(|dir| dir.join(CONFIG_FILE_NAME))
        .find(|path| path.is_file())
}

/// Reads the `[compiler]` settings of a `redsat.toml`.
///
/// # Errors
///
/// Describes unreadable files, TOML syntax errors and unknown keys.
pub fn load_config(path: &Path) -> Result<OptionLayer, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    parse_config(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Parses the text of a `redsat.toml`.
///
/// # Errors
///
/// Describes TOML syntax errors and unknown keys.
pub fn parse_config(text: &str) -> Result<OptionLayer, String> {
    toml::from_str::<ConfigFile>(text)
        .map(|file| file.compiler)
        .map_err(|error| error.to_string())
}

impl FromStr for ExplicitTypes {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        match text {
            "off" => Ok(Self::Off),
            "unknowns" => Ok(Self::Unknowns),
            "all" => Ok(Self::All),
            _ => Err(format!("expected off, unknowns or all, found `{text}`")),
        }
    }
}

impl FromStr for UnboundedInts {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        match text {
            "deepen" => Ok(Self::Deepen),
            "error" => Ok(Self::Error),
            _ => Err(format!("expected deepen or error, found `{text}`")),
        }
    }
}

impl fmt::Display for CompilerOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "strict = {}, explicit-types = {:?}, unbounded-ints = {:?}, fallback-int-bits = {}, \
             max-int-bits = {}",
            self.strict,
            self.explicit_types,
            self.unbounded_ints,
            self.fallback_int_bits,
            self.max_int_bits
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_permissive() {
        let options = CompilerOptions::default();
        assert_eq!(options.explicit_types, ExplicitTypes::Off);
        assert_eq!(options.unbounded_ints, UnboundedInts::Deepen);
        assert_eq!((options.fallback_int_bits, options.max_int_bits), (32, 256));
    }

    #[test]
    fn strict_turns_on_the_family_but_explicit_keys_win() {
        let file =
            parse_config("[compiler]\nstrict = true\nunbounded-ints = \"deepen\"\n").unwrap();
        let options = CompilerOptions::resolve(&file, &OptionLayer::default()).unwrap();
        assert_eq!(options.explicit_types, ExplicitTypes::Unknowns);
        assert_eq!(
            options.unbounded_ints,
            UnboundedInts::Deepen,
            "explicit key beats strict"
        );
    }

    #[test]
    fn command_line_beats_the_file() {
        let file =
            parse_config("[compiler]\nexplicit-types = \"all\"\nmax-int-bits = 64\n").unwrap();
        let cli = OptionLayer {
            explicit_types: Some(ExplicitTypes::Off),
            ..OptionLayer::default()
        };
        let options = CompilerOptions::resolve(&file, &cli).unwrap();
        assert_eq!(options.explicit_types, ExplicitTypes::Off);
        assert_eq!(options.max_int_bits, 64);
    }

    #[test]
    fn rejects_unknown_keys_and_bad_values() {
        assert!(
            parse_config("[compiler]\nstrcit = true\n")
                .unwrap_err()
                .contains("strcit")
        );
        assert!(parse_config("[compiler]\nexplicit-types = \"some\"\n").is_err());
        let bad = OptionLayer {
            fallback_int_bits: Some(512),
            ..OptionLayer::default()
        };
        assert!(CompilerOptions::resolve(&bad, &OptionLayer::default()).is_err());
    }

    #[test]
    fn an_empty_file_is_fine() {
        assert_eq!(parse_config("").unwrap(), OptionLayer::default());
    }
}
