use anyhow::{Context, Result};
use clap::Parser;
use daemon::Cleanup;
use daemon::Daemonize;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;
use tracing::{event, Level};

#[derive(Parser, Debug)]
#[clap(version, about)]
struct Args {
    #[clap(flatten)]
    daemonize: Daemonize,

    /// File to write a greeting message to upon startup.
    #[clap(long)]
    pub greeting_file: Option<PathBuf>
}

fn program_name() -> String {
    std::env::args().nth(0).unwrap()
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    let Args { daemonize, greeting_file } = Args::parse();
    let mut cleanup = unsafe { daemonize.run() };

    if let Err(e) = main_loop(&mut cleanup, &greeting_file) {
        event!(Level::ERROR, "{}", e);
        Err(e)?;
    }
    Ok(())
}

fn main_loop(cleanup: &mut Cleanup, greeting_file: &Option<PathBuf>) -> Result<()> {
    let greeting = format!("{}: running as pid {}", program_name(), std::process::id());
    if let Some(ref filename) = greeting_file {
        cleanup.register_remove_file(filename)?;
        std::fs::write(filename, greeting.as_bytes())
            .with_context(|| format!("could not write {}", filename.display()))?;
    }
    eprintln!("{}", greeting);
    loop {
        sleep(Duration::from_secs(1));
    }
}
