//! Command line interface for `docent`.

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::config::{self, ContextMode, InitReport};
use crate::context;
use crate::error::DocentError;
use crate::profile::{Profile, ProfileInfo, ProfileStore};
use crate::prompt::{self, EffectiveSettings};
use crate::provider::{self, Completion};
use crate::request::Request;
use crate::source::{self, LineRange, NoPrompter, SelectionArg, TerminalPrompter};

const AFTER_HELP: &str = "Examples:
  docent explain notes.md --lines 12:20
  docent ask notes.md --lines 12:20 --question \"Why does this work?\"
  docent translate notes.md --lines 12:20 --to French
  docent init

Machine interface:
  docent run --profile explain < request.json";

/// Command line arguments.
#[derive(Debug, Parser)]
#[command(
    name = "docent",
    version,
    about = "Context-aware AI explanations for anything you read",
    after_help = AFTER_HELP
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

    /// Command to run. With no command, prints a short getting-started message.
    #[command(subcommand)]
    pub command: Option<Command>,
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
    /// Explain a passage.
    Explain(HumanArgs),
    /// Ask a question about a passage.
    Ask(HumanArgs),
    /// Translate a passage.
    Translate(HumanArgs),
    /// Run a custom profile.
    Do(DoArgs),
    /// Run a JSON request from stdin (machine interface).
    Run(RunArgs),
    /// Print the resolved config directory.
    ConfigPath,
}

/// Arguments shared by the terminal-facing commands.
#[derive(Debug, Args)]
pub struct HumanArgs {
    /// Document file. Use - for stdin.
    #[arg(value_name = "FILE")]
    pub file: Option<PathBuf>,

    /// Selection options.
    #[command(flatten)]
    pub selection: SelectionArgs,

    /// Execution options.
    #[command(flatten)]
    pub common: CommonArgs,
}

/// Arguments for `docent do`.
#[derive(Debug, Args)]
pub struct DoArgs {
    /// Profile name.
    #[arg(value_name = "PROFILE")]
    pub profile: String,

    /// Document file. Use - for stdin.
    #[arg(value_name = "FILE")]
    pub file: Option<PathBuf>,

    /// Selection options.
    #[command(flatten)]
    pub selection: SelectionArgs,

    /// Execution options.
    #[command(flatten)]
    pub common: CommonArgs,
}

/// Options shared by `explain`, `ask`, `translate`, and `do`.
#[derive(Debug, Args)]
pub struct CommonArgs {
    /// Question for profiles that require one.
    #[arg(long)]
    pub question: Option<String>,

    /// Do not stream; print the complete answer at once.
    #[arg(long)]
    pub no_stream: bool,

    /// Context mode override.
    #[arg(long, value_enum)]
    pub context_mode: Option<ContextMode>,

    /// Translation target language.
    #[arg(long, visible_alias = "to", value_name = "LANG")]
    pub target_language: Option<String>,
}

/// How the terminal selected a passage.
#[derive(Debug, Args)]
#[group(id = "selection", multiple = false)]
pub struct SelectionArgs {
    /// Line range, 1-based and inclusive, for example 12:20.
    #[arg(long, value_parser = parse_line_range, value_name = "START:END")]
    pub lines: Option<LineRange>,

    /// Single line number.
    #[arg(long, value_parser = parse_line_number_value, value_name = "N")]
    pub line: Option<usize>,

    /// Selection text.
    #[arg(long, value_name = "TEXT")]
    pub text: Option<String>,

    /// Find the selection by exact phrase.
    #[arg(long, value_name = "PHRASE")]
    pub select: Option<String>,
}

impl SelectionArgs {
    fn into_arg(self) -> Option<SelectionArg> {
        if let Some(lines) = self.lines {
            Some(SelectionArg::Lines(lines))
        } else if let Some(line) = self.line {
            Some(SelectionArg::Lines(LineRange::single(line)))
        } else if let Some(text) = self.text {
            Some(SelectionArg::Text(text))
        } else {
            self.select.map(SelectionArg::Phrase)
        }
    }
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
    #[arg(long, visible_alias = "to")]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    PlainStreaming,
    PlainBuffered,
    Json,
    Ndjson,
    PrintPrompt,
}

/// Parses arguments, dispatches the command, and reports errors.
pub fn run() -> Result<(), DocentError> {
    let cli = Cli::parse();
    let Some(command) = cli.command else {
        print_getting_started();
        return Ok(());
    };
    let config_dir = config::config_dir_from_env(cli.config_dir)?;

    match command {
        Command::Init { force } => {
            let report = config::init(&config_dir, force)?;
            print_init_report(&report);
        }
        Command::Profiles { json } => run_profiles(&config_dir, json)?,
        Command::Explain(args) => run_human(&config_dir, "explain", args)?,
        Command::Ask(args) => run_human(&config_dir, "ask", args)?,
        Command::Translate(args) => run_human(&config_dir, "translate", args)?,
        Command::Do(args) => run_do(&config_dir, args)?,
        Command::Run(args) => run_request(&config_dir, args)?,
        Command::ConfigPath => println!("{}", config_dir.display()),
    }

    Ok(())
}

fn print_getting_started() {
    println!("docent: contextual explanations for what you read");
    println!();
    println!("Try:");
    println!("  docent explain notes.md --lines 12:20");
    println!("  docent ask notes.md --lines 12:20 --question \"Why does this work?\"");
    println!("  docent translate notes.md --lines 12:20 --to French");
    println!("  docent init");
    println!();
    println!("Run `docent --help` for all commands and options.");
}

fn run_human(config_dir: &Path, profile_name: &str, args: HumanArgs) -> Result<(), DocentError> {
    let HumanArgs {
        file,
        selection,
        common,
    } = args;
    let CommonArgs {
        question,
        no_stream,
        context_mode,
        target_language,
    } = common;

    let config = config::ensure_initialized(config_dir)?;
    let profile = ProfileStore::new(config::profiles_dir(config_dir)).load(profile_name)?;
    let settings = resolve_settings(&config, &profile, context_mode, target_language)?;
    let request = resolve_human_request(&profile, file.as_deref(), selection, question)?;
    let output = if no_stream {
        OutputMode::PlainBuffered
    } else {
        OutputMode::PlainStreaming
    };

    execute(&config, &profile, &settings, request, profile_name, output)
}

fn run_do(config_dir: &Path, args: DoArgs) -> Result<(), DocentError> {
    let DoArgs {
        profile,
        file,
        selection,
        common,
    } = args;
    run_human(
        config_dir,
        &profile,
        HumanArgs {
            file,
            selection,
            common,
        },
    )
}

fn resolve_settings(
    config: &config::Config,
    profile: &Profile,
    context_mode: Option<ContextMode>,
    target_language: Option<String>,
) -> Result<EffectiveSettings, DocentError> {
    prompt::EffectiveSettings::resolve(
        config,
        profile,
        &prompt::Overrides {
            context_mode,
            target_language,
        },
    )
}

fn resolve_human_request(
    profile: &Profile,
    file: Option<&Path>,
    selection: SelectionArgs,
    question: Option<String>,
) -> Result<Request, DocentError> {
    let stdin_is_terminal = std::io::stdin().is_terminal();
    let options = source::RequestOptions {
        file,
        selection: selection.into_arg(),
        question,
        requires: &profile.requires,
    };

    if stdin_is_terminal {
        let mut prompter = TerminalPrompter;
        let mut empty = std::io::empty();
        source::resolve_request(options, &mut prompter, true, &mut empty)
    } else {
        let mut stdin = std::io::stdin().lock();
        let mut prompter = NoPrompter;
        source::resolve_request(options, &mut prompter, false, &mut stdin)
    }
}

fn run_request(config_dir: &Path, args: RunArgs) -> Result<(), DocentError> {
    let RunArgs {
        profile: profile_name,
        question,
        target_language,
        context_mode,
        json,
        stream,
        print_prompt,
    } = args;

    let config = config::ensure_initialized(config_dir)?;
    let profile = ProfileStore::new(config::profiles_dir(config_dir)).load(&profile_name)?;
    let mut request = read_request()?;
    if question.is_some() {
        request.question = question;
    }
    let settings = resolve_settings(&config, &profile, context_mode, target_language)?;

    let output = if print_prompt {
        OutputMode::PrintPrompt
    } else if stream {
        OutputMode::Ndjson
    } else if json {
        OutputMode::Json
    } else {
        OutputMode::PlainBuffered
    };

    execute(&config, &profile, &settings, request, &profile_name, output)
}

fn execute(
    config: &config::Config,
    profile: &Profile,
    settings: &EffectiveSettings,
    request: Request,
    profile_name: &str,
    output: OutputMode,
) -> Result<(), DocentError> {
    let context = context::build(
        &request,
        context::ContextOptions {
            mode: settings.context_mode,
            max_chars: config.context.max_chars,
        },
    );
    let rendered = prompt::render(profile, &request, &context, settings)?;

    match output {
        OutputMode::PrintPrompt => {
            let output = PromptOutput {
                profile: profile_name,
                model: &settings.model,
                system: &rendered.system,
                user: &rendered.user,
                context_chars: context.chars,
                context_truncated: context.truncated,
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        OutputMode::PlainStreaming => stream_plain(config, settings, &rendered)?,
        OutputMode::PlainBuffered => {
            let completion = provider::complete(config, settings, &rendered)?;
            print_completion(&completion.text);
        }
        OutputMode::Json => {
            let completion = provider::complete(config, settings, &rendered)?;
            println!("{}", completion_json(&completion)?);
        }
        OutputMode::Ndjson => stream_ndjson(config, settings, &rendered)?,
    }

    Ok(())
}

fn print_completion(text: &str) {
    print!("{text}");
    if !text.ends_with('\n') {
        println!();
    }
}

fn completion_json(completion: &Completion) -> Result<String, DocentError> {
    Ok(serde_json::to_string(completion)?)
}

fn stream_plain(
    config: &config::Config,
    settings: &EffectiveSettings,
    rendered: &prompt::Prompt,
) -> Result<(), DocentError> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let mut wrote = false;
    let mut ends_with_newline = true;

    let result = provider::complete_streaming(config, settings, rendered, &mut |delta| {
        if delta.is_empty() {
            return Ok(());
        }
        writer
            .write_all(delta.as_bytes())
            .map_err(DocentError::WriteStdout)?;
        writer.flush().map_err(DocentError::WriteStdout)?;
        wrote = true;
        ends_with_newline = delta.ends_with('\n');
        Ok(())
    });

    match result {
        Ok(completion) => {
            if !wrote {
                writer
                    .write_all(completion.text.as_bytes())
                    .map_err(DocentError::WriteStdout)?;
                ends_with_newline = completion.text.ends_with('\n');
            }
            if !ends_with_newline {
                writer.write_all(b"\n").map_err(DocentError::WriteStdout)?;
            }
            Ok(())
        }
        Err(error) => {
            if wrote && !ends_with_newline {
                let _ = writer.write_all(b"\n");
            }
            Err(error)
        }
    }
}

fn stream_ndjson(
    config: &config::Config,
    settings: &EffectiveSettings,
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

fn parse_line_range(value: &str) -> Result<LineRange, String> {
    value.parse()
}

fn parse_line_number_value(value: &str) -> Result<usize, String> {
    let number = value
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("'{value}' is not a line number"))?;
    if number == 0 {
        return Err("line numbers start at 1".to_string());
    }
    Ok(number)
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
            match &profile.description {
                Some(description) if !description.is_empty() => {
                    println!("{}: {description}", profile.name);
                }
                _ => println!("{}", profile.name),
            }
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
