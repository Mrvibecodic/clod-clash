use super::menu_def::MenuNode;
use anyhow::Result;
use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
use ksni::{Handle, Icon, MenuItem, ToolTip, Tray, TrayMethods as _};
use std::sync::OnceLock;
use std::time::Duration;

static TRAY_HANDLE: OnceLock<Handle<ClodTray>> = OnceLock::new();

const SPAWN_ATTEMPTS: u8 = 3;
const SPAWN_RETRY_DELAY: Duration = Duration::from_millis(700);
const SPAWN_DEADLINE: Duration = Duration::from_secs(3);
const UPDATE_TIMEOUT: Duration = Duration::from_secs(2);
const ICON_SIZES: [u32; 3] = [22, 32, 48];

struct ClodTray {
    icon: Vec<Icon>,
    tooltip: String,
    menu: Vec<MenuNode>,
}

impl Tray for ClodTray {
    fn id(&self) -> String {
        super::TRAY_ID.into()
    }

    fn title(&self) -> String {
        crate::constants::branding::APP_NAME.into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icon.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        let (title, description) = self.tooltip.split_once('\n').unwrap_or((&self.tooltip, ""));
        ToolTip {
            title: title.into(),
            description: escape_markup(description),
            ..ToolTip::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        super::handle_primary_click();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        render_nodes(&self.menu)
    }
}

fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_label(label: &str) -> String {
    label.replace('_', "__")
}

fn parse_shortcut(accelerator: Option<&str>) -> Vec<Vec<String>> {
    let Some(accelerator) = accelerator.map(str::trim).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    let keys: Vec<String> = accelerator
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| match part.to_lowercase().as_str() {
            "ctrl" | "control" | "cmdorctrl" | "cmdorcontrol" | "commandorctrl" | "commandorcontrol" => {
                "Control".into()
            }
            "alt" | "option" => "Alt".into(),
            "shift" => "Shift".into(),
            "super" | "meta" | "cmd" | "command" => "Super".into(),
            _ => part.to_uppercase(),
        })
        .collect();
    if keys.is_empty() { Vec::new() } else { vec![keys] }
}

fn on_activate(id: &str) -> Box<dyn Fn(&mut ClodTray) + Send> {
    let id = id.to_owned();
    Box::new(move |_tray| super::handle_menu_click(id.clone()))
}

fn render_nodes(nodes: &[MenuNode]) -> Vec<MenuItem<ClodTray>> {
    nodes.iter().map(render_node).collect()
}

fn render_node(node: &MenuNode) -> MenuItem<ClodTray> {
    match node {
        MenuNode::Separator => MenuItem::Separator,
        MenuNode::Item {
            id,
            label,
            enabled,
            accelerator,
            ..
        } => StandardItem {
            label: escape_label(label),
            enabled: *enabled,
            shortcut: parse_shortcut(accelerator.as_deref()),
            activate: on_activate(id),
            ..StandardItem::default()
        }
        .into(),
        MenuNode::Check {
            id,
            label,
            enabled,
            checked,
            accelerator,
            ..
        } => CheckmarkItem {
            label: escape_label(label),
            enabled: *enabled,
            checked: *checked,
            shortcut: parse_shortcut(accelerator.as_deref()),
            activate: on_activate(id),
            ..CheckmarkItem::default()
        }
        .into(),
        MenuNode::Sub {
            label,
            enabled,
            children,
            ..
        } => SubMenu {
            label: escape_label(label),
            enabled: *enabled,
            submenu: render_nodes(children),
            ..SubMenu::default()
        }
        .into(),
    }
}

fn premultiply(image: &mut image::RgbaImage) {
    for pixel in image.pixels_mut() {
        let alpha = u32::from(pixel.0[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
}

fn unpremultiply(image: &mut image::RgbaImage) {
    for pixel in image.pixels_mut() {
        let alpha = u32::from(pixel.0[3]);
        if alpha == 0 {
            continue;
        }
        for channel in &mut pixel.0[..3] {
            *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
}

fn decode_icon(icon_bytes: &[u8]) -> Result<Vec<Icon>> {
    let source = tauri::image::Image::from_bytes(icon_bytes)?;
    let (source_width, source_height) = (source.width(), source.height());
    let mut source = image::RgbaImage::from_raw(source_width, source_height, source.rgba().to_vec())
        .ok_or_else(|| anyhow::anyhow!("размер иконки трея не совпадает с её данными"))?;
    if source_width == 0 || source_height == 0 {
        anyhow::bail!("иконка трея пустая");
    }
    premultiply(&mut source);

    ICON_SIZES
        .iter()
        .map(|size| {
            let (width, height) = if source_width >= source_height {
                (*size, (size * source_height / source_width).max(1))
            } else {
                ((size * source_width / source_height).max(1), *size)
            };
            let mut scaled = image::imageops::resize(&source, width, height, image::imageops::FilterType::Lanczos3);
            unpremultiply(&mut scaled);
            let mut data = Vec::with_capacity(scaled.len());
            for pixel in scaled.chunks_exact(4) {
                data.extend_from_slice(&[pixel[3], pixel[0], pixel[1], pixel[2]]);
            }
            Ok(Icon {
                width: width.try_into()?,
                height: height.try_into()?,
                data,
            })
        })
        .collect()
}

async fn decode_icon_off_thread(icon_bytes: &[u8]) -> Result<Vec<Icon>> {
    let icon_bytes = icon_bytes.to_vec();
    crate::process::AsyncHandler::spawn_blocking(move || decode_icon(&icon_bytes)).await?
}

pub fn is_active() -> bool {
    TRAY_HANDLE.get().is_some()
}

pub async fn create_tray(icon_bytes: &[u8]) -> bool {
    if is_active() {
        return true;
    }

    let icon = match decode_icon_off_thread(icon_bytes).await {
        Ok(icon) => icon,
        Err(err) => {
            crate::logging!(warn, crate::Type::Tray, "Не удалось разобрать иконку трея: {err}");
            return false;
        }
    };

    let spawned = tokio::time::timeout(SPAWN_DEADLINE, async {
        for attempt in 1..=SPAWN_ATTEMPTS {
            let tray = ClodTray {
                icon: icon.clone(),
                tooltip: crate::constants::branding::APP_NAME.into(),
                menu: Vec::new(),
            };
            match tray.spawn().await {
                Ok(handle) => return Some(handle),
                Err(err) => {
                    crate::logging!(
                        warn,
                        crate::Type::Tray,
                        "Попытка {attempt} зарегистрировать трей StatusNotifierItem не удалась: {err}"
                    );
                    if attempt < SPAWN_ATTEMPTS {
                        tokio::time::sleep(SPAWN_RETRY_DELAY).await;
                    }
                }
            }
        }
        None
    })
    .await;

    match spawned {
        Ok(Some(handle)) => {
            let _ = TRAY_HANDLE.set(handle);
            true
        }
        Ok(None) => false,
        Err(_) => {
            crate::logging!(
                warn,
                crate::Type::Tray,
                "Регистрация трея StatusNotifierItem не уложилась в срок"
            );
            false
        }
    }
}

async fn with_handle<F: FnOnce(&mut ClodTray) + Send>(what: &str, update: F) {
    let Some(handle) = TRAY_HANDLE.get() else {
        return;
    };
    match tokio::time::timeout(UPDATE_TIMEOUT, handle.update(update)).await {
        Ok(Some(())) => {}
        Ok(None) => crate::logging!(warn, crate::Type::Tray, "Служба трея закрыта, {what} не применено"),
        Err(_) => crate::logging!(
            warn,
            crate::Type::Tray,
            "Хост трея не ответил вовремя, {what} могло не примениться"
        ),
    }
}

pub async fn update_menu(menu: Vec<MenuNode>) {
    with_handle("меню", move |tray| tray.menu = menu).await;
}

pub async fn update_icon(icon_bytes: &[u8]) {
    match decode_icon_off_thread(icon_bytes).await {
        Ok(icon) => with_handle("иконка", move |tray| tray.icon = icon).await,
        Err(err) => crate::logging!(warn, crate::Type::Tray, "Не удалось разобрать иконку трея: {err}"),
    }
}

pub async fn update_tooltip(tooltip: String) {
    with_handle("подсказка", move |tray| tray.tooltip = tooltip).await;
}
