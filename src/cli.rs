//! Command line interface for `docent`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::config::{self, ContextMode, InitReport};
use crate::context;
use crate::error::DocentError;
use crate::profile::{ProfileInfo, ProfileStore};
use crate::prompt;
use crate::provider;
use crate::request::Request;

/// Command line arguments.
#[derive(Debug, Parser)]
#[command(
    name = "docent",
    version,
    about = "Context-aware AI explanations for anything you read"
)]
pub struct Cli {
    /// Config directory. Defaults to the platform config directory.
    #[arg(
        long,
        global = true,
        env = config::CONFIG_DIR_ENV,
        value_name = "PATH"
    )]
    pub config_dir: Option<PathBuf>,

    /// Command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Supported subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create the config directory and default profiles.
    Init {
        /// Overwrite existing default files.
        #[arg(long)]
        force: bool,
    },
    /// List available profiles.
    Profiles {
        /// Print profile metadata as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Explain, ask about, or translate a selected passage.
    Run(RunArgs),
    /// Print the resolved config directory.
    ConfigPath,
}

/// Arguments for `docent run`.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Profile to use.
    #[arg(long, default_value = "explain")]
    pub profile: String,
    /// Question for profiles that require one.
    #[arg(long)]
    pub question: Option<String>,
    /// Translation target language.
    #[arg(long)]
    pub target_language: Option<String>,
    /// Context mode override.
    #[arg(long, value_enum)]
    pub context_mode: Option<ContextMode>,
    /// Print the completion as JSON.
    #[arg(long, conflicts_with = "stream")]
    pub json: bool,
    /// Stream newline-delimited JSON events.
    #[arg(long)]
    pub stream: bool,
    /// Print the assembled prompt and exit without calling the provider.
    #[arg(long)]
    pub print_prompt: bool,
}

/// Parses arguments, dispatches the command, and reports errors.
pub fn run() -> Result<(), DocentError> {
    let cli = Cli::parse();
    let config_dir = config::config_dir_from_env(cli.config_dir)?;

    match cli.command {
        Command::Init { force } => {
            let report = config::init(&config_dir, force)?;
            print_init_report(&report);
        }
        Command::Profiles { json } => run_profiles(&config_dir, json)?,
        Command::Run(args) => run_request(&config_dir, args)?,
        Command::ConfigPath => println!("{}", config_dir.display()),
    }

    Ok(())
}

fn run_request(config_dir: &Path, args: RunArgs) -> Result<(), DocentError> {
    let config = config::ensure_initialized(config_dir)?;
    let store = ProfileStore::new(config::profiles_dir(config_dir));
    let profile = store.load(&args.profile)?;

    let mut request = read_request()?;
    if args.question.is_some() {
        request.question = args.question;
    }

    let overrides = prompt::Overrides {
        context_mode: args.context_mode,
        target_language: args.target_language,
    };
    let settings = prompt::EffectiveSettings::resolve(&config, &profile, &overrides)?;
    let context = context::build(
        &request,
        context::ContextOptions {
            mode: settings.context_mode,
            max_chars: config.context.max_chars,
        },
    );
    let rendered = prompt::render(&profile, &request, &context, &settings)?;

    if args.print_prompt {
        let output = PromptOutput {
            profile: &args.profile,
            model: &settings.model,
            system: &rendered.system,
            user: &rendered.user,
            context_chars: context.chars,
            context_truncated: context.truncated,
        };
        println!("{}", serde_json::to_string(&output)?);
        return Ok(());
    }

    if args.stream {
        return stream_completion(&config, &settings, &rendered);
    }

    let completion = provider::complete(&config, &settings, &rendered)?;
    if args.json {
        println!("{}", serde_json::to_string(&completion)?);
    } else {
        print!("{}", completion.text);
        if !completion.text.ends_with('\n') {
            println!();
        }
    }

    Ok(())
}

fn stream_completion(
    config: &config::Config,
    settings: &prompt::EffectiveSettings,
    rendered: &prompt::Prompt,
) -> Result<(), DocentError> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    let result = provider::complete_streaming(config, settings, rendered, &mut |delta| {
        write_stream_event(
            &mut writer,
            &provider::StreamEvent::Delta {
                text: delta.to_string(),
            },
        )
    });

    match result {
        Ok(completion) => {
            write_stream_event(
                &mut writer,
                &provider::StreamEvent::Done {
                    model: completion.model,
                    usage: completion.usage,
                },
            )?;
            Ok(())
        }
        Err(error) => {
            let _ = write_stream_event(
                &mut writer,
                &provider::StreamEvent::Error {
                    message: error.to_string(),
                },
            );
            Err(error)
        }
    }
}

fn write_stream_event(
    writer: &mut impl Write,
    event: &provider::StreamEvent,
) -> Result<(), DocentError> {
    serde_json::to_writer(&mut *writer, event)
        .map_err(|error| DocentError::WriteStdout(std::io::Error::other(error)))?;
    writer.write_all(b"\n").map_err(DocentError::WriteStdout)?;
    writer.flush().map_err(DocentError::WriteStdout)?;
    Ok(())
}

fn read_request() -> Result<Request, DocentError> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(DocentError::ReadStdin)?;

    if input.trim().is_empty() {
        return Err(DocentError::InvalidRequest(
            "expected a JSON request on stdin".to_string(),
        ));
    }

    serde_json::from_str(&input).map_err(|error| {
        DocentError::InvalidRequest(format!("failed to parse request JSON: {error}"))
    })
}

#[derive(Serialize)]
struct PromptOutput<'a> {
    profile: &'a str,
    model: &'a str,
    system: &'a str,
    user: &'a str,
    context_chars: usize,
    context_truncated: bool,
}

#[derive(Serialize)]
struct ProfilesOutput<'a> {
    profiles: &'a [ProfileInfo],
}

fn run_profiles(config_dir: &Path, json: bool) -> Result<(), DocentError> {
    config::ensure_initialized(config_dir)?;
    let store = ProfileStore::new(config::profiles_dir(config_dir));
    let catalog = store.discover()?;

    for warning in &catalog.warnings {
        log::warn!("{warning}");
    }

    if json {
        let output = ProfilesOutput {
            profiles: &catalog.profiles,
        };
        println!("{}", serde_json::to_string(&output)?);
    } else {
        for profile in &catalog.profiles {
            println!("{}", profile.name);
        }
    }

    Ok(())
}

fn print_init_report(report: &InitReport) {
    println!("initialized {}", report.config_dir.display());
    for path in &report.created {
        println!("  created {}", path.display());
    }
    for path in &report.skipped {
        println!(
            "  skipped {} (already exists; use --force to overwrite)",
            path.display()
        );
    }
}
