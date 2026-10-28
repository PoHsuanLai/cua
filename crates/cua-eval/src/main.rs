//! `cua-eval`: plays the task set against scripts or a model and writes the report.
//!
//! ```text
//! cua-eval --dir eval --label gate                                  (scripts: no model)
//! cua-eval --dir eval --engine cassette --cassette eval/reports/x.cassette.toml
//! cua-eval --dir eval --platform macos                              (the shortcuts of another platform)
//! cua-eval --dir eval --screens /tmp/screens                        (what each task starts on)
//! cua-eval --dir eval --engine local --entry catalog/holo-3.1-4b.toml --port 8000 --repeat 3
//! cua-eval ... --api-key-file <file>        (an engine that wants a bearer key; read from the file, never printed)
//! ```
//!
//! The local engine is an OpenAI-compatible server on this machine's loopback address that the
//! caller started; this program starts nothing, reaches no other address and uses no cloud model.
//! Exit status: 0 when every script met its expectation (or the live run finished), 1 when one
//! did not, 2 when the run could not be set up.

mod cli_error;

use chordkit::Platform;
use cli_error::{ArgError, PlayError};
use cua_eval::corpus::Corpus;
use cua_eval::eval::{Cassette, run_cassette, run_live, run_scripted};
use cua_eval::flags::FlagError;
use cua_eval::live::{Plan, Thinking};
use cua_eval::report::{Check, Report, render};
use cua_eval::run::RunRecord;
use model_http::{
    AuthHeader, HostName, HttpClient, HttpEndpoint, HttpTarget, Port, Proxy, Secret, Timeouts,
    UrlPath, WaitMs,
};
use model_openai_compat::{Flavor, OpenAiCodec};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

/// Which player the run uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Scripted,
    Cassette,
    Local,
}

#[derive(Debug)]
struct Args {
    dir: PathBuf,
    engine: Engine,
    label: String,
    only: Vec<String>,
    entry: Option<PathBuf>,
    cassette: Option<PathBuf>,
    port: u16,
    api_key_file: Option<PathBuf>,
    model: String,
    thinking: Thinking,
    repeat: u32,
    screens: Option<PathBuf>,
    platform: Platform,
}

fn parse(mut argv: impl Iterator<Item = String>) -> Result<Args, ArgError> {
    let mut args = Args {
        dir: PathBuf::from("eval"),
        engine: Engine::Scripted,
        label: "scripted".to_owned(),
        only: Vec::new(),
        entry: None,
        cassette: None,
        port: 8000,
        api_key_file: None,
        model: "holo-3.1-4b".to_owned(),
        thinking: Thinking::Off,
        repeat: 1,
        screens: None,
        platform: Corpus::PLATFORM,
    };
    while let Some(flag) = argv.next() {
        let mut value = || {
            argv.next()
                .ok_or_else(|| FlagError::NeedsValue(flag.clone()))
        };
        match flag.as_str() {
            "--dir" => args.dir = PathBuf::from(value()?),
            "--label" => args.label = value()?,
            "--task" => args.only.push(value()?),
            "--platform" => args.platform = value()?.parse()?,
            "--screens" => args.screens = Some(PathBuf::from(value()?)),
            "--entry" => args.entry = Some(PathBuf::from(value()?)),
            "--cassette" => args.cassette = Some(PathBuf::from(value()?)),
            "--api-key-file" => args.api_key_file = Some(PathBuf::from(value()?)),
            "--model" => args.model = value()?,
            "--port" => {
                args.port = value()?
                    .parse()
                    .map_err(|_| FlagError::NotANumber("--port"))?
            }
            "--repeat" => {
                args.repeat = value()?
                    .parse()
                    .map_err(|_| FlagError::NotANumber("--repeat"))?
            }
            "--engine" => {
                args.engine = match value()?.as_str() {
                    "scripted" => Engine::Scripted,
                    "cassette" => Engine::Cassette,
                    "local" => Engine::Local,
                    other => {
                        return Err(FlagError::NotAllowed {
                            flag: "--engine",
                            allowed: "scripted, cassette or local",
                            given: other.to_owned(),
                        }
                        .into());
                    }
                }
            }
            "--thinking" => {
                args.thinking = match value()?.as_str() {
                    "on" => Thinking::On,
                    "off" => Thinking::Off,
                    other => {
                        return Err(FlagError::NotAllowed {
                            flag: "--thinking",
                            allowed: "on or off",
                            given: other.to_owned(),
                        }
                        .into());
                    }
                }
            }
            other => return Err(FlagError::UnknownFlag(other.to_owned()).into()),
        }
    }
    Ok(args)
}

fn endpoint(port: u16, auth: AuthHeader) -> HttpEndpoint {
    HttpEndpoint {
        target: HttpTarget::Tcp {
            host: HostName("127.0.0.1".to_owned()),
            port: Port(port),
        },
        proxy: Proxy::Direct,
        base: UrlPath("/v1".to_owned()),
        auth,
        headers: Vec::new(),
        timeouts: Timeouts {
            connect: WaitMs(5_000),
            first_byte: WaitMs(180_000),
            idle: WaitMs(120_000),
        },
    }
}

struct Played {
    engine: String,
    runs: Vec<RunRecord>,
    checks: Vec<Check>,
}

async fn play(args: &Args, corpus: &Corpus) -> Result<Played, PlayError> {
    match args.engine {
        Engine::Scripted => {
            let (runs, checks) = run_scripted(corpus, &args.only).await?;
            Ok(Played {
                engine: "scripts (no model; each script is a cassette of a made-up run)".to_owned(),
                runs,
                checks,
            })
        }
        Engine::Cassette => {
            let path = args
                .cassette
                .as_ref()
                .ok_or(PlayError::Missing("--cassette needs a file"))?;
            let text = std::fs::read_to_string(path).map_err(PlayError::io(path))?;
            let cassette: Cassette = toml::from_str(&text)?;
            let (runs, checks) = run_cassette(corpus, &cassette, &args.only).await?;
            Ok(Played {
                engine: "a cassette of an earlier run, played back".to_owned(),
                runs,
                checks,
            })
        }
        Engine::Local => {
            let entry = args.entry.as_ref().ok_or(PlayError::Missing(
                "--engine local needs --entry <catalogue entry>",
            ))?;
            let text = std::fs::read_to_string(entry).map_err(PlayError::io(entry))?;
            let plan = Plan::of(&text, &args.model, args.thinking)?;
            let auth = match &args.api_key_file {
                None => AuthHeader::None,
                Some(path) => {
                    let key = std::fs::read_to_string(path).map_err(PlayError::io(path))?;
                    AuthHeader::Bearer(Secret(key.trim().to_owned()))
                }
            };
            let provider = Arc::new(
                OpenAiCodec::new(Flavor::Vllm).provider(HttpClient::new(endpoint(args.port, auth))),
            );
            let player = format!("local:{}", args.model);
            let runs = run_live(corpus, provider, &plan, &player, args.repeat, &args.only).await?;
            Ok(Played {
                engine: format!(
                    "{} on a local engine (loopback port {}), thinking {:?}, {} run(s) per task",
                    args.model, args.port, args.thinking, args.repeat
                ),
                runs,
                checks: Vec::new(),
            })
        }
    }
}

fn write_out(args: &Args, played: &Played, corpus: &Corpus) -> Result<(), PlayError> {
    let reports = args.dir.join("reports");
    std::fs::create_dir_all(&reports).map_err(PlayError::io(&reports))?;
    let report = Report {
        label: &args.label,
        engine: &played.engine,
        tasks: &corpus.tasks,
        runs: &played.runs,
        checks: &played.checks,
    };
    let path = reports.join(format!("{}.md", args.label));
    std::fs::write(&path, render(&report)).map_err(PlayError::io(&path))?;
    println!("wrote {}", path.display());
    if args.engine == Engine::Local {
        let take = toml::to_string(&Cassette::of(&played.runs))?;
        let path = reports.join(format!("{}.cassette.toml", args.label));
        std::fs::write(&path, take).map_err(PlayError::io(&path))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(why) => {
            eprintln!("cua-eval: {why}");
            return ExitCode::from(2);
        }
    };
    let corpus = match Corpus::load(&args.dir) {
        Ok(corpus) => corpus.on(args.platform),
        Err(why) => {
            eprintln!("cua-eval: {why}");
            return ExitCode::from(2);
        }
    };
    if let Some(out) = &args.screens {
        return screens(&corpus, &args, out);
    }
    let played = match play(&args, &corpus).await {
        Ok(played) => played,
        Err(why) => {
            eprintln!("cua-eval: {why}");
            return ExitCode::from(2);
        }
    };
    if let Err(why) = write_out(&args, &played, &corpus) {
        eprintln!("cua-eval: {why}");
        return ExitCode::from(2);
    }
    let missed = played.checks.iter().filter(|c| !c.met()).count();
    if missed > 0 {
        eprintln!("cua-eval: {missed} script(s) did not reach the verdict they were written for");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// Writes what each task's first screen looks like to `out`, one PNG a task.
fn screens(corpus: &Corpus, args: &Args, out: &std::path::Path) -> ExitCode {
    let shots = match cua_eval::eval::start_screens(corpus, &args.only) {
        Ok(shots) => shots,
        Err(why) => {
            eprintln!("cua-eval: {why}");
            return ExitCode::from(2);
        }
    };
    if let Err(why) = std::fs::create_dir_all(out) {
        eprintln!("cua-eval: {why}");
        return ExitCode::from(2);
    }
    for (task, png) in shots {
        let path = out.join(format!("{task}.png"));
        if let Err(why) = std::fs::write(&path, png) {
            eprintln!("cua-eval: {why}");
            return ExitCode::from(2);
        }
        println!("wrote {}", path.display());
    }
    ExitCode::SUCCESS
}
