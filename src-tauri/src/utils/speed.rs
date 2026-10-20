//! Скорость текстом — по тому же правилу, что в окне (`src/utils/parse-traffic.ts`):
//! три значащие цифры, от тысячи — уже следующая единица, единица английская
//! и через пробел, разделитель дробной части — по языку приложения.

const UNITS: [&str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];

/// Скорость текстом: «1.20 MB/s», «0.977 KB/s» (с запятой — по языку).
#[cfg(target_os = "macos")]
pub fn format_bytes_per_second(bytes_per_sec: u64) -> String {
    speed_text(bytes_per_sec, clash_verge_i18n::decimal_separator())
}

/// Делит нацело с округлением половины вверх, как `toPrecision` в окне.
const fn rounded_ratio(numerator: u128, denominator: u128) -> u128 {
    (numerator * 2 + denominator) / (denominator * 2)
}

fn speed_text(bytes_per_sec: u64, separator: &str) -> String {
    let bytes = u128::from(bytes_per_sec);
    let last = UNITS.len() - 1;
    let mut exp = bytes.checked_ilog2().map_or(0, |log| log as usize / 10).min(last);
    if exp < last && bytes * 2 >= 1999 << (10 * exp) {
        exp += 1;
    }
    let scale = 1_u128 << (10 * exp);

    let mut decimals = match [100, 10, 1].iter().position(|&digits| bytes >= digits * scale) {
        Some(decimals) => decimals,
        None if bytes == 0 => 2,
        None => 3,
    };
    let mut shown = rounded_ratio(bytes * 10_u128.pow(decimals as u32), scale);
    if shown >= 1000 && decimals > 0 {
        decimals -= 1;
        shown = rounded_ratio(bytes * 10_u128.pow(decimals as u32), scale);
    }

    let digits = format!("{shown:0>width$}", width = decimals + 1);
    let (whole, fraction) = digits.split_at(digits.len() - decimals);
    let number = if fraction.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}{separator}{fraction}")
    };
    format!("{number} {}/s", UNITS[exp])
}

#[cfg(test)]
mod tests {
    use super::speed_text;

    #[derive(serde::Deserialize)]
    struct Cases {
        point: Vec<(u64, String)>,
        comma: Vec<(u64, String)>,
    }

    /// Примеры общие с `src/utils/parse-traffic.test.ts`: строка меню и окно
    /// пишут одну скорость одинаково.
    #[test]
    fn the_menu_bar_writes_speed_as_the_window_does() {
        let cases: Option<Cases> = serde_json::from_str(include_str!("../../../src/utils/speed-cases.json")).ok();
        assert!(
            cases
                .as_ref()
                .is_some_and(|cases| !cases.point.is_empty() && !cases.comma.is_empty())
        );
        let Some(Cases { point, comma }) = cases else { return };
        for (separator, cases) in [(".", point), (",", comma)] {
            for (bytes, shown) in cases {
                assert_eq!(speed_text(bytes, separator), shown, "{bytes}");
            }
        }
    }
}
