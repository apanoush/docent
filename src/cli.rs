//! Command line interface for `docent`.

use std::fs;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::cache;
use crate::config::{self, ContextMode, InitReport};
use crate::context;
use crate::error::DocentError;
use crate::input::{self, LineRange, NoPrompter, PageRange, SelectionArg, TerminalPrompter};
use crate::markdown;
use crate::profile::{Profile, ProfileInfo, ProfileStore};
use crate::prompt::{self, EffectiveSettings};
use crate::provider::{self, Completion};
use crate::request::Request;
use crate::storage;
use crate::viewer::{
    AnswerTarget, Browser, FirefoxTarget, QutebrowserTarget, Viewer, ViewerOptions,
};

const AFTER_HELP: &str = "Examples:
  docent explain notes.md --lines 12:20
  docent ask notes.md --lines 12:20 --question \"Why does this work?\"
  docent translate notes.md --lines 12:20 --to French
  docent summarize paper.md
  docent summarize paper.pdf --pages 1:4
  docent explain notes.md --lines 12:20 --viewer qutebrowser
  docent explain notes.md --lines 12:20 --viewer firefox
  docent init

Machine interface:
  docent request --profile explain < request.json";

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

    /// Do not read or write the answer cache for this run.
    #[arg(long, global = true, conflicts_with = "refresh")]
    pub no_cache: bool,

    /// Ignore a cached answer and replace it with a fresh provider response.
    #[arg(long, global = true, conflicts_with = "no_cache")]
    pub refresh: bool,

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
    /// Summarize the document, or the selected passage.
    Summarize(HumanArgs),
    /// Run a custom profile.
    Do(DoArgs),
    /// Read a JSON request from stdin (machine interface).
    Request(RequestArgs),
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

    /// Page options for PDF documents.
    #[command(flatten)]
    pub pages: PageArgs,

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

    /// Page options for PDF documents.
    #[command(flatten)]
    pub pages: PageArgs,

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

    /// Answer viewer options.
    #[command(flatten)]
    pub viewer: ViewerArgs,
}

/// Options for the browser answer viewers.
#[derive(Debug, Args)]
pub struct ViewerArgs {
    /// Where to show the answer.
    #[arg(long, value_enum)]
    pub viewer: Option<AnswerTarget>,

    /// Qutebrowser executable or command name.
    #[arg(long)]
    pub qutebrowser: Option<String>,

    /// Qutebrowser --target value.
    #[arg(long, value_enum)]
    pub qutebrowser_target: Option<QutebrowserTarget>,

    /// Extra argument passed to qutebrowser before the answer URL.
    #[arg(
        long = "qutebrowser-arg",
        value_name = "ARG",
        action = clap::ArgAction::Append,
        allow_hyphen_values = true
    )]
    pub qutebrowser_arg: Vec<String>,

    /// Firefox executable or command name.
    #[arg(long)]
    pub firefox: Option<String>,

    /// Firefox answer target.
    #[arg(long, value_enum)]
    pub firefox_target: Option<FirefoxTarget>,

    /// CSS file appended after the answer styles.
    #[arg(long, visible_alias = "qutebrowser-css", value_name = "PATH")]
    pub answer_css: Option<PathBuf>,
}

impl ViewerArgs {
    /// Viewer selected by the flag or the config default.
    fn target(&self, config: &config::Config) -> AnswerTarget {
        self.viewer.unwrap_or(config.viewer.default)
    }

    /// Qutebrowser executable from the flag or the config.
    fn qutebrowser<'a>(&'a self, config: &'a config::Config) -> &'a str {
        self.qutebrowser
            .as_deref()
            .unwrap_or(&config.viewer.qutebrowser)
    }

    /// Qutebrowser target from the flag or the config.
    fn qutebrowser_target(&self, config: &config::Config) -> QutebrowserTarget {
        self.qutebrowser_target
            .unwrap_or(config.viewer.qutebrowser_target)
    }

    /// Extra qutebrowser arguments from the flags or the config.
    fn qutebrowser_args(&self, config: &config::Config) -> Vec<String> {
        if self.qutebrowser_arg.is_empty() {
            config.viewer.qutebrowser_args.clone()
        } else {
            self.qutebrowser_arg.clone()
        }
    }

    /// Firefox executable from the flag or the config.
    fn firefox<'a>(&'a self, config: &'a config::Config) -> &'a str {
        self.firefox.as_deref().unwrap_or(&config.viewer.firefox)
    }

    /// Firefox target from the flag or the config.
    fn firefox_target(&self, config: &config::Config) -> FirefoxTarget {
        self.firefox_target.unwrap_or(config.viewer.firefox_target)
    }
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
    pub phrase: Option<String>,
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
            self.phrase.map(SelectionArg::Phrase)
        }
    }
}

/// Page range options for PDF documents.
#[derive(Debug, Args)]
#[group(id = "page_scope", multiple = false)]
pub struct PageArgs {
    /// Page range, 1-based and inclusive, for example 5:7 (PDF only).
    #[arg(long, value_parser = parse_page_range, value_name = "START:END")]
    pub pages: Option<PageRange>,

    /// Single page number (PDF only).
    #[arg(long, value_parser = parse_page_number_value, value_name = "N")]
    pub page: Option<usize>,
}

impl PageArgs {
    fn range(&self) -> Option<PageRange> {
        self.pages.or_else(|| self.page.map(PageRange::single))
    }
}

/// Arguments for `docent request`.
#[derive(Debug, Args)]
pub struct RequestArgs {
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
    /// Include the final answer as rendered HTML in the stream.
    #[arg(long, requires = "stream", conflicts_with_all = ["json", "print_prompt"])]
    pub html: bool,
    /// Print the assembled prompt and exit without calling the provider.
    #[arg(long)]
    pub print_prompt: bool,
    /// Header shown on the browser answer page.
    #[arg(long, value_name = "TEXT")]
    pub header: Option<String>,
    /// Answer viewer options.
    #[command(flatten)]
    pub viewer: ViewerArgs,
}

#[derive(Debug, Clone)]
enum OutputMode {
    PlainStreaming,
    PlainBuffered,
    Json,
    Ndjson,
    NdjsonHtml,
    PrintPrompt,
    Browser(ViewerOptions),
}

impl OutputMode {
    /// Stable identifier used in the history database.
    fn as_str(&self) -> &'static str {
        match self {
            Self::PlainStreaming => "plain_stream",
            Self::PlainBuffered => "plain",
            Self::Json => "json",
            Self::Ndjson => "ndjson",
            Self::NdjsonHtml => "ndjson_html",
            Self::PrintPrompt => "print_prompt",
            Self::Browser(_) => "browser",
        }
    }
}

/// Parses arguments, dispatches the command, and reports errors.
pub fn run() -> Result<(), DocentError> {
    let Cli {
        config_dir: explicit_config_dir,
        source,
        no_history,
        no_cache,
        refresh,
        command,
    } = Cli::parse();
    let Some(command) = command else {
        print_getting_started();
        return Ok(());
    };
    let config_dir = config::config_dir_from_env(explicit_config_dir)?;
    let record_history = !no_history;
    let cache_mode = if no_cache {
        cache::Mode::Bypass
    } else if refresh {
        cache::Mode::Refresh
    } else {
        cache::Mode::Use
    };

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
                cache_mode,
            )?;
        }
        Command::Ask(args) => {
            run_human(
                &config_dir,
                "ask",
                "ask",
                args,
                &source,
                record_history,
                cache_mode,
            )?;
        }
        Command::Translate(args) => {
            run_human(
                &config_dir,
                "translate",
                "translate",
                args,
                &source,
                record_history,
                cache_mode,
            )?;
        }
        Command::Summarize(args) => {
            run_human(
                &config_dir,
                "summarize",
                "summarize",
                args,
                &source,
                record_history,
                cache_mode,
            )?;
        }
        Command::Do(args) => run_do(&config_dir, args, &source, record_history, cache_mode)?,
        Command::Request(args) => {
            run_request(&config_dir, args, &source, record_history, cache_mode)?;
        }
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
    println!("  docent summarize paper.md");
    println!("  docent explain notes.md --lines 12:20 --viewer qutebrowser");
    println!("  docent explain notes.md --lines 12:20 --viewer firefox");
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
    cache_mode: cache::Mode,
) -> Result<(), DocentError> {
    let HumanArgs {
        file,
        selection,
        pages,
        common,
    } = args;
    let CommonArgs {
        question,
        no_stream,
        context_mode,
        target_language,
        viewer,
    } = common;
    let label = human_label(file.as_deref(), &selection, pages.range());

    let config = config::ensure_initialized(config_dir)?;
    let target = viewer.target(&config);
    if target.is_browser() && no_stream {
        return Err(DocentError::Input(
            "browser viewers cannot be combined with --no-stream".to_string(),
        ));
    }

    let profile = ProfileStore::new(config::profiles_dir(config_dir)).load(profile_name)?;
    let settings = resolve_settings(&config, &profile, context_mode, target_language)?;
    let request = resolve_human_request(
        &profile,
        file.as_deref(),
        selection,
        pages.range(),
        question,
    )?;
    let output = if target.is_browser() {
        OutputMode::Browser(browser_options(
            &config,
            config_dir,
            target,
            profile_name,
            label,
            &viewer,
        )?)
    } else if no_stream {
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
        cache_mode,
        output,
    )
}

fn run_do(
    config_dir: &Path,
    args: DoArgs,
    source: &str,
    record_history: bool,
    cache_mode: cache::Mode,
) -> Result<(), DocentError> {
    let DoArgs {
        profile,
        file,
        selection,
        pages,
        common,
    } = args;
    run_human(
        config_dir,
        "do",
        &profile,
        HumanArgs {
            file,
            selection,
            pages,
            common,
        },
        source,
        record_history,
        cache_mode,
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

/// Build the viewer header from the file, page range, and terminal selection.
fn human_label(file: Option<&Path>, selection: &SelectionArgs, pages: Option<PageRange>) -> String {
    let name = file
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "(stdin)".to_string());
    let mut parts = Vec::new();
    if let Some(pages) = pages {
        let label = if pages.start == pages.end {
            "page"
        } else {
            "pages"
        };
        parts.push(format!("{label} {pages}"));
    }
    if let Some(lines) = selection.lines {
        parts.push(format!("lines {lines}"));
    } else if let Some(line) = selection.line {
        parts.push(format!("line {line}"));
    } else if selection.text.is_some() {
        parts.push("selection".to_string());
    } else if let Some(phrase) = &selection.phrase {
        parts.push(format!("\"{phrase}\""));
    }

    if parts.is_empty() {
        name
    } else {
        format!("{name} ({})", parts.join(", "))
    }
}

/// Build the browser viewer options for one command.
fn browser_options(
    config: &config::Config,
    config_dir: &Path,
    target: AnswerTarget,
    profile: &str,
    header: String,
    args: &ViewerArgs,
) -> Result<ViewerOptions, DocentError> {
    let browser = match target {
        AnswerTarget::Qutebrowser => Browser::Qutebrowser {
            command: config::expand_path(Path::new(args.qutebrowser(config)))?
                .display()
                .to_string(),
            target: args.qutebrowser_target(config),
            extra_args: args.qutebrowser_args(config),
        },
        AnswerTarget::Firefox => Browser::Firefox {
            command: config::expand_path(Path::new(args.firefox(config)))?
                .display()
                .to_string(),
            target: args.firefox_target(config),
        },
        AnswerTarget::Terminal => {
            unreachable!("the terminal viewer does not open a browser")
        }
    };
    Ok(ViewerOptions {
        profile: profile.to_string(),
        header,
        browser,
        custom_css: load_custom_css(config_dir, args.answer_css.as_deref())?,
    })
}

/// Read `<config>/answer.css` and an explicit `--answer-css` file, in order.
fn load_custom_css(
    config_dir: &Path,
    explicit: Option<&Path>,
) -> Result<Option<String>, DocentError> {
    let mut parts = Vec::new();
    let default = config::answer_css_file(config_dir);
    if default.is_file() {
        parts.push(
            fs::read_to_string(&default).map_err(|source| DocentError::Read {
                path: default,
                source,
            })?,
        );
    }
    if let Some(path) = explicit {
        let path = config::expand_path(path)?;
        parts.push(
            fs::read_to_string(&path).map_err(|source| DocentError::Read {
                path: path.clone(),
                source,
            })?,
        );
    }
    if parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parts.join("\n")))
    }
}

fn resolve_human_request(
    profile: &Profile,
    file: Option<&Path>,
    selection: SelectionArgs,
    pages: Option<PageRange>,
    question: Option<String>,
) -> Result<Request, DocentError> {
    let stdin_is_terminal = std::io::stdin().is_terminal();
    let options = input::RequestOptions {
        file,
        pages,
        selection: selection.into_arg(),
        question,
        requires: &profile.requires,
    };

    if stdin_is_terminal {
        let mut prompter = TerminalPrompter;
        let mut empty = std::io::empty();
        input::resolve_request(options, &mut prompter, true, &mut empty)
    } else {
        let mut stdin = std::io::stdin().lock();
        let mut prompter = NoPrompter;
        input::resolve_request(options, &mut prompter, false, &mut stdin)
    }
}

fn run_request(
    config_dir: &Path,
    args: RequestArgs,
    source: &str,
    record_history: bool,
    cache_mode: cache::Mode,
) -> Result<(), DocentError> {
    let RequestArgs {
        profile: profile_name,
        question,
        target_language,
        context_mode,
        json,
        stream,
        html,
        print_prompt,
        header,
        viewer,
    } = args;
    let browser_target = viewer.viewer.filter(|target| target.is_browser());
    if browser_target.is_some() && (json || stream || print_prompt || html) {
        return Err(DocentError::Input(
            "browser viewers cannot be combined with --stream, --json, --html, or --print-prompt"
                .to_string(),
        ));
    }

    let config = config::ensure_initialized(config_dir)?;
    let profile = ProfileStore::new(config::profiles_dir(config_dir)).load(&profile_name)?;
    let mut request = read_request()?;
    if question.is_some() {
        request.question = question;
    }
    let settings = resolve_settings(&config, &profile, context_mode, target_language)?;

    let output = if let Some(target) = browser_target {
        OutputMode::Browser(browser_options(
            &config,
            config_dir,
            target,
            &profile_name,
            header.or(request.file.clone()).unwrap_or_default(),
            &viewer,
        )?)
    } else if print_prompt {
        OutputMode::PrintPrompt
    } else if stream {
        if html {
            OutputMode::NdjsonHtml
        } else {
            OutputMode::Ndjson
        }
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
        "request",
        &profile_name,
        source,
        record_history,
        cache_mode,
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
    cache_mode: cache::Mode,
    output: OutputMode,
) -> Result<(), DocentError> {
    let context = context::build(
        &request,
        context::ContextOptions {
            mode: settings.context_mode,
            max_chars: config.context.max_chars,
        },
    );
    if context.truncated && request.selection_range().is_none() {
        log::warn!(
            "document context truncated to {} characters (context.max_chars = {}), so the answer covers only part of the document; use --context-mode full, raise context.max_chars, or narrow the input with --lines or --pages",
            context.chars,
            config.context.max_chars
        );
    }
    let rendered = prompt::render(profile, &request, &context, settings)?;

    if matches!(output, OutputMode::PrintPrompt) {
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

    let created_at_ms = storage::now_ms();
    let started = Instant::now();
    let mut streamed = String::new();
    let cache_state =
        cache::State::resolve(config, record_history, cache_mode, settings, &rendered);

    let outcome = match &output {
        OutputMode::PrintPrompt => unreachable!("print prompt returns before the provider call"),
        OutputMode::PlainStreaming => stream_plain(
            config,
            config_dir,
            settings,
            &rendered,
            &cache_state,
            &mut streamed,
        ),
        OutputMode::PlainBuffered => {
            obtain_buffered(config, config_dir, settings, &rendered, &cache_state)
        }
        OutputMode::Json => obtain_buffered(config, config_dir, settings, &rendered, &cache_state),
        OutputMode::Ndjson => stream_ndjson(
            config,
            config_dir,
            settings,
            &rendered,
            &cache_state,
            &mut streamed,
            false,
        ),
        OutputMode::NdjsonHtml => stream_ndjson(
            config,
            config_dir,
            settings,
            &rendered,
            &cache_state,
            &mut streamed,
            true,
        ),
        OutputMode::Browser(options) => stream_browser(
            config,
            config_dir,
            settings,
            &rendered,
            &cache_state,
            options,
            &mut streamed,
        ),
    };

    let duration_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
    let from_cache = matches!(&outcome, Ok(completion) if completion.cached);

    // A cache hit is not a provider call, so `runs` stays a record of provider
    // calls. The cache table counts uses instead.
    if record_history && !from_cache {
        let run = build_history_run(
            config,
            &request,
            &context,
            &rendered,
            settings,
            command,
            profile_name,
            source,
            &output,
            created_at_ms,
            duration_ms,
            &outcome,
            &streamed,
        );
        if let Err(error) = storage::write(config, config_dir, &run) {
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

    if matches!(&outcome, Err(DocentError::ViewerClosed)) {
        return Ok(());
    }
    outcome.map(|_| ())
}

/// Stream one answer into a qutebrowser page.
#[allow(clippy::too_many_arguments)]
fn stream_browser(
    config: &config::Config,
    config_dir: &Path,
    settings: &EffectiveSettings,
    rendered: &prompt::Prompt,
    cache_state: &cache::State,
    options: &ViewerOptions,
    streamed: &mut String,
) -> Result<Completion, DocentError> {
    let mut viewer = Viewer::open(options)?;
    let result = obtain_streaming(
        config,
        config_dir,
        settings,
        rendered,
        cache_state,
        &mut |delta| {
            streamed.push_str(delta);
            viewer.send_delta(delta)
        },
    );
    viewer.finish(&result);
    result
}

/// Returns a stored answer when one is available.
fn cached_answer(
    config: &config::Config,
    config_dir: &Path,
    cache_state: &cache::State,
) -> Option<Completion> {
    if !cache_state.read {
        return None;
    }
    let key = cache_state.key.as_ref()?;

    match cache::lookup(config, config_dir, key) {
        Ok(Some(answer)) => Some(Completion {
            text: answer.output,
            model: answer.model,
            usage: None,
            cached: true,
        }),
        Ok(None) => None,
        Err(error) => {
            log::warn!("could not read the answer cache: {error}");
            None
        }
    }
}

/// Stores a successful answer for later runs.
fn remember(
    config: &config::Config,
    config_dir: &Path,
    cache_state: &cache::State,
    answer: &Completion,
) {
    if !cache_state.write {
        return;
    }
    let Some(key) = cache_state.key.as_ref() else {
        return;
    };

    if let Err(error) = cache::store(
        config,
        config_dir,
        key,
        answer.model.as_deref(),
        &answer.text,
    ) {
        log::warn!("could not store the answer in the cache: {error}");
    }
}

/// Calls the provider without streaming, reusing a cached answer when possible.
fn obtain_buffered(
    config: &config::Config,
    config_dir: &Path,
    settings: &EffectiveSettings,
    rendered: &prompt::Prompt,
    cache_state: &cache::State,
) -> Result<Completion, DocentError> {
    if let Some(answer) = cached_answer(config, config_dir, cache_state) {
        return Ok(answer);
    }

    let completion = provider::complete(config, settings, rendered)?;
    remember(config, config_dir, cache_state, &completion);
    Ok(completion)
}

/// Calls the provider with streaming, replaying a cached answer as one delta.
fn obtain_streaming(
    config: &config::Config,
    config_dir: &Path,
    settings: &EffectiveSettings,
    rendered: &prompt::Prompt,
    cache_state: &cache::State,
    on_delta: &mut dyn FnMut(&str) -> Result<(), DocentError>,
) -> Result<Completion, DocentError> {
    if let Some(answer) = cached_answer(config, config_dir, cache_state) {
        on_delta(&answer.text)?;
        return Ok(answer);
    }

    let completion = provider::complete_streaming(config, settings, rendered, on_delta)?;
    remember(config, config_dir, cache_state, &completion);
    Ok(completion)
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
    output_mode: &OutputMode,
    created_at_ms: i64,
    duration_ms: i64,
    outcome: &Result<Completion, DocentError>,
    streamed: &str,
) -> storage::NewRun {
    let (status, error) = match outcome {
        Ok(_) => ("success".to_string(), None),
        Err(DocentError::ViewerClosed) => ("cancelled".to_string(), None),
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

    storage::NewRun {
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
        wire_format: config.provider.wire_format.as_str().to_string(),
        base_url: config.provider.base_url.clone(),
        model: model.unwrap_or(&settings.model).to_string(),
        temperature: f64::from(settings.temperature),
        max_tokens: i64::from(settings.max_tokens),
        timeout_secs: config.provider.timeout_secs as i64,
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

#[allow(clippy::too_many_arguments)]
fn stream_plain(
    config: &config::Config,
    config_dir: &Path,
    settings: &EffectiveSettings,
    rendered: &prompt::Prompt,
    cache_state: &cache::State,
    streamed: &mut String,
) -> Result<Completion, DocentError> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let mut wrote = false;
    let mut ends_with_newline = true;

    let result = obtain_streaming(
        config,
        config_dir,
        settings,
        rendered,
        cache_state,
        &mut |delta| {
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
        },
    );

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

#[allow(clippy::too_many_arguments)]
fn stream_ndjson(
    config: &config::Config,
    config_dir: &Path,
    settings: &EffectiveSettings,
    rendered: &prompt::Prompt,
    cache_state: &cache::State,
    streamed: &mut String,
    render_html: bool,
) -> Result<Completion, DocentError> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    let result = obtain_streaming(
        config,
        config_dir,
        settings,
        rendered,
        cache_state,
        &mut |delta| {
            streamed.push_str(delta);
            write_stream_event(
                &mut writer,
                &provider::StreamEvent::Delta {
                    text: delta.to_string(),
                },
            )
        },
    );

    match result {
        Ok(completion) => {
            if render_html {
                write_stream_event(
                    &mut writer,
                    &provider::StreamEvent::Html {
                        html: markdown::render_answer(&completion.text),
                    },
                )?;
            }
            write_stream_event(
                &mut writer,
                &provider::StreamEvent::Done {
                    model: completion.model.clone(),
                    usage: completion.usage.clone(),
                    cached: completion.cached,
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

fn parse_page_range(value: &str) -> Result<PageRange, String> {
    value.parse()
}

fn parse_page_number_value(value: &str) -> Result<usize, String> {
    if value.contains(':') {
        return Err(format!("'{value}' is not a page number"));
    }
    value.parse::<PageRange>().map(|range| range.start)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_source_accepts_a_plugin_name() {
        assert_eq!(parse_source(" my-plugin ").unwrap(), "my-plugin");
        assert_eq!(parse_source("plugin_1.2").unwrap(), "plugin_1.2");
        assert_eq!(parse_source(&"a".repeat(64)).unwrap(), "a".repeat(64));
    }

    #[test]
    fn parse_source_rejects_unusable_names() {
        let cases = [
            ("", "source must not be empty"),
            ("   ", "source must not be empty"),
            ("bad source", "source may only contain letters"),
            ("bad/source", "source may only contain letters"),
        ];

        for (value, expected) in cases {
            let error = parse_source(value).unwrap_err();
            assert!(error.contains(expected), "missing {expected:?} in {error}");
        }

        assert_eq!(
            parse_source(&"a".repeat(65)).unwrap_err(),
            "source must be at most 64 characters"
        );
    }

    #[test]
    fn parse_line_number_accepts_positive_numbers() {
        assert_eq!(parse_line_number_value("7").unwrap(), 7);
        assert_eq!(parse_line_number_value(" 12 ").unwrap(), 12);
    }

    #[test]
    fn parse_line_number_rejects_zero_and_non_numbers() {
        assert_eq!(
            parse_line_number_value("0").unwrap_err(),
            "line numbers start at 1"
        );
        assert_eq!(
            parse_line_number_value("abc").unwrap_err(),
            "'abc' is not a line number"
        );
    }

    #[test]
    fn parses_page_arguments() {
        let cli =
            Cli::try_parse_from(["docent", "summarize", "paper.pdf", "--pages", "2:4"]).unwrap();
        let Some(Command::Summarize(args)) = cli.command else {
            panic!("expected summarize");
        };
        assert_eq!(args.pages.range(), Some(PageRange { start: 2, end: 4 }));
        assert_eq!(args.selection.into_arg(), None);

        let cli = Cli::try_parse_from(["docent", "summarize", "paper.pdf", "--page", "3"]).unwrap();
        let Some(Command::Summarize(args)) = cli.command else {
            panic!("expected summarize");
        };
        assert_eq!(args.pages.range(), Some(PageRange::single(3)));
    }

    #[test]
    fn page_arguments_compose_with_selection_text() {
        let cli = Cli::try_parse_from([
            "docent",
            "ask",
            "paper.pdf",
            "--pages",
            "2:4",
            "--text",
            "passage",
        ])
        .unwrap();
        let Some(Command::Ask(args)) = cli.command else {
            panic!("expected ask");
        };
        assert_eq!(args.pages.range(), Some(PageRange { start: 2, end: 4 }));
        assert_eq!(
            args.selection.into_arg(),
            Some(SelectionArg::Text("passage".to_string()))
        );
    }

    #[test]
    fn page_arguments_reject_two_page_flags() {
        assert!(
            Cli::try_parse_from([
                "docent",
                "summarize",
                "paper.pdf",
                "--page",
                "2",
                "--pages",
                "2:4"
            ])
            .is_err()
        );
    }

    #[test]
    fn page_number_rejects_zero_ranges_and_colons() {
        assert_eq!(
            parse_page_number_value("0").unwrap_err(),
            "page numbers start at 1"
        );
        assert_eq!(
            parse_page_number_value("2:4").unwrap_err(),
            "'2:4' is not a page number"
        );
        assert_eq!(
            parse_page_number_value("abc").unwrap_err(),
            "'abc' is not a page number"
        );
    }
}
