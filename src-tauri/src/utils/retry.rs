//! Перебор маршрутов: тот же запрос по очереди через каждый, до первого успеха.

use std::future::Future;

/// Выполнить `operation` по маршрутам `first`, затем `rest` — до первого
/// успеха. Все неудачны — ошибка последнего. Маршрут, давший ответ, не
/// запоминается: следующий вызов снова начинает с `first`.
pub async fn try_strategies<S, T, E, Fut, const N: usize>(
    first: S,
    rest: [S; N],
    mut operation: impl FnMut(S) -> Fut + Send,
) -> Result<T, E>
where
    S: Copy + Send,
    T: Send,
    E: Send,
    Fut: Future<Output = Result<T, E>> + Send,
{
    let mut result = operation(first).await;
    for strategy in rest {
        if result.is_ok() {
            break;
        }
        result = operation(strategy).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::try_strategies;

    #[tokio::test]
    async fn routes_are_tried_in_order_until_the_first_success() {
        let mut tried = Vec::new();
        let found: Result<u8, &str> = try_strategies(1_u8, [2, 3], |route| {
            tried.push(route);
            async move { if route == 2 { Ok(route) } else { Err("no") } }
        })
        .await;
        assert_eq!(found, Ok(2));
        assert_eq!(tried, vec![1, 2]);
    }

    #[tokio::test]
    async fn when_every_route_fails_the_last_error_is_returned() {
        let mut tried = Vec::new();
        let found: Result<u8, u8> = try_strategies(1_u8, [2, 3], |route| {
            tried.push(route);
            async move { Err(route * 10) }
        })
        .await;
        assert_eq!(found, Err(30));
        assert_eq!(tried, vec![1, 2, 3]);
    }

    #[test]
    fn downloads_walk_their_routes_through_one_helper() {
        use crate::utils::source_scan::{fn_body, production_code};

        for (name, source, functions) in [
            (
                "geo_assets.rs",
                include_str!("../module/geo_assets.rs"),
                &["async fn download("][..],
            ),
            (
                "logo_cache.rs",
                include_str!("../module/logo_cache.rs"),
                &["async fn download("][..],
            ),
            (
                "core_updater.rs",
                include_str!("../core/core_updater.rs"),
                &[
                    "async fn fetch_release(",
                    "async fn download_asset(",
                    "async fn verify_sha256(",
                ][..],
            ),
        ] {
            let code = production_code(source);
            assert!(!code.contains("for proxy in ["), "{name}: свой перебор маршрутов");
            assert!(
                !code.contains("Some(hwid::user_agent())"),
                "{name}: умолчание передано явно"
            );
            for function in functions {
                let body = fn_body(code, function).unwrap_or_default();
                assert!(body.contains("try_strategies("), "{name} {function}");
            }
        }

        let logo = production_code(include_str!("../module/logo_cache.rs"));
        assert!(
            !logo.contains(".part"),
            "временный файл картинки — мимо общей атомарной записи"
        );
        assert!(!logo.contains("fn sweep_parts("));
    }
}
