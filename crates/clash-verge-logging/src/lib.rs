use flexi_logger::DeferredNow;
use flexi_logger::filter::LogLineFilter;
use flexi_logger::writers::FileLogWriter;
use log::Record;
use std::{fmt, sync::Arc};
use tokio::sync::Mutex;

pub type SharedWriter = Arc<Mutex<FileLogWriter>>;

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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};

    const HELD_CAP: usize = 200;
    static INSTALLED: AtomicBool = AtomicBool::new(false);

    struct HeldLine {
        level: Level,
        target: String,
        module: Option<String>,
        line: String,
    }

    pub struct StartupLog {
        inner: OnceLock<Box<dyn Log>>,
        held: Mutex<Vec<HeldLine>>,
    }

    static STARTUP_LOG: StartupLog = StartupLog {
        inner: OnceLock::new(),
        held: Mutex::new(Vec::new()),
    };

    fn held() -> std::sync::MutexGuard<'static, Vec<HeldLine>> {
        match STARTUP_LOG.held.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
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
            let line = format!("[{}] [{}] {}", stamp(), record.level(), record.args());
            let _ = writeln!(std::io::stderr(), "{line}");
            let mut held = held();
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

    pub fn install() -> bool {
        if log::set_logger(&STARTUP_LOG).is_err() {
            return false;
        }
        INSTALLED.store(true, Ordering::Release);
        log::set_max_level(LevelFilter::Info);
        true
    }

    pub fn hand_over(logger: Box<dyn Log>) -> Result<(), log::SetLoggerError> {
        if !INSTALLED.load(Ordering::Acquire) {
            return log::set_boxed_logger(logger);
        }
        let Err(logger) = STARTUP_LOG.inner.set(logger) else {
            let lines: Vec<HeldLine> = std::mem::take(&mut *held());
            if let Some(inner) = STARTUP_LOG.inner.get() {
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
        held().iter().map(|held_line| held_line.line.clone()).collect()
    }
}
