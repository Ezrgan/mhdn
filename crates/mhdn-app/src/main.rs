fn main() {
    if let Err(err) = mhdn_app::run() {
        eprintln!("mhdn: {err}");
        std::process::exit(1);
    }
}
