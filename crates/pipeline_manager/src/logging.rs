use colored::ColoredString;
use env_logger::Env;
use std::io::Write;

pub fn init_logging(name: ColoredString) {
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format(move |buf, record| {
            let t = chrono::Utc::now();
            let t = format!("{}", t.format("%Y-%m-%d %H:%M:%S"));
            writeln!(
                buf,
                "{} {} {} {}",
                t,
                buf.default_styled_level(record.level()),
                name,
                record.args()
            )
        })
        .init();
}
