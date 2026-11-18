#[cfg(target_os = "macos")]
mod macos;

fn main() {
    #[cfg(target_os = "macos")]
    if let Err(error) = macos::run() {
        eprintln!("EcoPasteNeo: {error:#}");
        std::process::exit(1);
    }

    #[cfg(not(target_os = "macos"))]
    {
        // TODO: Validate the GPUI Windows panel before enabling this entry point.
        eprintln!("The EcoPasteNeo GPUI panel currently supports macOS only.");
        std::process::exit(1);
    }
}
