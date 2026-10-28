//! `cua-eval-cloud`: plays the task set against a hosted model through inferd, under a spend and a
//! step budget, and writes the same report and cassette as `cua-eval --engine local`.
//!
//! ```text
//! cua-eval-cloud --dir eval --entry eval/cloud-catalog/claude-sonnet-5.5.toml --label claude-sonnet-5.5 \
//!     --tier balanced --max-steps 400 --ledger $XDG_STATE_HOME/quire/inferd/spend.json
//! ```
//!
//! It must run on a bus with inferd (and accountd) already up, as a caller inferd serves: use
//! `scripts/cloud-eval.sh`, which builds that world in scratch directories and starts this program
//! with an environment that holds no key. The program checks that itself and says so.
//! Exit status: 0 when the run finished or was stopped by its budget, 2 when it could not be set up.

use cua_eval::corpus::Corpus;
use cua_eval::eval::{Cassette, Lot, run_live_until};
use cua_eval::flags::FlagError;
use cua_eval::live::{Plan, Thinking};
use cua_eval::report::{Report, render};
use cua_eval::run::RunRecord;
use cua_eval_cloud::entry::price_of;
use cua_eval_cloud::error::{ArgError, RunError};
use cua_eval_cloud::isolation;
use cua_eval_cloud::provider::InferdProvider;
use cua_eval_cloud::request::SamplingFrom;
use cua_eval_cloud::watch::{Totals, Watch};
use model_catalog::parse_entry;
use porter_client::{DbusTransport, OpenOptions, Transport};
use porter_core::capability::LlmFeature;
use porter_core::consent::Usage;
use porter_core::need::LlmNeed;
use porter_core::{DataClass, Need, Tier, Tokens};
use porter_infer::ServedBy;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

#[derive(Debug)]
struct Args {
    dir: PathBuf,
    entry: PathBuf,
    label: String,
    only: Vec<String>,
    tier: Tier,
    thinking: Thinking,
    sampling: SamplingFrom,
    repeat: u32,
    max_steps: u32,
    ledger: Option<PathBuf>,
}

fn tier_of(word: &str) -> Result<Tier, FlagError> {
    match word {
        "fast" => Ok(Tier::Fast),
        "balanced" => Ok(Tier::Balanced),
        "best" => Ok(Tier::Best),
        other => Err(FlagError::NotAllowed {
            flag: "--tier",
            allowed: "fast, balanced or best",
            given: other.to_owned(),
        }),
    }
}

fn parse(mut argv: impl Iterator<Item = String>) -> Result<Args, ArgError> {
    let mut args = Args {
        dir: PathBuf::from("eval"),
        entry: PathBuf::new(),
        label: String::new(),
        only: Vec::new(),
        tier: Tier::Balanced,
        thinking: Thinking::Off,
        sampling: SamplingFrom::Entry,
        repeat: 1,
        max_steps: 400,
        ledger: None,
    };
    while let Some(flag) = argv.next() {
        let mut value = || {
            argv.next()
                .ok_or_else(|| FlagError::NeedsValue(flag.clone()))
        };
        match flag.as_str() {
            "--dir" => args.dir = PathBuf::from(value()?),
            "--entry" => args.entry = PathBuf::from(value()?),
            "--label" => args.label = value()?,
            "--task" => args.only.push(value()?),
            "--tier" => args.tier = tier_of(&value()?)?,
            "--ledger" => args.ledger = Some(PathBuf::from(value()?)),
            "--repeat" => {
                args.repeat = value()?
                    .parse()
                    .map_err(|_| FlagError::NotANumber("--repeat"))?
            }
            "--max-steps" => {
                args.max_steps = value()?
                    .parse()
                    .map_err(|_| FlagError::NotANumber("--max-steps"))?
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
            "--sampling" => {
                args.sampling = match value()?.as_str() {
                    "entry" => SamplingFrom::Entry,
                    "provider" => SamplingFrom::Provider,
                    other => {
                        return Err(FlagError::NotAllowed {
                            flag: "--sampling",
                            allowed: "entry or provider",
                            given: other.to_owned(),
                        }
                        .into());
                    }
                }
            }
            other => return Err(FlagError::UnknownFlag(other.to_owned()).into()),
        }
    }
    if args.entry.as_os_str().is_empty() || args.label.is_empty() {
        return Err(ArgError::Required);
    }
    Ok(args)
}

/// What inferd's own ledger says was spent today, in micro-dollars: the largest daily total of any
/// scope (every request is counted once on its account and once on its app).
fn ledger_micro_usd(path: &std::path::Path) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    doc["rows"]
        .as_array()?
        .iter()
        .filter(|row| row["period"] == "daily")
        .filter_map(|row| row["spent"]["micro_usd"].as_u64())
        .max()
}

fn dollars(micro: u64) -> String {
    format!("${}.{:04}", micro / 1_000_000, (micro % 1_000_000) / 100)
}

fn served() -> Result<ServedBy, RunError> {
    Ok(ServedBy {
        account: porter_core::AccountId::parse("openrouter")?,
        model: porter_core::ModelId::parse("hosted")?,
        locality: porter_core::Locality::Cloud { region: None },
    })
}

fn steps_of(records: &[RunRecord]) -> u32 {
    records
        .iter()
        .map(|r| u32::try_from(r.steps.len()).unwrap_or(u32::MAX))
        .sum()
}

fn write_out(
    args: &Args,
    corpus: &Corpus,
    engine: &str,
    records: &[RunRecord],
) -> Result<(), RunError> {
    let reports = args.dir.join("reports");
    let io = |path: &std::path::Path| {
        let path = path.to_owned();
        move |source: std::io::Error| RunError::Io { path, source }
    };
    std::fs::create_dir_all(&reports).map_err(io(&reports))?;
    let report = Report {
        label: &args.label,
        engine,
        tasks: &corpus.tasks,
        runs: records,
        checks: &[],
    };
    let path = reports.join(format!("{}.md", args.label));
    std::fs::write(&path, render(&report)).map_err(io(&path))?;
    println!("wrote {}", path.display());
    let take = toml::to_string(&Cassette::of(records))?;
    let path = reports.join(format!("{}.cassette.toml", args.label));
    std::fs::write(&path, take).map_err(io(&path))?;
    println!("wrote {}", path.display());
    Ok(())
}

async fn warm(transport: &DbusTransport, tier: Tier) -> Result<(), RunError> {
    let need = Need::Llm(LlmNeed::new(
        BTreeSet::from([LlmFeature::Chat, LlmFeature::Vision, LlmFeature::Tools]),
        Tokens(16_000),
    ));
    let options = OpenOptions::default().with_usage(Usage::Interactive);
    let ready = transport
        .prepare(&need, DataClass::Screen, tier, &options)
        .await
        .map_err(|source| RunError::Prepare { tier, source })?;
    println!(
        "cua-eval-cloud: inferd answers Prepare for the {tier:?} chat with {ready:?} (a hosted model has nothing to load)"
    );
    Ok(())
}

async fn run(args: Args) -> Result<(), RunError> {
    let seen = isolation::look();
    println!("{}", seen.line());
    if !seen.clean() {
        return Err(RunError::CredentialReachable);
    }
    let corpus = Corpus::load(&args.dir)?;
    let text = std::fs::read_to_string(&args.entry).map_err(|source| RunError::Io {
        path: args.entry.clone(),
        source,
    })?;
    let entry = parse_entry(&text)?;
    let price = price_of(&entry).ok_or(RunError::NoPrice)?;
    let plan = Plan::of(&text, &entry.id.0, args.thinking)?;
    let connection = zbus::Connection::session().await?;
    let transport = DbusTransport::over(connection.clone());
    warm(&transport, args.tier).await?;
    let watch = Arc::new(Watch::default());
    let provider = Arc::new(InferdProvider::new(
        DbusTransport::over(connection),
        args.tier,
        args.sampling,
        watch.clone(),
    ));
    let player = format!("cloud:{}", entry.id.0);
    let lot = Lot {
        player: &player,
        repeat: args.repeat,
        only: &args.only,
        served: served()?,
        spend_microusd: Some(1_000_000),
    };
    let halting = watch.clone();
    let budget = args.max_steps;
    let (records, halted) = run_live_until(&corpus, provider, &plan, &lot, move |so_far| {
        if halting.spend_refused() {
            return Some("inferd refused on a spend cap".to_owned());
        }
        (steps_of(so_far) >= budget).then(|| format!("the step budget of {budget} was used"))
    })
    .await?;
    let totals: Totals = watch.totals();
    let estimate = price.of(totals);
    let ledger = args
        .ledger
        .as_deref()
        .and_then(ledger_micro_usd)
        .map(dollars)
        .unwrap_or_else(|| "unknown".to_owned());
    let engine = format!(
        "{} through inferd on a private bus (Need::Llm with images on class screen, tier {:?}, the key held by accountd), thinking {:?}, sampling from the {}, {} run(s) per task; {} model turns, {} prompt and {} reply tokens, {} at the list price, inferd's ledger {}; {}; {}",
        entry.id.0,
        args.tier,
        args.thinking,
        match args.sampling {
            SamplingFrom::Entry => "entry",
            SamplingFrom::Provider => "provider's defaults",
        },
        args.repeat,
        totals.turns,
        totals.input,
        totals.output,
        dollars(estimate),
        ledger,
        halted.map_or_else(
            || "ran to the end".to_owned(),
            |why| format!("STOPPED: {why}")
        ),
        seen.line().trim_start_matches("cua-eval-cloud: ")
    );
    write_out(&args, &corpus, &engine, &records)?;
    println!("cua-eval-cloud: {engine}");
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(why) => {
            eprintln!("cua-eval-cloud: {why}");
            return ExitCode::from(2);
        }
    };
    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("cua-eval-cloud: {why}");
            ExitCode::from(2)
        }
    }
}
