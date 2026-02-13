mod colors;
mod config;
mod context;
mod h2_handler;
mod h3_handler;
mod http;
mod log;
mod metrics;
mod modules;
mod pool;
mod script;
mod server;
#[cfg(test)]
mod tests;

fn main() {
    metrics::init();
    let mut defaults = modules::collect_defaults();
    let script_defaults = script::collect_script_defaults();
    for (k, v) in script_defaults {
        defaults.entry(k).or_insert(v);
    }
    let c = config::load_config(&defaults);
    log::init(c.server.logging);
    log::set_level(&c.server.log_level);
    log::separator();
    log::info("Loading modules...");
    let mut p = modules::Pipeline::new(c.server.client_timeout);
    modules::register_all(&mut p, &c.modules, &c.server);
    script::load_script_modules(&mut p, &c.modules, &c.server);
    p.sort();
    log::separator();
    if let Err(e) = server::Server::new(c.server, p).run() {
        log::error(&format!("Server failed: {e}"));
        std::process::exit(1);
    }
}
