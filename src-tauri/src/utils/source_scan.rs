//! Тестовые помощники для проверок, которые читают исходник: порядок вызовов
//! и проводку, которых таблицы состояний не видят.

/// Код до первого тестового модуля файла.
pub fn production_code(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap_or(source)
}

/// Код до первого тестового модуля: `mod`, перед которым стоит `#[cfg(test)]` или
/// `#[cfg(all(test, …))]` (возможно, вместе с другими атрибутами). Такие же атрибуты
/// у отдельных помощников посреди файла файл не обрезают. Перевод строки любой —
/// `\n` или `\r\n`: Git под Windows отдаёт исходники с CRLF, и `include_str!`
/// читает их как есть.
pub fn without_test_modules(source: &str) -> &str {
    let gates_a_module = |at: usize| {
        let mut rest = &source[at..];
        while let Some(attribute) = rest.strip_prefix("#[") {
            let Some(close) = attribute.find(']') else {
                return false;
            };
            rest = attribute[close + 1..].trim_start();
        }
        rest.starts_with("mod ")
    };
    let end = source
        .match_indices("#[cfg(test)]")
        .chain(source.match_indices("#[cfg(all(test"))
        .map(|(at, _)| at)
        .filter(|at| gates_a_module(*at))
        .min()
        .unwrap_or(source.len());
    &source[..end]
}

/// Тело функции (или блока) от первой `{` после `signature` до парной `}`.
///
/// Подпись сразу за кавычкой — строка в самом тесте, а не определение: иначе
/// тест, читающий свой же файл, нашёл бы «тело» в себе, когда функцию
/// переименуют, и проверка отсутствия прошла бы вслепую.
pub fn fn_body<'a>(source: &'a str, signature: &str) -> Option<&'a str> {
    let at = source
        .match_indices(signature)
        .map(|(at, _)| at)
        .find(|at| !source[..*at].ends_with('"'))?;
    let rest = &source[at..];
    let open = rest.find('{')?;
    let mut depth = 0usize;
    for (index, byte) in rest.bytes().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[open..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{fn_body, without_test_modules};

    #[test]
    fn the_test_module_is_found_whatever_the_line_endings() {
        let lf = "fn real() {}\n#[cfg(test)]\nfn helper() {}\nfn more() {}\n#[cfg(test)]\nmod tests {\n    fn real_in_test() {}\n}\n";
        let crlf = lf.replace('\n', "\r\n");
        for source in [lf.to_owned(), crlf] {
            let code = without_test_modules(&source);
            assert!(code.contains("fn more()"), "{code:?}");
            assert!(
                code.contains("fn helper()"),
                "помощник под #[cfg(test)] посреди файла не обрезает: {code:?}"
            );
            assert!(!code.contains("real_in_test"), "{code:?}");
        }
        let gated = "fn real() {}\r\n#[cfg(all(test, target_os = \"linux\"))]\r\nmod linux {}\r\n";
        assert!(!without_test_modules(gated).contains("mod linux"));
        let gated_helper =
            "fn a() {}\r\n#[cfg(all(test, target_os = \"macos\"))]\r\nfn only_in_tests() {}\r\nfn b() {}\r\n";
        assert!(
            without_test_modules(gated_helper).contains("fn b()"),
            "помощник под cfg(all(test)) не обрезает"
        );
        let attributed = "fn real() {}\r\n#[cfg(test)]\r\n#[allow(clippy::x)]\r\nmod t {}\r\n";
        assert!(!without_test_modules(attributed).contains("mod t"));
    }

    #[test]
    fn a_signature_quoted_by_a_test_is_not_the_definition() {
        let source = "fn scan() { if find(\"fn real(\") { hit() } }\nfn real() { work() }";
        assert_eq!(fn_body(source, "fn real("), Some("{ work() }"));
        assert_eq!(
            fn_body("fn scan() { if find(\"fn gone(\") { hit() } }", "fn gone("),
            None
        );
    }
}
