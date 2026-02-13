mod colors;
mod config;
mod context;
mod http;
mod log;
mod metrics;
mod modules;
mod pool;
mod server;

fn main() {
    metrics::init();
    let defaults = modules::collect_defaults();
    let c = config::load_config(&defaults);
    log::init(c.server.logging);
    log::set_level(&c.server.log_level);
    log::separator();
    log::info("Loading modules...");
    let mut p = modules::Pipeline::new(c.server.client_timeout);
    modules::register_all(&mut p, &c.modules, &c.server);
    log::separator();
    if let Err(e) = server::Server::new(c.server, p).run() {
        log::error(&format!("Server failed: {e}"));
        std::process::exit(1);
    }
}
