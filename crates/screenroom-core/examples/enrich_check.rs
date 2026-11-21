use std::time::{SystemTime, UNIX_EPOCH};

use screenroom_core::{
    CatalogueMetadataProvider, Library, MediaKind, MetadataQuery, ProviderConfig, TmdbProvider,
};

const DEFAULT_ROUNDS: usize = 8;

const TASKS_PER_ROUND: i64 = 40;

const EXIT_PROVIDER_REJECTED: i32 = 4;

#[derive(Debug)]
struct EnrichOptions {
    path: String,
    rounds: usize,
    drain_only: bool,
}

#[derive(Debug)]
enum OptionError {
    MissingPath,
    MissingRounds,
    UnknownFlag(String),
    UnexpectedArgument(String),
    InvalidRounds(String),
}

impl OptionError {
    fn message(&self) -> String {
        match self {
            Self::MissingPath => "the library path is missing".to_string(),
            Self::MissingRounds => "--rounds needs a number of rounds after it".to_string(),
            Self::UnknownFlag(flag) => format!("unknown flag {flag}"),
            Self::UnexpectedArgument(value) => format!("unexpected argument {value}"),
            Self::InvalidRounds(value) => format!("{value} is not a number of rounds"),
        }
    }
}

fn parse_rounds(value: &str) -> Result<usize, OptionError> {
    value
        .parse::<usize>()
        .map_err(|_| OptionError::InvalidRounds(value.to_string()))
}

fn parse_options(arguments: &[String]) -> Result<EnrichOptions, OptionError> {
    let mut path: Option<String> = None;
    let mut rounds: Option<usize> = None;
    let mut drain_only = false;
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument == "--drain-only" {
            drain_only = true;
        } else if argument == "--rounds" {
            let value = arguments.get(index + 1).ok_or(OptionError::MissingRounds)?;
            rounds = Some(parse_rounds(value)?);
            index += 1;
        } else if let Some(value) = argument.strip_prefix("--rounds=") {
            rounds = Some(parse_rounds(value)?);
        } else if argument.starts_with("--") {
            return Err(OptionError::UnknownFlag(argument.to_string()));
        } else if path.is_none() {
            path = Some(argument.to_string());
        } else if rounds.is_none() {
            rounds = Some(parse_rounds(argument)?);
        } else {
            return Err(OptionError::UnexpectedArgument(argument.to_string()));
        }
        index += 1;
    }
    Ok(EnrichOptions {
        path: path.ok_or(OptionError::MissingPath)?,
        rounds: rounds.unwrap_or(DEFAULT_ROUNDS),
        drain_only,
    })
}

fn epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse_options(&arguments) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{}", error.message());
            eprintln!("usage: enrich_check <library.db> [ROUNDS] [--drain-only] [--rounds N]");
            std::process::exit(2);
        }
    };

    let library = match Library::open(&options.path) {
        Ok(library) => library,
        Err(error) => {
            eprintln!("cannot open {}: {}", options.path, error.code());
            std::process::exit(1);
        }
    };

    let provider = TmdbProvider::new(ProviderConfig::from_environment());
    let capability = provider.capability();
    println!(
        "provider: configured={} language={} detail={}",
        capability.is_configured(),
        capability.language,
        capability.detail
    );
    if !capability.is_configured() {
        std::process::exit(3);
    }

    let probe = provider.search(&MetadataQuery {
        title: "Inception".to_string(),
        year: Some(2010),
        kind: MediaKind::Movie,
    });
    if let Err(error) = probe {
        eprintln!("probe rejected: {}", error.code());
        std::process::exit(EXIT_PROVIDER_REJECTED);
    }
    println!("probe: ok");

    if options.drain_only {
        println!("enqueue: skipped");
    } else {
        match library.enqueue_whole_library("en-US") {
            Ok(queued) => println!("queued: {queued}"),
            Err(error) => {
                eprintln!("cannot queue the library: {}", error.code());
                std::process::exit(1);
            }
        }
    }

    let depth = match library.enrichment_queue_depth() {
        Ok(depth) => depth,
        Err(error) => {
            eprintln!("cannot read the queue: {}", error.code());
            std::process::exit(1);
        }
    };
    println!(
        "queue: queued={} running={} rateLimited={} waitingNetwork={} failed={} done={}",
        depth.queued,
        depth.running,
        depth.rate_limited,
        depth.waiting_network,
        depth.failed,
        depth.done
    );

    let mut succeeded = 0_u64;
    let mut failed = 0_u64;
    let mut remaining = depth.remaining();
    for round in 0..options.rounds {
        match library.drain_enrichment_queue(&provider, TASKS_PER_ROUND, epoch_ms()) {
            Ok(report) => {
                succeeded += report.succeeded;
                failed += report.failed;
                remaining = report.remaining;
                println!(
                    "round {}: attempted={} succeeded={} failed={} rateLimited={} waitingNetwork={} remaining={}",
                    round + 1,
                    report.attempted,
                    report.succeeded,
                    report.failed,
                    report.rate_limited,
                    report.waiting_network,
                    report.remaining
                );
                if report.rate_limited > 0 || report.waiting_network > 0 {
                    println!(
                        "stopped: the provider deferred the round, so the rest waits for a later run"
                    );
                    std::process::exit(EXIT_PROVIDER_REJECTED);
                }
                if report.attempted == 0 {
                    break;
                }
            }
            Err(error) => {
                eprintln!("round {} failed: {}", round + 1, error.code());
                std::process::exit(1);
            }
        }
    }

    println!("done: succeeded={succeeded} failed={failed} remaining={remaining}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn the_plain_shape_takes_the_library_and_eight_rounds() {
        let options = parse_options(&arguments(&["library.db"])).expect("options");
        assert_eq!(options.path, "library.db");
        assert_eq!(options.rounds, DEFAULT_ROUNDS);
        assert!(!options.drain_only);
    }

    #[test]
    fn the_flag_may_come_before_between_or_after_the_positional_values() {
        let trailing = parse_options(&arguments(&["library.db", "20", "--drain-only"]))
            .expect("trailing flag");
        assert_eq!(trailing.path, "library.db");
        assert_eq!(trailing.rounds, 20);
        assert!(trailing.drain_only);

        let middle =
            parse_options(&arguments(&["library.db", "--drain-only", "20"])).expect("middle flag");
        assert_eq!(middle.rounds, 20);
        assert!(middle.drain_only);

        let leading =
            parse_options(&arguments(&["--drain-only", "library.db", "20"])).expect("leading flag");
        assert_eq!(leading.path, "library.db");
        assert_eq!(leading.rounds, 20);
        assert!(leading.drain_only);
    }

    #[test]
    fn an_unknown_flag_is_refused_rather_than_taken_as_a_path() {
        let error = parse_options(&arguments(&["--wash-only", "library.db"])).expect_err("flag");
        assert!(matches!(error, OptionError::UnknownFlag(flag) if flag == "--wash-only"));
        let error = parse_options(&arguments(&["library.db", "--bogus"])).expect_err("flag");
        assert!(matches!(error, OptionError::UnknownFlag(flag) if flag == "--bogus"));
    }

    #[test]
    fn a_value_flag_without_its_value_is_refused() {
        assert!(matches!(
            parse_options(&arguments(&["--rounds"])),
            Err(OptionError::MissingRounds)
        ));
        assert!(matches!(
            parse_options(&arguments(&["library.db", "--rounds"])),
            Err(OptionError::MissingRounds)
        ));
    }

    #[test]
    fn rounds_may_be_named_and_an_unusable_value_is_refused() {
        let named = parse_options(&arguments(&["library.db", "--rounds", "14"])).expect("named");
        assert_eq!(named.rounds, 14);
        let joined = parse_options(&arguments(&["library.db", "--rounds=3"])).expect("joined");
        assert_eq!(joined.rounds, 3);
        assert!(matches!(
            parse_options(&arguments(&["library.db", "many"])),
            Err(OptionError::InvalidRounds(value)) if value == "many"
        ));
    }

    #[test]
    fn a_missing_path_and_a_surplus_argument_are_refused() {
        assert!(matches!(
            parse_options(&arguments(&[])),
            Err(OptionError::MissingPath)
        ));
        assert!(matches!(
            parse_options(&arguments(&["--drain-only"])),
            Err(OptionError::MissingPath)
        ));
        assert!(matches!(
            parse_options(&arguments(&["library.db", "5", "6"])),
            Err(OptionError::UnexpectedArgument(value)) if value == "6"
        ));
    }
}
