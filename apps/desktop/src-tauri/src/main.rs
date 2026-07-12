fn main() {
    if let Err(error) = novatype_desktop_lib::run() {
        eprintln!("failed to run NovaType desktop app: {error}");
    }
}
