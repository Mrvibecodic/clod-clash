use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool,
    Ordering::{Acquire, Relaxed, Release},
};
use tokio::sync::Notify;

pub type SharedDraft<T> = Arc<Box<T>>;
// (committed_snapshot, optional_draft_snapshot)
type DraftData<T> = (SharedDraft<T>, Option<SharedDraft<T>>);
// Retry one scheduler turn before parking on Notify for long async updates.
const DATA_MODIFY_FAST_RETRY_YIELDS: usize = 1;

#[derive(Debug)]
struct DraftInner<T> {
    data: Mutex<DraftData<T>>,
    data_modifying: AtomicBool,
    data_modify_notify: Notify,
}

struct DataModifyPermit<'a>(&'a AtomicBool, &'a Notify);

impl Drop for DataModifyPermit<'_> {
    fn drop(&mut self) {
        self.0.store(false, Release);
        self.1.notify_one();
    }
}

/// Управление черновиком: committed и optional draft хранятся как Arc<Box<T>>.
#[derive(Debug, Clone)]
pub struct Draft<T> {
    inner: Arc<DraftInner<T>>,
}

impl<T: Clone> Draft<T> {
    #[inline]
    pub fn new(data: T) -> Self {
        Self {
            inner: Arc::new(DraftInner {
                data: Mutex::new((Arc::new(Box::new(data)), None)),
                data_modifying: AtomicBool::new(false),
                data_modify_notify: Notify::new(),
            }),
        }
    }
    /// Возвращает снапшот текущих "committed (основных)" данных как Arc<Box<T>>
    /// (без копирования, клонируется только Arc)
    #[inline]
    pub fn data_arc(&self) -> SharedDraft<T> {
        let guard = self.inner.data.lock();
        Arc::clone(&guard.0)
    }

    /// Возвращает снапшот текущих данных (черновик, если он есть, иначе committed)
    /// Тоже без копирования: клонируется только Arc, не T
    #[inline]
    pub fn latest_arc(&self) -> SharedDraft<T> {
        let guard = self.inner.data.lock();
        guard.1.clone().unwrap_or_else(|| Arc::clone(&guard.0))
    }

    /// Изменяет черновик через замыкание (в замыкание передаётся &mut T)
    /// - Отложенное копирование: если ссылка на Arc всего одна, правим напрямую,
    ///   T не клонируется;
    /// - если черновик разделяют другие читатели, Arc::make_mut сделает одно
    ///   T.clone (минимально необходимая копия).
    #[inline]
    pub fn edit_draft<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut guard = self.inner.data.lock();
        let mut draft_arc = guard.1.take().unwrap_or_else(|| Arc::clone(&guard.0));
        let data_mut = Arc::make_mut(&mut draft_arc);
        let result = f(data_mut.as_mut());
        guard.1 = Some(draft_arc);
        result
    }

    /// Фиксирует черновик в committed-позиции (замена) и очищает черновик
    #[inline]
    pub fn apply(&self) {
        let mut guard = self.inner.data.lock();
        if let Some(d) = guard.1.take() {
            guard.0 = d;
        }
    }

    /// Отбрасывает черновик (если он есть)
    #[inline]
    pub fn discard(&self) {
        let mut guard = self.inner.data.lock();
        guard.1 = None;
    }

    /// Асинхронно изменяет committed-данные через владение Box<T>: committed-данные
    /// клонируются один раз локально, асинхронное замыкание возвращает новый
    /// Box<T> (заменяет committed-данные) и результат R.
    #[inline]
    pub async fn with_data_modify<F, Fut, R>(&self, f: F) -> Result<R, anyhow::Error>
    where
        T: Send + Sync + 'static,
        F: FnOnce(T) -> Fut + Send,
        Fut: std::future::Future<Output = Result<(T, R), anyhow::Error>> + Send,
    {
        let _permit = self.acquire_data_modify_permit().await;
        let (local, original_arc) = {
            let guard = self.inner.data.lock();
            let arc = Arc::clone(&guard.0);
            ((**arc).clone(), arc)
        };
        let (new_local, res) = f(local).await?;
        let mut guard = self.inner.data.lock();
        if !Arc::ptr_eq(&guard.0, &original_arc) {
            return Err(anyhow::anyhow!(
                "Optimistic lock failed: Committed data has changed during async operation"
            ));
        }
        guard.0 = Arc::new(Box::new(new_local));
        Ok(res)
    }

    #[inline]
    fn try_acquire_data_modify_permit(&self) -> Option<DataModifyPermit<'_>> {
        self.inner
            .data_modifying
            .compare_exchange(false, true, Acquire, Relaxed)
            .ok()
            .map(|_| DataModifyPermit(&self.inner.data_modifying, &self.inner.data_modify_notify))
    }

    #[inline]
    async fn acquire_data_modify_permit(&self) -> DataModifyPermit<'_> {
        for _ in 0..DATA_MODIFY_FAST_RETRY_YIELDS {
            if let Some(permit) = self.try_acquire_data_modify_permit() {
                return permit;
            }
            tokio::task::yield_now().await;
        }

        loop {
            let notified = self.inner.data_modify_notify.notified();
            if let Some(permit) = self.try_acquire_data_modify_permit() {
                return permit;
            }
            notified.await;
        }
    }
}
