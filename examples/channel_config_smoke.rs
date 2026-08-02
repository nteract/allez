//! Manual smoke test for GEN-23's `.condarc` channel resolution (see
//! `specs/GEN-23_condarc_package_selection/quickstart.md`). Exercises the
//! full read -> parse -> resolve pipeline against whatever `~/.condarc`
//! (if any) exists on the machine running it. There is deliberately no
//! automated test of the real, zero-argument
//! `allez::channel_config::resolve_channel_config()` entry point
//! (research.md R8) -- this example is its only, manually-run coverage.
//!
//! Run with: `cargo run --example channel_config_smoke`

use allez::ephemeral::redact_channel_url;

fn main() {
    match allez::channel_config::resolve_channel_config() {
        allez::channel_config::ChannelConfigResolution::Ready { config, fallback } => {
            if let Some(reason) = fallback {
                println!("fell back due to: {reason:?}"); // FR-017 -- visible without reading logs
            }
            println!("channel_priority: {:?}", config.channel_priority);
            println!("channels:");
            for channel in &config.channels {
                // redact_channel_url applied explicitly at this format site --
                // ResolvedChannels no longer carries a redacting Debug impl of
                // its own (research.md R15); GEN-24's own credential-redaction
                // property is preserved as a plain function call instead.
                println!("  {:?}", redact_channel_url(channel));
            }
        }
        allez::channel_config::ChannelConfigResolution::NoChannels => {
            // FR-020 -- a fully successful resolution whose allow/deny
            // filtering removed every channel; never printed as an empty list.
            println!("no usable channels after allow/deny filtering");
        }
        // `ChannelConfigResolution` is `#[non_exhaustive]` (data-model.md) --
        // this example is a separate crate (Cargo compiles `examples/` as
        // such), so a wildcard arm is required for a future, additive
        // variant, exactly like matching `condarc::ChannelPriority`
        // elsewhere in this ticket's own scope.
        _ => {}
    }
}
