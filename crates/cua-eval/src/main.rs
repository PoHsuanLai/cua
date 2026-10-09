//! `cua-eval`: plays the task set against scripts or a model and writes the report.
//!
//! ```text
//! cua-eval --dir eval --label gate                                  (scripts: no model)
//! cua-eval --dir eval --engine cassette --cassette eval/reports/x.cassette.toml
//! cua-eval --dir eval --screens /tmp/screens                        (what each task starts on)
//! cua-eval --dir eval --engine local --entry catalog/holo-3.1-4b.toml --port 8000 --repeat 3
//! ```
//!
//! The local engine is an OpenAI-compatible server on this machine's loopback address that the
//! caller started; this program starts nothing, reaches no other address and uses no cloud model.
//! Exit status: 0 when every script met its expectation (or the live run finished), 1 when one
//! did not, 2 when the run could not be set up.

use cua_eval::corpus::Corpus;
use cua_eval::eval::{Cassette, run_cassette, run_live, run_scripted};
use cua_eval::live::{Plan, Thinking};
use cua_eval::report::{Check, Report, render};
use cua_eval::run::RunRecord;
use model_http::{
    AuthHeader, HostName, HttpClient, HttpEndpoint, HttpTarget, Port, Proxy, Timeouts, UrlPath,
    WaitMs,
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
    model: String,
    thinking: Thinking,
    repeat: u32,
    screens: Option<PathBuf>,
}

fn parse(mut argv: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut args = Args {
        dir: PathBuf::from("eval"),
        engine: Engine::Scripted,
        label: "scripted".to_owned(),
        only: Vec::new(),
        entry: None,
        cassette: None,
        port: 8000,
        model: "holo-3.1-4b".to_owned(),
        thinking: Thinking::Off,
        repeat: 1,
        screens: None,
    };
    while let Some(flag) = argv.next() {
        let mut value = || argv.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--dir" => args.dir = PathBuf::from(value()?),
            "--label" => args.label = value()?,
            "--task" => args.only.push(value()?),
            "--screens" => args.screens = Some(PathBuf::from(value()?)),
            "--entry" => args.entry = Some(PathBuf::from(value()?)),
            "--cassette" => args.cassette = Some(PathBuf::from(value()?)),
            "--model" => args.model = value()?,
            "--port" => args.port = value()?.parse().map_err(|_| "--port is a number")?,
            "--repeat" => args.repeat = value()?.parse().map_err(|_| "--repeat is a number")?,
            "--engine" => {
                args.engine = match value()?.as_str() {
                    "scripted" => Engine::Scripted,
                    "cassette" => Engine::Cassette,
                    "local" => Engine::Local,
                    other => {
                        return Err(format!(
                            "--engine takes scripted, cassette or local, not {other}"
                        ));
                    }
                }
            }
            "--thinking" => {
                args.thinking = match value()?.as_str() {
                    "on" => Thinking::On,
                    "off" => Thinking::Off,
                    other => return Err(format!("--thinking takes on or off, not {other}")),
                }
            }
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(args)
}

fn endpoint(port: u16) -> HttpEndpoint {
    HttpEndpoint {
        target: HttpTarget::Tcp {
            host: HostName("127.0.0.1".to_owned()),
            port: Port(port),
        },
        proxy: Proxy::Direct,
        base: UrlPath("/v1".to_owned()),
        auth: AuthHeader::None,
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

async fn play(args: &Args, corpus: &Corpus) -> Result<Played, String> {
    let fail = |e: &dyn std::fmt::Display| e.to_string();
    match args.engine {
        Engine::Scripted => {
            let (runs, checks) = run_scripted(corpus, &args.only)
                .await
                .map_err(|e| fail(&e))?;
            Ok(Played {
                engine: "scripts (no model; each script is a cassette of a made-up run)".to_owned(),
                runs,
                checks,
            })
        }
        Engine::Cassette => {
            let path = args.cassette.as_ref().ok_or("--cassette needs a file")?;
            let text = std::fs::read_to_string(path).map_err(|e| fail(&e))?;
            let cassette: Cassette = toml::from_str(&text).map_err(|e| fail(&e))?;
            let (runs, checks) = run_cassette(corpus, &cassette, &args.only)
                .await
                .map_err(|e| fail(&e))?;
            Ok(Played {
                engine: "a cassette of an earlier run, played back".to_owned(),
                runs,
                checks,
            })
        }
        Engine::Local => {
            let entry = args
                .entry
                .as_ref()
                .ok_or("--engine local needs --entry <catalogue entry>")?;
            let text = std::fs::read_to_string(entry).map_err(|e| fail(&e))?;
            let plan = Plan::of(&text, &args.model, args.thinking).map_err(|e| fail(&e))?;
            let provider = Arc::new(
                OpenAiCodec::new(Flavor::Vllm).provider(HttpClient::new(endpoint(args.port))),
            );
            let player = format!("local:{}", args.model);
            let runs = run_live(corpus, provider, &plan, &player, args.repeat, &args.only)
                .await
                .map_err(|e| fail(&e))?;
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

fn write_out(args: &Args, played: &Played, corpus: &Corpus) -> Result<(), String> {
    let reports = args.dir.join("reports");
    std::fs::create_dir_all(&reports).map_err(|e| e.to_string())?;
    let report = Report {
        label: &args.label,
        engine: &played.engine,
        tasks: &corpus.tasks,
        runs: &played.runs,
        checks: &played.checks,
    };
    let path = reports.join(format!("{}.md", args.label));
    std::fs::write(&path, render(&report)).map_err(|e| e.to_string())?;
    println!("wrote {}", path.display());
    if args.engine == Engine::Local {
        let take = toml::to_string(&Cassette::of(&played.runs)).map_err(|e| e.to_string())?;
        let path = reports.join(format!("{}.cassette.toml", args.label));
        std::fs::write(&path, take).map_err(|e| e.to_string())?;
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
        Ok(corpus) => corpus,
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
