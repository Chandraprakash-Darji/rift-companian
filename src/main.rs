mod app;
mod service;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("service") {
        let command = args.get(2).map(String::as_str);
        std::process::exit(service::run(command));
    }
    app::run();
}