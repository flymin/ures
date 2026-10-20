#[path = "../tests/support/mod.rs"]
mod support;
fn main() {
    let config = ures::config::Config::builtin();
    let report = support::report();
    if std::env::args().any(|s| s == "--json") {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return;
    }
    for line in ures::render::summary(&report, false) {
        println!("{line}");
    }
    println!();
    let table = ures::render::table(
        &report,
        &config,
        &config.views["tree"],
        &ures::render::Options::default(),
        110,
    );
    println!("{}", table.header);
    for line in table.lines {
        println!("{line}");
    }
    println!("\nIllustrative session. CPU in cores; tree totals overlap; MEM=PSS.");
}
