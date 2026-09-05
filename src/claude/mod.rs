//! Everything about talking to the `claude` CLI: is it usable
//! (`preflight`), how to invoke it and parse its output (`process`), and
//! the continuation-signal trailer protocol layered on top of its prompts
//! and responses (`signal`).

pub(crate) mod preflight;
pub(crate) mod process;
pub(crate) mod signal;
