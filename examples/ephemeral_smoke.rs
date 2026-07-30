//! Manual smoke test for GEN-24's ephemeral environment core (see
//! `specs/GEN-24_ephemeral_env_core/quickstart.md`). Exercises the real
//! create -> reap flow against the checked-in local fixture channel,
//! printing the resolved location and installed packages so a human can
//! visually confirm the manual validation steps in `quickstart.md`
//! (owner-only permissions, explicit reap).
//!
//! Run with: `ALLEZ_EPHEMERAL_ROOT=/tmp/allez-smoke cargo run --example ephemeral_smoke`

use allez::ephemeral::{
    ChannelConfig, RequestedPackages, create_ephemeral_environment, reap_ephemeral_environments,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ephemeral_channel");
    let channel_url = rattler_conda_types::Channel::try_from_directory(&fixture)?.canonical_name();
    let channels = ChannelConfig::from_urls(vec![channel_url]);
    let ready = create_ephemeral_environment(
        RequestedPackages::Explicit(vec![allez::ephemeral::PackageSpec::parse("fixture-probe")?]),
        channels,
        None,
    )
    .await?;
    println!("environment ready at {}", ready.location.display());
    for pkg in &ready.installed_packages {
        println!("  {} {}", pkg.name, pkg.version);
    }

    // Nothing above tore the environment down automatically -- it stays on
    // disk, usable, until reaped explicitly.
    assert!(ready.location.exists());

    let outcomes = reap_ephemeral_environments()?;
    println!("reaped {} environment(s)", outcomes.len());
    assert!(!ready.location.exists());
    println!("environment removed: {}", ready.location.display());
    Ok(())
}
