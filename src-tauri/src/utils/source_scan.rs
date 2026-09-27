//! Тестовые помощники для проверок, которые читают исходник: порядок вызовов
//! и проводку, которых таблицы состояний не видят.

/// Код до первого тестового модуля файла.
pub fn production_code(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap_or(source)
}

/// Тело функции (или блока) от первой `{` после `signature` до парной `}`.
pub fn fn_body<'a>(source: &'a str, signature: &str) -> Option<&'a str> {
    let at = source.find(signature)?;
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
