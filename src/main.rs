fn main() {
    if let Err(error) = hibiscus::run() {
        eprintln!("hibiscus: {error}");
        std::process::exit(1);
    }
}
