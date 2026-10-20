fn main() {
    ures::util::install_signals();
    let result = ures::cli::run(std::env::args().skip(1).collect());
    let signal = ures::util::SIGNAL.load(std::sync::atomic::Ordering::Relaxed);
    if signal > 0 {
        std::process::exit(128 + signal);
    }
    if let Err(error) = result {
        eprintln!("ures: {}", ures::procfs::clean(&error));
        std::process::exit(2);
    }
    if ures::util::STOP.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(130);
    }
}
