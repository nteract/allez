//! Manual smoke test for GEN-24's ephemeral environment core (see
//! `specs/GEN-24_ephemeral_env_core/quickstart.md`). Exercises the real
//! creation flow against the checked-in local fixture channel, printing
//! the resolved location and installed packages so a human can visually
//! confirm the manual validation steps in `quickstart.md` (owner-only
//! permissions, no teardown).
//!
//! Run with: `ALLEZ_EPHEMERAL_ROOT=/tmp/allez-smoke cargo run --example ephemeral_smoke`

use allez::ephemeral::{RequestedPackages, create_ephemeral_environment};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ephemeral_channel");
    let channel_url = rattler_conda_types::Channel::try_from_directory(&fixture)?.canonical_name();
    let channels = condarc::ResolvedChannels::from_channels(vec![channel_url]);
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

    // Nothing above tore the environment down -- it stays on disk,
    // usable, indefinitely (past this process's own exit); this crate
    // exposes no way to remove it afterward.
    assert!(ready.location.exists());
    Ok(())
}
