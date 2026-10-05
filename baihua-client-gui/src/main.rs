//! The thin desktop entry point: everything lives in the library, which the
//! Android package loads through `android_main` instead of this `main`.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    report_version();
    baihua_client_gui::run_interface()
}

/// Read-only `version` subcommand the command-line `baihua` aggregates; the
/// graphical end handles no other argument.
fn report_version() {
    let Some(first_argument) = std::env::args().nth(1) else {
        return;
    };
    match first_argument.as_str() {
        "version" | "--version" | "-V" => {
            println!("baihua-gui {}", env!("CARGO_PKG_VERSION"));
            println!("baihua-core {}", baihua_core::core_version());
            std::process::exit(0);
        }
        other => {
            eprintln!(
                "baihua-gui does not handle the option `{other}`; run `baihua help` for the command line entry"
            );
            std::process::exit(2);
        }
    }
}
