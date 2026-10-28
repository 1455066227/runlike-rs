use std::io::Read;

use clap::{CommandFactory, Parser};
use runlike::Inspector;

/// Shows the command line necessary to run a copy of an existing Docker container.
#[derive(Parser, Debug)]
#[command(
    name = "runlike",
    version,
    about = "Shows the command line necessary to run a copy of an existing Docker container."
)]
struct Cli {
    /// Container name or id
    #[arg(value_name = "CONTAINER")]
    container: Option<String>,

    /// Do not include the container name in the output
    #[arg(long)]
    no_name: bool,

    /// Keep the automatically assigned volume id
    #[arg(long)]
    use_volume_id: bool,

    /// Pretty-print the command on multiple lines
    #[arg(short = 'p', long)]
    pretty: bool,

    /// Read `docker inspect` output from stdin instead of calling docker
    #[arg(short = 's', long)]
    stdin: bool,

    /// Do not include labels in the output
    #[arg(short = 'l', long)]
    no_labels: bool,
}

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();

    if cli.container.is_none() && !cli.stdin {
        let mut command = Cli::command();
        command
            .error(
                clap::error::ErrorKind::MissingRequiredArgument,
                "usage error: provide a container name or use --stdin",
            )
            .exit();
    }

    let mut inspector = Inspector::new(
        cli.container,
        cli.no_name,
        cli.use_volume_id,
        cli.pretty,
        cli.no_labels,
    );

    if inspector.container.is_some() {
        inspector.inspect()?;
    } else {
        let mut raw_json = String::new();
        std::io::stdin()
            .read_to_string(&mut raw_json)
            .map_err(|e| format!("could not read stdin: {e}"))?;
        inspector.set_container_facts(&raw_json)?;
    }

    println!("{}", inspector.format_cli()?);
    Ok(())
}
