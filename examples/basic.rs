//! Basic end-to-end example for vcs-runner.
//!
//! Run from a jj or git repository: cargo run --example basic

use vcs_runner::{detect_vcs, jj_available, run_jj, run_git, RunError};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Detect what VCS manages this directory (walks ancestor dirs)
    let cwd = std::env::current_dir()?;
    let (backend, root) = match detect_vcs(&cwd) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("not in a vcs repo: {e}");
            return Ok(());
        }
    };
    println!("backend: {backend:?}");
    println!("root:    {}", root.display());

    // 2. Run a VCS command and inspect typed output
    if backend.is_jj() && jj_available() {
        let output = run_jj(&root, &["log", "-r", "@", "--no-graph", "-T", "change_id"])?;
        println!("@ change_id: {}", output.stdout_lossy().trim());
    } else if backend.has_git() {
        match run_git(&root, &["rev-parse", "HEAD"]) {
            Ok(output) => println!("HEAD sha: {}", output.stdout_lossy().trim()),
            Err(RunError::NonZeroExit { stderr, .. }) => {
                // e.g., empty repo — legitimate signal, not a panic
                println!("no HEAD yet: {}", stderr.trim());
            }
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
}
