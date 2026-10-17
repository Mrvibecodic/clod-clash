use crate::process::AsyncHandler;

use super::field::{use_lowercase, use_lowercase_owned};
use anyhow::{Error, Result};
use boa_engine::{Context, JsString, JsValue, Source, native_function::NativeFunction};
use clash_verge_logging::{Type, logging};
use parking_lot::Mutex;
use serde_yaml_ng::Mapping;
use smartstring::alias::String;
use std::sync::Arc;

const MAX_OUTPUTS: usize = 1000;
const MAX_OUTPUT_SIZE: usize = 1024 * 1024; // 1MB
const MAX_JSON_SIZE: usize = 10 * 1024 * 1024; // 10MB
const MAX_LOOP_ITERATIONS: u64 = 10_000_000;
const SCRIPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub async fn use_script(script: String, config: Mapping, name: String) -> Result<Mapping> {
    let handle = AsyncHandler::spawn_blocking(move || use_script_sync(script, &config, &name));
    match tokio::time::timeout(SCRIPT_TIMEOUT, handle).await {
        Ok(Ok(result)) => result,
        Ok(Err(join_err)) => Err(anyhow::anyhow!("script task panicked: {join_err}")),
        Err(_elapsed) => Err(anyhow::anyhow!("script execution timed out after {:?}", SCRIPT_TIMEOUT)),
    }
}

/// Метка, с которой обёртка возвращает исключение `main`.
const ERROR_FLAG: &str = "__error_flag__ ";

fn use_script_sync(script: String, config: &Mapping, name: &String) -> Result<Mapping> {
    let mut context = Context::default();

    context
        .runtime_limits_mut()
        .set_loop_iteration_limit(MAX_LOOP_ITERATIONS);

    // Сколько раз скрипт уже писал в консоль и сколько написал.
    let written = Arc::new(Mutex::new((0usize, 0usize)));
    let written_clone = Arc::clone(&written);
    let script_name = name.clone();

    let _ = context.register_global_builtin_callable("__verge_log__".into(), 2, unsafe {
        NativeFunction::from_closure(move |_: &JsValue, args: &[JsValue], context: &mut Context| {
            let level = args
                .first()
                .ok_or_else(|| boa_engine::JsError::from_opaque(JsString::from("Missing level argument").into()))?;
            let level = level.to_string(context)?;
            let level = level.to_std_string().map_err(|_| {
                boa_engine::JsError::from_opaque(JsString::from("Failed to convert level to string").into())
            })?;

            let data = args
                .get(1)
                .ok_or_else(|| boa_engine::JsError::from_opaque(JsString::from("Missing data argument").into()))?;
            let data = data.to_string(context)?;
            let data = data.to_std_string().map_err(|_| {
                boa_engine::JsError::from_opaque(JsString::from("Failed to convert data to string").into())
            })?;

            let mut written = written_clone.lock();
            if written.0 >= MAX_OUTPUTS {
                return Err(boa_engine::JsError::from_opaque(
                    JsString::from("Maximum number of log outputs exceeded").into(),
                ));
            }
            let new_size = written.1 + level.len() + data.len();
            if new_size > MAX_OUTPUT_SIZE {
                return Err(boa_engine::JsError::from_opaque(
                    JsString::from("Maximum output size exceeded").into(),
                ));
            }
            *written = (written.0 + 1, new_size);
            drop(written);
            logging!(debug, Type::Config, "[script {script_name}] console.{level}: {data}");
            Ok(JsValue::undefined())
        })
    });

    let _ = context.eval(Source::from_bytes(
        r#"var console = Object.freeze({
        log(data){__verge_log__("log",JSON.stringify(data, null, 2))},
        info(data){__verge_log__("info",JSON.stringify(data, null, 2))},
        error(data){__verge_log__("error",JSON.stringify(data, null, 2))},
        debug(data){__verge_log__("debug",JSON.stringify(data, null, 2))},
        warn(data){__verge_log__("warn",JSON.stringify(data, null, 2))},
        table(data){__verge_log__("table",JSON.stringify(data, null, 2))},
      });"#,
    ));

    let config = use_lowercase(config);
    let config_str = serde_json::to_string(&config)?;
    if config_str.len() > MAX_JSON_SIZE {
        anyhow::bail!("Configuration size exceeds maximum allowed size");
    }

    // Обрабатываем только специальные символы в параметре name
    let safe_name = escape_js_string_for_single_quote(name);
    if safe_name.len() > 1024 {
        anyhow::bail!("Name parameter too long");
    }

    let code = format!(
        r"try{{
        {script};
        JSON.stringify(main({config_str},'{safe_name}')||'')
      }} catch(err) {{
        `{ERROR_FLAG}${{err.toString()}}`
      }}"
    );

    let result = context
        .eval(Source::from_bytes(code.as_str()))
        .map_err(|err| anyhow::anyhow!("the script does not run: {err}"))?;
    if !result.is_string() {
        anyhow::bail!("main function should return object");
    }
    let result = result
        .to_string(&mut context)
        .map_err(|e| anyhow::anyhow!("Failed to convert JS result to string: {}", e))?;
    let result = result
        .to_std_string()
        .map_err(|_| anyhow::anyhow!("Failed to convert JS string to std string"))?;

    if let Some(thrown) = result.strip_prefix(ERROR_FLAG) {
        anyhow::bail!("main threw: {thrown}");
    }
    if result.len() > MAX_JSON_SIZE {
        anyhow::bail!("Script result exceeds maximum allowed size");
    }

    parse_json_safely(&result)
        .map(use_lowercase_owned)
        .map_err(|err| anyhow::anyhow!("main function should return object: {err}"))
}

fn parse_json_safely(json_str: &str) -> Result<Mapping, Error> {
    if json_str.len() > MAX_JSON_SIZE {
        anyhow::bail!("JSON string too large");
    }

    Ok(serde_json::from_str::<Mapping>(json_str)?)
}

// Безопасно экранирует строку
fn escape_js_string_for_single_quote(s: &str) -> String {
    // Ограничиваем длину обрабатываемой строки
    if s.len() > 10240 {
        return s[..10240].replace('\\', "\\\\").replace('\'', "\\'").into();
    }

    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n") // Добавляем экранирование перевода строки
        .replace('\r', "\\r") // Добавляем экранирование возврата каретки
        .into()
}

#[test]
#[allow(clippy::expect_used)]
fn test_script() {
    let script = r#"
    function main(config) {
      if (Array.isArray(config.rules)) {
        config.rules = [...config.rules, "add"];
      }
      console.log(config);
      config.proxies = ["111"];
      return config;
    }
  "#;

    let config = r"
    rules:
      - 111
      - 222
    tun:
      enable: false
    dns:
      enable: false
  ";

    let config = &serde_yaml_ng::from_str(config).expect("Failed to parse test config YAML");
    let config =
        use_script_sync(script.into(), config, &String::from("")).expect("Script execution should succeed in test");

    let _ = serde_yaml_ng::to_string(&config).expect("Failed to serialize config to YAML");
    let yaml_config_size = std::mem::size_of_val(&config);
    let box_yaml_config_size = std::mem::size_of_val(&Box::new(config));
    assert!(box_yaml_config_size < yaml_config_size);
}

// Тестирует экранирование специальных символов
#[test]
#[allow(clippy::expect_used)]
fn test_escape_unescape() {
    let test_string = r#"Hello "World"!\nThis is a test with \u00A9 copyright symbol."#;
    let escaped = escape_js_string_for_single_quote(test_string);
    println!("Original: {test_string}");
    println!("Escaped: {escaped}");

    let json_str = r#"{"key":"value","nested":{"key":"value"}}"#;
    let parsed = parse_json_safely(json_str).expect("Failed to parse test JSON safely");

    assert!(parsed.contains_key("key"));
    assert!(parsed.contains_key("nested"));

    // Скрипт, вернувший строку вместо объекта, — ошибка, а не пустой конфиг.
    assert!(parse_json_safely(r#""{}""#).is_err());
}

#[test]
fn test_memory_limits() {
    // Тестируем лимит вывода
    let script = r#"
    function main(config) {
      for(let i = 0; i < 2000; i++) {
        console.log("test");
      }
      return config;
    }
  "#;

    #[allow(clippy::expect_used)]
    let config = &serde_yaml_ng::from_str("test: value").expect("Failed to parse test YAML");
    let result = use_script_sync(script.into(), config, &String::from(""));
    // Лимит вывода — исключение в скрипте, а не panic.
    assert!(
        result
            .as_ref()
            .is_err_and(|err| err.to_string().contains("Maximum number of log outputs exceeded")),
        "{result:?}"
    );
}

#[test]
fn a_script_error_keeps_its_own_words() {
    let config = serde_yaml_ng::from_str::<Mapping>("test: value").unwrap_or_default();
    let thrown = use_script_sync(
        "function main(config) { throw new Error('no proxies here'); }".into(),
        &config,
        &String::from("sub"),
    );
    assert!(
        thrown
            .as_ref()
            .is_err_and(|err| err.to_string().contains("no proxies here")),
        "{thrown:?}"
    );
    let not_an_object = use_script_sync(
        "function main(config) { return 1; }".into(),
        &config,
        &String::from("sub"),
    );
    assert!(not_an_object.is_err(), "{not_an_object:?}");
}
