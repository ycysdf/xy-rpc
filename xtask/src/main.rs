use std::env;
use std::fmt;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

#[derive(Clone, Copy)]
enum CargoAction {
    Check,
    Test,
}

impl CargoAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Test => "test",
        }
    }
}

struct MatrixEntry {
    name: &'static str,
    action: CargoAction,
    target: Option<&'static str>,
    no_default_features: bool,
    features: &'static [&'static str],
}

struct MatrixResult<'a> {
    entry: &'a MatrixEntry,
    exit_code: Option<i32>,
}

const MATRIX: &[MatrixEntry] = &[
    MatrixEntry {
        name: "default-check",
        action: CargoAction::Check,
        target: None,
        no_default_features: false,
        features: &[],
    },
    MatrixEntry {
        name: "no-std",
        action: CargoAction::Check,
        target: None,
        no_default_features: true,
        features: &[],
    },
    MatrixEntry {
        name: "no-std-send-sync",
        action: CargoAction::Check,
        target: None,
        no_default_features: true,
        features: &["send_sync"],
    },
    MatrixEntry {
        name: "tokio-json",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["rt_tokio", "format_json"],
    },
    MatrixEntry {
        name: "tokio-no-send-json",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["rt_tokio_without_send_sync", "format_json"],
    },
    MatrixEntry {
        name: "tokio-duplex-json",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["rt_tokio", "duplex", "format_json"],
    },
    MatrixEntry {
        name: "axum-json",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["axum", "format_json"],
    },
    MatrixEntry {
        name: "tokio-stream-json",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["rt_tokio", "stream", "format_json"],
    },
    MatrixEntry {
        name: "compio-json",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["rt_compio", "format_json"],
    },
    MatrixEntry {
        name: "web-json-wasm32",
        action: CargoAction::Check,
        target: Some("wasm32-unknown-unknown"),
        no_default_features: false,
        features: &["format_json"],
    },
    MatrixEntry {
        name: "tokio-message-pack",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["rt_tokio", "format_message_pack"],
    },
    MatrixEntry {
        name: "tokio-no-send-message-pack",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["rt_tokio_without_send_sync", "format_message_pack"],
    },
    MatrixEntry {
        name: "tokio-duplex-message-pack",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["rt_tokio", "duplex", "format_message_pack"],
    },
    MatrixEntry {
        name: "axum-message-pack",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["axum", "format_message_pack"],
    },
    MatrixEntry {
        name: "tokio-stream-message-pack",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["rt_tokio", "stream", "format_message_pack"],
    },
    MatrixEntry {
        name: "compio-message-pack",
        action: CargoAction::Test,
        target: None,
        no_default_features: false,
        features: &["rt_compio", "format_message_pack"],
    },
];

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("list") => {
            print_matrix();
            ExitCode::SUCCESS
        }
        Some("run") | None => run_matrix(args.get(1).map(String::as_str)),
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!("Usage:");
    println!("  cargo run -p xtask -- list");
    println!("  cargo run -p xtask -- run");
    println!("  cargo run -p xtask -- run <name>");
}

fn print_matrix() {
    for entry in MATRIX {
        println!("{:<28} cargo {}", entry.name, render_args(entry).join(" "));
    }
}

fn run_matrix(filter: Option<&str>) -> ExitCode {
    let root = workspace_root();
    let selected = MATRIX
        .iter()
        .filter(|entry| filter.is_none_or(|name| entry.name == name))
        .collect::<Vec<_>>();

    if selected.is_empty() {
        eprintln!("no matrix entry matched");
        return ExitCode::FAILURE;
    }

    let total = selected.len();
    let mut failures = Vec::new();

    for entry in selected {
        let args = render_args(entry);
        println!("\n==> {}: cargo {}", entry.name, args.join(" "));

        let status = Command::new("cargo")
            .args(&args)
            .current_dir(&root)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .expect("failed to spawn cargo");

        if !status.success() {
            failures.push(MatrixResult {
                entry,
                exit_code: status.code(),
            });
        }
    }

    if failures.is_empty() {
        println!("\nMatrix passed: {total} entries");
        ExitCode::SUCCESS
    } else {
        eprintln!("\nMatrix failed: {}/{} entries", failures.len(), total);
        for failure in failures {
            eprintln!(
                "  - {} ({})",
                failure.entry.name,
                ExitStatusDisplay(failure.exit_code)
            );
        }
        ExitCode::FAILURE
    }
}

fn render_args(entry: &MatrixEntry) -> Vec<String> {
    let mut args = vec![entry.action.as_str().to_string()];

    if let Some(target) = entry.target {
        args.push("--target".to_string());
        args.push(target.to_string());
    }

    if entry.no_default_features {
        args.push("--no-default-features".to_string());
    }

    if !entry.features.is_empty() {
        args.push("--features".to_string());
        args.push(entry.features.join(","));
    }

    args
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live under workspace root")
        .to_path_buf()
}

struct ExitStatusDisplay(Option<i32>);

impl fmt::Display for ExitStatusDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(code) => write!(f, "exit code {code}"),
            None => f.write_str("terminated by signal"),
        }
    }
}
