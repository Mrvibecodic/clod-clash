use super::{
    CAP_SUBSCRIPTION_STACK, ChainUids, LanSharing, SeqMap, a_usable_subscription_port, decide_lan_sharing, dns_page,
    known_mode, ladder_value_is_usable, mode_choice_of, parse_tun_overrides, read_lan_request, subscription_stack,
    the_stack_is_capped,
};
use crate::{
    config::{IProfiles, IVerge, PrfItem, PrfOption},
    utils::{dirs, help, tmpl},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use smartstring::alias::String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    Mode,
    Logs,
    Ports,
    Tun,
    Tunnels,
    Dns,
    Edits,
}

impl Group {
    const ALL: [Self; 7] = [
        Self::Mode,
        Self::Logs,
        Self::Ports,
        Self::Tun,
        Self::Tunnels,
        Self::Dns,
        Self::Edits,
    ];

    const fn shared(self) -> bool {
        matches!(self, Self::Logs | Self::Ports | Self::Tun | Self::Tunnels)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Line {
    pub key: String,
    pub now: Value,
    pub template: Value,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub edited: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupView {
    pub group: Group,
    pub shared: bool,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SeenLine {
    pub group: Group,
    pub key: String,
    pub now: Value,
    pub template: Value,
    pub edited: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Edits {
    pub merge: Mapping,
    pub script: String,
    pub rules: SeqMap,
    pub proxies: SeqMap,
    pub groups: SeqMap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Merge,
    Script,
    Rules,
    Proxies,
    Groups,
}

impl Edit {
    const ALL: [Self; 5] = [Self::Merge, Self::Script, Self::Rules, Self::Proxies, Self::Groups];

    const fn key(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Script => "script",
            Self::Rules => "rules",
            Self::Proxies => "proxies",
            Self::Groups => "groups",
        }
    }

    const fn chain(self, uids: &ChainUids) -> &String {
        match self {
            Self::Merge => &uids.merge,
            Self::Script => &uids.script,
            Self::Rules => &uids.rules,
            Self::Proxies => &uids.proxies,
            Self::Groups => &uids.groups,
        }
    }

    pub const fn template(self) -> &'static str {
        match self {
            Self::Merge => tmpl::ITEM_MERGE_EMPTY,
            Self::Script => tmpl::ITEM_SCRIPT,
            Self::Rules => tmpl::ITEM_RULES,
            Self::Proxies => tmpl::ITEM_PROXIES,
            Self::Groups => tmpl::ITEM_GROUPS,
        }
    }
}

pub struct Settings<'a> {
    pub clash: &'a Mapping,
    pub verge: &'a IVerge,
    pub mode_choice: Option<&'a str>,
    pub dns_page: Option<&'a dns_page::Page>,
    pub edits: &'a Edits,
}

const LOG_KEYS: [&str; 2] = ["log-level", "unified-delay"];
const TUN_APP_KEYS: [&str; 3] = ["device", "mtu", "route-exclude-address"];
const LAN_LISTS: [&str; 2] = ["lan-allowed-ips", "lan-disallowed-ips"];

fn line(key: &str, now: Value, template: Value) -> Line {
    Line {
        key: key.into(),
        now,
        template,
        edited: false,
    }
}

fn differing(key: &str, now: Option<Value>, template: Option<Value>) -> Option<Line> {
    let (now, template) = (now?, template?);
    (now != template).then(|| line(key, now, template))
}

fn set_in<'a>(merge: &'a Mapping, path: &[&str]) -> Option<&'a Value> {
    let (top, rest) = path.split_first()?;
    let mut value = merge
        .iter()
        .find(|(name, _)| name.as_str().is_some_and(|name| name.eq_ignore_ascii_case(top)))
        .map(|(_, value)| value)?;
    for key in rest {
        value = value.as_mapping()?.get(*key)?;
    }
    Some(value)
}

fn marked(mut line: Line, merge: &Mapping, path: &[&str]) -> Line {
    line.edited = set_in(merge, path).is_some();
    line
}

fn mode_lines(template: &Mapping, settings: &Settings) -> Vec<Line> {
    let template_mode = known_mode(template.get("mode").and_then(Value::as_str));
    let choice = known_mode(settings.mode_choice);
    differing("mode", choice.map(Value::from), template_mode.map(Value::from))
        .into_iter()
        .collect()
}

fn log_lines(template: &Mapping, settings: &Settings) -> Vec<Line> {
    LOG_KEYS
        .into_iter()
        .filter_map(|key| {
            let usable = |value: &&Value| ladder_value_is_usable(key, value);
            differing(
                key,
                settings.clash.get(key).filter(usable).cloned(),
                template.get(key).filter(usable).cloned(),
            )
        })
        .collect()
}

fn sharing_line(template: &Mapping, settings: &Settings) -> Option<Line> {
    let subscription_wants = template.get("allow-lan").and_then(Value::as_bool)?;
    let user_wants = settings.clash.get("allow-lan").and_then(Value::as_bool) == Some(true);
    let user_declined = settings.verge.lan_sharing_declined == Some(true);
    if !user_wants && !user_declined {
        return None;
    }
    let merge = &settings.edits.merge;
    let mut with_merge_lists = template.clone();
    for key in LAN_LISTS {
        if let Some(list) = set_in(merge, &[key]) {
            with_merge_lists.insert(key.into(), list.clone());
        }
    }
    let opens = |lists: &Mapping, wants, declined| {
        let request = read_lan_request(lists, subscription_wants);
        matches!(decide_lan_sharing(wants, declined, &request), LanSharing::On { .. })
    };
    let mut line = differing(
        "allow-lan",
        Some(Value::Bool(opens(&with_merge_lists, user_wants, user_declined))),
        Some(Value::Bool(opens(template, false, false))),
    )?;
    line.edited = LAN_LISTS.into_iter().any(|key| set_in(merge, &[key]).is_some());
    Some(line)
}

fn port_lines(template: &Mapping, settings: &Settings) -> Vec<Line> {
    let port = differing(
        "mixed-port",
        settings
            .clash
            .get("mixed-port")
            .filter(|value| ladder_value_is_usable("mixed-port", value))
            .cloned(),
        template
            .get("mixed-port")
            .filter(|value| a_usable_subscription_port(value))
            .cloned(),
    );
    port.into_iter().chain(sharing_line(template, settings)).collect()
}

fn tun_lines(template: &Mapping, settings: &Settings) -> Vec<Line> {
    let Some(template_tun) = template.get("tun").and_then(Value::as_mapping) else {
        return Vec::new();
    };
    let overrides = parse_tun_overrides(
        settings.verge.tun_stack.as_deref(),
        settings.verge.tun_strict_route.as_deref(),
        settings.verge.tun_dns_hijack.as_deref(),
    );
    let stack = subscription_stack(template_tun)
        .filter(|stack| !the_stack_is_capped(stack, CAP_SUBSCRIPTION_STACK))
        .map(Value::from);
    let strict_route = template_tun
        .get("strict-route")
        .and_then(Value::as_bool)
        .map(Value::from);
    let dns_hijack = template_tun
        .get("dns-hijack")
        .filter(|value| value.is_sequence())
        .cloned();
    let users_tun = settings.clash.get("tun").and_then(Value::as_mapping);

    let deferred = [
        ("stack", differing("tun.stack", overrides.stack, stack)),
        (
            "strict-route",
            differing("tun.strict-route", overrides.strict_route, strict_route),
        ),
        (
            "dns-hijack",
            differing("tun.dns-hijack", overrides.dns_hijack, dns_hijack),
        ),
    ];
    let pinned = TUN_APP_KEYS.map(|key| {
        let typed = |value: &&Value| match key {
            "mtu" => value.as_u64().is_some(),
            "device" => value.is_string(),
            _ => value.is_sequence(),
        };
        (
            key,
            differing(
                &format!("tun.{key}"),
                users_tun.and_then(|tun| tun.get(key)).filter(typed).cloned(),
                template_tun.get(key).filter(typed).cloned(),
            ),
        )
    });
    let merge = &settings.edits.merge;
    let merge_stack_is_capped = set_in(merge, &["tun", "stack"])
        .and_then(Value::as_str)
        .is_some_and(|stack| the_stack_is_capped(&stack.trim().to_ascii_lowercase(), CAP_SUBSCRIPTION_STACK));
    deferred
        .into_iter()
        .chain(pinned)
        .filter_map(|(key, line)| {
            let mut line = marked(line?, merge, &["tun", key]);
            line.edited &= !(key == "stack" && merge_stack_is_capped);
            Some(line)
        })
        .collect()
}

fn tunnel_lines(template: &Mapping, settings: &Settings) -> Vec<Line> {
    differing(
        "tunnels",
        settings
            .clash
            .get("tunnels")
            .filter(|value| value.is_sequence())
            .cloned(),
        template.get("tunnels").filter(|value| value.is_sequence()).cloned(),
    )
    .map(|line| marked(line, &settings.edits.merge, &["tunnels"]))
    .into_iter()
    .collect()
}

fn dns_lines(template: &Mapping, settings: &Settings) -> Vec<Line> {
    let Some(page) = settings.dns_page else {
        return Vec::new();
    };
    let tun_forces_dns = settings.verge.enable_tun_mode == Some(true) && !crate::feat::tun::is_suppressed();
    let template_dns = template.get("dns").and_then(Value::as_mapping);
    let dns = page.dns.iter().filter_map(|(key, value)| {
        let name = key.as_str()?;
        if name == "listen" || (name == "enable" && tun_forces_dns) {
            return None;
        }
        let line = differing(
            &format!("dns.{name}"),
            Some(value.clone()),
            template_dns.and_then(|dns| dns.get(key)).cloned(),
        )?;
        Some(marked(line, &settings.edits.merge, &["dns", name]))
    });
    let hosts = differing(
        "hosts",
        page.hosts
            .as_ref()
            .filter(|hosts| !hosts.is_empty())
            .cloned()
            .map(Value::Mapping),
        template.get("hosts").filter(|hosts| hosts.is_mapping()).cloned(),
    )
    .map(|line| marked(line, &settings.edits.merge, &["hosts"]));
    dns.chain(hosts).collect()
}

fn seq_line(edit: Edit, seq: &SeqMap) -> Option<Line> {
    let own = seq.prepend.len() + seq.append.len();
    let hidden = seq.delete.len();
    if own == 0 && hidden == 0 {
        return None;
    }
    let mut now = Mapping::new();
    now.insert("own".into(), Value::from(own));
    now.insert("hidden".into(), Value::from(hidden));
    Some(line(edit.key(), Value::Mapping(now), Value::Null))
}

fn edit_lines(edits: &Edits) -> Vec<Line> {
    Edit::ALL
        .into_iter()
        .filter_map(|edit| match edit {
            Edit::Merge => {
                (!edits.merge.is_empty()).then(|| line(edit.key(), Value::from(edits.merge.len()), Value::Null))
            }
            Edit::Script => (edits.script.replace("\r\n", "\n").trim() != tmpl::ITEM_SCRIPT.trim())
                .then(|| line(edit.key(), Value::Bool(true), Value::Null)),
            Edit::Rules => seq_line(edit, &edits.rules),
            Edit::Proxies => seq_line(edit, &edits.proxies),
            Edit::Groups => seq_line(edit, &edits.groups),
        })
        .collect()
}

pub fn differences(template: &Mapping, settings: &Settings) -> Vec<GroupView> {
    Group::ALL
        .into_iter()
        .map(|group| GroupView {
            group,
            shared: group.shared(),
            lines: match group {
                Group::Mode => mode_lines(template, settings),
                Group::Logs => log_lines(template, settings),
                Group::Ports => port_lines(template, settings),
                Group::Tun => tun_lines(template, settings),
                Group::Tunnels => tunnel_lines(template, settings),
                Group::Dns => dns_lines(template, settings),
                Group::Edits => edit_lines(settings.edits),
            },
        })
        .filter(|view| !view.lines.is_empty())
        .collect()
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reset {
    pub mode: Option<std::string::String>,
    pub clash_keys: Vec<&'static str>,
    pub tun_keys: Vec<&'static str>,
    pub sharing: bool,
    pub tun_stack: bool,
    pub tun_strict_route: bool,
    pub tun_dns_hijack: bool,
    pub dns_keys: Vec<std::string::String>,
    pub hosts: bool,
    pub edits: Vec<Edit>,
}

impl Reset {
    pub fn confirmed(view: &[GroupView], seen: &[SeenLine]) -> Option<Self> {
        let mut selected: Vec<Group> = seen.iter().map(|line| line.group).collect();
        selected.dedup();
        if selected.is_empty() {
            return None;
        }
        for group in &selected {
            let fresh = view.iter().find(|view| view.group == *group)?;
            let shown: Vec<_> = seen.iter().filter(|line| line.group == *group).collect();
            let same = fresh.lines.len() == shown.len()
                && fresh.lines.iter().all(|line| {
                    shown.iter().any(|seen| {
                        seen.key == line.key
                            && seen.now == line.now
                            && seen.template == line.template
                            && seen.edited == line.edited
                    })
                });
            if !same {
                return None;
            }
        }
        Some(Self::of(view, &selected))
    }

    fn of(view: &[GroupView], selected: &[Group]) -> Self {
        let mut reset = Self::default();
        let lines = view
            .iter()
            .filter(|group| selected.contains(&group.group))
            .flat_map(|group| &group.lines);
        for line in lines {
            let key = line.key.as_str();
            match key {
                "mode" => reset.mode = line.template.as_str().map(str::to_owned),
                "allow-lan" => reset.sharing = true,
                "tun.stack" => reset.tun_stack = true,
                "tun.strict-route" => reset.tun_strict_route = true,
                "tun.dns-hijack" => reset.tun_dns_hijack = true,
                "hosts" => reset.hosts = true,
                _ => {
                    if let Some(known) = LOG_KEYS
                        .into_iter()
                        .chain(["mixed-port", "tunnels"])
                        .find(|k| *k == key)
                    {
                        reset.clash_keys.push(known);
                    } else if let Some(field) = key.strip_prefix("tun.")
                        && let Some(known) = TUN_APP_KEYS.into_iter().find(|k| *k == field)
                    {
                        reset.tun_keys.push(known);
                    } else if let Some(field) = key.strip_prefix("dns.") {
                        reset.dns_keys.push(field.to_owned());
                    } else if let Some(edit) = Edit::ALL.into_iter().find(|edit| edit.key() == key) {
                        reset.edits.push(edit);
                    }
                }
            }
        }
        reset
    }

    pub fn touches_verge(&self) -> bool {
        self.sharing || self.tun_stack || self.tun_strict_route || self.tun_dns_hijack || self.resets_the_port()
    }

    fn resets_the_port(&self) -> bool {
        self.clash_keys.contains(&"mixed-port")
    }

    pub const fn touches_the_dns_page(&self) -> bool {
        !self.dns_keys.is_empty() || self.hosts
    }

    pub fn clash_patch(&self) -> Mapping {
        let mut patch = Mapping::new();
        for key in &self.clash_keys {
            patch.insert((*key).into(), crate::config::IClashTemp::FOLLOW_THE_SUBSCRIPTION.into());
        }
        if self.sharing {
            patch.insert("allow-lan".into(), Value::Bool(false));
        }
        if !self.tun_keys.is_empty() {
            let tun = self.tun_keys.iter().map(|key| ((*key).into(), Value::Null)).collect();
            patch.insert("tun".into(), Value::Mapping(tun));
        }
        patch
    }

    pub fn apply_to_verge(&self, verge: &mut IVerge) {
        if self.sharing {
            verge.lan_sharing_declined = None;
        }
        if self.tun_stack {
            verge.tun_stack = None;
        }
        if self.tun_strict_route {
            verge.tun_strict_route = None;
        }
        if self.tun_dns_hijack {
            verge.tun_dns_hijack = None;
        }
        if self.resets_the_port() {
            verge.verge_mixed_port = None;
        }
    }

    pub fn apply_to_dns_page(&self, page: &mut dns_page::Page) {
        page.dns
            .retain(|key, _| key.as_str().is_none_or(|name| !self.dns_keys.iter().any(|k| k == name)));
        if self.hosts {
            page.hosts = None;
        }
    }
}

pub struct Gathered {
    pub uid: String,
    pub name: String,
    pub template: Mapping,
    pub mode_choice: Option<String>,
    pub dns_page: Option<dns_page::Page>,
    pub edits: Edits,
    pub chain_items: Vec<(Edit, PrfItem)>,
}

impl Gathered {
    pub fn differences(&self, clash: &Mapping, verge: &IVerge) -> Vec<GroupView> {
        differences(
            &self.template,
            &Settings {
                clash,
                verge,
                mode_choice: self.mode_choice.as_deref(),
                dns_page: self.dns_page.as_ref(),
                edits: &self.edits,
            },
        )
    }
}

async fn read_quietly(edit: Edit, item: &PrfItem, edits: &mut Edits) -> Option<()> {
    let raw = item.read_file().await.ok()?;
    match edit {
        Edit::Script => edits.script = raw,
        Edit::Merge => {
            let mut value = serde_yaml_ng::from_str::<Value>(&raw).ok()?;
            help::apply_merge_transitively(&mut value, "merge").ok()?;
            edits.merge = value.as_mapping().cloned().unwrap_or_default();
        }
        Edit::Rules => edits.rules = serde_yaml_ng::from_str(&raw).ok()?,
        Edit::Proxies => edits.proxies = serde_yaml_ng::from_str(&raw).ok()?,
        Edit::Groups => edits.groups = serde_yaml_ng::from_str(&raw).ok()?,
    }
    Some(())
}

async fn the_dns_page_in_force(uid: &str, verge: &IVerge) -> Option<dns_page::Page> {
    if !verge.enable_dns_settings.unwrap_or(false) {
        return None;
    }
    let path = dirs::dns_page_path(uid).ok()?;
    match tokio::fs::read_to_string(path).await {
        Ok(raw) => dns_page::Page::parse(&raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(dns_page::Page::default()),
        Err(_) => None,
    }
}

async fn read_the_template(item: &PrfItem) -> Option<Mapping> {
    let raw = item.read_file().await.ok()?;
    let mut value = serde_yaml_ng::from_str::<Value>(&raw).ok()?;
    help::apply_merge_transitively(&mut value, "subscription").ok()?;
    value.as_mapping().cloned()
}

pub async fn gather(profiles: &IProfiles, verge: &IVerge) -> Result<Option<Gathered>> {
    let Some(uid) = profiles.get_current().cloned() else {
        return Ok(None);
    };
    let item = profiles.get_item(&uid)?;
    if item.itype.as_deref() != Some("remote") {
        return Ok(None);
    }
    let Some(template) = read_the_template(item).await else {
        return Ok(None);
    };

    let mut edits = Edits {
        script: tmpl::ITEM_SCRIPT.into(),
        ..Edits::default()
    };
    let mut chain_items = Vec::new();
    let uids = ChainUids::of(item);
    for edit in Edit::ALL {
        let Ok(chain) = profiles.get_item(edit.chain(&uids)) else {
            continue;
        };
        if read_quietly(edit, chain, &mut edits).await.is_some() {
            chain_items.push((edit, chain.clone()));
        }
    }

    Ok(Some(Gathered {
        name: item.display_name().unwrap_or_default(),
        mode_choice: mode_choice_of(item),
        dns_page: the_dns_page_in_force(&uid, verge).await,
        template,
        edits,
        chain_items,
        uid,
    }))
}

/// Кандидат для сборки: звенья `edits` указывают в пустоту, и сборка берёт
/// пустые — сами файлы переписываются, только когда ядро такую сборку приняло.
pub fn without_choice_and_edits(profiles: &mut IProfiles, uid: &str, mode: bool, edits: &[Edit]) -> Result<()> {
    let item = profiles.item_mut(uid)?;
    if mode {
        item.mode_choice = None;
    }
    if !edits.is_empty() {
        let option = item.option.get_or_insert_with(PrfOption::default);
        for edit in edits {
            let chain = match edit {
                Edit::Merge => &mut option.merge,
                Edit::Script => &mut option.script,
                Edit::Rules => &mut option.rules,
                Edit::Proxies => &mut option.proxies,
                Edit::Groups => &mut option.groups,
            };
            *chain = Some(String::new());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Edit, Edits, Group, GroupView, Reset, SeenLine, Settings, differences, without_choice_and_edits};
    use crate::{
        config::{IClashTemp, IProfiles, IVerge, PrfItem, PrfOption},
        enhance::dns_page::Page,
    };
    use serde_yaml_ng::{Mapping, Value};

    fn yaml(text: &str) -> Mapping {
        serde_yaml_ng::from_str(text).unwrap_or_default()
    }

    fn keys(view: &[GroupView]) -> Vec<(Group, Vec<std::string::String>)> {
        view.iter()
            .map(|view| (view.group, view.lines.iter().map(|line| line.key.to_string()).collect()))
            .collect()
    }

    fn settings<'a>(clash: &'a Mapping, verge: &'a IVerge, edits: &'a Edits) -> Settings<'a> {
        Settings {
            clash,
            verge,
            mode_choice: None,
            dns_page: None,
            edits,
        }
    }

    fn untouched_edits() -> Edits {
        Edits {
            script: crate::utils::tmpl::ITEM_SCRIPT.into(),
            ..Edits::default()
        }
    }

    fn seen(view: &[GroupView], groups: &[Group]) -> Vec<SeenLine> {
        view.iter()
            .filter(|view| groups.contains(&view.group))
            .flat_map(|view| {
                view.lines.iter().map(|line| SeenLine {
                    group: view.group,
                    key: line.key.clone(),
                    now: line.now.clone(),
                    template: line.template.clone(),
                    edited: line.edited,
                })
            })
            .collect()
    }

    const LISTED: &str = "allow-lan: true\nlan-allowed-ips: [192.168.0.0/16]";

    #[test]
    fn nothing_differs_when_the_user_pinned_nothing() {
        let template = yaml("mode: global\nlog-level: error\nmixed-port: 7890\nallow-lan: true\ntun: {stack: gvisor}");
        let clash = yaml("allow-lan: false\ntun: {enable: false, auto-route: true}");
        let verge = IVerge::default();
        let edits = untouched_edits();
        assert!(differences(&template, &settings(&clash, &verge, &edits)).is_empty());
    }

    #[test]
    fn a_key_the_template_does_not_set_gives_no_line() {
        let template = yaml("mode: rule");
        let clash = yaml("log-level: debug\nunified-delay: false\nmixed-port: 7897\ntunnels: [x]\ntun: {mtu: 1400}");
        let verge = IVerge {
            tun_stack: Some("system".into()),
            lan_sharing_declined: Some(true),
            ..IVerge::default()
        };
        let edits = untouched_edits();
        let mut with_choice = settings(&clash, &verge, &edits);
        with_choice.mode_choice = Some("global");
        assert_eq!(
            keys(&differences(&template, &with_choice)),
            vec![(Group::Mode, vec!["mode".to_owned()])]
        );
    }

    #[test]
    fn a_pin_equal_to_the_template_gives_no_line() {
        let template = yaml("log-level: info\nunified-delay: true\nmixed-port: 7890\ntun: {stack: gvisor, mtu: 1500}");
        let clash = yaml("log-level: info\nunified-delay: true\nmixed-port: 7890\ntun: {mtu: 1500}");
        let verge = IVerge {
            tun_stack: Some("gvisor".into()),
            ..IVerge::default()
        };
        let edits = untouched_edits();
        assert!(differences(&template, &settings(&clash, &verge, &edits)).is_empty());
    }

    #[test]
    fn every_differing_pin_is_a_line_in_its_group() {
        let template = yaml(&format!(
            "log-level: info\nunified-delay: true\nmixed-port: 7890\n{LISTED}\n\
             tun: {{stack: gvisor, strict-route: true, dns-hijack: [any:53], mtu: 9000, route-exclude-address: [10.0.0.0/8]}}\n\
             tunnels: [tcp/udp,127.0.0.1:6553,example.com:53,PROXY]"
        ));
        let clash = yaml(
            "log-level: debug\nunified-delay: false\nmixed-port: 7897\nallow-lan: false\n\
             tun: {mtu: 1400, route-exclude-address: [192.168.0.0/16]}\ntunnels: []",
        );
        let verge = IVerge {
            tun_stack: Some("mips".into()),
            tun_strict_route: Some("off".into()),
            tun_dns_hijack: Some("198.18.0.2:53".into()),
            lan_sharing_declined: Some(true),
            ..IVerge::default()
        };
        let edits = untouched_edits();
        assert_eq!(
            keys(&differences(&template, &settings(&clash, &verge, &edits))),
            vec![
                (Group::Logs, vec!["log-level".to_owned(), "unified-delay".to_owned()]),
                (Group::Ports, vec!["mixed-port".to_owned(), "allow-lan".to_owned()]),
                (
                    Group::Tun,
                    vec![
                        "tun.stack".to_owned(),
                        "tun.strict-route".to_owned(),
                        "tun.dns-hijack".to_owned(),
                        "tun.mtu".to_owned(),
                        "tun.route-exclude-address".to_owned(),
                    ]
                ),
                (Group::Tunnels, vec!["tunnels".to_owned()]),
            ]
        );
    }

    #[test]
    fn sharing_the_subscription_could_not_open_is_no_line() {
        let template = yaml("allow-lan: true");
        let clash = yaml("allow-lan: false");
        let verge = IVerge {
            lan_sharing_declined: Some(true),
            ..IVerge::default()
        };
        let edits = untouched_edits();
        assert!(differences(&template, &settings(&clash, &verge, &edits)).is_empty());
    }

    #[test]
    fn sharing_the_user_turned_on_against_the_template_is_a_line() {
        let template = yaml("allow-lan: false");
        let clash = yaml("allow-lan: true");
        let verge = IVerge::default();
        let edits = untouched_edits();
        assert_eq!(
            keys(&differences(&template, &settings(&clash, &verge, &edits))),
            vec![(Group::Ports, vec!["allow-lan".to_owned()])]
        );
    }

    #[test]
    fn an_unusable_template_port_is_not_a_provider_value() {
        let template = yaml("mixed-port: 0");
        let clash = yaml("mixed-port: 7897");
        let verge = IVerge::default();
        let edits = untouched_edits();
        assert!(differences(&template, &settings(&clash, &verge, &edits)).is_empty());
    }

    #[test]
    fn an_equal_or_unknown_mode_choice_gives_no_line() {
        let template = yaml("mode: Global");
        let clash = Mapping::new();
        let verge = IVerge::default();
        let edits = untouched_edits();
        let mut same = settings(&clash, &verge, &edits);
        same.mode_choice = Some("global");
        assert!(differences(&template, &same).is_empty());
        let mut unknown = settings(&clash, &verge, &edits);
        unknown.mode_choice = Some("script");
        assert!(differences(&template, &unknown).is_empty());
    }

    #[test]
    fn dns_lines_only_for_keys_of_the_template_block() {
        let template = yaml(
            "dns: {enable: false, enhanced-mode: fake-ip, nameserver: [https://example.com/dns-query]}\nhosts: {a.example.com: 10.0.0.1}",
        );
        let clash = Mapping::new();
        let edits = untouched_edits();
        let page = Page {
            dns: yaml(
                "enable: true\nenhanced-mode: redir-host\nnameserver: [https://example.com/dns-query]\nlisten: 127.0.0.1:1053",
            ),
            hosts: Some(yaml("b.example.com: 10.0.0.2")),
        };
        let without_tun = IVerge::default();
        let mut with_page = settings(&clash, &without_tun, &edits);
        with_page.dns_page = Some(&page);
        assert_eq!(
            keys(&differences(&template, &with_page)),
            vec![(
                Group::Dns,
                vec![
                    "dns.enable".to_owned(),
                    "dns.enhanced-mode".to_owned(),
                    "hosts".to_owned()
                ]
            )]
        );

        let under_tun = IVerge {
            enable_tun_mode: Some(true),
            ..IVerge::default()
        };
        let mut tun_page = settings(&clash, &under_tun, &edits);
        tun_page.dns_page = Some(&page);
        assert_eq!(
            keys(&differences(&template, &tun_page)),
            vec![(Group::Dns, vec!["dns.enhanced-mode".to_owned(), "hosts".to_owned()])]
        );
    }

    #[test]
    fn a_key_the_users_merge_also_sets_is_marked() {
        let template = yaml("tun: {mtu: 9000}");
        let clash = yaml("tun: {mtu: 1400}");
        let verge = IVerge::default();
        let marked = |merge: &str| {
            let mut edits = untouched_edits();
            edits.merge = yaml(merge);
            let view = differences(&template, &settings(&clash, &verge, &edits));
            let tun = view.iter().find(|view| view.group == Group::Tun);
            tun.and_then(|tun| tun.lines.first()).map(|line| line.edited)
        };
        assert_eq!(marked("TUN: {mtu: 1280}"), Some(true));
        assert_eq!(marked("tun: {MTU: 1280}"), Some(false));
    }

    #[test]
    fn a_stack_from_merge_the_build_caps_is_not_marked() {
        let template = yaml("tun: {stack: gvisor}");
        let clash = Mapping::new();
        let verge = IVerge {
            tun_stack: Some("mips".into()),
            ..IVerge::default()
        };
        let mut edits = untouched_edits();
        edits.merge = yaml("tun: {stack: System}");
        let view = differences(&template, &settings(&clash, &verge, &edits));
        let stack = view
            .iter()
            .flat_map(|view| &view.lines)
            .find(|line| line.key == "tun.stack");
        assert_eq!(stack.map(|line| line.edited), Some(!cfg!(target_os = "windows")));
    }

    #[test]
    fn sharing_decided_by_lists_from_merge_is_marked() {
        let template = yaml("allow-lan: true");
        let clash = yaml("allow-lan: true");
        let verge = IVerge::default();
        let mut edits = untouched_edits();
        edits.merge = yaml("lan-allowed-ips: [192.168.0.0/16]");
        let view = differences(&template, &settings(&clash, &verge, &edits));
        let sharing = view
            .iter()
            .flat_map(|view| &view.lines)
            .find(|line| line.key == "allow-lan");
        assert_eq!(
            sharing.map(|line| (line.now.clone(), line.template.clone(), line.edited)),
            Some((Value::Bool(true), Value::Bool(false), true))
        );
    }

    #[test]
    fn values_the_build_would_not_take_give_no_line() {
        let template = yaml("tun: {mtu: '1400', device: 5}\ntunnels: {}\ndns: {listen: 0.0.0.0:1053}");
        let clash = yaml("tun: {mtu: 1300, device: utun9}\ntunnels: [tcp/udp,127.0.0.1:6553,example.com:53,PROXY]");
        let verge = IVerge::default();
        let edits = untouched_edits();
        let page = Page {
            dns: yaml("listen: 127.0.0.1:1053"),
            hosts: None,
        };
        let mut with_page = settings(&clash, &verge, &edits);
        with_page.dns_page = Some(&page);
        assert!(differences(&template, &with_page).is_empty());
    }

    #[test]
    fn a_script_with_windows_line_ends_is_not_an_edit_and_merge_tunnels_mark_the_pin() {
        let template = yaml("tunnels: [tcp/udp,127.0.0.1:6553,example.com:53,PROXY]");
        let clash = yaml("tunnels: []");
        let verge = IVerge::default();
        let mut edits = untouched_edits();
        edits.script = crate::utils::tmpl::ITEM_SCRIPT.replace('\n', "\r\n").into();
        edits.merge = yaml("tunnels: []");
        let view = differences(&template, &settings(&clash, &verge, &edits));
        assert_eq!(
            keys(&view),
            vec![
                (Group::Tunnels, vec!["tunnels".to_owned()]),
                (Group::Edits, vec!["merge".to_owned()])
            ]
        );
        assert_eq!(
            view.first().and_then(|view| view.lines.first()).map(|line| line.edited),
            Some(true)
        );
    }

    #[test]
    fn edits_count_when_not_empty() {
        let template = Mapping::new();
        let clash = Mapping::new();
        let verge = IVerge::default();
        let mut edits = untouched_edits();
        edits.rules.prepend.push(Value::from("DOMAIN,example.com,DIRECT"));
        edits.groups.delete.push("Example".into());
        edits.merge.insert("ipv6".into(), Value::Bool(true));
        edits.script = "function main(config) { config.ipv6 = true; return config }".into();
        assert_eq!(
            keys(&differences(&template, &settings(&clash, &verge, &edits))),
            vec![(
                Group::Edits,
                vec![
                    "merge".to_owned(),
                    "script".to_owned(),
                    "rules".to_owned(),
                    "groups".to_owned()
                ]
            )]
        );
    }

    #[test]
    fn the_reset_takes_only_what_was_seen_and_checked() {
        let template =
            yaml("mode: rule\nlog-level: info\nmixed-port: 7890\ntun: {stack: gvisor, mtu: 9000}\ndns: {ipv6: true}");
        let clash = yaml("log-level: debug\nmixed-port: 7897\ntun: {mtu: 1400, device: utun9}");
        let verge = IVerge {
            tun_stack: Some("system".into()),
            ..IVerge::default()
        };
        let mut edits = untouched_edits();
        edits.rules.append.push(Value::from("MATCH,DIRECT"));
        let page = Page {
            dns: yaml("ipv6: false\nlisten: 127.0.0.1:1053"),
            hosts: None,
        };
        let mut all = settings(&clash, &verge, &edits);
        all.mode_choice = Some("direct");
        all.dns_page = Some(&page);
        let view = differences(&template, &all);

        let reset = Reset::confirmed(&view, &seen(&view, &[Group::Mode, Group::Tun, Group::Dns]));
        assert_eq!(
            reset,
            Some(Reset {
                mode: Some("rule".to_owned()),
                tun_keys: vec!["mtu"],
                tun_stack: true,
                dns_keys: vec!["ipv6".to_owned()],
                ..Reset::default()
            })
        );

        let mut clash_after = IClashTemp(clash.clone());
        let logs_ports_tun = Reset::confirmed(&view, &seen(&view, &[Group::Logs, Group::Ports, Group::Tun]));
        clash_after.patch_config(&logs_ports_tun.unwrap_or_default().clash_patch());
        assert_eq!(clash_after.0, yaml("tun: {device: utun9}"));

        let mut page_after = page.clone();
        reset.unwrap_or_default().apply_to_dns_page(&mut page_after);
        assert_eq!(page_after.dns, yaml("listen: 127.0.0.1:1053"));

        let edits_reset = Reset::confirmed(&view, &seen(&view, &[Group::Edits]));
        assert_eq!(edits_reset.map(|reset| reset.edits), Some(vec![Edit::Rules]));
    }

    #[test]
    fn a_window_that_saw_something_else_resets_nothing() {
        let template = yaml("log-level: info\nunified-delay: true");
        let clash = yaml("log-level: debug\nunified-delay: false");
        let verge = IVerge::default();
        let edits = untouched_edits();
        let view = differences(&template, &settings(&clash, &verge, &edits));

        let mut changed_value = seen(&view, &[Group::Logs]);
        if let Some(first) = changed_value.first_mut() {
            first.now = Value::from("warning");
        }
        assert_eq!(Reset::confirmed(&view, &changed_value), None);

        let mut template_moved = seen(&view, &[Group::Logs]);
        if let Some(first) = template_moved.first_mut() {
            first.template = Value::from("error");
        }
        assert_eq!(Reset::confirmed(&view, &template_moved), None);

        let mut mark_moved = seen(&view, &[Group::Logs]);
        if let Some(first) = mark_moved.first_mut() {
            first.edited = true;
        }
        assert_eq!(Reset::confirmed(&view, &mark_moved), None);

        let mut one_line_missing = seen(&view, &[Group::Logs]);
        one_line_missing.pop();
        assert_eq!(Reset::confirmed(&view, &one_line_missing), None);

        let gone = vec![SeenLine {
            group: Group::Mode,
            key: "mode".into(),
            now: Value::from("global"),
            template: Value::from("rule"),
            edited: false,
        }];
        assert_eq!(Reset::confirmed(&view, &gone), None);
        assert_eq!(Reset::confirmed(&view, &[]), None);
    }

    #[test]
    fn resetting_sharing_forgets_the_users_word() {
        let template = yaml(LISTED);
        let clash = yaml("allow-lan: false");
        let verge = IVerge {
            lan_sharing_declined: Some(true),
            ..IVerge::default()
        };
        let edits = untouched_edits();
        let view = differences(&template, &settings(&clash, &verge, &edits));
        let reset = Reset::confirmed(&view, &seen(&view, &[Group::Ports])).unwrap_or_default();
        let mut verge_after = verge;
        reset.apply_to_verge(&mut verge_after);
        assert_eq!(verge_after.lan_sharing_declined, None);
        let mut clash_after = IClashTemp(clash);
        clash_after.patch_config(&reset.clash_patch());
        assert_eq!(clash_after.0.get("allow-lan"), Some(&Value::Bool(false)));
    }

    #[test]
    fn the_candidate_points_the_reset_chains_nowhere_and_forgets_the_mode() {
        let item = |uid: &str| PrfItem {
            uid: Some(uid.into()),
            mode_choice: Some("global".into()),
            option: Some(PrfOption {
                rules: Some(format!("r-{uid}").into()),
                merge: Some(format!("m-{uid}").into()),
                ..PrfOption::default()
            }),
            ..PrfItem::default()
        };
        let mut profiles = IProfiles {
            current: Some("A".into()),
            items: Some(vec![item("A"), item("B")]),
        };
        assert!(without_choice_and_edits(&mut profiles, "A", true, &[Edit::Rules]).is_ok());
        let summary: Vec<_> = profiles
            .items
            .unwrap_or_default()
            .into_iter()
            .map(|item| {
                let option = item.option.unwrap_or_default();
                (item.mode_choice, option.rules, option.merge)
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (None, Some("".into()), Some("m-A".into())),
                (Some("global".into()), Some("r-B".into()), Some("m-B".into())),
            ]
        );
    }
}
