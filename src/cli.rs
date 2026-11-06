//! Command line interface for `docent`.

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::config::{self, ContextMode, InitReport};
use crate::context;
use crate::error::DocentError;
use crate::history;
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

    /// Source recorded in the history database, for example cli, nvim, or a plugin name.
    #[arg(
        long,
        global = true,
        value_name = "NAME",
        default_value = "cli",
        value_parser = parse_source
    )]
    pub source: String,

    /// Do not record this run in the history database.
    #[arg(long, global = true)]
    pub no_history: bool,

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

impl OutputMode {
    /// Stable identifier used in the history database.
    fn as_str(self) -> &'static str {
        match self {
            Self::PlainStreaming => "plain_stream",
            Self::PlainBuffered => "plain",
            Self::Json => "json",
            Self::Ndjson => "ndjson",
            Self::PrintPrompt => "print_prompt",
        }
    }
}

/// Parses arguments, dispatches the command, and reports errors.
pub fn run() -> Result<(), DocentError> {
    let Cli {
        config_dir: explicit_config_dir,
        source,
        no_history,
        command,
    } = Cli::parse();
    let Some(command) = command else {
        print_getting_started();
        return Ok(());
    };
    let config_dir = config::config_dir_from_env(explicit_config_dir)?;
    let record_history = !no_history;

    match command {
        Command::Init { force } => {
            let report = config::init(&config_dir, force)?;
            print_init_report(&report);
        }
        Command::Profiles { json } => run_profiles(&config_dir, json)?,
        Command::Explain(args) => {
            run_human(
                &config_dir,
                "explain",
                "explain",
                args,
                &source,
                record_history,
            )?;
        }
        Command::Ask(args) => {
            run_human(&config_dir, "ask", "ask", args, &source, record_history)?;
        }
        Command::Translate(args) => {
            run_human(
                &config_dir,
                "translate",
                "translate",
                args,
                &source,
                record_history,
            )?;
        }
        Command::Do(args) => run_do(&config_dir, args, &source, record_history)?,
        Command::Run(args) => run_request(&config_dir, args, &source, record_history)?,
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

fn run_human(
    config_dir: &Path,
    command: &str,
    profile_name: &str,
    args: HumanArgs,
    source: &str,
    record_history: bool,
) -> Result<(), DocentError> {
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

    execute(
        config_dir,
        &config,
        &profile,
        &settings,
        request,
        command,
        profile_name,
        source,
        record_history,
        output,
    )
}

fn run_do(
    config_dir: &Path,
    args: DoArgs,
    source: &str,
    record_history: bool,
) -> Result<(), DocentError> {
    let DoArgs {
        profile,
        file,
        selection,
        common,
    } = args;
    run_human(
        config_dir,
        "do",
        &profile,
        HumanArgs {
            file,
            selection,
            common,
        },
        source,
        record_history,
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

fn run_request(
    config_dir: &Path,
    args: RunArgs,
    source: &str,
    record_history: bool,
) -> Result<(), DocentError> {
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

    execute(
        config_dir,
        &config,
        &profile,
        &settings,
        request,
        "run",
        &profile_name,
        source,
        record_history,
        output,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute(
    config_dir: &Path,
    config: &config::Config,
    profile: &Profile,
    settings: &EffectiveSettings,
    request: Request,
    command: &str,
    profile_name: &str,
    source: &str,
    record_history: bool,
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

    if output == OutputMode::PrintPrompt {
        let output = PromptOutput {
            profile: profile_name,
            model: &settings.model,
            system: &rendered.system,
            user: &rendered.user,
            context_chars: context.chars,
            context_truncated: context.truncated,
        };
        println!("{}", serde_json::to_string(&output)?);
        return Ok(());
    }

    let created_at_ms = history::now_ms();
    let started = Instant::now();
    let mut streamed = String::new();

    let outcome = match output {
        OutputMode::PrintPrompt => unreachable!("print prompt returns before the provider call"),
        OutputMode::PlainStreaming => stream_plain(config, settings, &rendered, &mut streamed),
        OutputMode::PlainBuffered => provider::complete(config, settings, &rendered),
        OutputMode::Json => provider::complete(config, settings, &rendered),
        OutputMode::Ndjson => stream_ndjson(config, settings, &rendered, &mut streamed),
    };

    let duration_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;

    if record_history {
        let run = build_history_run(
            config,
            &request,
            &context,
            &rendered,
            settings,
            command,
            profile_name,
            source,
            output,
            created_at_ms,
            duration_ms,
            &outcome,
            &streamed,
        );
        if let Err(error) = history::write(config, config_dir, &run) {
            log::warn!("could not record run history: {error}");
        }
    }

    match (&output, &outcome) {
        (OutputMode::PlainBuffered, Ok(completion)) => print_completion(&completion.text),
        (OutputMode::Json, Ok(completion)) => {
            println!("{}", completion_json(completion)?);
        }
        _ => {}
    }

    outcome.map(|_| ())
}

#[allow(clippy::too_many_arguments)]
fn build_history_run(
    config: &config::Config,
    request: &Request,
    context: &context::ContextResult,
    rendered: &prompt::Prompt,
    settings: &EffectiveSettings,
    command: &str,
    profile_name: &str,
    source: &str,
    output_mode: OutputMode,
    created_at_ms: i64,
    duration_ms: i64,
    outcome: &Result<Completion, DocentError>,
    streamed: &str,
) -> history::NewRun {
    let (status, error) = match outcome {
        Ok(_) => ("success".to_string(), None),
        Err(error) => ("error".to_string(), Some(error.to_string())),
    };
    let (model, usage) = match outcome {
        Ok(completion) => (completion.model.as_deref(), completion.usage.as_ref()),
        Err(_) => (None, None),
    };
    let output = match outcome {
        Ok(completion) => completion.text.clone(),
        Err(_) => streamed.to_string(),
    };

    history::NewRun {
        created_at_ms,
        duration_ms,
        command: command.to_string(),
        profile: profile_name.to_string(),
        source: source.to_string(),
        output_mode: output_mode.as_str().to_string(),
        status,
        error,
        file: request.file.clone(),
        selection: request.selection.clone(),
        question: request.question.clone(),
        document_bytes: request
            .document
            .as_ref()
            .map(|document| document.len() as i64),
        byte_start: request.byte_start.map(|value| value as i64),
        byte_end: request.byte_end.map(|value| value as i64),
        context_mode: settings.context_mode.as_str().to_string(),
        context_max_chars: config.context.max_chars as i64,
        context_chars: context.chars as i64,
        context_truncated: context.truncated,
        provider: config.api.kind.as_str().to_string(),
        base_url: config.api.base_url.clone(),
        model: model.unwrap_or(&settings.model).to_string(),
        temperature: f64::from(settings.temperature),
        max_tokens: i64::from(settings.max_tokens),
        timeout_secs: config.api.timeout_secs as i64,
        response_language: settings.response_language.clone(),
        target_language: settings.target_language.clone(),
        system_prompt: rendered.system.clone(),
        user_prompt: rendered.user.clone(),
        output,
        input_tokens: usage
            .and_then(|usage| usage.input_tokens)
            .map(|value| value as i64),
        output_tokens: usage
            .and_then(|usage| usage.output_tokens)
            .map(|value| value as i64),
        docent_version: env!("CARGO_PKG_VERSION").to_string(),
    }
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
    streamed: &mut String,
) -> Result<Completion, DocentError> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let mut wrote = false;
    let mut ends_with_newline = true;

    let result = provider::complete_streaming(config, settings, rendered, &mut |delta| {
        if delta.is_empty() {
            return Ok(());
        }
        streamed.push_str(delta);
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
                streamed.push_str(&completion.text);
                writer
                    .write_all(completion.text.as_bytes())
                    .map_err(DocentError::WriteStdout)?;
                ends_with_newline = completion.text.ends_with('\n');
            }
            if !ends_with_newline {
                writer.write_all(b"\n").map_err(DocentError::WriteStdout)?;
            }
            Ok(completion)
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
    streamed: &mut String,
) -> Result<Completion, DocentError> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    let result = provider::complete_streaming(config, settings, rendered, &mut |delta| {
        streamed.push_str(delta);
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
                    model: completion.model.clone(),
                    usage: completion.usage.clone(),
                },
            )?;
            Ok(completion)
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

fn parse_source(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("source must not be empty".to_string());
    }
    if value.len() > 64 {
        return Err("source must be at most 64 characters".to_string());
    }
    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        return Err("source may only contain letters, digits, '-', '_', and '.'".to_string());
    }
    Ok(value.to_string())
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
