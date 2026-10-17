use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "triage",
    version,
    about,
    help_template = "{name} {version}\n{about-with-newline}\n{usage-heading} {usage}\n\n{all-args}{after-help}"
)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
