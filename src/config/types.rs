/// Verbosity level controlling tracing output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    /// Suppress all output except errors
    Quiet,
    /// Normal output (info level)
    Normal,
    /// Verbose output (debug level)
    Verbose,
    /// Maximum output (trace level)
    Trace,
}

impl From<(bool, u8)> for Verbosity {
    /// Convert from (quiet_flag, verbose_count) to Verbosity.
    ///
    /// - quiet=true -> Quiet (regardless of verbose count)
    /// - verbose=0  -> Normal
    /// - verbose=1  -> Verbose
    /// - verbose=2+ -> Trace
    fn from((quiet, verbose_count): (bool, u8)) -> Self {
        if quiet {
            Verbosity::Quiet
        } else {
            match verbose_count {
                0 => Verbosity::Normal,
                1 => Verbosity::Verbose,
                _ => Verbosity::Trace,
            }
        }
    }
}

impl Verbosity {
    /// Return the tracing filter string for this verbosity level.
    pub fn as_tracing_filter(&self) -> &'static str {
        match self {
            Verbosity::Quiet => "error",
            Verbosity::Normal => "info",
            Verbosity::Verbose => "debug",
            Verbosity::Trace => "trace",
        }
    }
}

/// Application configuration (skeleton for future expansion).
#[derive(Debug, Clone)]
pub struct FluxConfig {
    pub verbosity: Verbosity,
}
