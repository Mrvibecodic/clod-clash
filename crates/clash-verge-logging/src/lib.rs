use flexi_logger::DeferredNow;
use flexi_logger::filter::LogLineFilter;
use log::Record;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum Type {
    Cmd,
    Core,
    Config,
    Setup,
    System,
    SystemSignal,
    Service,
    Hotkey,
    Window,
    Tray,
    Timer,
    Frontend,
    Backup,
    File,
    Lightweight,
    Network,
    ProxyMode,
    Validate,
    ClashVergeRev,
}

impl fmt::Display for Type {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cmd => write!(f, "[Cmd]"),
            Self::Core => write!(f, "[Core]"),
            Self::Config => write!(f, "[Config]"),
            Self::Setup => write!(f, "[Setup]"),
            Self::System => write!(f, "[System]"),
            Self::SystemSignal => write!(f, "[SysSignal]"),
            Self::Service => write!(f, "[Service]"),
            Self::Hotkey => write!(f, "[Hotkey]"),
            Self::Window => write!(f, "[Window]"),
            Self::Tray => write!(f, "[Tray]"),
            Self::Timer => write!(f, "[Timer]"),
            Self::Frontend => write!(f, "[Frontend]"),
            Self::Backup => write!(f, "[Backup]"),
            Self::File => write!(f, "[File]"),
            Self::Lightweight => write!(f, "[Lightweight]"),
            Self::Network => write!(f, "[Network]"),
            Self::ProxyMode => write!(f, "[ProxMode]"),
            Self::Validate => write!(f, "[Validate]"),
            Self::ClashVergeRev => write!(f, "[ClashVergeRev]"),
        }
    }
}

#[macro_export]
macro_rules! logging {
    // Версия без параметра print (по умолчанию не печатает)
    ($level:ident, $type:expr, $($arg:tt)*) => {
        log::$level!(target: "app", "{} {}", $type, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! logging_error {
    // Handle Result<T, E>
    ($type:expr, $expr:expr) => {
        if let Err(err) = $expr {
            log::error!(target: "app", "[{}] {}", $type, err);
        }
    };

    // Handle formatted message: always print to stdout and log as error
    ($type:expr, $fmt:literal $(, $arg:expr)*) => {
        log::error!(target: "app", "[{}] {}", $type, format_args!($fmt $(, $arg)*));
    };
}

pub struct NoModuleFilter<'a>(pub Vec<&'a str>);

impl<'a> NoModuleFilter<'a> {
    #[inline]
    pub fn filter(&self, record: &Record) -> bool {
        if let Some(module) = record.module_path() {
            for blocked in self.0.iter() {
                if module.len() >= blocked.len() && module.as_bytes()[..blocked.len()] == blocked.as_bytes()[..] {
                    return false;
                }
            }
        }
        true
    }
}

impl<'a> LogLineFilter for NoModuleFilter<'a> {
    #[inline]
    fn write(
        &self,
        now: &mut DeferredNow,
        record: &Record,
        writer: &dyn flexi_logger::filter::LogLineWriter,
    ) -> std::io::Result<()> {
        if !self.filter(record) {
            return Ok(());
        }
        writer.write(now, record)
    }
}

pub mod startup {
    use log::{Level, LevelFilter, Log, Metadata, Record};
    use std::io::Write as _;
    use std::sync::{Mutex, OnceLock};

    const HELD_CAP: usize = 200;

    struct HeldLine {
        level: Level,
        target: String,
        module: Option<String>,
        line: String,
    }

    pub struct StartupLog {
        redact: fn(&str) -> String,
        inner: OnceLock<Box<dyn Log>>,
        held: Mutex<Option<Vec<HeldLine>>>,
    }

    static STARTUP_LOG: OnceLock<&'static StartupLog> = OnceLock::new();

    impl StartupLog {
        fn new(redact: fn(&str) -> String) -> Self {
            Self {
                redact,
                inner: OnceLock::new(),
                held: Mutex::new(Some(Vec::new())),
            }
        }

        fn held(&self) -> std::sync::MutexGuard<'_, Option<Vec<HeldLine>>> {
            match self.held.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            }
        }

        fn held_lines(&self) -> Vec<String> {
            self.held()
                .iter()
                .flatten()
                .map(|held_line| held_line.line.clone())
                .collect()
        }

        fn stop_holding(&self) {
            *self.held() = None;
        }
    }

    fn stamp() -> String {
        flexi_logger::DeferredNow::new()
            .format("%Y-%m-%d %H:%M:%S%.3f")
            .to_string()
    }

    impl Log for StartupLog {
        fn enabled(&self, metadata: &Metadata) -> bool {
            self.inner
                .get()
                .map_or(metadata.level() <= Level::Info, |inner| inner.enabled(metadata))
        }

        fn log(&self, record: &Record) {
            if let Some(inner) = self.inner.get() {
                inner.log(record);
                return;
            }
            if record.level() > Level::Info {
                return;
            }
            let message = (self.redact)(&record.args().to_string());
            let line = format!("[{}] [{}] {}", stamp(), record.level(), message);
            let _ = writeln!(std::io::stderr(), "{line}");
            let mut held = self.held();
            let Some(held) = held.as_mut() else {
                return;
            };
            if held.len() >= HELD_CAP {
                held.remove(0);
            }
            held.push(HeldLine {
                level: record.level(),
                target: record.target().to_owned(),
                module: record.module_path().map(str::to_owned),
                line,
            });
        }

        fn flush(&self) {
            if let Some(inner) = self.inner.get() {
                inner.flush();
            }
        }
    }

    pub fn install(redact: fn(&str) -> String) -> bool {
        let startup_log: &'static StartupLog = Box::leak(Box::new(StartupLog::new(redact)));
        if log::set_logger(startup_log).is_err() {
            return false;
        }
        let _ = STARTUP_LOG.set(startup_log);
        log::set_max_level(LevelFilter::Info);
        true
    }

    pub fn hand_over(logger: Box<dyn Log>) -> Result<(), log::SetLoggerError> {
        let Some(startup_log) = STARTUP_LOG.get() else {
            return log::set_boxed_logger(logger);
        };
        let Err(logger) = startup_log.inner.set(logger) else {
            let lines: Vec<HeldLine> = startup_log.held().take().unwrap_or_default();
            if let Some(inner) = startup_log.inner.get() {
                for held_line in lines {
                    inner.log(
                        &Record::builder()
                            .level(held_line.level)
                            .target(&held_line.target)
                            .module_path(held_line.module.as_deref())
                            .args(format_args!("(до запуска журнала) {}", held_line.line))
                            .build(),
                    );
                }
            }
            return Ok(());
        };
        log::set_boxed_logger(logger)
    }

    pub fn held_lines() -> Vec<String> {
        STARTUP_LOG
            .get()
            .map_or_else(Vec::new, |startup_log| startup_log.held_lines())
    }

    pub fn stop_holding() {
        if let Some(startup_log) = STARTUP_LOG.get() {
            startup_log.stop_holding();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{HELD_CAP, StartupLog};
        use log::{Level, Log as _, Record};

        fn log_line(startup_log: &StartupLog, text: &str) {
            startup_log.log(
                &Record::builder()
                    .level(Level::Info)
                    .target("app")
                    .args(format_args!("{text}"))
                    .build(),
            );
        }

        #[test]
        fn lines_are_not_held_once_the_main_log_failed() {
            let startup_log = StartupLog::new(str::to_owned);
            log_line(&startup_log, "before");
            assert_eq!(startup_log.held_lines().len(), 1);

            startup_log.stop_holding();
            for _ in 0..=HELD_CAP {
                log_line(&startup_log, "after");
            }
            assert!(startup_log.held_lines().is_empty(), "{:?}", startup_log.held_lines());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::startup;

    fn mask(line: &str) -> String {
        line.replace("AbCd1234EfGh5678", "***")
    }

    #[test]
    fn lines_before_the_main_log_are_edited() {
        assert!(startup::install(mask));
        log::info!(target: "app", "fetch https://panel.example.com/sub/AbCd1234EfGh5678 failed");
        let lines = startup::held_lines();
        assert!(
            lines.iter().any(|line| line.contains("panel.example.com/sub/***")),
            "{lines:?}"
        );
        assert!(lines.iter().all(|line| !line.contains("AbCd1234EfGh5678")), "{lines:?}");
    }
}
