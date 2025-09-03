use ferri_core::config::Config;
use ferri_core::logger::init_logger;

fn main() {
    let cfg = Config::with_dirs().expect("Failed to create config with dirs");
    let _guards = init_logger(&cfg).expect("Failed to initialize logger");

    // Application logic here...
    println!("Ferri application started on port {}", cfg.port);
}
