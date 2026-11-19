//! macOS хранит системный прокси у каждой сетевой службы своей записью, а
//! библиотека пишет и снимает его только у первичной. Сменилась первичная
//! служба (Wi-Fi → кабель) — запись на прежней переживает снятие и уводит
//! трафик в закрытый порт, когда служба снова станет первичной.
//!
//! Пока прокси включён, записи на прежних службах не трогаются: вернувшаяся
//! служба сразу ведёт в прокси, а не напрямую. Обход идёт, когда прокси сняли.
//!
//! Наш прокси у службы выключается, только если ни один её переключатель не
//! занят чужим, — условие не мягче, чем у первичной. Потому обходу и незачем
//! знать, какая служба первичная: снятая читается чистой, а оставленная как
//! есть — занятой чужим. Выключает обход только наши переключатели, остальные
//! не трогает.

use super::{OursInTheSystem as Ours, bare_host};

struct Switch {
    read: &'static str,
    write: &'static str,
    is_ours: fn(&str, &[Ours]) -> bool,
}

const SWITCHES: [Switch; 4] = [
    Switch {
        read: "-getautoproxyurl",
        write: "-setautoproxystate",
        is_ours: auto_is_ours,
    },
    Switch {
        read: "-getwebproxy",
        write: "-setwebproxystate",
        is_ours: manual_is_ours,
    },
    Switch {
        read: "-getsecurewebproxy",
        write: "-setsecurewebproxystate",
        is_ours: manual_is_ours,
    },
    Switch {
        read: "-getsocksfirewallproxy",
        write: "-setsocksfirewallproxystate",
        is_ours: manual_is_ours,
    },
];

fn field<'a>(reading: &'a str, key: &str) -> Option<&'a str> {
    reading.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim().eq_ignore_ascii_case(key).then(|| value.trim())
    })
}

/// Службы из `networksetup -listallnetworkservices`: первая строка — пояснение
/// про звёздочку, звёздочка перед именем — служба выключена.
fn services_listed(listing: &str) -> Vec<&str> {
    listing
        .lines()
        .skip(1)
        .map(|line| line.strip_prefix('*').unwrap_or(line))
        .filter(|name| !name.trim().is_empty())
        .collect()
}

/// Ручной прокси службы смотрит на наш адрес.
fn manual_is_ours(reading: &str, ours: &[Ours]) -> bool {
    let (Some(server), Some(port)) = (field(reading, "Server"), field(reading, "Port")) else {
        return false;
    };
    let Ok(port) = port.parse::<u16>() else {
        return false;
    };
    ours.iter()
        .any(|(host, our_port, _)| bare_host(server) == bare_host(host) && port == *our_port)
}

/// Автонастройка службы — наш PAC.
fn auto_is_ours(reading: &str, ours: &[Ours]) -> bool {
    field(reading, "URL").is_some_and(|url| ours.iter().any(|(_, _, pac_url)| !pac_url.is_empty() && url == pac_url))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Standing {
    Off,
    Ours,
    /// Включён чужой — или переключатель не прочитался, и чей он, неизвестно.
    NotOurs,
}

fn standing(switch: &Switch, reading: Option<&str>, ours: &[Ours]) -> Standing {
    let Some(reading) = reading else {
        return Standing::NotOurs;
    };
    match field(reading, "Enabled") {
        Some(enabled) if enabled.eq_ignore_ascii_case("no") => Standing::Off,
        Some(enabled) if enabled.eq_ignore_ascii_case("yes") && (switch.is_ours)(reading, ours) => Standing::Ours,
        _ => Standing::NotOurs,
    }
}

/// Что обход сделал: служба и переключатель, которым её читали.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Swept {
    pub switched_off: Vec<(std::string::String, &'static str)>,
    pub refused: Vec<(std::string::String, &'static str)>,
    /// Наш прокси включён, но соседний переключатель включён чужим или не
    /// прочитался: служба оставлена как есть.
    pub left_alone: Vec<std::string::String>,
    pub out_of_time_at: Option<std::string::String>,
}

/// Выключить наш прокси на всех службах, где он ещё включён и чужого рядом нет.
///
/// Выключение идёт под удержанием от вызывающего: пока оно в руках, своя
/// запись прокси не идёт и не начнётся. Иначе обход выключал бы переключатель,
/// который приложение пишет в эту же секунду. Удержания не дали — прокси снова
/// включают, обход закончен.
pub(super) fn switch_ours_off<Held>(
    ours: &[Ours],
    networksetup: impl Fn(&[&str]) -> Option<std::string::String>,
    hold_our_writes_off: impl Fn() -> Option<Held>,
    out_of_time: impl Fn() -> bool,
) -> Swept {
    let mut swept = Swept::default();
    let Some(listing) = networksetup(&["-listallnetworkservices"]) else {
        return swept;
    };
    for service in services_listed(&listing) {
        if out_of_time() {
            swept.out_of_time_at = Some(service.to_owned());
            break;
        }
        let standings = SWITCHES.each_ref().map(|switch| {
            let reading = networksetup(&[switch.read, service]);
            standing(switch, reading.as_deref(), ours)
        });
        if !standings.contains(&Standing::Ours) {
            continue;
        }
        if standings.contains(&Standing::NotOurs) {
            swept.left_alone.push(service.to_owned());
            continue;
        }
        let Some(_held) = hold_our_writes_off() else {
            break;
        };
        for (switch, _) in SWITCHES.iter().zip(standings).filter(|(_, on)| *on == Standing::Ours) {
            if networksetup(&[switch.write, service, "off"]).is_some() {
                swept.switched_off.push((service.to_owned(), switch.read));
            } else {
                swept.refused.push((service.to_owned(), switch.read));
            }
        }
    }
    swept
}

/// `None` — команда не выполнилась или система отказала.
#[cfg(target_os = "macos")]
pub(super) fn networksetup(args: &[&str]) -> Option<std::string::String> {
    use std::process::{Command, Stdio};

    let output = Command::new("/usr/sbin/networksetup")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let said = std::string::String::from_utf8_lossy(&output.stdout).into_owned();
    // networksetup умеет сообщить об отказе строкой «** Error» и выйти с нулём.
    (output.status.success() && !said.contains("** Error")).then_some(said)
}

#[cfg(test)]
mod tests {
    use super::{Ours, SWITCHES, Standing, Swept, services_listed, standing, switch_ours_off};
    use std::cell::{Cell, RefCell};

    const OUR_PAC: &str = "http://127.0.0.1:33331/commands/pac";
    const LISTING: &str =
        "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\nUSB 10/100/1000 LAN\n*Bluetooth PAN\n";

    fn ours() -> Vec<Ours> {
        vec![
            ("127.0.0.1".into(), 7897, OUR_PAC.into()),
            (
                "192.168.1.5".into(),
                7897,
                "http://192.168.1.5:33331/commands/pac".into(),
            ),
        ]
    }

    #[test]
    fn the_listing_drops_its_caption_and_the_mark_of_a_disabled_service() {
        let listing = "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\nUSB 10/100/1000 LAN\n*Bluetooth PAN\n\nThunderbolt Bridge\n";
        assert_eq!(
            services_listed(listing),
            ["Wi-Fi", "USB 10/100/1000 LAN", "Bluetooth PAN", "Thunderbolt Bridge"]
        );
        assert!(services_listed("").is_empty());
        assert!(services_listed("An asterisk (*) denotes that a network service is disabled.\n").is_empty());
    }

    fn manual(enabled: &str, server: &str, port: &str) -> std::string::String {
        format!("Enabled: {enabled}\nServer: {server}\nPort: {port}\nAuthenticated Proxy Enabled: 0\n")
    }

    fn auto(enabled: &str, url: &str) -> std::string::String {
        format!("URL: {url}\nEnabled: {enabled}\n")
    }

    #[test]
    fn a_manual_proxy_is_ours_only_switched_on_and_at_our_address() {
        let web = &SWITCHES[1];
        for (reading, expected, why) in [
            (manual("Yes", "127.0.0.1", "7897"), Standing::Ours, "ours"),
            (
                manual("Yes", "192.168.1.5", "7897"),
                Standing::Ours,
                "ours at the shared address",
            ),
            (
                manual("No", "127.0.0.1", "7897"),
                Standing::Off,
                "switched off, the address stays behind",
            ),
            (
                manual("No", "10.0.0.1", "3128"),
                Standing::Off,
                "someone's, switched off",
            ),
            (manual("Yes", "127.0.0.1", "8080"), Standing::NotOurs, "another port"),
            (manual("Yes", "10.0.0.1", "7897"), Standing::NotOurs, "another host"),
            (
                manual("Yes", "", "0"),
                Standing::NotOurs,
                "switched on and never filled in",
            ),
            (
                manual("Yes", "127.0.0.1", "70000"),
                Standing::NotOurs,
                "the port is out of range",
            ),
            (
                "Server: 127.0.0.1\nPort: 7897\nAuthenticated Proxy Enabled: 1\n".to_owned(),
                Standing::NotOurs,
                "the authentication line is not the switch, and the switch is not told",
            ),
            (std::string::String::new(), Standing::NotOurs, "nothing is told"),
            (
                "Enabled: Yes\nServer: 127.0.0.1\n".to_owned(),
                Standing::NotOurs,
                "cut short",
            ),
        ] {
            assert_eq!(standing(web, Some(&reading), &ours()), expected, "{why}: {reading}");
        }
        assert_eq!(standing(web, None, &ours()), Standing::NotOurs, "could not be read");
        assert_eq!(
            standing(
                web,
                Some("Enabled: Yes\r\nServer: 127.0.0.1\r\nPort: 7897\r\n"),
                &ours()
            ),
            Standing::Ours
        );
        let v6: Vec<Ours> = vec![("[::1]".into(), 7897, std::string::String::new())];
        assert_eq!(standing(web, Some(&manual("Yes", "::1", "7897")), &v6), Standing::Ours);
    }

    #[test]
    fn an_autoconfig_is_ours_only_switched_on_and_at_our_address() {
        let pac = &SWITCHES[0];
        for (reading, expected, why) in [
            (auto("Yes", OUR_PAC), Standing::Ours, "ours"),
            (
                auto("No", OUR_PAC),
                Standing::Off,
                "switched off, the address stays behind",
            ),
            (
                auto("Yes", "http://example.com/proxy.pac"),
                Standing::NotOurs,
                "someone else's",
            ),
            (auto("No", "(null)"), Standing::Off, "never set"),
            (
                auto("Yes", "(null)"),
                Standing::NotOurs,
                "switched on without an address",
            ),
            ("Enabled: Yes\n".to_owned(), Standing::NotOurs, "cut short"),
        ] {
            assert_eq!(standing(pac, Some(&reading), &ours()), expected, "{why}: {reading}");
        }
        let without_pac: Vec<Ours> = vec![("127.0.0.1".into(), 7897, std::string::String::new())];
        assert_eq!(
            standing(pac, Some(&auto("Yes", "")), &without_pac),
            Standing::NotOurs,
            "an empty address of ours matches nothing"
        );
    }

    type Reading = ((&'static str, &'static str), std::string::String);

    /// Система понарошку: что отвечает каждое чтение и какие команды она получила.
    /// Переключатель, о котором ничего не сказано, читается выключенным.
    struct Pretend {
        listing: Option<&'static str>,
        readings: Vec<Reading>,
        unreadable: Vec<(&'static str, &'static str)>,
        refuses_to_write: bool,
        calls: RefCell<Vec<std::string::String>>,
    }

    impl Pretend {
        fn with(readings: &[Reading]) -> Self {
            Self {
                listing: Some(LISTING),
                readings: readings.to_vec(),
                unreadable: Vec::new(),
                refuses_to_write: false,
                calls: RefCell::new(Vec::new()),
            }
        }

        fn networksetup(&self, args: &[&str]) -> Option<std::string::String> {
            self.calls.borrow_mut().push(args.join(" "));
            match args {
                ["-listallnetworkservices"] => self.listing.map(str::to_owned),
                [_, _, "off"] => (!self.refuses_to_write).then(std::string::String::new),
                [read, service] if self.unreadable.contains(&(*read, *service)) => None,
                [read, service] => Some(
                    self.readings
                        .iter()
                        .find(|((asked, of), _)| asked == read && of == service)
                        .map_or_else(|| "Enabled: No\n".to_owned(), |(_, reading)| reading.clone()),
                ),
                _ => None,
            }
        }

        fn writes(&self) -> Vec<std::string::String> {
            self.calls
                .borrow()
                .iter()
                .filter(|call| call.ends_with(" off"))
                .cloned()
                .collect()
        }
    }

    fn sweep(system: &Pretend) -> Swept {
        switch_ours_off(&ours(), |args| system.networksetup(args), || Some(()), || false)
    }

    #[test]
    fn our_switches_are_switched_off_wherever_nothing_foreign_is_on() {
        let system = Pretend::with(&[
            (("-getwebproxy", "Wi-Fi"), manual("Yes", "127.0.0.1", "7897")),
            (("-getsecurewebproxy", "Wi-Fi"), manual("No", "10.1.1.1", "3128")),
            (("-getsocksfirewallproxy", "Wi-Fi"), manual("Yes", "127.0.0.1", "7897")),
            (
                ("-getautoproxyurl", "Wi-Fi"),
                auto("No", "http://example.com/proxy.pac"),
            ),
            (("-getwebproxy", "Bluetooth PAN"), manual("No", "127.0.0.1", "7897")),
            (("-getautoproxyurl", "Bluetooth PAN"), auto("Yes", OUR_PAC)),
        ]);
        let swept = sweep(&system);
        assert_eq!(
            system.writes(),
            [
                "-setwebproxystate Wi-Fi off",
                "-setsocksfirewallproxystate Wi-Fi off",
                "-setautoproxystate Bluetooth PAN off",
            ]
        );
        assert_eq!(
            swept,
            Swept {
                switched_off: vec![
                    ("Wi-Fi".to_owned(), "-getwebproxy"),
                    ("Wi-Fi".to_owned(), "-getsocksfirewallproxy"),
                    ("Bluetooth PAN".to_owned(), "-getautoproxyurl"),
                ],
                ..Swept::default()
            }
        );
    }

    /// Рядом с нашим включён чужой — служба остаётся как есть. Непрочитанный
    /// переключатель считается чужим.
    #[test]
    fn a_service_with_something_foreign_switched_on_is_left_as_it_is() {
        let mut system = Pretend::with(&[
            (("-getwebproxy", "Wi-Fi"), manual("Yes", "127.0.0.1", "7897")),
            (
                ("-getautoproxyurl", "Wi-Fi"),
                auto("Yes", "http://example.com/proxy.pac"),
            ),
            (
                ("-getwebproxy", "USB 10/100/1000 LAN"),
                manual("Yes", "127.0.0.1", "7897"),
            ),
            (
                ("-getsocksfirewallproxy", "USB 10/100/1000 LAN"),
                manual("Yes", "10.1.1.1", "1080"),
            ),
            (("-getwebproxy", "Bluetooth PAN"), manual("Yes", "127.0.0.1", "7897")),
        ]);
        system.unreadable.push(("-getsecurewebproxy", "Bluetooth PAN"));
        let swept = sweep(&system);
        assert!(system.writes().is_empty(), "{:?}", system.writes());
        assert_eq!(
            swept,
            Swept {
                left_alone: vec![
                    "Wi-Fi".to_owned(),
                    "USB 10/100/1000 LAN".to_owned(),
                    "Bluetooth PAN".to_owned()
                ],
                ..Swept::default()
            }
        );
    }

    #[test]
    fn a_system_without_our_proxy_is_only_read() {
        let system = Pretend::with(&[
            (("-getwebproxy", "Wi-Fi"), manual("Yes", "10.0.0.1", "7897")),
            (("-getautoproxyurl", "Bluetooth PAN"), auto("No", OUR_PAC)),
        ]);
        let held = Cell::new(0_u32);
        let swept = switch_ours_off(
            &ours(),
            |args| system.networksetup(args),
            || {
                held.set(held.get() + 1);
                Some(())
            },
            || false,
        );
        assert_eq!(swept, Swept::default());
        assert!(system.writes().is_empty());
        assert_eq!(held.get(), 0, "our writes are held off only to write");

        let unlisted = Pretend {
            listing: None,
            ..Pretend::with(&[(("-getwebproxy", "Wi-Fi"), manual("Yes", "127.0.0.1", "7897"))])
        };
        assert_eq!(sweep(&unlisted), Swept::default());
        assert_eq!(unlisted.calls.borrow().len(), 1, "nothing is asked past the listing");
    }

    #[test]
    fn a_refused_switch_is_reported_and_the_rest_still_go() {
        let system = Pretend {
            refuses_to_write: true,
            ..Pretend::with(&[
                (("-getwebproxy", "Wi-Fi"), manual("Yes", "127.0.0.1", "7897")),
                (("-getsocksfirewallproxy", "Wi-Fi"), manual("Yes", "127.0.0.1", "7897")),
                (("-getwebproxy", "Bluetooth PAN"), manual("Yes", "127.0.0.1", "7897")),
            ])
        };
        let swept = sweep(&system);
        assert!(swept.switched_off.is_empty(), "{swept:?}");
        assert_eq!(
            swept.refused,
            [
                ("Wi-Fi".to_owned(), "-getwebproxy"),
                ("Wi-Fi".to_owned(), "-getsocksfirewallproxy"),
                ("Bluetooth PAN".to_owned(), "-getwebproxy"),
            ]
        );
    }

    /// Прокси включают посреди обхода: удержания больше не дают, и после
    /// этого ничего не выключается.
    #[test]
    fn the_sweep_stops_as_soon_as_it_is_not_let_to_hold_our_writes_off() {
        let system = Pretend::with(&[
            (("-getwebproxy", "Wi-Fi"), manual("Yes", "127.0.0.1", "7897")),
            (
                ("-getwebproxy", "USB 10/100/1000 LAN"),
                manual("Yes", "127.0.0.1", "7897"),
            ),
            (("-getwebproxy", "Bluetooth PAN"), manual("Yes", "127.0.0.1", "7897")),
        ]);
        let asked = Cell::new(0_u32);
        let swept = switch_ours_off(
            &ours(),
            |args| system.networksetup(args),
            || {
                asked.set(asked.get() + 1);
                (asked.get() == 1).then_some(())
            },
            || false,
        );
        assert_eq!(system.writes(), ["-setwebproxystate Wi-Fi off"]);
        assert_eq!(swept.switched_off, [("Wi-Fi".to_owned(), "-getwebproxy")]);
        assert_eq!(
            system.calls.borrow().len(),
            10,
            "the listing, four reads and a write, four reads more: {:?}",
            system.calls.borrow()
        );
    }

    #[test]
    fn the_sweep_says_where_it_ran_out_of_time() {
        let system = Pretend::with(&[
            (("-getwebproxy", "Wi-Fi"), manual("Yes", "127.0.0.1", "7897")),
            (
                ("-getwebproxy", "USB 10/100/1000 LAN"),
                manual("Yes", "127.0.0.1", "7897"),
            ),
        ]);
        let asked = Cell::new(0_u32);
        let swept = switch_ours_off(
            &ours(),
            |args| system.networksetup(args),
            || Some(()),
            || {
                asked.set(asked.get() + 1);
                asked.get() > 1
            },
        );
        assert_eq!(system.writes(), ["-setwebproxystate Wi-Fi off"]);
        assert_eq!(swept.out_of_time_at.as_deref(), Some("USB 10/100/1000 LAN"));
    }
}
